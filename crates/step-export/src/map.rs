//! Maps the AP203/AP214 B-rep subset of a parsed file to the neutral [`Model`].
//!
//! Written from the ISO 10303-42 (geometry and topology) and ISO 10303-41 (units)
//! entity definitions as used by AP203 and AP214. Supported: `MANIFOLD_SOLID_BREP`,
//! `BREP_WITH_VOIDS`, `FACETED_BREP`, `SHELL_BASED_SURFACE_MODEL`; closed, open and
//! oriented shells; `ADVANCED_FACE`, `FACE_SURFACE`, `FACE` (planar polygon) and
//! `ORIENTED_FACE`; edge, vertex and poly loops; the curves and surfaces of
//! [`crate::model`]; `SURFACE_CURVE`, `SEAM_CURVE`, `INTERSECTION_CURVE` and
//! `TRIMMED_CURVE` as edge geometry (their 3D / basis curve); SI and conversion-based
//! length and plane-angle units. Anything else reachable from a body makes that body
//! [`Skipped`] with `unsupported: <ENTITY>`; it never panics.
//!
//! A face that no shell references (AP203 gives such a face no body) is mapped as a
//! single-face open-shell body of kind [`BodyKind::ShellModel`], with a synthetic shell
//! (`step_id` 0). Faces of a body that was skipped stay skipped with that body.
//!
//! Every reference is checked to exist, reference chains are bounded by
//! [`Limits::max_ref_depth`], and no allocation is sized from a value in the file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::geom::{self, build_frame, conic_angle, dist, unit};
use solidcore_record::step::model::{
    BSplineCurve, BSplineSurface, Body, BodyKind, Bound, Curve, Curve2d, Diagnostic, Edge, Face,
    FilePcurves, Frame, Loop, Model, OrientedEdge, Pcurve, Shell, Skipped, Surface, Vertex, P3,
};
use solidcore_record::step::p21::{self, Entity, Exchange as StepFile, Param, Record};

/// Bounds the mapper applies to untrusted input (the parser's own are [`p21`]'s `MAX_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Deepest chain of references the mapper follows from a representation item
    /// (e.g. surface of revolution -> curve -> ...). Cycles stop here.
    pub max_ref_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_ref_depth: 64 }
    }
}

/// An entity instance as the mapper reads it: one record, or the parts of a complex instance.
#[derive(Debug, Clone, Copy)]
enum Instance<'a> {
    Simple(&'a Record),
    Complex(&'a [Record]),
}

impl<'a> Instance<'a> {
    fn of(entity: &'a Entity) -> Self {
        match entity.records.as_slice() {
            [record] => Self::Simple(record),
            records => Self::Complex(records),
        }
    }

    fn record(self, name: &str) -> Option<&'a Record> {
        match self {
            Self::Simple(r) => (r.name == name).then_some(r),
            Self::Complex(rs) => rs.iter().find(|r| r.name == name),
        }
    }

    fn type_name(self) -> String {
        match self {
            Self::Simple(r) => r.name.clone(),
            Self::Complex(rs) => rs
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join("+"),
        }
    }
}

/// Reads `bytes` as a Part 21 file and maps its bodies to the neutral [`Model`].
///
/// # Errors
/// The [`p21::Error`] when the text is not a readable exchange structure.
pub fn import(bytes: &[u8], limits: &Limits) -> Result<Model, p21::Error> {
    let text = String::from_utf8_lossy(bytes);
    let file = p21::parse(&text)?;
    Ok(map_file(&file, limits))
}

/// Why a body could not be mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// An entity outside the supported subset.
    Unsupported(String),
    /// Malformed data (dangling reference, wrong parameter, bad B-spline, ...).
    Invalid(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Unsupported(e) => write!(f, "unsupported: {e}"),
            MapError::Invalid(e) => write!(f, "invalid: {e}"),
        }
    }
}

type R<T> = Result<T, MapError>;

fn invalid<T>(msg: impl Into<String>) -> R<T> {
    Err(MapError::Invalid(msg.into()))
}

/// Units of one representation context.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Units {
    /// Millimetres per length unit.
    mm: f64,
    /// Degrees per plane-angle unit.
    deg: f64,
    /// Distance uncertainty in mm.
    uncertainty: Option<f64>,
}

impl Default for Units {
    fn default() -> Self {
        Self {
            mm: 1.0,
            deg: 180.0 / std::f64::consts::PI,
            uncertainty: None,
        }
    }
}

const BODY_NAMES: [(&str, BodyKind); 4] = [
    ("MANIFOLD_SOLID_BREP", BodyKind::ManifoldSolid),
    ("BREP_WITH_VOIDS", BodyKind::SolidWithVoids),
    ("FACETED_BREP", BodyKind::Faceted),
    ("SHELL_BASED_SURFACE_MODEL", BodyKind::ShellModel),
];

struct Mapper<'a> {
    file: &'a StepFile,
    limits: Limits,
}

/// Per-body state.
struct Ctx {
    u: Units,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    faces: Vec<Face>,
    vmap: BTreeMap<u64, usize>,
    emap: BTreeMap<u64, usize>,
    /// Edges made from poly-loop segments, by their point ids (low, high).
    pmap: BTreeMap<(u64, u64), usize>,
}

impl<'a> Mapper<'a> {
    fn inst(&self, id: u64) -> R<Instance<'a>> {
        self.file
            .entities
            .get(&id)
            .map(Instance::of)
            .map_or_else(|| invalid(format!("dangling reference #{id}")), Ok)
    }

    /// The simple record of `id`, any name.
    fn simple(&self, id: u64) -> R<&'a Record> {
        match self.inst(id)? {
            Instance::Simple(r) => Ok(r),
            Instance::Complex(_) => Err(MapError::Unsupported(format!(
                "complex instance #{id} ({})",
                self.inst(id)?.type_name()
            ))),
        }
    }

    fn check_depth(&self, depth: usize) -> R<()> {
        if depth > self.limits.max_ref_depth {
            return invalid("reference chain too deep (cycle?)");
        }
        Ok(())
    }

    fn point(&self, p: &Param, s: f64) -> R<P3> {
        let id = ref_of(p)?;
        let r = self.simple(id)?;
        if r.name != "CARTESIAN_POINT" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let c = list(param(r, 1)?)?;
        let x = num_at(c, 0)?;
        let y = num_at(c, 1)?;
        let z = if c.len() > 2 { num_at(c, 2)? } else { 0.0 };
        Ok([x * s, y * s, z * s])
    }

    fn direction(&self, p: &Param) -> R<P3> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "DIRECTION" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let c = list(param(r, 1)?)?;
        let z = if c.len() > 2 { num_at(c, 2)? } else { 0.0 };
        Ok([num_at(c, 0)?, num_at(c, 1)?, z])
    }

    /// `VECTOR(name, orientation, magnitude)` as (unit direction, magnitude in mm).
    fn vector(&self, p: &Param, s: f64) -> R<(P3, f64)> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "VECTOR" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let d =
            unit(self.direction(param(r, 1)?)?).map_or_else(|| invalid("zero direction"), Ok)?;
        let m = num(param(r, 2)?)? * s;
        Ok((d, m))
    }

    fn placement(&self, p: &Param, s: f64) -> R<Frame> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "AXIS2_PLACEMENT_3D" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let o = self.point(param(r, 1)?, s)?;
        let axis = match param(r, 2)? {
            Param::Null => None,
            q => Some(self.direction(q)?),
        };
        let rd = match r.params.get(3) {
            None | Some(Param::Null) => None,
            Some(q) => Some(self.direction(q)?),
        };
        build_frame(o, axis, rd).map_or_else(|| invalid("degenerate AXIS2_PLACEMENT_3D"), Ok)
    }

    // ---- units ---------------------------------------------------------------------

    /// (kind, factor): kind `L` length (mm per unit), `A` plane angle (degrees per
    /// unit), or other.
    fn unit_factor(&self, id: u64, depth: usize) -> R<(char, f64)> {
        self.check_depth(depth)?;
        let inst = self.inst(id)?;
        let kind = if inst.record("LENGTH_UNIT").is_some() {
            'L'
        } else if inst.record("PLANE_ANGLE_UNIT").is_some() {
            'A'
        } else {
            return Ok(('?', 1.0));
        };
        if let Some(si) = inst.record("SI_UNIT") {
            let prefix = match si.params.first() {
                Some(Param::Enum(e)) => Some(e.as_str()),
                _ => None,
            };
            let f = match prefix {
                None => 1.0,
                Some("EXA") => 1e18,
                Some("PETA") => 1e15,
                Some("TERA") => 1e12,
                Some("GIGA") => 1e9,
                Some("MEGA") => 1e6,
                Some("KILO") => 1e3,
                Some("HECTO") => 1e2,
                Some("DECA") => 1e1,
                Some("DECI") => 1e-1,
                Some("CENTI") => 1e-2,
                Some("MILLI") => 1e-3,
                Some("MICRO") => 1e-6,
                Some("NANO") => 1e-9,
                Some("PICO") => 1e-12,
                Some(p) => return Err(MapError::Unsupported(format!("SI prefix {p}"))),
            };
            return Ok(match kind {
                'L' => ('L', f * 1000.0),
                _ => ('A', f * 180.0 / std::f64::consts::PI),
            });
        }
        if let Some(cb) = inst.record("CONVERSION_BASED_UNIT") {
            let m = self.inst(ref_of(param(cb, 1)?)?)?;
            let mr = match m {
                Instance::Simple(r) => r,
                Instance::Complex(rs) => rs
                    .iter()
                    .find(|r| r.name == "MEASURE_WITH_UNIT")
                    .map_or_else(|| invalid("conversion factor"), Ok)?,
            };
            let v = num(param(mr, 0)?)?;
            let (k2, f2) = self.unit_factor(ref_of(param(mr, 1)?)?, depth + 1)?;
            if k2 != kind {
                return invalid("conversion unit of another kind");
            }
            return Ok((kind, v * f2));
        }
        Err(MapError::Unsupported(format!(
            "unit #{id} ({})",
            inst.type_name()
        )))
    }

    fn context_units(&self, id: u64) -> R<Units> {
        let ctx = self.inst(id)?;
        let mut u = Units::default();
        if let Some(g) = ctx.record("GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for p in list(param(g, 0)?)? {
                match self.unit_factor(ref_of(p)?, 0)? {
                    ('L', f) => u.mm = f,
                    ('A', f) => u.deg = f,
                    _ => {}
                }
            }
        }
        if let Some(g) = ctx.record("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
            for p in list(param(g, 0)?)? {
                let r = self.simple(ref_of(p)?)?;
                if r.name != "UNCERTAINTY_MEASURE_WITH_UNIT" {
                    continue;
                }
                if let (Ok(v), Ok(uid)) = (num(param(r, 0)?), ref_of(param(r, 1)?)) {
                    if let ('L', f) = self.unit_factor(uid, 0)? {
                        u.uncertainty = Some(v * f);
                    }
                }
            }
        }
        if !(u.mm.is_finite() && u.mm > 0.0 && u.deg.is_finite() && u.deg > 0.0) {
            return invalid("bad unit factor");
        }
        Ok(u)
    }

    /// Representation item id -> context id, from every representation's items.
    fn item_contexts(&self) -> BTreeMap<u64, u64> {
        let mut out = BTreeMap::new();
        for inst in self.file.entities.values().map(Instance::of) {
            let rec = match inst {
                Instance::Simple(r) if r.name.ends_with("REPRESENTATION") => r,
                Instance::Complex(rs) => match rs.iter().find(|r| r.name == "REPRESENTATION") {
                    Some(r) => r,
                    None => continue,
                },
                Instance::Simple(_) => continue,
            };
            let (Some(Param::List(items)), Some(Param::Ref(ctx))) =
                (rec.params.get(1), rec.params.get(2))
            else {
                continue;
            };
            for it in items {
                if let Param::Ref(i) = it {
                    out.entry(*i).or_insert(*ctx);
                }
            }
        }
        out
    }

    // ---- curves --------------------------------------------------------------------

    /// An edge geometry: the curve and whether its direction is reversed relative to
    /// the referenced entity's own parameterisation (a `TRIMMED_CURVE` with
    /// `sense_agreement = .F.`).
    fn edge_curve_geom(&self, id: u64, c: &Ctx, depth: usize) -> R<(Curve, bool)> {
        self.check_depth(depth)?;
        let inst = self.inst(id)?;
        if let Instance::Simple(r) = inst {
            match r.name.as_str() {
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                    return self.edge_curve_geom(ref_of(param(r, 1)?)?, c, depth + 1);
                }
                "TRIMMED_CURVE" => {
                    let (cv, rev) = self.edge_curve_geom(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let sense = boolean(param(r, 4)?)?;
                    return Ok((cv, rev ^ !sense));
                }
                _ => {}
            }
        }
        Ok((self.curve(id, c, depth)?, false))
    }

    fn curve(&self, id: u64, c: &Ctx, depth: usize) -> R<Curve> {
        self.curve_scaled(id, c.u.mm, depth)
    }

    /// [`Self::curve`] with lengths times `s`.
    fn curve_scaled(&self, id: u64, s: f64, depth: usize) -> R<Curve> {
        self.check_depth(depth)?;
        let inst = self.inst(id)?;
        match inst {
            Instance::Simple(r) => match r.name.as_str() {
                "LINE" => {
                    let origin = self.point(param(r, 1)?, s)?;
                    let (dir, magnitude) = self.vector(param(r, 2)?, s)?;
                    if !(magnitude.is_finite() && magnitude > 0.0) {
                        return invalid("LINE with a zero vector");
                    }
                    Ok(Curve::Line {
                        origin,
                        dir,
                        magnitude,
                    })
                }
                "CIRCLE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let radius = positive(num(param(r, 2)?)? * s, "CIRCLE radius")?;
                    Ok(Curve::Circle { frame, radius })
                }
                "ELLIPSE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let semi1 = positive(num(param(r, 2)?)? * s, "ELLIPSE semi axis")?;
                    let semi2 = positive(num(param(r, 3)?)? * s, "ELLIPSE semi axis")?;
                    Ok(Curve::Ellipse {
                        frame,
                        semi1,
                        semi2,
                    })
                }
                "POLYLINE" => {
                    let pts = list(param(r, 1)?)?;
                    if pts.len() < 2 {
                        return invalid("POLYLINE with fewer than 2 points");
                    }
                    let mut points = Vec::new();
                    for p in pts {
                        points.push(self.point(p, s)?);
                    }
                    let n = points.len();
                    let knots: Vec<f64> = (0..n).map(|i| i as f64).collect();
                    let mut mults = vec![1u32; n];
                    if let Some(m) = mults.first_mut() {
                        *m = 2;
                    }
                    if let Some(m) = mults.last_mut() {
                        *m = 2;
                    }
                    Ok(Curve::BSpline(BSplineCurve {
                        degree: 1,
                        points,
                        weights: None,
                        knots,
                        mults,
                    }))
                }
                "B_SPLINE_CURVE_WITH_KNOTS" => {
                    let b = self.bspline_curve_base(r.params.as_slice(), 1, s)?;
                    let tail = r.params.get(6..).unwrap_or_default();
                    self.finish_bspline_curve(b, tail, None)
                }
                "QUASI_UNIFORM_CURVE" | "UNIFORM_CURVE" | "BEZIER_CURVE" => {
                    let b = self.bspline_curve_base(r.params.as_slice(), 1, s)?;
                    implied_knots_curve(b, &r.name, None)
                }
                // As a swept curve: the basis / 3D curve. The trimmed portion only
                // narrows the swept surface, which the face's loops bound anyway.
                "TRIMMED_CURVE" | "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                    self.curve_scaled(ref_of(param(r, 1)?)?, s, depth + 1)
                }
                other => Err(MapError::Unsupported(other.to_string())),
            },
            Instance::Complex(rs) => {
                let base = rs
                    .iter()
                    .find(|r| r.name == "B_SPLINE_CURVE")
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                let b = self.bspline_curve_base(base.params.as_slice(), 0, s)?;
                let weights = match rs.iter().find(|r| r.name == "RATIONAL_B_SPLINE_CURVE") {
                    Some(w) => Some(num_list(list(param(w, 0)?)?)?),
                    None => None,
                };
                if let Some(k) = rs.iter().find(|r| r.name == "B_SPLINE_CURVE_WITH_KNOTS") {
                    self.finish_bspline_curve(b, k.params.as_slice(), weights)
                } else if let Some(k) = rs.iter().find(|r| {
                    matches!(
                        r.name.as_str(),
                        "QUASI_UNIFORM_CURVE" | "UNIFORM_CURVE" | "BEZIER_CURVE"
                    )
                }) {
                    implied_knots_curve(b, &k.name, weights)
                } else {
                    Err(MapError::Unsupported(inst.type_name()))
                }
            }
        }
    }

    /// (degree, points) from `B_SPLINE_CURVE` attributes starting at `at` (after the
    /// name when the record is a simple instance).
    fn bspline_curve_base(&self, ps: &[Param], at: usize, s: f64) -> R<(u32, Vec<P3>)> {
        let degree = degree_of(ps.get(at))?;
        let pts = list(
            ps.get(at + 1)
                .map_or_else(|| invalid("missing points"), Ok)?,
        )?;
        let mut points = Vec::new();
        for p in pts {
            points.push(self.point(p, s)?);
        }
        Ok((degree, points))
    }

    /// Completes a B-spline from `B_SPLINE_CURVE_WITH_KNOTS`'s own attributes
    /// (multiplicities, knots, knot spec).
    #[allow(clippy::unused_self)]
    fn finish_bspline_curve(
        &self,
        (degree, points): (u32, Vec<P3>),
        ps: &[Param],
        weights: Option<Vec<f64>>,
    ) -> R<Curve> {
        let mults = mult_list(list(
            ps.first().map_or_else(|| invalid("missing mults"), Ok)?,
        )?)?;
        let knots = num_list(list(
            ps.get(1).map_or_else(|| invalid("missing knots"), Ok)?,
        )?)?;
        let b = BSplineCurve {
            degree,
            points,
            weights,
            knots,
            mults,
        };
        check_bspline_curve(&b)?;
        Ok(Curve::BSpline(b))
    }

    // ---- pcurves -------------------------------------------------------------------

    /// The pcurves of `EDGE_CURVE` `id`: the `PCURVE`s in the associated geometry of its
    /// `SURFACE_CURVE` / `SEAM_CURVE` / `INTERSECTION_CURVE` / `BOUNDED_SURFACE_CURVE`
    /// (through any `TRIMMED_CURVE`s), in the model's units. A pcurve that cannot be
    /// mapped is reported in `diagnostics` and left out.
    fn edge_pcurves(&self, id: u64, u: Units, diagnostics: &mut Vec<Diagnostic>) -> R<Vec<Pcurve>> {
        let edge = self.simple(id)?;
        let mut geometry = ref_of(param(edge, 3)?)?;
        let mut depth = 0;
        let surface_curve = loop {
            self.check_depth(depth)?;
            let Instance::Simple(r) = self.inst(geometry)? else {
                return Ok(Vec::new());
            };
            match r.name.as_str() {
                "TRIMMED_CURVE" => geometry = ref_of(param(r, 1)?)?,
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                    break r;
                }
                _ => return Ok(Vec::new()),
            }
            depth += 1;
        };
        let mut out = Vec::new();
        for item in list(param(surface_curve, 2)?)? {
            let pid = ref_of(item)?;
            // The associated geometry also lists surfaces; only pcurves are mapped.
            let Instance::Simple(r) = self.inst(pid)? else {
                continue;
            };
            if r.name != "PCURVE" {
                continue;
            }
            match self.pcurve(r, u) {
                Ok(pcurve) => out.push(pcurve),
                Err(error) => diagnostics.push(Diagnostic {
                    step_id: Some(pid),
                    message: format!("pcurve not used: {error}"),
                }),
            }
        }
        Ok(out)
    }

    /// `PCURVE(name, basis_surface, reference_to_curve)`, its curve scaled to the
    /// model's parameter units of the basis surface.
    fn pcurve(&self, r: &Record, u: Units) -> R<Pcurve> {
        let surface = ref_of(param(r, 1)?)?;
        let (su, sv) = self.pcurve_scale(surface, u)?;
        let rep = self.simple(ref_of(param(r, 2)?)?)?;
        if rep.name != "DEFINITIONAL_REPRESENTATION" {
            return Err(MapError::Unsupported(rep.name.clone()));
        }
        let mut curves = Vec::new();
        for item in list(param(rep, 1)?)? {
            let id = ref_of(item)?;
            if self.inst(id)?.record("AXIS2_PLACEMENT_2D").is_none() {
                curves.push(id);
            }
        }
        let [id] = curves.as_slice() else {
            return invalid(format!(
                "DEFINITIONAL_REPRESENTATION with {} curves",
                curves.len()
            ));
        };
        let (curve, trim) = self.curve_2d(*id, u, 0)?;
        Ok(Pcurve {
            surface,
            curve: scale_curve_2d(curve, su, sv),
            trim,
        })
    }

    /// The factors from a `PCURVE` basis surface's (u, v) in file units to the model's
    /// (mm for lengths, degrees for angles; ISO 10303-42 surface parameterisations).
    fn pcurve_scale(&self, surface: u64, u: Units) -> R<(f64, f64)> {
        let inst = self.inst(surface)?;
        let name = match inst {
            Instance::Simple(r) => r.name.as_str(),
            Instance::Complex(_) if inst.record("B_SPLINE_SURFACE").is_some() => {
                return Ok((1.0, 1.0));
            }
            Instance::Complex(_) => return Err(MapError::Unsupported(inst.type_name())),
        };
        match name {
            "PLANE" => Ok((u.mm, u.mm)),
            "CYLINDRICAL_SURFACE" | "CONICAL_SURFACE" => Ok((u.deg, u.mm)),
            "SPHERICAL_SURFACE" | "TOROIDAL_SURFACE" => Ok((u.deg, u.deg)),
            "B_SPLINE_SURFACE_WITH_KNOTS"
            | "QUASI_UNIFORM_SURFACE"
            | "UNIFORM_SURFACE"
            | "BEZIER_SURFACE" => Ok((1.0, 1.0)),
            other => Err(MapError::Unsupported(format!("pcurve on {other}"))),
        }
    }

    /// `AXIS2_PLACEMENT_2D(name, location, ref_direction)` as (origin, unit x, unit y),
    /// in file units.
    fn placement_2d(&self, p: &Param) -> R<([f64; 2], [f64; 2], [f64; 2])> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "AXIS2_PLACEMENT_2D" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let o = self.point(param(r, 1)?, 1.0)?;
        let x = match r.params.get(2) {
            None | Some(Param::Null) => [1.0, 0.0, 0.0],
            Some(q) => self.direction(q)?,
        };
        let x = unit([x[0], x[1], 0.0]).map_or_else(|| invalid("zero 2D direction"), Ok)?;
        Ok(([o[0], o[1]], [x[0], x[1]], [-x[1], x[0]]))
    }

    /// A pcurve's 2D curve in file units, with the parameter trims of a `TRIMMED_CURVE`
    /// around it (a conic's converted to degrees).
    fn curve_2d(&self, id: u64, u: Units, depth: usize) -> R<(Curve2d, Option<(f64, f64)>)> {
        self.check_depth(depth)?;
        if let Instance::Simple(r) = self.inst(id)? {
            match r.name.as_str() {
                "TRIMMED_CURVE" => {
                    let (curve, _) = self.curve_2d(ref_of(param(r, 1)?)?, u, depth + 1)?;
                    let scale = if matches!(curve, Curve2d::Conic { .. }) {
                        u.deg
                    } else {
                        1.0
                    };
                    let trim = trim_parameter(param(r, 2)?)
                        .zip(trim_parameter(param(r, 3)?))
                        .map(|(a, b)| (a * scale, b * scale));
                    return Ok((curve, trim));
                }
                "CIRCLE" | "ELLIPSE" => {
                    let (centre, x, y) = self.placement_2d(param(r, 1)?)?;
                    let a = positive(num(param(r, 2)?)?, "pcurve conic semi axis")?;
                    let b = if r.name == "ELLIPSE" {
                        positive(num(param(r, 3)?)?, "pcurve conic semi axis")?
                    } else {
                        a
                    };
                    let curve = Curve2d::Conic {
                        centre,
                        axis1: [x[0] * a, x[1] * a],
                        axis2: [y[0] * b, y[1] * b],
                    };
                    return Ok((curve, None));
                }
                _ => {}
            }
        }
        let curve = match self.curve_scaled(id, 1.0, depth + 1)? {
            Curve::Line {
                origin,
                dir,
                magnitude,
            } => Curve2d::Line {
                origin: [origin[0], origin[1]],
                dir: [dir[0] * magnitude, dir[1] * magnitude],
            },
            Curve::BSpline(b) => Curve2d::BSpline(b),
            Curve::Circle { .. } | Curve::Ellipse { .. } => {
                return Err(MapError::Unsupported("3D conic in a pcurve".into()));
            }
        };
        Ok((curve, None))
    }

    // ---- surfaces ------------------------------------------------------------------

    fn surface(&self, id: u64, c: &Ctx, depth: usize) -> R<Surface> {
        self.check_depth(depth)?;
        let s = c.u.mm;
        let inst = self.inst(id)?;
        match inst {
            Instance::Simple(r) => match r.name.as_str() {
                "PLANE" => Ok(Surface::Plane {
                    frame: self.placement(param(r, 1)?, s)?,
                }),
                "CYLINDRICAL_SURFACE" => Ok(Surface::Cylinder {
                    frame: self.placement(param(r, 1)?, s)?,
                    radius: positive(num(param(r, 2)?)? * s, "cylinder radius")?,
                }),
                "CONICAL_SURFACE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let radius = num(param(r, 2)?)? * s;
                    if !(radius.is_finite() && radius >= 0.0) {
                        return invalid("cone radius");
                    }
                    let a = num(param(r, 3)?)? * c.u.deg;
                    if !(a > 0.0 && a < 90.0) {
                        return invalid(format!("cone semi angle {a} degrees"));
                    }
                    Ok(Surface::Cone {
                        frame,
                        radius,
                        semi_angle_deg: a,
                    })
                }
                "SPHERICAL_SURFACE" => Ok(Surface::Sphere {
                    frame: self.placement(param(r, 1)?, s)?,
                    radius: positive(num(param(r, 2)?)? * s, "sphere radius")?,
                }),
                "TOROIDAL_SURFACE" => Ok(Surface::Torus {
                    frame: self.placement(param(r, 1)?, s)?,
                    major: positive(num(param(r, 2)?)? * s, "torus major radius")?,
                    minor: positive(num(param(r, 3)?)? * s, "torus minor radius")?,
                }),
                "SURFACE_OF_REVOLUTION" => {
                    let curve = self.curve(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let ax = self.simple(ref_of(param(r, 2)?)?)?;
                    if ax.name != "AXIS1_PLACEMENT" {
                        return Err(MapError::Unsupported(ax.name.clone()));
                    }
                    let axis_origin = self.point(param(ax, 1)?, s)?;
                    let axis_dir = match ax.params.get(2) {
                        None | Some(Param::Null) => [0.0, 0.0, 1.0],
                        Some(p) => self.direction(p)?,
                    };
                    let axis_dir = unit(axis_dir).map_or_else(|| invalid("zero axis"), Ok)?;
                    Ok(Surface::Revolution {
                        curve: Box::new(curve),
                        axis_origin,
                        axis_dir,
                    })
                }
                "SURFACE_OF_LINEAR_EXTRUSION" => {
                    let curve = self.curve(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let (dir, magnitude) = self.vector(param(r, 2)?, s)?;
                    if !(magnitude.is_finite() && magnitude > 0.0) {
                        return invalid("extrusion with a zero vector");
                    }
                    Ok(Surface::Extrusion {
                        curve: Box::new(curve),
                        dir,
                        magnitude,
                    })
                }
                "B_SPLINE_SURFACE_WITH_KNOTS" => {
                    let (ud, vd, pts) = self.bspline_surface_base(r.params.as_slice(), 1, s)?;
                    let tail = r.params.get(8..).unwrap_or_default();
                    finish_bspline_surface(ud, vd, pts, tail, None)
                }
                "QUASI_UNIFORM_SURFACE" | "UNIFORM_SURFACE" | "BEZIER_SURFACE" => {
                    let (ud, vd, pts) = self.bspline_surface_base(r.params.as_slice(), 1, s)?;
                    implied_knots_surface(ud, vd, pts, &r.name, None)
                }
                other => Err(MapError::Unsupported(other.to_string())),
            },
            Instance::Complex(rs) => {
                let base = rs
                    .iter()
                    .find(|r| r.name == "B_SPLINE_SURFACE")
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                let (ud, vd, pts) = self.bspline_surface_base(base.params.as_slice(), 0, s)?;
                let weights = match rs.iter().find(|r| r.name == "RATIONAL_B_SPLINE_SURFACE") {
                    Some(w) => {
                        let mut rows = Vec::new();
                        for row in list(param(w, 0)?)? {
                            rows.push(num_list(list(row)?)?);
                        }
                        Some(rows)
                    }
                    None => None,
                };
                if let Some(k) = rs.iter().find(|r| r.name == "B_SPLINE_SURFACE_WITH_KNOTS") {
                    return finish_bspline_surface(ud, vd, pts, k.params.as_slice(), weights);
                }
                let k = rs
                    .iter()
                    .find(|r| {
                        matches!(
                            r.name.as_str(),
                            "QUASI_UNIFORM_SURFACE" | "UNIFORM_SURFACE" | "BEZIER_SURFACE"
                        )
                    })
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                implied_knots_surface(ud, vd, pts, &k.name, weights)
            }
        }
    }

    fn bspline_surface_base(&self, ps: &[Param], at: usize, s: f64) -> R<(u32, u32, Vec<Vec<P3>>)> {
        let ud = degree_of(ps.get(at))?;
        let vd = degree_of(ps.get(at + 1))?;
        let rows = list(
            ps.get(at + 2)
                .map_or_else(|| invalid("missing points"), Ok)?,
        )?;
        let mut pts = Vec::new();
        for row in rows {
            let mut out = Vec::new();
            for p in list(row)? {
                out.push(self.point(p, s)?);
            }
            pts.push(out);
        }
        Ok((ud, vd, pts))
    }

    // ---- topology ------------------------------------------------------------------

    fn vertex(&self, id: u64, c: &mut Ctx) -> R<usize> {
        if let Some(&i) = c.vmap.get(&id) {
            return Ok(i);
        }
        let r = self.simple(id)?;
        if r.name != "VERTEX_POINT" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let point = self.point(param(r, 1)?, c.u.mm)?;
        let i = c.vertices.len();
        c.vertices.push(Vertex { step_id: id, point });
        c.vmap.insert(id, i);
        Ok(i)
    }

    fn edge(&self, id: u64, c: &mut Ctx) -> R<usize> {
        if let Some(&i) = c.emap.get(&id) {
            return Ok(i);
        }
        let r = self.simple(id)?;
        if r.name != "EDGE_CURVE" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let start = self.vertex(ref_of(param(r, 1)?)?, c)?;
        let end = self.vertex(ref_of(param(r, 2)?)?, c)?;
        let (curve, reversed) = self.edge_curve_geom(ref_of(param(r, 3)?)?, c, 0)?;
        let same_sense = boolean(param(r, 4)?)? ^ reversed;
        let ps = c.vertices.get(start).map(|v| v.point);
        let pe = c.vertices.get(end).map(|v| v.point);
        let (Some(ps), Some(pe)) = (ps, pe) else {
            return invalid("edge vertex");
        };
        let (t_start, t_end) = edge_params(&curve, ps, pe, start == end, same_sense)?;
        let i = c.edges.len();
        c.edges.push(Edge {
            step_id: id,
            start,
            end,
            curve,
            same_sense,
            t_start,
            t_end,
        });
        c.emap.insert(id, i);
        Ok(i)
    }

    /// `ORIENTED_EDGE` (possibly of another oriented edge) -> (edge, forward).
    fn oriented_edge(&self, id: u64, c: &mut Ctx, depth: usize) -> R<OrientedEdge> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "ORIENTED_EDGE" => {
                let inner = ref_of(param(r, 3)?)?;
                let fwd = boolean(param(r, 4)?)?;
                let e = self.oriented_edge(inner, c, depth + 1)?;
                Ok(OrientedEdge {
                    edge: e.edge,
                    forward: e.forward == fwd,
                })
            }
            "EDGE_CURVE" => Ok(OrientedEdge {
                edge: self.edge(id, c)?,
                forward: true,
            }),
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    /// A poly-loop point as a vertex (keyed by the point's id).
    fn poly_vertex(&self, p: &Param, c: &mut Ctx) -> R<(u64, usize)> {
        let id = ref_of(p)?;
        if let Some(&i) = c.vmap.get(&id) {
            return Ok((id, i));
        }
        let point = self.point(p, c.u.mm)?;
        let i = c.vertices.len();
        c.vertices.push(Vertex { step_id: id, point });
        c.vmap.insert(id, i);
        Ok((id, i))
    }

    fn poly_loop(&self, r: &Record, c: &mut Ctx) -> R<Vec<OrientedEdge>> {
        let pts = list(param(r, 1)?)?;
        if pts.len() < 3 {
            return invalid("POLY_LOOP with fewer than 3 points");
        }
        let mut vs = Vec::new();
        for p in pts {
            vs.push(self.poly_vertex(p, c)?);
        }
        let mut out = Vec::new();
        for (k, &(a_id, a)) in vs.iter().enumerate() {
            let &(b_id, b) = vs
                .get((k + 1) % vs.len())
                .map_or_else(|| invalid("poly"), Ok)?;
            if a_id == b_id {
                continue;
            }
            let key = (a_id.min(b_id), a_id.max(b_id));
            let edge = if let Some(&e) = c.pmap.get(&key) {
                e
            } else {
                let (s, e) = if a_id < b_id { (a, b) } else { (b, a) };
                let ps = c.vertices.get(s).map(|v| v.point);
                let pe = c.vertices.get(e).map(|v| v.point);
                let (Some(ps), Some(pe)) = (ps, pe) else {
                    return invalid("poly vertex");
                };
                let len = dist(ps, pe);
                let dir =
                    unit(geom::sub(pe, ps)).map_or_else(|| invalid("zero-length poly edge"), Ok)?;
                let i = c.edges.len();
                c.edges.push(Edge {
                    step_id: 0,
                    start: s,
                    end: e,
                    curve: Curve::Line {
                        origin: ps,
                        dir,
                        magnitude: len,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 1.0,
                });
                c.pmap.insert(key, i);
                i
            };
            out.push(OrientedEdge {
                edge,
                forward: a_id < b_id,
            });
        }
        Ok(out)
    }

    fn face_bound(&self, id: u64, c: &mut Ctx) -> R<(Loop, Option<Vec<P3>>)> {
        let r = self.simple(id)?;
        let outer = match r.name.as_str() {
            "FACE_OUTER_BOUND" => true,
            "FACE_BOUND" => false,
            other => return Err(MapError::Unsupported(other.to_string())),
        };
        let orientation = boolean(param(r, 2)?)?;
        let lid = ref_of(param(r, 1)?)?;
        let lr = self.simple(lid)?;
        let mut poly = None;
        let bound = match lr.name.as_str() {
            "EDGE_LOOP" => {
                let mut es = Vec::new();
                for p in list(param(lr, 1)?)? {
                    es.push(self.oriented_edge(ref_of(p)?, c, 0)?);
                }
                if es.is_empty() {
                    return invalid("empty EDGE_LOOP");
                }
                Bound::Edges(es)
            }
            "VERTEX_LOOP" => Bound::Vertex(self.vertex(ref_of(param(lr, 1)?)?, c)?),
            "POLY_LOOP" => {
                let mut pts = Vec::new();
                for p in list(param(lr, 1)?)? {
                    pts.push(self.point(p, c.u.mm)?);
                }
                poly = Some(pts);
                Bound::Edges(self.poly_loop(lr, c)?)
            }
            other => return Err(MapError::Unsupported(other.to_string())),
        };
        Ok((
            Loop {
                outer,
                orientation,
                bound,
            },
            poly,
        ))
    }

    fn face(&self, id: u64, c: &mut Ctx, depth: usize) -> R<usize> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "ORIENTED_FACE" => {
                let inner = self.face(ref_of(param(r, 2)?)?, c, depth + 1)?;
                if !boolean(param(r, 3)?)? {
                    // Face rows are not memoized, so this freshly mapped inner row
                    // is not yet referenced by a shell. Reverse it in place rather
                    // than appending a clone and leaving an orphaned inner face.
                    let f = c.faces.get_mut(inner).map_or_else(|| invalid("face"), Ok)?;
                    f.step_id = id;
                    f.same_sense = !f.same_sense;
                    for l in &mut f.loops {
                        l.orientation = !l.orientation;
                    }
                    return Ok(inner);
                }
                Ok(inner)
            }
            "ADVANCED_FACE" | "FACE_SURFACE" | "FACE" => {
                let mut loops = Vec::new();
                let mut first_poly = None;
                for p in list(param(r, 1)?)? {
                    let (l, poly) = self.face_bound(ref_of(p)?, c)?;
                    if first_poly.is_none() {
                        first_poly = poly;
                    }
                    loops.push(l);
                }
                let (surface, same_sense) = if r.name == "FACE" {
                    let pts = first_poly.map_or_else(
                        || Err(MapError::Unsupported("FACE without geometry".into())),
                        Ok,
                    )?;
                    (polygon_plane(&pts)?, true)
                } else {
                    (
                        self.surface(ref_of(param(r, 2)?)?, c, 0)?,
                        boolean(param(r, 3)?)?,
                    )
                };
                c.faces.push(Face {
                    step_id: id,
                    surface,
                    same_sense,
                    loops,
                });
                Ok(c.faces.len() - 1)
            }
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    fn shell(&self, id: u64, c: &mut Ctx, depth: usize) -> R<Shell> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "CLOSED_SHELL" | "OPEN_SHELL" => {
                let mut faces = Vec::new();
                for p in list(param(r, 1)?)? {
                    faces.push(self.face(ref_of(p)?, c, 0)?);
                }
                Ok(Shell {
                    step_id: id,
                    closed: r.name == "CLOSED_SHELL",
                    orientation: true,
                    faces,
                })
            }
            "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
                let mut sh = self.shell(ref_of(param(r, 2)?)?, c, depth + 1)?;
                if !boolean(param(r, 3)?)? {
                    sh.orientation = !sh.orientation;
                }
                sh.step_id = id;
                Ok(sh)
            }
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    fn body(&self, id: u64, kind: BodyKind, r: &Record, u: Units) -> R<Body> {
        let mut c = Ctx {
            u,
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
            vmap: BTreeMap::new(),
            emap: BTreeMap::new(),
            pmap: BTreeMap::new(),
        };
        let mut shells = Vec::new();
        match kind {
            BodyKind::ManifoldSolid | BodyKind::Faceted => {
                shells.push(self.shell(ref_of(param(r, 1)?)?, &mut c, 0)?);
            }
            BodyKind::SolidWithVoids => {
                shells.push(self.shell(ref_of(param(r, 1)?)?, &mut c, 0)?);
                for p in list(param(r, 2)?)? {
                    shells.push(self.shell(ref_of(p)?, &mut c, 0)?);
                }
            }
            BodyKind::ShellModel => {
                for p in list(param(r, 1)?)? {
                    shells.push(self.shell(ref_of(p)?, &mut c, 0)?);
                }
            }
        }
        Ok(Body {
            step_id: id,
            name: match r.params.first() {
                Some(Param::Str(name)) if !name.is_empty() => Some(name.clone()),
                _ => None,
            },
            kind,
            unit_mm: u.mm,
            uncertainty_mm: u.uncertainty,
            shells,
            faces: c.faces,
            edges: c.edges,
            vertices: c.vertices,
        })
    }

    /// A STEP face that no shell references (a "naked" face, e.g. a hand-written or
    /// `GEOMETRICALLY_BOUNDED` sheet) as a one-face open-shell body.
    ///
    /// AP203 sheet models normally wrap their faces in an `OPEN_SHELL` inside a
    /// `SHELL_BASED_SURFACE_MODEL`; some writers and hand-written files omit that
    /// wrapper. The face still carries complete geometry and topology, so importing it
    /// is the same exercise as an open shell of one face. The synthetic shell has
    /// `step_id` 0 because no shell entity exists.
    fn sheet_body(&self, id: u64, u: Units) -> R<Body> {
        let r = self.simple(id)?;
        if !matches!(
            r.name.as_str(),
            "ADVANCED_FACE" | "FACE_SURFACE" | "FACE" | "ORIENTED_FACE"
        ) {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let mut c = Ctx {
            u,
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
            vmap: BTreeMap::new(),
            emap: BTreeMap::new(),
            pmap: BTreeMap::new(),
        };
        let face = self.face(id, &mut c, 0)?;
        Ok(Body {
            step_id: id,
            name: match r.params.first() {
                Some(Param::Str(name)) if !name.is_empty() => Some(name.clone()),
                _ => None,
            },
            kind: BodyKind::ShellModel,
            unit_mm: u.mm,
            uncertainty_mm: u.uncertainty,
            shells: vec![Shell {
                step_id: 0,
                closed: false,
                orientation: true,
                faces: vec![face],
            }],
            faces: c.faces,
            edges: c.edges,
            vertices: c.vertices,
        })
    }
}

/// Parameters of an edge's start and end vertices on its curve (see [`Edge`]).
fn edge_params(curve: &Curve, ps: P3, pe: P3, closed: bool, same_sense: bool) -> R<(f64, f64)> {
    let periodic = |ts: f64, te: f64| -> (f64, f64) {
        let mut te = te;
        if same_sense {
            if closed {
                te = ts + 360.0;
            }
            while te <= ts {
                te += 360.0;
            }
            while te > ts + 360.0 {
                te -= 360.0;
            }
        } else {
            if closed {
                te = ts - 360.0;
            }
            while te >= ts {
                te -= 360.0;
            }
            while te < ts - 360.0 {
                te += 360.0;
            }
        }
        // Keep both ends in [-360, 360] (SMLib's angular domain limits).
        let (lo, hi) = (ts.min(te), ts.max(te));
        if hi > 360.0 {
            (ts - 360.0, te - 360.0)
        } else if lo < -360.0 {
            (ts + 360.0, te + 360.0)
        } else {
            (ts, te)
        }
    };
    Ok(match curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            if closed {
                return invalid("closed edge on a LINE");
            }
            let t = |p: P3| geom::dot(geom::sub(p, *origin), *dir) / magnitude;
            (t(ps), t(pe))
        }
        Curve::Circle { frame, radius } => periodic(
            conic_angle(frame, *radius, *radius, ps),
            conic_angle(frame, *radius, *radius, pe),
        ),
        Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => periodic(
            conic_angle(frame, *semi1, *semi2, ps),
            conic_angle(frame, *semi1, *semi2, pe),
        ),
        Curve::BSpline(b) => {
            let (lo, hi) = geom::bspline_range(b).map_or_else(|| invalid("B-spline range"), Ok)?;
            if closed {
                if same_sense {
                    (lo, hi)
                } else {
                    (hi, lo)
                }
            } else {
                let ts = geom::bspline_param(b, ps).map_or_else(|| invalid("B-spline eval"), Ok)?;
                let te = geom::bspline_param(b, pe).map_or_else(|| invalid("B-spline eval"), Ok)?;
                (ts, te)
            }
        }
    })
}

/// The plane through a polygon (Newell's normal), X along its first side.
fn polygon_plane(pts: &[P3]) -> R<Surface> {
    let mut n = [0.0; 3];
    for (k, a) in pts.iter().enumerate() {
        let b = pts.get((k + 1) % pts.len()).copied().unwrap_or(*a);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let o = *pts.first().map_or_else(|| invalid("empty polygon"), Ok)?;
    let x = pts.get(1).map(|p| geom::sub(*p, o));
    let frame = build_frame(o, Some(n), x).map_or_else(|| invalid("degenerate polygon"), Ok)?;
    Ok(Surface::Plane { frame })
}

/// Checks a B-spline curve is well formed.
///
/// # Errors
/// [`MapError::Invalid`] naming the fault.
pub fn check_bspline_curve(b: &BSplineCurve) -> R<()> {
    let n = b.points.len();
    let p = b.degree as usize;
    if n < p + 1 {
        return invalid("B-spline with too few control points");
    }
    if geom::expand_knots(&b.knots, &b.mults, n + p + 1).is_none() {
        return invalid("B-spline knot multiplicities do not match the control points");
    }
    check_knots(&b.knots, &b.mults, p)?;
    if let Some(w) = &b.weights {
        if w.len() != n || w.iter().any(|x| !(x.is_finite() && *x > 0.0)) {
            return invalid("B-spline weights");
        }
    }
    Ok(())
}

/// Checks a knot vector increases and its multiplicities fit `degree`.
///
/// # Errors
/// [`MapError::Invalid`] naming the fault.
pub fn check_knots(knots: &[f64], mults: &[u32], degree: usize) -> R<()> {
    if knots
        .windows(2)
        .any(|w| matches!(w, [a, b] if a.partial_cmp(b) != Some(std::cmp::Ordering::Less)))
    {
        return invalid("knots not increasing");
    }
    if mults.iter().any(|&m| m == 0 || m as usize > degree + 1) {
        return invalid("knot multiplicity out of range");
    }
    Ok(())
}

fn implied_knots_curve(
    (degree, points): (u32, Vec<P3>),
    form: &str,
    weights: Option<Vec<f64>>,
) -> R<Curve> {
    let (knots, mults) = implied_knots(form, degree, points.len())?;
    let b = BSplineCurve {
        degree,
        points,
        weights,
        knots,
        mults,
    };
    check_bspline_curve(&b)?;
    Ok(Curve::BSpline(b))
}

/// ISO 10303-42 knots implied by `UNIFORM_*`, `QUASI_UNIFORM_*` and `BEZIER_*` forms
/// for `n` control points of degree `d`.
fn implied_knots(form: &str, d: u32, n: usize) -> R<(Vec<f64>, Vec<u32>)> {
    let dd = d as usize;
    if n < dd + 1 {
        return invalid("too few control points");
    }
    Ok(if form.starts_with("UNIFORM") {
        // knots -d .. n, all simple: n + d + 1 of them
        let k: Vec<f64> = (0..=(n + dd)).map(|i| i as f64 - f64::from(d)).collect();
        let m = vec![1; k.len()];
        (k, m)
    } else if form.starts_with("QUASI_UNIFORM") {
        let count = n - dd + 1;
        let k: Vec<f64> = (0..count).map(|i| i as f64).collect();
        let mut m = vec![1; count];
        if let Some(x) = m.first_mut() {
            *x = d + 1;
        }
        if let Some(x) = m.last_mut() {
            *x = d + 1;
        }
        (k, m)
    } else {
        // BEZIER: segments of d points sharing ends
        if dd == 0 || !(n - 1).is_multiple_of(dd) {
            return invalid("Bezier control point count");
        }
        let segs = (n - 1) / dd;
        let k: Vec<f64> = (0..=segs).map(|i| i as f64).collect();
        let mut m = vec![d; segs + 1];
        if let Some(x) = m.first_mut() {
            *x = d + 1;
        }
        if let Some(x) = m.last_mut() {
            *x = d + 1;
        }
        (k, m)
    })
}

fn implied_knots_surface(
    ud: u32,
    vd: u32,
    points: Vec<Vec<P3>>,
    form: &str,
    weights: Option<Vec<Vec<f64>>>,
) -> R<Surface> {
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    let (uk, um) = implied_knots(form, ud, nu)?;
    let (vk, vm) = implied_knots(form, vd, nv)?;
    let tail = [
        knot_params(&um),
        knot_params(&vm),
        real_params(&uk),
        real_params(&vk),
    ];
    finish_bspline_surface(ud, vd, points, &tail, weights)
}

fn knot_params(m: &[u32]) -> Param {
    Param::List(m.iter().map(|&x| Param::Int(i64::from(x))).collect())
}

fn real_params(k: &[f64]) -> Param {
    Param::List(k.iter().map(|&x| Param::Real(x)).collect())
}

fn finish_bspline_surface(
    ud: u32,
    vd: u32,
    points: Vec<Vec<P3>>,
    ps: &[Param],
    weights: Option<Vec<Vec<f64>>>,
) -> R<Surface> {
    let u_mults = mult_list(list(
        ps.first().map_or_else(|| invalid("missing mults"), Ok)?,
    )?)?;
    let v_mults = mult_list(list(
        ps.get(1).map_or_else(|| invalid("missing mults"), Ok)?,
    )?)?;
    let u_knots = num_list(list(
        ps.get(2).map_or_else(|| invalid("missing knots"), Ok)?,
    )?)?;
    let v_knots = num_list(list(
        ps.get(3).map_or_else(|| invalid("missing knots"), Ok)?,
    )?)?;
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    if nu < ud as usize + 1 || nv < vd as usize + 1 || points.iter().any(|r| r.len() != nv) {
        return invalid("B-spline surface control net");
    }
    if geom::expand_knots(&u_knots, &u_mults, nu + ud as usize + 1).is_none()
        || geom::expand_knots(&v_knots, &v_mults, nv + vd as usize + 1).is_none()
    {
        return invalid("B-spline surface knot multiplicities do not match the net");
    }
    check_knots(&u_knots, &u_mults, ud as usize)?;
    check_knots(&v_knots, &v_mults, vd as usize)?;
    if let Some(w) = &weights {
        if w.len() != nu
            || w.iter()
                .any(|r| r.len() != nv || r.iter().any(|x| !(x.is_finite() && *x > 0.0)))
        {
            return invalid("B-spline surface weights");
        }
    }
    Ok(Surface::BSpline(BSplineSurface {
        u_degree: ud,
        v_degree: vd,
        points,
        weights,
        u_knots,
        u_mults,
        v_knots,
        v_mults,
    }))
}

fn param(r: &Record, i: usize) -> R<&Param> {
    r.params
        .get(i)
        .map_or_else(|| invalid(format!("{} has no parameter {i}", r.name)), Ok)
}

fn ref_of(p: &Param) -> R<u64> {
    p.as_ref_id()
        .map_or_else(|| invalid(format!("expected a reference, found {p:?}")), Ok)
}

fn list(p: &Param) -> R<&[Param]> {
    p.as_list()
        .map_or_else(|| invalid(format!("expected a list, found {p:?}")), Ok)
}

fn num(p: &Param) -> R<f64> {
    match p.as_f64() {
        Some(x) if x.is_finite() => Ok(x),
        _ => invalid(format!("expected a number, found {p:?}")),
    }
}

fn num_at(l: &[Param], i: usize) -> R<f64> {
    num(l
        .get(i)
        .map_or_else(|| invalid("short coordinate list"), Ok)?)
}

fn num_list(l: &[Param]) -> R<Vec<f64>> {
    l.iter().map(num).collect()
}

fn mult_list(l: &[Param]) -> R<Vec<u32>> {
    l.iter()
        .map(|p| match p {
            Param::Int(i) if (1..=33).contains(i) => Ok(*i as u32),
            _ => invalid(format!("bad knot multiplicity {p:?}")),
        })
        .collect()
}

fn degree_of(p: Option<&Param>) -> R<u32> {
    match p {
        Some(Param::Int(d)) if (1..=32).contains(d) => Ok(*d as u32),
        _ => Err(MapError::Unsupported(format!("B-spline degree {p:?}"))),
    }
}

fn boolean(p: &Param) -> R<bool> {
    match p.as_enum() {
        Some("T") => Ok(true),
        Some("F") => Ok(false),
        _ => invalid(format!("expected .T. or .F., found {p:?}")),
    }
}

fn positive(x: f64, what: &str) -> R<f64> {
    if x.is_finite() && x > 0.0 {
        Ok(x)
    } else {
        invalid(format!("{what} {x}"))
    }
}

/// The parameter value of a `TRIMMED_CURVE` trim select, when it gives one.
fn trim_parameter(p: &Param) -> Option<f64> {
    p.as_list()?
        .iter()
        .find_map(|select| match select {
            Param::Typed(name, _) if name == "PARAMETER_VALUE" => select.as_f64(),
            Param::Real(_) | Param::Int(_) => select.as_f64(),
            _ => None,
        })
        .filter(|x| x.is_finite())
}

/// `curve` with its u coordinates times `su` and its v coordinates times `sv`.
fn scale_curve_2d(curve: Curve2d, su: f64, sv: f64) -> Curve2d {
    let s = |p: [f64; 2]| [p[0] * su, p[1] * sv];
    match curve {
        Curve2d::Line { origin, dir } => Curve2d::Line {
            origin: s(origin),
            dir: s(dir),
        },
        Curve2d::Conic {
            centre,
            axis1,
            axis2,
        } => Curve2d::Conic {
            centre: s(centre),
            axis1: s(axis1),
            axis2: s(axis2),
        },
        Curve2d::BSpline(mut b) => {
            for point in &mut b.points {
                *point = [point[0] * su, point[1] * sv, 0.0];
            }
            Curve2d::BSpline(b)
        }
    }
}

/// The `#id` of face `id`'s surface (through `ORIENTED_FACE`s), when it has one.
fn face_surface_id(file: &StepFile, id: u64, max_depth: usize) -> Option<u64> {
    let base = resolve_face(file, id, max_depth)?;
    let Instance::Simple(record) = file.entities.get(&base).map(Instance::of)? else {
        return None;
    };
    if !matches!(record.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE") {
        return None;
    }
    record.params.get(2)?.as_ref_id()
}

/// Fills [`Model::file_pcurves`] for every mapped body, in the units of the body's
/// representation context (chosen as when the body was mapped).
fn map_pcurves(m: &Mapper<'_>, contexts: &BTreeMap<u64, u64>, fallback: Units, model: &mut Model) {
    let mut out = FilePcurves::default();
    for body in &model.bodies {
        let units = match contexts.get(&body.step_id) {
            Some(ctx) => m.context_units(*ctx),
            None => Ok(fallback),
        };
        let Ok(units) = units else {
            continue;
        };
        for edge in &body.edges {
            if edge.step_id == 0 || out.edges.contains_key(&edge.step_id) {
                continue;
            }
            match m.edge_pcurves(edge.step_id, units, &mut out.diagnostics) {
                Ok(pcurves) if pcurves.is_empty() => {}
                Ok(pcurves) => {
                    out.edges.insert(edge.step_id, pcurves);
                }
                Err(error) => out.diagnostics.push(Diagnostic {
                    step_id: Some(edge.step_id),
                    message: format!("pcurves not used: {error}"),
                }),
            }
        }
        for face in &body.faces {
            if let Some(surface) = face_surface_id(m.file, face.step_id, m.limits.max_ref_depth) {
                out.face_surfaces.insert(face.step_id, surface);
            }
        }
    }
    model.file_pcurves = out;
}

/// Report references that point at no entity in the data section.
fn collect_dangling_references(file: &StepFile, diagnostics: &mut Vec<Diagnostic>) {
    fn visit(param: &Param, missing: &mut Vec<u64>) {
        match param {
            Param::Ref(id) => missing.push(*id),
            Param::List(items) => {
                for item in items {
                    visit(item, missing);
                }
            }
            Param::Typed(_, values) => {
                for value in values {
                    visit(value, missing);
                }
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    for (id, inst) in file.entities.iter().map(|(id, e)| (id, Instance::of(e))) {
        let records: &[Record] = match inst {
            Instance::Simple(record) => std::slice::from_ref(record),
            Instance::Complex(records) => records,
        };
        for record in records {
            for param in &record.params {
                let mut missing = Vec::new();
                visit(param, &mut missing);
                for target in missing {
                    if !file.entities.contains_key(&target) && seen.insert((*id, target)) {
                        diagnostics.push(Diagnostic {
                            step_id: Some(*id),
                            message: format!("dangling reference #{target}"),
                        });
                    }
                }
            }
        }
    }
}

/// Resolves an `ORIENTED_FACE` chain (at most `max_depth` links) to the face it wraps.
fn resolve_face(file: &StepFile, id: u64, max_depth: usize) -> Option<u64> {
    let mut current = id;
    for _ in 0..=max_depth {
        let Instance::Simple(record) = file.entities.get(&current).map(Instance::of)? else {
            return Some(current);
        };
        if record.name != "ORIENTED_FACE" {
            return Some(current);
        }
        let Param::Ref(inner) = record.params.get(2)? else {
            return Some(current);
        };
        current = *inner;
    }
    Some(current)
}

/// Every face id a shell references, plus the `ORIENTED_FACE` wrapper ids themselves.
fn shell_referenced_faces(file: &StepFile, max_depth: usize) -> BTreeSet<u64> {
    let mut referenced = BTreeSet::new();
    for instance in file.entities.values().map(Instance::of) {
        let Instance::Simple(record) = instance else {
            continue;
        };
        if !matches!(record.name.as_str(), "CLOSED_SHELL" | "OPEN_SHELL") {
            continue;
        }
        let Some(Param::List(faces)) = record.params.get(1) else {
            continue;
        };
        for face in faces {
            let Param::Ref(id) = face else {
                continue;
            };
            referenced.insert(*id);
            if let Some(base) = resolve_face(file, *id, max_depth) {
                referenced.insert(base);
            }
        }
    }
    referenced
}

/// The inner face of every `ORIENTED_FACE` (mapped through the wrapper instead).
fn oriented_face_targets(file: &StepFile) -> BTreeSet<u64> {
    let mut targets = BTreeSet::new();
    for instance in file.entities.values().map(Instance::of) {
        let Instance::Simple(record) = instance else {
            continue;
        };
        if record.name != "ORIENTED_FACE" {
            continue;
        }
        if let Some(Param::Ref(inner)) = record.params.get(2) {
            targets.insert(*inner);
        }
    }
    targets
}

/// A "naked" face: no shell references it, so AP203 gives it no body. Map it as a
/// one-face sheet so sheet-only files (and loose faces beside solids) still import.
fn map_loose_faces(
    m: &Mapper<'_>,
    contexts: &BTreeMap<u64, u64>,
    fallback: Units,
    model: &mut Model,
) {
    let referenced = shell_referenced_faces(m.file, m.limits.max_ref_depth);
    let inner = oriented_face_targets(m.file);
    let mut candidates = Vec::new();
    for (id, instance) in m.file.entities.iter().map(|(id, e)| (id, Instance::of(e))) {
        let Instance::Simple(record) = instance else {
            continue;
        };
        if !matches!(
            record.name.as_str(),
            "ADVANCED_FACE" | "FACE_SURFACE" | "FACE" | "ORIENTED_FACE"
        ) {
            continue;
        }
        if referenced.contains(id) || inner.contains(id) {
            continue;
        }
        // A loose wrapper over a face that a shell already maps would duplicate it.
        if record.name == "ORIENTED_FACE"
            && resolve_face(m.file, *id, m.limits.max_ref_depth)
                .is_some_and(|base| referenced.contains(&base))
        {
            continue;
        }
        candidates.push(*id);
    }
    let mut sheets = 0usize;
    for id in candidates {
        let units = if let Some(ctx) = contexts.get(&id) {
            m.context_units(*ctx)
        } else {
            model.diagnostics.push(Diagnostic {
                step_id: Some(id),
                message: "sheet face has no representation context; using the file unit fallback"
                    .into(),
            });
            Ok(fallback)
        };
        match units.and_then(|u| m.sheet_body(id, u)) {
            Ok(body) => {
                model.bodies.push(body);
                sheets += 1;
            }
            Err(error) => model.skipped.push(Skipped {
                step_id: id,
                kind: m
                    .file
                    .entities
                    .get(&id)
                    .map_or_else(String::new, |e| Instance::of(e).type_name()),
                reason: error.to_string(),
            }),
        }
    }
    // A surface model can wrap its faces in a non-shell representation, so the
    // loose-face heuristic above sees no candidate. If the file has no solid bodies at
    // all, treat every face as a sheet rather than returning an empty model.
    if model.bodies.is_empty() {
        for (id, instance) in m.file.entities.iter().map(|(id, e)| (id, Instance::of(e))) {
            let Instance::Simple(record) = instance else {
                continue;
            };
            if !matches!(
                record.name.as_str(),
                "ADVANCED_FACE" | "FACE_SURFACE" | "FACE"
            ) {
                continue;
            }
            let units = if let Some(ctx) = contexts.get(id) {
                m.context_units(*ctx)
            } else {
                Ok(fallback)
            };
            if let Ok(body) = units.and_then(|u| m.sheet_body(*id, u)) {
                model.bodies.push(body);
                sheets += 1;
            }
        }
    }
    if sheets > 0 {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: format!(
                "{sheets} face(s) are not referenced by any shell; imported as single-face sheet bodies"
            ),
        });
    }
}

/// Maps every B-rep body of `file`.
pub fn map_file(file: &StepFile, limits: &Limits) -> Model {
    let m = Mapper {
        file,
        limits: *limits,
    };
    let contexts = m.item_contexts();
    // Fallback units when an item is in no representation: those of the file's only
    // context with a unit assignment, if there is exactly one kind.
    let mut all_units: Vec<Units> = Vec::new();
    for (id, inst) in file.entities.iter().map(|(id, e)| (id, Instance::of(e))) {
        if inst.record("GLOBAL_UNIT_ASSIGNED_CONTEXT").is_some() {
            if let Ok(u) = m.context_units(*id) {
                if !all_units.contains(&u) {
                    all_units.push(u);
                }
            }
        }
    }
    let fallback = match all_units.as_slice() {
        [u] => *u,
        _ => Units::default(),
    };
    let mut model = Model {
        header: file.header.clone(),
        schemas: file.schemas.clone(),
        ..Model::default()
    };
    if all_units.is_empty() {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: "no GLOBAL_UNIT_ASSIGNED_CONTEXT; assuming millimetres/degrees".into(),
        });
    } else if all_units.len() > 1 {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: format!(
                "{} distinct unit contexts; using the default fallback for bodies without an explicit context",
                all_units.len()
            ),
        });
    }
    for (id, inst) in file.entities.iter().map(|(id, e)| (id, Instance::of(e))) {
        let rec = match inst {
            Instance::Simple(r) => r,
            Instance::Complex(_) => {
                if inst.record("MAPPED_ITEM").is_some() {
                    model.assembly_instances += 1;
                }
                continue;
            }
        };
        if rec.name == "MAPPED_ITEM" || rec.name == "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" {
            model.assembly_instances += 1;
            continue;
        }
        let Some(&(name, kind)) = BODY_NAMES.iter().find(|(n, _)| *n == rec.name) else {
            continue;
        };
        let units = if let Some(ctx) = contexts.get(id) {
            m.context_units(*ctx)
        } else {
            model.diagnostics.push(Diagnostic {
                step_id: Some(*id),
                message: "body has no representation context; using the file unit fallback".into(),
            });
            Ok(fallback)
        };
        match units.and_then(|u| m.body(*id, kind, rec, u)) {
            Ok(b) => model.bodies.push(b),
            Err(e) => model.skipped.push(Skipped {
                step_id: *id,
                kind: name.to_string(),
                reason: e.to_string(),
            }),
        }
    }
    map_loose_faces(&m, &contexts, fallback, &mut model);
    model.bodies.sort_by_key(|body| body.step_id);
    map_pcurves(&m, &contexts, fallback, &mut model);
    if model.assembly_instances > 0 {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: format!(
                "{} assembly instance(s) are counted but not applied; bodies are in their defining representation coordinates",
                model.assembly_instances
            ),
        });
    }
    collect_dangling_references(file, &mut model.diagnostics);
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::step::p21::parse;

    fn model(data: &str) -> Model {
        let text = format!(
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
        );
        let f = parse(&text).unwrap();
        map_file(&f, &Limits::default())
    }

    /// Millimetre units and the representation context that assigns them to the
    /// `SHELL_BASED_SURFACE_MODEL` #5 of the oriented-face fixtures.
    const ORIENTED_FACE_CONTEXT: &str = "#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
         #21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('',''));\
         #22=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#5),#21);";

    /// An ordinary planar `FACE` #6 on the non-collinear triangle #9/#10/#11,
    /// with its only bound `#7` a `.T.` `FACE_OUTER_BOUND`.
    const ORIENTED_FACE_TRIANGLE: &str = "#6=FACE('',(#7));\
         #7=FACE_OUTER_BOUND('',#8,.T.);\
         #8=POLY_LOOP('',(#9,#10,#11));\
         #9=CARTESIAN_POINT('',(0.,0.,0.));\
         #10=CARTESIAN_POINT('',(3.,0.,0.));\
         #11=CARTESIAN_POINT('',(0.,4.,0.));";

    fn oriented_face_model(body: &str) -> Model {
        model(&format!(
            "{ORIENTED_FACE_TRIANGLE}{body}{ORIENTED_FACE_CONTEXT}"
        ))
    }

    /// Two `ORIENTED_FACE`s over the same base `FACE` #6, each in its own open
    /// shell; `shells` is the item list of `SHELL_BASED_SURFACE_MODEL` #5.
    fn oriented_face_mixed_model(shells: &str) -> Model {
        oriented_face_model(&format!(
            "#3=ORIENTED_FACE('',*,#6,.T.);\
             #4=ORIENTED_FACE('',*,#6,.F.);\
             #5=SHELL_BASED_SURFACE_MODEL('',({shells}));\
             #13=OPEN_SHELL('',(#3));\
             #14=OPEN_SHELL('',(#4));"
        ))
    }

    /// A fixture is complete only if it maps to exactly one millimetre body
    /// without skips or diagnostics.
    fn assert_single_oriented_face_body(m: &Model) -> &Body {
        assert_eq!(m.bodies.len(), 1);
        assert!(m.skipped.is_empty(), "skipped: {:?}", m.skipped);
        assert!(m.diagnostics.is_empty(), "diagnostics: {:?}", m.diagnostics);
        let b = &m.bodies[0];
        assert_eq!(b.unit_mm, 1.0);
        b
    }

    #[test]
    fn implied_knot_forms() {
        let (k, m) = implied_knots("QUASI_UNIFORM_CURVE", 3, 6).unwrap();
        assert_eq!(k, vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(m, vec![4, 1, 1, 4]);
        let (k, m) = implied_knots("BEZIER_CURVE", 3, 7).unwrap();
        assert_eq!(k, vec![0.0, 1.0, 2.0]);
        assert_eq!(m, vec![4, 3, 4]);
        let (k, _) = implied_knots("UNIFORM_CURVE", 2, 4).unwrap();
        assert_eq!(k.len(), 7);
        assert!(implied_knots("BEZIER_CURVE", 3, 6).is_err());
    }

    #[test]
    fn diagnostics_report_missing_and_multiple_unit_contexts() {
        let m = model("#1=CARTESIAN_POINT('',(0.,0.,0.));");
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no GLOBAL_UNIT")));

        let m = model(
            "#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#10)) REPRESENTATION_CONTEXT('',''));             #2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#11)) REPRESENTATION_CONTEXT('',''));             #10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));             #11=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));",
        );
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("distinct unit contexts")));
    }

    #[test]
    fn diagnostics_report_unapplied_assembly_instances() {
        let m = model("#1=MAPPED_ITEM('',#2,#3);");
        assert_eq!(m.assembly_instances, 1);
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("assembly instance")));
    }

    #[test]
    fn body_name_and_missing_context_are_reported() {
        let m = model(
            "#1=MANIFOLD_SOLID_BREP('named',#2);\
             #2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);\
             #4=PLANE('',#5);\
             #5=AXIS2_PLACEMENT_3D('',#6,$,$);\
             #6=CARTESIAN_POINT('',(0.,0.,0.));",
        );
        assert_eq!(m.bodies.len(), 1);
        assert_eq!(m.bodies[0].name.as_deref(), Some("named"));
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no representation context")));
    }

    #[test]
    fn dangling_references_outside_bodies_are_reported() {
        let m = model("#1=PRODUCT('p','p','',(#9));");
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.step_id == Some(1) && d.message.contains("dangling reference #9")));
    }

    #[test]
    fn dangling_and_unsupported() {
        let m = model("#1=MANIFOLD_SOLID_BREP('',#2);");
        assert_eq!(m.bodies.len(), 0);
        assert_eq!(m.skipped.len(), 1);
        assert!(m.skipped[0].reason.contains("dangling"));
        let m = model(
            "#1=MANIFOLD_SOLID_BREP('',#2);#2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);#4=OFFSET_SURFACE('',#5,1.,.F.);",
        );
        assert_eq!(m.skipped[0].reason, "unsupported: OFFSET_SURFACE");
        // a reference cycle stops at the depth bound
        let m = model("#1=MANIFOLD_SOLID_BREP('',#2);#2=ORIENTED_CLOSED_SHELL('',*,#2,.F.);");
        assert!(m.skipped[0].reason.contains("too deep"));
    }

    #[test]
    fn oriented_face_reverse_maps_wrapper_in_place() {
        let m = oriented_face_model(
            "#3=ORIENTED_FACE('',*,#6,.F.);\
             #5=SHELL_BASED_SURFACE_MODEL('',(#13));\
             #13=OPEN_SHELL('',(#3));",
        );
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.kind, BodyKind::ShellModel);
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.shells.len(), 1);
        assert_eq!(b.shells[0].faces, vec![0]);
        let f = &b.faces[0];
        assert_eq!(f.step_id, 3);
        assert!(!f.same_sense);
        assert_eq!(f.loops.len(), 1);
        assert!(f.loops[0].outer);
        assert!(!f.loops[0].orientation);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    #[test]
    fn oriented_face_positive_control_keeps_inner_face() {
        let m = oriented_face_model(
            "#3=ORIENTED_FACE('',*,#6,.T.);\
             #5=SHELL_BASED_SURFACE_MODEL('',(#13));\
             #13=OPEN_SHELL('',(#3));",
        );
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.shells.len(), 1);
        assert_eq!(b.shells[0].faces, vec![0]);
        let f = &b.faces[0];
        assert_eq!(f.step_id, 6);
        assert!(f.same_sense);
        assert_eq!(f.loops.len(), 1);
        assert!(f.loops[0].outer);
        assert!(f.loops[0].orientation);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    #[test]
    fn oriented_face_mixed_positive_then_reverse_shares_base_face() {
        let m = oriented_face_mixed_model("#13,#14");
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 2);
        assert_eq!(b.shells.len(), 2);
        assert_eq!(b.shells[0].step_id, 13);
        assert_eq!(b.shells[0].faces, vec![0]);
        assert_eq!(b.shells[1].step_id, 14);
        assert_eq!(b.shells[1].faces, vec![1]);
        // The `.T.` wrapper row is mapped first and must not be mutated by the
        // later `.F.` reverse.
        assert_eq!(b.faces[0].step_id, 6);
        assert!(b.faces[0].same_sense);
        assert!(b.faces[0].loops[0].orientation);
        assert_eq!(b.faces[1].step_id, 4);
        assert!(!b.faces[1].same_sense);
        assert!(!b.faces[1].loops[0].orientation);
        // Both wrappers describe the same bounds and geometry.
        assert!(b.faces[0].loops[0].outer);
        assert_eq!(b.faces[0].loops[0].bound, b.faces[1].loops[0].bound);
        assert_eq!(b.faces[0].surface, b.faces[1].surface);
        // POLY_LOOP vertex/edge caching is shared between the two uses.
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    /// A hand-written B-spline sheet with no shell or representation wrapper. It must
    /// still map to one body so the official reader does not report "no B-rep bodies".
    const BSPLINE_SHEET: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));\
         #2=CARTESIAN_POINT('',(0.,0.5,0.25));\
         #3=CARTESIAN_POINT('',(0.,1.,1.));\
         #4=CARTESIAN_POINT('',(0.5,0.,0.25));\
         #5=CARTESIAN_POINT('',(0.5,0.5,0.5));\
         #6=CARTESIAN_POINT('',(0.5,1.,1.25));\
         #7=CARTESIAN_POINT('',(1.,0.,1.));\
         #8=CARTESIAN_POINT('',(1.,0.5,1.25));\
         #9=CARTESIAN_POINT('',(1.,1.,2.));\
         #10=B_SPLINE_SURFACE_WITH_KNOTS('',2,2,((#1,#2,#3),(#4,#5,#6),(#7,#8,#9)),\
              .UNSPECIFIED.,.F.,.F.,.F.,(3,3),(3,3),(0.,1.),(0.,1.),.UNSPECIFIED.);\
         #20=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#4,#7),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\
         #21=B_SPLINE_CURVE_WITH_KNOTS('',2,(#7,#8,#9),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\
         #22=B_SPLINE_CURVE_WITH_KNOTS('',2,(#3,#6,#9),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\
         #23=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\
         #30=VERTEX_POINT('',#1);\
         #31=VERTEX_POINT('',#7);\
         #32=VERTEX_POINT('',#9);\
         #33=VERTEX_POINT('',#3);\
         #40=EDGE_CURVE('',#30,#31,#20,.T.);\
         #41=EDGE_CURVE('',#31,#32,#21,.T.);\
         #42=EDGE_CURVE('',#33,#32,#22,.T.);\
         #43=EDGE_CURVE('',#30,#33,#23,.T.);\
         #50=ORIENTED_EDGE('',*,*,#40,.T.);\
         #51=ORIENTED_EDGE('',*,*,#41,.T.);\
         #52=ORIENTED_EDGE('',*,*,#42,.F.);\
         #53=ORIENTED_EDGE('',*,*,#43,.F.);\
         #60=EDGE_LOOP('',(#50,#51,#52,#53));\
         #61=FACE_OUTER_BOUND('',#60,.T.);\
         #70=ADVANCED_FACE('',(#61),#10,.T.);";

    fn assert_bspline_sheet_body(b: &Body) {
        assert_eq!(b.step_id, 70);
        assert_eq!(b.kind, BodyKind::ShellModel);
        assert_eq!(b.unit_mm, 1.0);
        assert_eq!(b.shells.len(), 1);
        assert_eq!(b.shells[0].step_id, 0, "synthetic shell has no STEP id");
        assert!(!b.shells[0].closed);
        assert!(b.shells[0].orientation);
        assert_eq!(b.shells[0].faces, vec![0]);
        assert_eq!(b.faces.len(), 1);
        let f = &b.faces[0];
        assert_eq!(f.step_id, 70);
        assert!(f.same_sense);
        assert_eq!(f.loops.len(), 1);
        assert!(f.loops[0].outer && f.loops[0].orientation);
        let Bound::Edges(edges) = &f.loops[0].bound else {
            panic!("expected an edge loop")
        };
        assert_eq!(edges.len(), 4);
        assert_eq!(
            edges.iter().map(|e| e.forward).collect::<Vec<_>>(),
            vec![true, true, false, false]
        );
        let Surface::BSpline(surface) = &f.surface else {
            panic!("expected a B-spline surface")
        };
        assert_eq!((surface.u_degree, surface.v_degree), (2, 2));
        assert_eq!(surface.points.len(), 3);
        assert!(surface.points.iter().all(|row| row.len() == 3));
        assert_eq!(surface.points[2][2], [1.0, 1.0, 2.0]);
        assert_eq!(surface.u_knots, vec![0.0, 1.0]);
        assert_eq!(surface.u_mults, vec![3, 3]);
        assert_eq!(surface.v_knots, vec![0.0, 1.0]);
        assert_eq!(surface.v_mults, vec![3, 3]);
        assert_eq!(
            b.edges.iter().map(|e| e.step_id).collect::<Vec<_>>(),
            vec![40, 41, 42, 43]
        );
        assert_eq!(b.vertices.len(), 4);
        assert_eq!(b.vertices[0].point, [0.0, 0.0, 0.0]);
        assert_eq!(b.vertices[3].point, [0.0, 1.0, 1.0]);
        for edge in &b.edges {
            let Curve::BSpline(curve) = &edge.curve else {
                panic!("expected a B-spline edge")
            };
            assert_eq!(curve.degree, 2);
            assert_eq!((edge.t_start, edge.t_end), (0.0, 1.0));
        }
    }

    #[test]
    fn loose_bspline_face_maps_as_a_sheet_body() {
        let m = model(BSPLINE_SHEET);
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(m.bodies.len(), 1);
        assert_bspline_sheet_body(&m.bodies[0]);
        assert!(
            m.diagnostics
                .iter()
                .any(|d| d.message.contains("not referenced by any shell")),
            "{:?}",
            m.diagnostics
        );
        // The hand-written fixture has no representation, so units fall back to mm
        // with the usual diagnostic.
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no GLOBAL_UNIT")));
    }

    #[test]
    fn loose_face_uses_its_representation_units() {
        let m = model(&format!(
            "{BSPLINE_SHEET}\
             #80=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));\
             #81=(GEOMETRIC_REPRESENTATION_CONTEXT(3)\
                  GLOBAL_UNIT_ASSIGNED_CONTEXT((#80)) REPRESENTATION_CONTEXT('',''));\
             #82=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#70),#81);"
        ));
        assert_eq!(m.bodies.len(), 1);
        let b = &m.bodies[0];
        assert_eq!(b.unit_mm, 10.0);
        assert_eq!(b.vertices[0].point, [0.0, 0.0, 0.0]);
        assert_eq!(b.vertices[2].point, [10.0, 10.0, 20.0]);
        let Surface::BSpline(surface) = &b.faces[0].surface else {
            panic!("expected a B-spline surface")
        };
        assert_eq!(surface.points[2][2], [10.0, 10.0, 20.0]);
    }

    #[test]
    fn loose_oriented_face_maps_once_with_reversed_sense() {
        let m = model(&format!("{BSPLINE_SHEET}#71=ORIENTED_FACE('',*,#70,.F.);"));
        assert_eq!(m.bodies.len(), 1);
        let b = &m.bodies[0];
        assert_eq!(b.step_id, 71);
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.faces[0].step_id, 71);
        assert!(!b.faces[0].same_sense);
        assert!(!b.faces[0].loops[0].orientation);
        // The face was mapped through the wrapper, not as a separate sheet.
        assert_eq!(b.shells[0].faces, vec![0]);
    }

    #[test]
    fn faces_of_a_skipped_solid_are_not_loose_sheets() {
        let m = model(
            "#1=MANIFOLD_SOLID_BREP('',#2);#2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);#4=OFFSET_SURFACE('',#5,1.,.F.);",
        );
        // The solid is skipped as a whole; its faces are not re-imported as sheets.
        assert!(m.bodies.is_empty());
        assert_eq!(m.skipped.len(), 1);
        assert!(m.skipped[0].reason.contains("OFFSET_SURFACE"));
    }

    /// Nested loops on a cylinder with closed and trimmed conic edges: the edge
    /// parameters come from the 3D conics, not from a projection into cylinder UV
    /// (the legacy mixed reader projected intersection curves into UV and produced a
    /// bogus 446-degree loop). A closed edge spans exactly one period.
    #[test]
    fn conic_edge_parameters_stay_in_3d() {
        let m = model(
            "#1=CARTESIAN_POINT('',(0.,0.,0.));\
             #2=DIRECTION('',(0.,0.,1.));\
             #3=DIRECTION('',(1.,0.,0.));\
             #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\
             #5=CYLINDRICAL_SURFACE('',#4,2.5);\
             #10=CARTESIAN_POINT('',(-2.5,0.,0.));\
             #11=VERTEX_POINT('',#10);\
             #12=AXIS2_PLACEMENT_3D('',#1,#2,#3);\
             #13=CIRCLE('',#12,2.5);\
             #14=EDGE_CURVE('',#11,#11,#13,.T.);\
             #15=ORIENTED_EDGE('',*,*,#14,.T.);\
             #16=EDGE_LOOP('',(#15));\
             #17=FACE_BOUND('',#16,.T.);\
             #20=CARTESIAN_POINT('',(1.7320508075688772,0.5,0.));\
             #21=VERTEX_POINT('',#20);\
             #22=CARTESIAN_POINT('',(-1.,0.8660254037844386,0.));\
             #23=VERTEX_POINT('',#22);\
             #24=AXIS2_PLACEMENT_3D('',#1,#2,#3);\
             #25=ELLIPSE('',#24,2.,1.);\
             #26=EDGE_CURVE('',#21,#23,#25,.T.);\
             #27=ORIENTED_EDGE('',*,*,#26,.T.);\
             #28=EDGE_CURVE('',#23,#21,#25,.F.);\
             #29=ORIENTED_EDGE('',*,*,#28,.T.);\
             #30=EDGE_LOOP('',(#27,#29));\
             #31=FACE_BOUND('',#30,.T.);\
             #40=ADVANCED_FACE('',(#17,#31),#5,.T.);",
        );
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(m.bodies.len(), 1);
        let b = &m.bodies[0];
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.faces[0].loops.len(), 2);
        assert!(!b.faces[0].loops[0].outer);
        assert!(!b.faces[0].loops[1].outer);
        assert_eq!(b.edges.len(), 3);
        // Closed circle: start == end spans one full period.
        let circle = b.edges.iter().find(|e| e.step_id == 14).unwrap();
        assert_eq!((circle.start, circle.end), (0, 0));
        assert!((circle.t_start + 180.0).abs() < 1e-9, "{}", circle.t_start);
        assert!((circle.t_end - 180.0).abs() < 1e-9, "{}", circle.t_end);
        // Trimmed ellipse: parameters are the 3D conic angles, 30 and 120 degrees.
        let forward = b.edges.iter().find(|e| e.step_id == 26).unwrap();
        assert!((forward.t_start - 30.0).abs() < 1e-9, "{}", forward.t_start);
        assert!((forward.t_end - 120.0).abs() < 1e-9, "{}", forward.t_end);
        let backward = b.edges.iter().find(|e| e.step_id == 28).unwrap();
        assert!(!backward.same_sense);
        assert!(
            (backward.t_start - 120.0).abs() < 1e-9,
            "{}",
            backward.t_start
        );
        assert!((backward.t_end - 30.0).abs() < 1e-9, "{}", backward.t_end);
        // The curves stay analytic: nothing was projected into surface UV.
        assert!(
            matches!(circle.curve, Curve::Circle { radius, .. } if (radius - 2.5).abs() < 1e-12)
        );
        assert!(
            matches!(forward.curve, Curve::Ellipse { semi1, semi2, .. } if (semi1 - 2.0).abs() < 1e-12 && (semi2 - 1.0).abs() < 1e-12)
        );
    }

    #[test]
    fn pcurves_map_in_model_units() {
        let m = model(
            "#1=CARTESIAN_POINT('',(0.,0.,0.));\
             #2=DIRECTION('',(0.,0.,1.));\
             #3=DIRECTION('',(1.,0.,0.));\
             #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\
             #5=CYLINDRICAL_SURFACE('',#4,2.5);\
             #10=CARTESIAN_POINT('',(2.5,0.,0.));\
             #11=VERTEX_POINT('',#10);\
             #12=AXIS2_PLACEMENT_3D('',#1,#2,#3);\
             #13=CIRCLE('',#12,2.5);\
             #14=SURFACE_CURVE('',#13,(#56,#66,#71,#5),.CURVE_3D.);\
             #15=EDGE_CURVE('',#11,#11,#14,.T.);\
             #16=ORIENTED_EDGE('',*,*,#15,.T.);\
             #17=EDGE_LOOP('',(#16));\
             #18=FACE_BOUND('',#17,.T.);\
             #40=ADVANCED_FACE('',(#18),#5,.T.);\
             #50=CARTESIAN_POINT('',(0.,0.));\
             #51=DIRECTION('',(1.,0.));\
             #52=VECTOR('',#51,1.);\
             #53=LINE('',#50,#52);\
             #54=TRIMMED_CURVE('',#53,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(6.283185307179586)),.T.,.PARAMETER.);\
             #55=DEFINITIONAL_REPRESENTATION('',(#54),#99);\
             #56=PCURVE('',#5,#55);\
             #60=PLANE('',#4);\
             #61=CARTESIAN_POINT('',(0.,0.));\
             #62=DIRECTION('',(1.,0.));\
             #63=AXIS2_PLACEMENT_2D('',#61,#62);\
             #64=CIRCLE('',#63,2.5);\
             #65=DEFINITIONAL_REPRESENTATION('',(#64),#99);\
             #66=PCURVE('',#60,#65);\
             #70=SURFACE_OF_LINEAR_EXTRUSION('',#13,#52);\
             #71=PCURVE('',#70,#55);\
             #99=PARAMETRIC_REPRESENTATION_CONTEXT('','');",
        );
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(m.bodies.len(), 1);
        let p = &m.file_pcurves;
        assert_eq!(p.face_surfaces.get(&40), Some(&5));
        let pcurves = p.edges.get(&15).expect("edge pcurves");
        assert_eq!(pcurves.len(), 2, "{pcurves:?}");
        // No unit context: angles are radians, so the cylinder's u is scaled to degrees
        // and its v (a length) is not. Line trims are line parameters, not angles.
        let deg = 180.0 / std::f64::consts::PI;
        assert_eq!(pcurves[0].surface, 5);
        match &pcurves[0].curve {
            Curve2d::Line { origin, dir } => {
                assert_eq!(*origin, [0.0, 0.0]);
                assert!((dir[0] - deg).abs() < 1e-12 && dir[1] == 0.0, "{dir:?}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(pcurves[0].trim, Some((0.0, std::f64::consts::TAU)));
        // A plane's u and v are lengths.
        assert_eq!(pcurves[1].surface, 60);
        assert!(
            matches!(pcurves[1].curve, Curve2d::Conic { centre, axis1, axis2 }
                if centre == [0.0, 0.0] && axis1 == [2.5, 0.0] && axis2 == [0.0, 2.5]),
            "{:?}",
            pcurves[1].curve
        );
        assert_eq!(pcurves[1].trim, None);
        // The extrusion's pcurve is reported and left out; the model's own
        // diagnostics do not change.
        assert_eq!(p.diagnostics.len(), 1, "{:?}", p.diagnostics);
        assert_eq!(p.diagnostics[0].step_id, Some(71));
        assert!(m.diagnostics.iter().all(|d| !d.message.contains("pcurve")));
    }

    #[test]
    fn oriented_face_mixed_reverse_then_positive_shares_base_face() {
        let m = oriented_face_mixed_model("#14,#13");
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 2);
        assert_eq!(b.shells.len(), 2);
        assert_eq!(b.shells[0].step_id, 14);
        assert_eq!(b.shells[0].faces, vec![0]);
        assert_eq!(b.shells[1].step_id, 13);
        assert_eq!(b.shells[1].faces, vec![1]);
        assert_eq!(b.faces[0].step_id, 4);
        assert!(!b.faces[0].same_sense);
        assert!(!b.faces[0].loops[0].orientation);
        // The later `.T.` wrapper maps a fresh positive row.
        assert_eq!(b.faces[1].step_id, 6);
        assert!(b.faces[1].same_sense);
        assert!(b.faces[1].loops[0].orientation);
        assert!(b.faces[0].loops[0].outer);
        assert_eq!(b.faces[0].loops[0].bound, b.faces[1].loops[0].bound);
        assert_eq!(b.faces[0].surface, b.faces[1].surface);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }
}
