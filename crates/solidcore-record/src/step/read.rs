//! STEP geometry entities (ISO 10303-42) to the plain data of [`super::geom`], from a parsed
//! [`Exchange`] (ISO 10303-21) in one representation context's [`Units`].
//!
//! Input is hostile: every count, number and reference is checked, nesting is capped
//! ([`MAX_GEOM_DEPTH`]), counts are capped ([`MAX_CTRL`], [`MAX_DEGREE`]) and checked before
//! anything is expanded, numbers must be finite and below [`MAX_COORD`], nothing is
//! pre-allocated from a count in the file, shared entities are read once, and every error is a
//! message, never a panic.
//!
//! The only arithmetic is unit conversion: a length read times the length unit, an angle times
//! the plane angle unit, a coordinate in a parameter space times that parameter's unit, and the
//! knot counting of [`BSplineCurve`]s given without knots. Directions keep the ratios the file
//! writes; checks that would need a geometric quantity are the kernel's when it builds from this
//! data (#2578, slice 4), and the checks kept here are on the data alone: a direction that is
//! exactly zero in every component, a vector of magnitude zero, a radius, magnitude or weight of
//! the wrong sign, a knot vector that does not fit its control points.
//!
//! Origin: SolidCraft's `crates/step/src/read.rs` at `a5b71ca2` (MIT OR Apache-2.0).
//!
//! Deviations from SolidCraft, each without a geometric computation in its place (the kernel
//! takes those over in slice 4):
//! - Directions are not normalised (was `unit`): a direction is refused when every ratio is
//!   exactly 0 (was: when its length is at most 1e-12). Model-space directions are the ratios
//!   as written; in a surface's parameter space each ratio is times its parameter's unit, so it
//!   points the way the file's direction does in millimetres and radians.
//! - A placement keeps `axis` and `ref_direction` as given, each optional (was: the reference
//!   direction made orthogonal to the axis, or `any_perpendicular` when it is absent or
//!   parallel; neither refused a file).
//! - A `VECTOR` keeps its orientation and magnitude and the space's unit factors (was: a unit
//!   direction and the scaled length); it is refused when its magnitude is negative or zero (was:
//!   negative, or a scaled length that is not positive).
//! - A conic in a 2D parameter space must have equal unit factors on both parameters (was: equal
//!   within 1e-12 relative).
//! - Uniform implied knots are counted in integers before conversion (`i - degree`; was the
//!   same difference in `f64`, exact for every count below 2^53).
//! - The depth bound is checked for every curve read, including the basis of a 2D trimmed curve
//!   (was: only at a 2D curve's top, so a 2D trimmed curve whose basis chain cycles recursed
//!   without bound).
//! - [`Reader::pcurve`] reads a `PCURVE` on its own (new; it is what `associated_geometry` and a
//!   pcurve used as an edge's geometry hold).
//! - The caches are `BTreeMap`s (were `HashMap`s); nothing iterates them. Collections grow as
//!   they are read (no capacity from a count in the file). [`Units`] is `super::units::Units`.
//!   Rustfmt, clippy-driven rewrites that keep the result, named bounds for the cone's semi
//!   angle, and doc comments on every public item.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use super::geom::{
    Associated, Axis1, BSplineCurve, BSplineSurface, Curve, CurveForm, KnotSpec, Logical, P3,
    Pcurve, Placement, PreferredRep, Surface, SurfaceCurveKind, SurfaceForm, Trim, TrimPreference,
    Vector,
};
use super::p21::{Entity, Exchange, Param};
use super::units::Units;

/// A result whose error is a message.
pub type R<T> = Result<T, String>;

/// Geometry nesting we follow (a trimmed curve of a surface curve of …). Every curve and
/// surface read checks it, so a reference cycle ends here.
pub const MAX_GEOM_DEPTH: usize = 12;
/// Most control points in one B-spline (the whole net for a surface).
pub const MAX_CTRL: usize = 1_000_000;
/// Largest coordinate or length, in file units.
pub const MAX_COORD: f64 = 1e9;
/// Highest B-spline degree.
pub const MAX_DEGREE: usize = 32;
/// A cone's semi angle, in radians, must be larger than this in size …
const MIN_SEMI_ANGLE: f64 = 1e-12;
/// … and smaller than this (π/2 less the same margin).
const MAX_SEMI_ANGLE: f64 = FRAC_PI_2 - 1e-12;

/// How a parameter of a curve or surface scales from file units: angles by the angle unit,
/// lengths by the length unit, anything else (B-spline knots, ratios) not at all.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ParamKind {
    Length,
    Angle,
    Plain,
}

/// The space an entity is read in: the unit factors of its coordinates and of its direction
/// ratios, and its dimension.
#[derive(Clone, Copy, Debug)]
struct Space {
    /// Factor per coordinate of a point (and of a vector's components).
    pts: [f64; 3],
    /// Factor per direction ratio: 1 in model space, the parameter's unit in a parameter space.
    dirs: [f64; 3],
    /// 3 for model space, 2 for a parameter space or a 2D placement.
    dim: usize,
}

impl Space {
    /// Model space in the length unit `l`.
    fn model(l: f64, dim: usize) -> Self {
        Space {
            pts: [l, l, l],
            dirs: [1.0; 3],
            dim,
        }
    }
    /// A surface's parameter space whose parameters have the factors `s`.
    fn parameters(s: [f64; 3]) -> Self {
        Space {
            pts: s,
            dirs: s,
            dim: 2,
        }
    }
}

/// Reads geometry from one exchange structure, in one context's units.
pub struct Reader<'a> {
    ex: &'a Exchange,
    units: Units,
    curves: BTreeMap<u64, Arc<Curve>>,
    curves_2d: BTreeMap<(u64, [u64; 2]), Arc<Curve>>,
    surfaces: BTreeMap<u64, Arc<Surface>>,
}

fn entity(ex: &Exchange, id: u64) -> R<&Entity> {
    ex.get(id).ok_or_else(|| format!("missing entity #{id}"))
}

fn reference(p: Option<&Param>, what: &str) -> R<u64> {
    p.and_then(Param::as_ref_id)
        .ok_or_else(|| format!("{what}: expected a reference"))
}

fn num(p: Option<&Param>, what: &str) -> R<f64> {
    let x = p
        .and_then(Param::as_f64)
        .ok_or_else(|| format!("{what}: expected a number"))?;
    if x.is_finite() && x.abs() < MAX_COORD {
        Ok(x)
    } else {
        Err(format!("{what}: {x} out of range"))
    }
}

fn logical(p: Option<&Param>, what: &str) -> R<Logical> {
    match p.and_then(Param::as_enum) {
        Some("T") => Ok(Logical::True),
        Some("F") => Ok(Logical::False),
        Some("U") => Ok(Logical::Unknown),
        _ => Err(format!("{what}: expected a logical")),
    }
}

fn boolean(p: Option<&Param>, what: &str) -> R<bool> {
    p.and_then(Param::as_bool)
        .ok_or_else(|| format!("{what}: expected a boolean"))
}

fn positive(x: f64, what: &str) -> R<f64> {
    if x > 0.0 {
        Ok(x)
    } else {
        Err(format!("{what} must be positive"))
    }
}

fn curve_form(p: Option<&Param>) -> CurveForm {
    match p.and_then(Param::as_enum) {
        Some("POLYLINE_FORM") => CurveForm::PolylineForm,
        Some("CIRCULAR_ARC") => CurveForm::CircularArc,
        Some("ELLIPTIC_ARC") => CurveForm::EllipticArc,
        Some("PARABOLIC_ARC") => CurveForm::ParabolicArc,
        Some("HYPERBOLIC_ARC") => CurveForm::HyperbolicArc,
        _ => CurveForm::Unspecified,
    }
}

fn surface_form(p: Option<&Param>) -> SurfaceForm {
    match p.and_then(Param::as_enum) {
        Some("PLANE_SURF") => SurfaceForm::PlaneSurf,
        Some("CYLINDRICAL_SURF") => SurfaceForm::CylindricalSurf,
        Some("CONICAL_SURF") => SurfaceForm::ConicalSurf,
        Some("SPHERICAL_SURF") => SurfaceForm::SphericalSurf,
        Some("TOROIDAL_SURF") => SurfaceForm::ToroidalSurf,
        Some("SURF_OF_REVOLUTION") => SurfaceForm::SurfOfRevolution,
        Some("RULED_SURF") => SurfaceForm::RuledSurf,
        Some("GENERALISED_CONE") => SurfaceForm::GeneralisedCone,
        Some("QUADRIC_SURF") => SurfaceForm::QuadricSurf,
        Some("SURF_OF_LINEAR_EXTRUSION") => SurfaceForm::SurfOfLinearExtrusion,
        _ => SurfaceForm::Unspecified,
    }
}

fn knot_spec(p: Option<&Param>) -> KnotSpec {
    match p.and_then(Param::as_enum) {
        Some("UNIFORM_KNOTS") => KnotSpec::UniformKnots,
        Some("QUASI_UNIFORM_KNOTS") => KnotSpec::QuasiUniformKnots,
        Some("PIECEWISE_BEZIER_KNOTS") => KnotSpec::PiecewiseBezierKnots,
        _ => KnotSpec::Unspecified,
    }
}

/// Distinct knots and multiplicities, checked against `n_ctrl` points of `degree` before
/// anything is expanded (a file can claim multiplicities that would need gigabytes).
fn knots(
    mults: Option<&Param>,
    values: Option<&Param>,
    n_ctrl: usize,
    degree: usize,
) -> R<(Vec<f64>, Vec<usize>)> {
    let m: Vec<usize> = mults
        .and_then(Param::as_list)
        .ok_or("B-spline without knot multiplicities")?
        .iter()
        .map(|x| {
            x.as_i64()
                .filter(|v| *v >= 1 && *v as usize <= degree + 1)
                .map(|v| v as usize)
                .ok_or_else(|| "bad knot multiplicity".to_string())
        })
        .collect::<R<_>>()?;
    let raw: Vec<f64> = values
        .and_then(Param::as_list)
        .ok_or("B-spline without knots")?
        .iter()
        .map(|x| num(Some(x), "knot"))
        .collect::<R<_>>()?;
    if m.len() != raw.len() || raw.windows(2).any(|w| w.first() > w.get(1)) {
        return Err("knots must not decrease, one multiplicity each".into());
    }
    // Some writers list a knot twice; that is one knot with both multiplicities.
    let (mut k, mut mm): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for (x, n) in raw.into_iter().zip(m) {
        match (k.last(), mm.last_mut()) {
            (Some(last), Some(lm)) if *last == x => *lm += n,
            _ => {
                k.push(x);
                mm.push(n);
            }
        }
    }
    let m = mm;
    if k.len() < 2 || m.iter().any(|v| *v > degree + 1) {
        return Err("bad knot vector".into());
    }
    if m.iter().try_fold(0usize, |a, v| a.checked_add(*v)) != Some(n_ctrl + degree + 1) {
        return Err("knot multiplicities do not match the control points".into());
    }
    Ok((k, m))
}

/// Knots of a B-spline given by its type alone (`BEZIER_`, `UNIFORM_`, `QUASI_UNIFORM_CURVE`):
/// the knot values are whole numbers and the multiplicities counts, laid out from the number of
/// control points and the degree (integer arithmetic only).
fn implied_knots(spec: KnotSpec, n_ctrl: usize, degree: usize) -> R<(Vec<f64>, Vec<usize>)> {
    if n_ctrl <= degree {
        return Err("too few control points".into());
    }
    match spec {
        KnotSpec::PiecewiseBezierKnots => {
            // Segments of `degree`, full interior multiplicity.
            let segs = (n_ctrl - 1) / degree;
            if segs * degree + 1 != n_ctrl {
                return Err("Bezier control point count".into());
            }
            let k = (0..=segs).map(|s| s as f64).collect();
            let mut m = vec![degree; segs + 1];
            if let Some(f) = m.first_mut() {
                *f = degree + 1;
            }
            if let Some(l) = m.last_mut() {
                *l = degree + 1;
            }
            Ok((k, m))
        }
        KnotSpec::UniformKnots => {
            let n = n_ctrl + degree + 1;
            // n ≤ MAX_CTRL + MAX_DEGREE + 1, so the counts fit in i64 and convert exactly.
            let k = (0..n).map(|i| (i as i64 - degree as i64) as f64).collect();
            Ok((k, vec![1; n]))
        }
        KnotSpec::QuasiUniformKnots | KnotSpec::Unspecified => {
            // Quasi-uniform: clamped ends, uniform interior.
            let inner = n_ctrl - degree;
            let k = (0..=inner).map(|i| i as f64).collect();
            let mut m = vec![1; inner + 1];
            if let Some(f) = m.first_mut() {
                *f = degree + 1;
            }
            if let Some(l) = m.last_mut() {
                *l = degree + 1;
            }
            Ok((k, m))
        }
    }
}

fn weights(p: Option<&Param>, n: usize) -> R<Vec<f64>> {
    let w: Vec<f64> = p
        .and_then(Param::as_list)
        .ok_or("rational B-spline without weights")?
        .iter()
        .map(|w| {
            num(Some(w), "weight").and_then(|w| {
                if w > 0.0 {
                    Ok(w)
                } else {
                    Err("non-positive weight".into())
                }
            })
        })
        .collect::<R<_>>()?;
    if w.len() == n {
        Ok(w)
    } else {
        Err("weights do not match the control points".into())
    }
}

fn degree(p: Option<&Param>) -> R<usize> {
    p.and_then(Param::as_i64)
        .filter(|d| (1..=MAX_DEGREE as i64).contains(d))
        .map(|d| d as usize)
        .ok_or_else(|| "bad B-spline degree".into())
}

/// The knot type a B-spline entity's (sub)type implies when it lists no knots.
fn spec_of_type(e: &Entity) -> KnotSpec {
    let has = |n: &str| e.name() == n || e.has(n);
    if has("BEZIER_CURVE") || has("BEZIER_SURFACE") {
        KnotSpec::PiecewiseBezierKnots
    } else if has("UNIFORM_CURVE") || has("UNIFORM_SURFACE") {
        KnotSpec::UniformKnots
    } else {
        KnotSpec::QuasiUniformKnots
    }
}

/// The parameters of a simple or complex B-spline entity, as the simple form lays them out:
/// the name first, then the `B_SPLINE_*` attributes, then the `*_WITH_KNOTS` ones.
fn bspline_params(e: &Entity, base: &str, with_knots: &str) -> R<(Vec<Param>, Option<Vec<Param>>)> {
    if e.is_complex() {
        let b = e
            .record(base)
            .ok_or_else(|| format!("complex entity without {base}"))?;
        let mut p = vec![Param::Str(String::new())];
        p.extend(b.iter().cloned());
        Ok((p, e.record(with_knots).map(<[Param]>::to_vec)))
    } else {
        let p = e.params();
        let n = if base == "B_SPLINE_CURVE" { 6 } else { 8 };
        let k =
            (e.name() == with_knots).then(|| p.get(n..).map(<[Param]>::to_vec).unwrap_or_default());
        Ok((p.to_vec(), k))
    }
}

impl<'a> Reader<'a> {
    /// A reader of `ex` whose lengths and angles are in `units`.
    pub fn new(ex: &'a Exchange, units: Units) -> Self {
        Reader {
            ex,
            units,
            curves: BTreeMap::new(),
            curves_2d: BTreeMap::new(),
            surfaces: BTreeMap::new(),
        }
    }

    /// The units the reader converts from.
    pub fn units(&self) -> Units {
        self.units
    }

    fn length(&self, p: Option<&Param>, what: &str) -> R<f64> {
        Ok(num(p, what)? * self.units.len)
    }

    fn angle(&self, p: Option<&Param>, what: &str) -> R<f64> {
        Ok(num(p, what)? * self.units.ang)
    }

    /// A `CARTESIAN_POINT`, each coordinate times its factor in `sp`.
    fn point_in(&self, id: u64, sp: Space) -> R<P3> {
        let e = entity(self.ex, id)?;
        if e.name() != "CARTESIAN_POINT" {
            return Err(format!(
                "#{id}: expected CARTESIAN_POINT, found {}",
                e.name()
            ));
        }
        let c = e
            .params()
            .get(1)
            .and_then(Param::as_list)
            .ok_or("CARTESIAN_POINT without coordinates")?;
        if c.is_empty() || c.len() > sp.dim {
            return Err(format!("#{id}: point with {} coordinates", c.len()));
        }
        let g = |i: usize| -> R<f64> {
            if i < c.len() {
                num(c.get(i), "coordinate")
            } else {
                Ok(0.0)
            }
        };
        let [sx, sy, sz] = sp.pts;
        Ok([
            g(0)? * sx,
            g(1)? * sy,
            if sp.dim == 3 { g(2)? * sz } else { 0.0 },
        ])
    }

    /// A `CARTESIAN_POINT` in millimetres.
    pub fn point(&self, id: u64) -> R<P3> {
        self.point_in(id, Space::model(self.units.len, 3))
    }

    /// The ratios of a `DIRECTION`, each times `s`. Refused when every ratio is exactly zero:
    /// that needs no arithmetic. A direction too short to normalise is the kernel's to refuse.
    fn ratios(&self, id: u64, s: [f64; 3], dim: usize) -> R<P3> {
        let e = entity(self.ex, id)?;
        if e.name() != "DIRECTION" {
            return Err(format!("#{id}: expected DIRECTION, found {}", e.name()));
        }
        let c = e
            .params()
            .get(1)
            .and_then(Param::as_list)
            .ok_or("DIRECTION without ratios")?;
        if c.is_empty() || c.len() > dim {
            return Err(format!("#{id}: direction with {} ratios", c.len()));
        }
        let g = |i: usize| -> R<f64> {
            if i < c.len() {
                num(c.get(i), "direction ratio")
            } else {
                Ok(0.0)
            }
        };
        let [sx, sy, sz] = s;
        let d = [
            g(0)? * sx,
            g(1)? * sy,
            if dim == 3 { g(2)? * sz } else { 0.0 },
        ];
        if d == [0.0; 3] {
            return Err(format!("#{id}: zero direction"));
        }
        Ok(d)
    }

    /// A `DIRECTION` in model space: its ratios as written (not normalised).
    pub fn direction(&self, id: u64) -> R<P3> {
        self.ratios(id, [1.0; 3], 3)
    }

    /// `AXIS2_PLACEMENT_3D` in 3D, or `AXIS2_PLACEMENT_2D` (no axis) in 2D.
    fn placement_in(&self, id: u64, sp: Space) -> R<Placement> {
        let e = entity(self.ex, id)?;
        let p = e.params();
        let origin = self.point_in(reference(p.get(1), "placement location")?, sp)?;
        let (axis, rf) = match (e.name(), sp.dim) {
            ("AXIS2_PLACEMENT_3D", 3) => {
                let z = match p.get(2).and_then(Param::as_ref_id) {
                    Some(d) => Some(self.ratios(d, sp.dirs, sp.dim)?),
                    None => None,
                };
                (z, p.get(3).and_then(Param::as_ref_id))
            }
            ("AXIS2_PLACEMENT_2D", 2) => (None, p.get(2).and_then(Param::as_ref_id)),
            (n, _) => return Err(format!("#{id}: unexpected placement {n} in {}D", sp.dim)),
        };
        let ref_direction = match rf {
            Some(d) => Some(self.ratios(d, sp.dirs, sp.dim)?),
            None => None,
        };
        Ok(Placement {
            origin,
            axis,
            ref_direction,
        })
    }

    /// An `AXIS2_PLACEMENT_3D` in millimetres.
    pub fn placement(&self, id: u64) -> R<Placement> {
        self.placement_in(id, Space::model(self.units.len, 3))
    }

    /// A placement in 3D space (an assembly's or a mapped item's): `AXIS2_PLACEMENT_3D`, or an
    /// `AXIS2_PLACEMENT_2D` as the placement in the XY plane it describes.
    pub fn frame(&self, id: u64) -> R<Placement> {
        let dim = if entity(self.ex, id)?.name() == "AXIS2_PLACEMENT_2D" {
            2
        } else {
            3
        };
        self.placement_in(id, Space::model(self.units.len, dim))
    }

    /// An `AXIS1_PLACEMENT` in millimetres.
    pub fn axis1(&self, id: u64) -> R<Axis1> {
        let e = entity(self.ex, id)?;
        if e.name() != "AXIS1_PLACEMENT" {
            return Err(format!(
                "#{id}: expected AXIS1_PLACEMENT, found {}",
                e.name()
            ));
        }
        let p = e.params();
        let origin = self.point(reference(p.get(1), "axis location")?)?;
        let direction = match p.get(2).and_then(Param::as_ref_id) {
            Some(d) => Some(self.direction(d)?),
            None => None,
        };
        Ok(Axis1 { origin, direction })
    }

    /// A `VECTOR`: its orientation's ratios and its magnitude as written, with the factors of
    /// `sp`. Refused when the magnitude is negative or zero.
    fn vector_in(&self, id: u64, sp: Space) -> R<Vector> {
        let e = entity(self.ex, id)?;
        if e.name() != "VECTOR" {
            return Err(format!("#{id}: expected VECTOR, found {}", e.name()));
        }
        let p = e.params();
        let orientation =
            self.ratios(reference(p.get(1), "vector orientation")?, [1.0; 3], sp.dim)?;
        let magnitude = num(p.get(2), "vector magnitude")?;
        if magnitude < 0.0 {
            return Err("negative vector magnitude".into());
        }
        if magnitude == 0.0 {
            return Err("zero vector".into());
        }
        Ok(Vector {
            orientation,
            magnitude,
            scale: sp.pts,
        })
    }

    /// A 3D curve entity.
    pub fn curve(&mut self, id: u64) -> R<Arc<Curve>> {
        self.curve_in(id, 0)
    }

    fn curve_in(&mut self, id: u64, depth: usize) -> R<Arc<Curve>> {
        if let Some(c) = self.curves.get(&id) {
            return Ok(c.clone());
        }
        let c = Arc::new(self.read_curve(id, Space::model(self.units.len, 3), depth)?);
        self.curves.insert(id, c.clone());
        Ok(c)
    }

    /// A 2D curve in a surface's parameter space, scaled by the surface's parameter units.
    fn curve_2d(&mut self, id: u64, kinds: (ParamKind, ParamKind), depth: usize) -> R<Arc<Curve>> {
        let s = [self.factor(kinds.0), self.factor(kinds.1), 1.0];
        let key = (id, [s[0].to_bits(), s[1].to_bits()]);
        if let Some(c) = self.curves_2d.get(&key) {
            return Ok(c.clone());
        }
        let c = Arc::new(self.read_curve(id, Space::parameters(s), depth)?);
        self.curves_2d.insert(key, c.clone());
        Ok(c)
    }

    fn factor(&self, k: ParamKind) -> f64 {
        match k {
            ParamKind::Length => self.units.len,
            ParamKind::Angle => self.units.ang,
            ParamKind::Plain => 1.0,
        }
    }

    /// The factor of a conic's radii: its space's, which must be the same on both axes (a
    /// conic in a parameter space with an angle on one axis and a length on the other is not
    /// a conic in millimetres and radians).
    fn conic_scale(sp: Space) -> R<f64> {
        let [sx, sy, _] = sp.pts;
        if sx != sy {
            return Err("not supported yet: a conic in a parameter space with mixed units".into());
        }
        Ok(sx)
    }

    fn read_curve(&mut self, id: u64, sp: Space, depth: usize) -> R<Curve> {
        if depth > MAX_GEOM_DEPTH {
            return Err("curve definitions nested too deeply".into());
        }
        let e = entity(self.ex, id)?;
        if e.is_complex() {
            return self.bspline_curve(e, sp);
        }
        let p = e.params();
        match e.name() {
            "LINE" => {
                let origin = self.point_in(reference(p.get(1), "line point")?, sp)?;
                let vector = self.vector_in(reference(p.get(2), "line vector")?, sp)?;
                Ok(Curve::Line { origin, vector })
            }
            "CIRCLE" => {
                let k = Self::conic_scale(sp)?;
                let position = self.placement_in(reference(p.get(1), "circle placement")?, sp)?;
                Ok(Curve::Circle {
                    position,
                    radius: positive(num(p.get(2), "radius")? * k, "circle radius")?,
                })
            }
            "ELLIPSE" => {
                let k = Self::conic_scale(sp)?;
                let position = self.placement_in(reference(p.get(1), "ellipse placement")?, sp)?;
                Ok(Curve::Ellipse {
                    position,
                    semi_axis_1: positive(num(p.get(2), "semi axis")? * k, "ellipse axis")?,
                    semi_axis_2: positive(num(p.get(3), "semi axis")? * k, "ellipse axis")?,
                })
            }
            "HYPERBOLA" => {
                let k = Self::conic_scale(sp)?;
                let position =
                    self.placement_in(reference(p.get(1), "hyperbola placement")?, sp)?;
                Ok(Curve::Hyperbola {
                    position,
                    semi_axis: positive(num(p.get(2), "semi axis")? * k, "hyperbola axis")?,
                    semi_imag_axis: positive(
                        num(p.get(3), "semi imaginary axis")? * k,
                        "hyperbola axis",
                    )?,
                })
            }
            "PARABOLA" => {
                let k = Self::conic_scale(sp)?;
                let position = self.placement_in(reference(p.get(1), "parabola placement")?, sp)?;
                let focal_dist = num(p.get(2), "focal distance")? * k;
                if focal_dist == 0.0 {
                    return Err("parabola focal distance must not be zero".into());
                }
                Ok(Curve::Parabola {
                    position,
                    focal_dist,
                })
            }
            "B_SPLINE_CURVE_WITH_KNOTS"
            | "BEZIER_CURVE"
            | "UNIFORM_CURVE"
            | "QUASI_UNIFORM_CURVE" => self.bspline_curve(e, sp),
            "POLYLINE" => {
                let ids = p
                    .get(1)
                    .and_then(Param::as_list)
                    .ok_or("POLYLINE without points")?;
                if ids.len() < 2 || ids.len() > MAX_CTRL {
                    return Err("polyline point count".into());
                }
                let pts = ids
                    .iter()
                    .map(|c| self.point_in(reference(Some(c), "polyline point")?, sp))
                    .collect::<R<_>>()?;
                Ok(Curve::Polyline(pts))
            }
            "TRIMMED_CURVE" => {
                let b = reference(p.get(1), "trimmed curve basis")?;
                let basis = if sp.dim == 3 {
                    self.curve_in(b, depth + 1)?
                } else {
                    Arc::new(self.read_curve(b, sp, depth + 1)?)
                };
                // A trimmed curve's parameters are its basis's: angles on a conic, also through
                // an offset, trimmed or surface curve of one; plain numbers otherwise.
                let kind = Self::curve_param_kind(&basis);
                let trim_1 = self.trims(p.get(2), kind, sp)?;
                let trim_2 = self.trims(p.get(3), kind, sp)?;
                let sense = boolean(p.get(4), "trimmed curve sense")?;
                let preference = match p.get(5).and_then(Param::as_enum) {
                    Some("CARTESIAN") => TrimPreference::Cartesian,
                    Some("PARAMETER") => TrimPreference::Parameter,
                    _ => TrimPreference::Unspecified,
                };
                Ok(Curve::Trimmed {
                    basis,
                    trim_1,
                    trim_2,
                    sense,
                    preference,
                })
            }
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE"
                if sp.dim == 3 =>
            {
                let kind = match e.name() {
                    "SEAM_CURVE" => SurfaceCurveKind::Seam,
                    "INTERSECTION_CURVE" => SurfaceCurveKind::Intersection,
                    "BOUNDED_SURFACE_CURVE" => SurfaceCurveKind::BoundedSurface,
                    _ => SurfaceCurveKind::Surface,
                };
                let curve_3d =
                    self.curve_in(reference(p.get(1), "surface curve 3D curve")?, depth + 1)?;
                let items = p
                    .get(2)
                    .and_then(Param::as_list)
                    .ok_or("surface curve without associated geometry")?;
                if items.len() > 2 {
                    return Err("surface curve with more than two associated items".into());
                }
                let mut associated = Vec::new();
                for item in items {
                    associated.push(
                        self.associated(reference(Some(item), "associated geometry")?, depth + 1)?,
                    );
                }
                let master = match p.get(3).and_then(Param::as_enum) {
                    Some("PCURVE_S1") => PreferredRep::PcurveS1,
                    Some("PCURVE_S2") => PreferredRep::PcurveS2,
                    _ => PreferredRep::Curve3d,
                };
                Ok(Curve::SurfaceCurve {
                    kind,
                    curve_3d,
                    associated,
                    master,
                })
            }
            "OFFSET_CURVE_3D" if sp.dim == 3 => {
                let basis = self.curve_in(reference(p.get(1), "offset curve basis")?, depth + 1)?;
                let distance = self.length(p.get(2), "offset distance")?;
                let self_intersect = logical(p.get(3), "offset curve self_intersect")?;
                let ref_direction =
                    self.direction(reference(p.get(4), "offset curve direction")?)?;
                Ok(Curve::OffsetCurve3d {
                    basis,
                    distance,
                    self_intersect,
                    ref_direction,
                })
            }
            n => Err(format!(
                "not supported yet: curve {n}{}",
                if sp.dim == 2 {
                    " in parameter space"
                } else {
                    ""
                }
            )),
        }
    }

    fn trims(&self, p: Option<&Param>, kind: ParamKind, sp: Space) -> R<Vec<Trim>> {
        let items = p
            .and_then(Param::as_list)
            .ok_or("trimmed curve without trims")?;
        if items.is_empty() || items.len() > 2 {
            return Err("trimmed curve trim count".into());
        }
        items
            .iter()
            .map(|t| match t {
                Param::Ref(r) => Ok(Trim::Point(self.point_in(*r, sp)?)),
                Param::Typed(name, v) if name == "PARAMETER_VALUE" => Ok(Trim::Parameter(
                    num(v.first(), "trim parameter")? * self.factor(kind),
                )),
                Param::Real(_) | Param::Int(_) => Ok(Trim::Parameter(
                    num(Some(t), "trim parameter")? * self.factor(kind),
                )),
                _ => Err("bad trim".into()),
            })
            .collect()
    }

    /// A `PCURVE`: its surface, and the one 2D curve of its `DEFINITIONAL_REPRESENTATION` in
    /// the surface's parameter units.
    pub fn pcurve(&mut self, id: u64) -> R<Pcurve> {
        self.pcurve_in(id, 0)
    }

    fn pcurve_in(&mut self, id: u64, depth: usize) -> R<Pcurve> {
        let e = entity(self.ex, id)?;
        if e.name() != "PCURVE" {
            return Err(format!("#{id}: expected PCURVE, found {}", e.name()));
        }
        let p = e.params();
        let surface = self.surface_in(reference(p.get(1), "pcurve surface")?, depth)?;
        let rep = entity(self.ex, reference(p.get(2), "pcurve representation")?)?;
        if rep.name() != "DEFINITIONAL_REPRESENTATION" {
            return Err(format!("pcurve representation is {}", rep.name()));
        }
        let items = rep
            .params()
            .get(1)
            .and_then(Param::as_list)
            .ok_or("pcurve representation without items")?;
        let [item] = items else {
            return Err("pcurve representation must hold one curve".into());
        };
        let kinds = Self::param_kinds(&surface);
        let curve = self.curve_2d(reference(Some(item), "pcurve curve")?, kinds, depth)?;
        Ok(Pcurve { surface, curve })
    }

    fn associated(&mut self, id: u64, depth: usize) -> R<Associated> {
        if entity(self.ex, id)?.name() == "PCURVE" {
            Ok(Associated::Pcurve(self.pcurve_in(id, depth)?))
        } else {
            Ok(Associated::Surface(self.surface_in(id, depth)?))
        }
    }

    /// How a surface's (u, v) parameters scale from file units. Recursion follows the
    /// surface's own basis and profile links, which reading bounded by [`MAX_GEOM_DEPTH`].
    fn param_kinds(s: &Surface) -> (ParamKind, ParamKind) {
        use ParamKind::{Angle, Length, Plain};
        match s {
            Surface::Plane { .. } => (Length, Length),
            Surface::Cylinder { .. } | Surface::Cone { .. } => (Angle, Length),
            Surface::Sphere { .. } | Surface::Torus { .. } | Surface::DegenerateTorus { .. } => {
                (Angle, Angle)
            }
            Surface::Revolution { profile, .. } => (Angle, Self::curve_param_kind(profile)),
            Surface::Extrusion { profile, .. } => (Self::curve_param_kind(profile), Plain),
            Surface::BSpline(_) => (Plain, Plain),
            Surface::Offset { basis, .. } | Surface::RectangularTrimmed { basis, .. } => {
                Self::param_kinds(basis)
            }
        }
    }

    /// How a curve's parameter scales from file units (bounded like [`Self::param_kinds`]).
    fn curve_param_kind(c: &Curve) -> ParamKind {
        match c {
            Curve::Circle { .. } | Curve::Ellipse { .. } => ParamKind::Angle,
            Curve::Trimmed { basis, .. } | Curve::OffsetCurve3d { basis, .. } => {
                Self::curve_param_kind(basis)
            }
            Curve::SurfaceCurve { curve_3d, .. } => Self::curve_param_kind(curve_3d),
            _ => ParamKind::Plain,
        }
    }

    fn bspline_curve(&mut self, e: &Entity, sp: Space) -> R<Curve> {
        let (base, with_knots) = bspline_params(e, "B_SPLINE_CURVE", "B_SPLINE_CURVE_WITH_KNOTS")?;
        let degree = degree(base.get(1))?;
        let ids = base
            .get(2)
            .and_then(Param::as_list)
            .ok_or("B-spline without control points")?;
        if ids.len() > MAX_CTRL {
            return Err("B-spline with too many control points".into());
        }
        let points: Vec<P3> = ids
            .iter()
            .map(|c| self.point_in(reference(Some(c), "control point")?, sp))
            .collect::<R<_>>()?;
        let (knots, multiplicities, knot_spec) = if let Some(k) = &with_knots {
            let (kv, m) = knots(k.first(), k.get(1), points.len(), degree)?;
            (kv, m, knot_spec(k.get(2)))
        } else {
            let spec = spec_of_type(e);
            let (kv, m) = implied_knots(spec, points.len(), degree)?;
            (kv, m, spec)
        };
        let weights = match e.record("RATIONAL_B_SPLINE_CURVE") {
            Some(r) => Some(weights(r.first(), points.len())?),
            None => None,
        };
        Ok(Curve::BSpline(Box::new(BSplineCurve {
            degree,
            points,
            weights,
            knots,
            multiplicities,
            form: curve_form(base.get(3)),
            closed: logical(base.get(4), "B-spline closed_curve").unwrap_or(Logical::Unknown),
            self_intersect: logical(base.get(5), "B-spline self_intersect")
                .unwrap_or(Logical::Unknown),
            knot_spec,
        })))
    }

    /// A surface entity.
    pub fn surface(&mut self, id: u64) -> R<Arc<Surface>> {
        self.surface_in(id, 0)
    }

    fn surface_in(&mut self, id: u64, depth: usize) -> R<Arc<Surface>> {
        if let Some(s) = self.surfaces.get(&id) {
            return Ok(s.clone());
        }
        if depth > MAX_GEOM_DEPTH {
            return Err("surface definitions nested too deeply".into());
        }
        let s = Arc::new(self.read_surface(id, depth)?);
        self.surfaces.insert(id, s.clone());
        Ok(s)
    }

    fn read_surface(&mut self, id: u64, depth: usize) -> R<Surface> {
        let e = entity(self.ex, id)?;
        if e.is_complex() {
            return self.bspline_surface(e);
        }
        let p = e.params();
        let position = |r: &Self| r.placement(reference(p.get(1), "surface placement")?);
        match e.name() {
            "PLANE" => Ok(Surface::Plane {
                position: position(self)?,
            }),
            "CYLINDRICAL_SURFACE" => Ok(Surface::Cylinder {
                position: position(self)?,
                radius: positive(self.length(p.get(2), "radius")?, "cylinder radius")?,
            }),
            "CONICAL_SURFACE" => {
                let radius = self.length(p.get(2), "radius")?;
                let semi_angle = self.angle(p.get(3), "semi angle")?;
                if radius < 0.0
                    || !(semi_angle.abs() > MIN_SEMI_ANGLE && semi_angle.abs() < MAX_SEMI_ANGLE)
                {
                    return Err(
                        "cone with a negative radius or a semi angle outside (0, 90°)".into(),
                    );
                }
                Ok(Surface::Cone {
                    position: position(self)?,
                    radius,
                    semi_angle,
                })
            }
            "SPHERICAL_SURFACE" => Ok(Surface::Sphere {
                position: position(self)?,
                radius: positive(self.length(p.get(2), "radius")?, "sphere radius")?,
            }),
            "TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => {
                let major_radius = self.length(p.get(2), "major radius")?;
                let minor_radius =
                    positive(self.length(p.get(3), "minor radius")?, "torus minor radius")?;
                // A negative major radius (outside the schema, which some writers use for a
                // spindle torus) is the same formula: its tube on the other side of the axis.
                if !major_radius.is_finite() {
                    return Err("torus major radius must be finite".into());
                }
                let position = position(self)?;
                if e.name() == "DEGENERATE_TOROIDAL_SURFACE" {
                    let select_outer = boolean(p.get(4), "degenerate torus select_outer")?;
                    Ok(Surface::DegenerateTorus {
                        position,
                        major_radius,
                        minor_radius,
                        select_outer,
                    })
                } else {
                    Ok(Surface::Torus {
                        position,
                        major_radius,
                        minor_radius,
                    })
                }
            }
            "B_SPLINE_SURFACE_WITH_KNOTS"
            | "BEZIER_SURFACE"
            | "UNIFORM_SURFACE"
            | "QUASI_UNIFORM_SURFACE" => self.bspline_surface(e),
            "SURFACE_OF_REVOLUTION" => {
                let profile =
                    self.curve_in(reference(p.get(1), "revolution profile")?, depth + 1)?;
                let axis = self.axis1(reference(p.get(2), "revolution axis")?)?;
                Ok(Surface::Revolution { profile, axis })
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let profile =
                    self.curve_in(reference(p.get(1), "extrusion profile")?, depth + 1)?;
                let vector = self.vector_in(
                    reference(p.get(2), "extrusion vector")?,
                    Space::model(self.units.len, 3),
                )?;
                Ok(Surface::Extrusion { profile, vector })
            }
            "OFFSET_SURFACE" => {
                let basis = self.surface_in(reference(p.get(1), "offset basis")?, depth + 1)?;
                let distance = self.length(p.get(2), "offset distance")?;
                let self_intersect = logical(p.get(3), "offset self_intersect")?;
                Ok(Surface::Offset {
                    basis,
                    distance,
                    self_intersect,
                })
            }
            "RECTANGULAR_TRIMMED_SURFACE" => {
                let basis =
                    self.surface_in(reference(p.get(1), "trimmed surface basis")?, depth + 1)?;
                let (ku, kv) = Self::param_kinds(&basis);
                let (fu, fv) = (self.factor(ku), self.factor(kv));
                Ok(Surface::RectangularTrimmed {
                    u1: num(p.get(2), "u1")? * fu,
                    u2: num(p.get(3), "u2")? * fu,
                    v1: num(p.get(4), "v1")? * fv,
                    v2: num(p.get(5), "v2")? * fv,
                    u_sense: boolean(p.get(6), "usense")?,
                    v_sense: boolean(p.get(7), "vsense")?,
                    basis,
                })
            }
            n => Err(format!("not supported yet: surface {n}")),
        }
    }

    fn bspline_surface(&mut self, e: &Entity) -> R<Surface> {
        let (base, with_knots) =
            bspline_params(e, "B_SPLINE_SURFACE", "B_SPLINE_SURFACE_WITH_KNOTS")?;
        let (u_degree, v_degree) = (degree(base.get(1))?, degree(base.get(2))?);
        let rows = base
            .get(3)
            .and_then(Param::as_list)
            .ok_or("B-spline surface without control points")?;
        let mut points: Vec<Vec<P3>> = Vec::new();
        let mut total = 0usize;
        for r in rows {
            let r = r.as_list().ok_or("bad control point row")?;
            total = total.saturating_add(r.len());
            if total > MAX_CTRL {
                return Err("B-spline surface with too many control points".into());
            }
            points.push(
                r.iter()
                    .map(|c| self.point(reference(Some(c), "control point")?))
                    .collect::<R<_>>()?,
            );
        }
        let nu = points.len();
        let nv = points.first().map_or(0, Vec::len);
        if nu == 0 || nv == 0 || points.iter().any(|r| r.len() != nv) {
            return Err("ragged B-spline control net".into());
        }
        let ((u_knots, u_multiplicities), (v_knots, v_multiplicities), knot_spec) =
            if let Some(k) = &with_knots {
                (
                    knots(k.first(), k.get(2), nu, u_degree)?,
                    knots(k.get(1), k.get(3), nv, v_degree)?,
                    knot_spec(k.get(4)),
                )
            } else {
                let spec = spec_of_type(e);
                (
                    implied_knots(spec, nu, u_degree)?,
                    implied_knots(spec, nv, v_degree)?,
                    spec,
                )
            };
        let weights = match e.record("RATIONAL_B_SPLINE_SURFACE") {
            Some(r) => {
                let w = r
                    .first()
                    .and_then(Param::as_list)
                    .ok_or("rational surface without weights")?;
                if w.len() != nu {
                    return Err("weights do not match the control net".into());
                }
                Some(
                    w.iter()
                        .map(|row| weights(Some(row), nv))
                        .collect::<R<_>>()?,
                )
            }
            None => None,
        };
        Ok(Surface::BSpline(Box::new(BSplineSurface {
            u_degree,
            v_degree,
            points,
            weights,
            u_knots,
            u_multiplicities,
            v_knots,
            v_multiplicities,
            form: surface_form(base.get(4)),
            u_closed: logical(base.get(5), "u_closed").unwrap_or(Logical::Unknown),
            v_closed: logical(base.get(6), "v_closed").unwrap_or(Logical::Unknown),
            self_intersect: logical(base.get(7), "self_intersect").unwrap_or(Logical::Unknown),
            knot_spec,
        })))
    }
}

#[cfg(test)]
mod tests;
