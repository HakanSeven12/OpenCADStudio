//! ISO 10303-21 writer for the neutral [`solidcore_record::step::Model`], and the mapper ([`map`])
//! that reads a Part 21 exchange ([`solidcore_record::step::p21`]) into it.
//!
//! The writer emits one `MANIFOLD_SOLID_BREP` per manifold body (one closed shell), or
//! a `BREP_WITH_VOIDS` for one with voids (its outer closed shell, and each void's
//! closed shell under an `ORIENTED_CLOSED_SHELL` of orientation `.F.`), and one
//! `FACETED_BREP` per faceted body, together with a complete AP203 product structure and
//! unit context. Unsupported geometry is rejected with an [`ExportError`] instead of
//! being silently approximated.
//!
//! # Shared entities
//!
//! Within a body every emitted `CARTESIAN_POINT` is shared: a vertex, a face placement
//! origin and a line origin at the same coordinates reference one entity, so a five
//! vertex pyramid contains five points rather than one per use. Face edges retain one
//! STEP edge identity, with a separate orientation per use.
//!
//! Faceted bodies additionally weld coincident vertices (in millimetres) before they
//! are written, and every `POLY_LOOP` references those welded points. A closed
//! tessellation therefore round-trips as a closed shell with one shared edge per pair
//! of faces, even when the input listed one vertex and edge per face. [`write_faceted`]
//! applies the same treatment to every body, so a tessellated body can be exported as
//! `FACETED_BREP` without first rebuilding its topology.
//!
//! A display tessellation is often open or seamed: its boundary edges are used once
//! and its patches may not join into a watertight shell. [`write_faceted`] rejects it
//! because a `FACETED_BREP` solid must be closed. [`write_faceted_display`] (and
//! [`write_faceted_display_with_metadata`]) writes the same welded `FACETED_BREP`
//! geometry without asserting closure: open boundaries and seams are written as given,
//! and a body's shells are flattened into the one shell that `FACETED_BREP` requires.
//! The shell is still named `CLOSED_SHELL`, because `FACETED_BREP`'s `outer` attribute
//! is typed `closed_shell`; the display writer never claims the mesh is watertight and
//! never discards or rewinds a face to make it so.
//!
//! # Metadata
//!
//! [`write()`] emits a neutral product named `part` with an AP203 schema. Callers that
//! need a product name, author, organization, schema, layer, colour or material use
//! [`write_with_metadata`] (or [`write_faceted_with_metadata`]), which emits those
//! entities directly; no post-write text rewriting is required.

pub mod geom;
pub mod map;
pub mod validate;

pub use map::{import, Limits};

#[cfg(test)]
mod import_tests;

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use solidcore_record::step::model::{
    Body, BodyKind, Bound, Curve, Frame, Model, PcurveKey, Surface,
};

/// Default distance, in millimetres, within which faceted vertices are welded.
pub const WELD_TOLERANCE_MM: f64 = 1e-6;

/// Why a model could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    /// Human-readable reason.
    pub message: String,
}

impl ExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportError {}

/// An application protocol the writer can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// AP203 `CONFIG_CONTROL_DESIGN`.
    Ap203,
    /// AP214 `AUTOMOTIVE_DESIGN`.
    Ap214,
    /// AP242 `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF`.
    Ap242,
}

impl Schema {
    /// The `FILE_SCHEMA` identifier.
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Ap203 => "CONFIG_CONTROL_DESIGN",
            Self::Ap214 => "AUTOMOTIVE_DESIGN",
            Self::Ap242 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF",
        }
    }

    /// The `APPLICATION_CONTEXT` description.
    pub const fn application_context(self) -> &'static str {
        match self {
            Self::Ap203 => "configuration controlled 3d designs of mechanical parts and assemblies",
            Self::Ap214 => "core data for automotive mechanical design processes",
            Self::Ap242 => "managed model based 3d engineering",
        }
    }

    /// The `APPLICATION_PROTOCOL_DEFINITION` protocol name.
    pub const fn protocol(self) -> &'static str {
        match self {
            Self::Ap203 => "config_control_design",
            Self::Ap214 => "automotive_design",
            Self::Ap242 => "ap242_managed_model_based_3d_engineering",
        }
    }

    /// The `APPLICATION_PROTOCOL_DEFINITION` year.
    pub const fn protocol_year(self) -> u64 {
        match self {
            Self::Ap203 => 1994,
            Self::Ap214 => 2000,
            Self::Ap242 => 2014,
        }
    }

    /// Parses the schema names accepted by `SolidNode` (`ap203`, `214`, …).
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ap203" | "203" | "config_control_design" => Some(Self::Ap203),
            "ap214" | "214" | "automotive_design" => Some(Self::Ap214),
            "ap242" | "242" | "ap242_managed_model_based_3d_engineering_mim_lf" => {
                Some(Self::Ap242)
            }
            _ => None,
        }
    }
}

/// Header and presentation metadata emitted with a model.
///
/// Every field is optional. The schema defaults to AP203 and the product to `part`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportMetadata {
    /// `FILE_NAME` name.
    pub file_name: Option<String>,
    /// `PRODUCT` name (and, unless [`Self::product_id`] is set, its id).
    pub product_name: Option<String>,
    /// `PRODUCT` id.
    pub product_id: Option<String>,
    /// `FILE_NAME` author.
    pub author: Option<String>,
    /// `FILE_NAME` organization.
    pub organization: Option<String>,
    /// Target application protocol.
    pub schema: Option<Schema>,
    /// `PRESENTATION_LAYER_ASSIGNMENT` layer name.
    pub layer: Option<String>,
    /// `COLOUR_RGB` presentation colour.
    pub color: Option<[f64; 3]>,
    /// `MATERIAL_DESIGNATION` name.
    pub material: Option<String>,
    /// OCS: each body's own layer and colour, by index into [`Model::bodies`]. A body
    /// without an entry (or with an empty field) takes [`Self::layer`] / [`Self::color`].
    pub body_styles: Vec<BodyStyle>,
}

/// OCS: one body's presentation layer and colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyStyle {
    /// `PRESENTATION_LAYER_ASSIGNMENT` layer name.
    pub layer: Option<String>,
    /// `COLOUR_RGB` presentation colour, components in `0..=1`.
    pub color: Option<[f64; 3]>,
}

impl ExportMetadata {
    /// Returns an empty metadata set (AP203, product `part`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the `FILE_NAME` name.
    #[must_use]
    pub fn with_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// Sets the product name.
    #[must_use]
    pub fn with_product_name(mut self, product_name: impl Into<String>) -> Self {
        self.product_name = Some(product_name.into());
        self
    }

    /// Sets the product id.
    #[must_use]
    pub fn with_product_id(mut self, product_id: impl Into<String>) -> Self {
        self.product_id = Some(product_id.into());
        self
    }

    /// Sets the `FILE_NAME` author.
    #[must_use]
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Sets the `FILE_NAME` organization.
    #[must_use]
    pub fn with_organization(mut self, organization: impl Into<String>) -> Self {
        self.organization = Some(organization.into());
        self
    }

    /// Sets the target schema.
    #[must_use]
    pub fn with_schema(mut self, schema: Schema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Sets the presentation layer.
    #[must_use]
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.layer = Some(layer.into());
        self
    }

    /// Sets the presentation colour.
    #[must_use]
    pub fn with_color(mut self, color: [f64; 3]) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the material designation.
    #[must_use]
    pub fn with_material(mut self, material: impl Into<String>) -> Self {
        self.material = Some(material.into());
        self
    }

    fn validate(&self) -> Result<(), ExportError> {
        let colours = self
            .color
            .iter()
            .chain(self.body_styles.iter().filter_map(|s| s.color.as_ref()));
        for color in colours {
            if color
                .iter()
                .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
            {
                return Err(ExportError::new(
                    "color components must be finite and between 0 and 1",
                ));
            }
        }
        Ok(())
    }
}

/// A Part 21 entity table with stable ids in insertion order.
struct Entities {
    next: u64,
    rows: Vec<(u64, String)>,
}

impl Entities {
    fn new() -> Self {
        Self {
            next: 1,
            rows: Vec::new(),
        }
    }

    fn add(&mut self, text: String) -> u64 {
        let id = self.next;
        self.next += 1;
        self.rows.push((id, text));
        id
    }
}

/// STEP string literal, with embedded quotes doubled.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for character in value.chars() {
        if character == '\'' {
            out.push('\'');
        }
        out.push(character);
    }
    out.push('\'');
    out
}

fn point(point: [f64; 3]) -> Result<String, ExportError> {
    Ok(format!(
        "CARTESIAN_POINT('',({},{},{}))",
        number(point[0])?,
        number(point[1])?,
        number(point[2])?
    ))
}

fn direction(direction: [f64; 3]) -> Result<String, ExportError> {
    Ok(format!(
        "DIRECTION('',({},{},{}))",
        number(direction[0])?,
        number(direction[1])?,
        number(direction[2])?
    ))
}

fn number(value: f64) -> Result<String, ExportError> {
    if value == 0.0 {
        return Ok("0.".to_string());
    }
    if !value.is_finite() {
        return Err(ExportError::new("non-finite number in STEP export"));
    }
    // Part 21 REAL literals need a decimal point. Debug prints the shortest
    // round-tripping representation (and scientific notation for extremes).
    let text = format!("{value:?}");
    Ok(match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let mantissa = if mantissa.contains('.') {
                mantissa.to_string()
            } else {
                format!("{mantissa}.")
            };
            format!("{mantissa}E{exponent}")
        }
        None if text.contains('.') => text,
        None => format!("{text}."),
    })
}

/// Per-body `CARTESIAN_POINT` cache: equal coordinates share one entity.
#[derive(Default)]
struct PointIds {
    ids: BTreeMap<[u64; 3], u64>,
}

impl PointIds {
    fn shared(&mut self, entities: &mut Entities, position: [f64; 3]) -> Result<u64, ExportError> {
        let key = [
            canonical_bits(position[0]),
            canonical_bits(position[1]),
            canonical_bits(position[2]),
        ];
        if let Some(&id) = self.ids.get(&key) {
            return Ok(id);
        }
        let id = entities.add(point(position)?);
        self.ids.insert(key, id);
        Ok(id)
    }
}

/// Canonical key bits: distinguishes values exactly but treats `-0.0` as `0.0`.
fn canonical_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

fn frame_placement(
    entities: &mut Entities,
    points: &mut PointIds,
    frame: &Frame,
) -> Result<u64, ExportError> {
    let origin = points.shared(entities, frame.origin)?;
    let axis = entities.add(direction(frame.z)?);
    let ref_direction = entities.add(direction(frame.x)?);
    Ok(entities.add(format!(
        "AXIS2_PLACEMENT_3D('',#{origin},#{axis},#{ref_direction})"
    )))
}

fn write_surface(
    entities: &mut Entities,
    points: &mut PointIds,
    surface: &Surface,
) -> Result<u64, ExportError> {
    let placement = |entities: &mut Entities,
                     points: &mut PointIds,
                     frame: &Frame|
     -> Result<u64, ExportError> { frame_placement(entities, points, frame) };
    match surface {
        Surface::Plane { frame } => {
            let p = placement(entities, points, frame)?;
            Ok(entities.add(format!("PLANE('',#{p})")))
        }
        Surface::Cylinder { frame, radius } => {
            let p = placement(entities, points, frame)?;
            Ok(entities.add(format!("CYLINDRICAL_SURFACE('',#{p},{})", number(*radius)?)))
        }
        Surface::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => {
            let p = placement(entities, points, frame)?;
            if semi_angle_deg.abs() < 1.0e-12 {
                Ok(entities.add(format!("CYLINDRICAL_SURFACE('',#{p},{})", number(*radius)?)))
            } else {
                Ok(entities.add(format!(
                    "CONICAL_SURFACE('',#{p},{},{})",
                    number(*radius)?,
                    number(semi_angle_deg.to_radians())?
                )))
            }
        }
        Surface::Sphere { frame, radius } => {
            let p = placement(entities, points, frame)?;
            Ok(entities.add(format!("SPHERICAL_SURFACE('',#{p},{})", number(*radius)?)))
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let p = placement(entities, points, frame)?;
            Ok(entities.add(format!(
                "TOROIDAL_SURFACE('',#{p},{},{})",
                number(*major)?,
                number(*minor)?
            )))
        }
        Surface::Revolution {
            curve,
            axis_origin,
            axis_dir,
        } => {
            let curve = write_curve(entities, points, curve)?;
            let origin = points.shared(entities, *axis_origin)?;
            let axis = entities.add(direction(*axis_dir)?);
            let axis = entities.add(format!("AXIS1_PLACEMENT('',#{origin},#{axis})"));
            Ok(entities.add(format!("SURFACE_OF_REVOLUTION('',#{curve},#{axis})")))
        }
        Surface::Extrusion {
            curve,
            dir,
            magnitude,
        } => {
            let curve = write_curve(entities, points, curve)?;
            let direction_id = entities.add(direction(*dir)?);
            let vector = entities.add(format!(
                "VECTOR('',#{direction_id},{})",
                number(*magnitude)?
            ));
            Ok(entities.add(format!(
                "SURFACE_OF_LINEAR_EXTRUSION('',#{curve},#{vector})"
            )))
        }
        Surface::BSpline(surface) => write_bspline_surface(entities, points, surface),
    }
}

fn write_bspline_surface(
    entities: &mut Entities,
    points: &mut PointIds,
    surface: &solidcore_record::step::model::BSplineSurface,
) -> Result<u64, ExportError> {
    let mut ids = Vec::with_capacity(surface.points.len());
    for row in &surface.points {
        let mut row_ids = Vec::with_capacity(row.len());
        for &point in row {
            row_ids.push(points.shared(entities, point)?);
        }
        ids.push(row_ids);
    }
    let list = ids
        .iter()
        .map(|row| {
            format!(
                "({})",
                row.iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let umults = surface
        .u_mults
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let vmults = surface
        .v_mults
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let uknots = surface
        .u_knots
        .iter()
        .map(|v| number(*v))
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let vknots = surface
        .v_knots
        .iter()
        .map(|v| number(*v))
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let text = if let Some(weights) = &surface.weights {
        let weights = weights
            .iter()
            .map(|row| {
                format!(
                    "({})",
                    row.iter()
                        .map(|v| number(*v))
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap_or_default()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "(\n BOUNDED_SURFACE()\n B_SPLINE_SURFACE({},{},({list}),.UNSPECIFIED.,.U.,.U.,.U.)\n B_SPLINE_SURFACE_WITH_KNOTS(({umults}),({vmults}),({uknots}),({vknots}),.UNSPECIFIED.)\n GEOMETRIC_REPRESENTATION_ITEM()\n RATIONAL_B_SPLINE_SURFACE(({weights}))\n REPRESENTATION_ITEM('')\n SURFACE()\n)",
            surface.u_degree, surface.v_degree
        )
    } else {
        format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',{},{},({list}),.UNSPECIFIED.,.U.,.U.,.U.,({umults}),({vmults}),({uknots}),({vknots}),.UNSPECIFIED.)",
            surface.u_degree, surface.v_degree
        )
    };
    Ok(entities.add(text))
}

fn write_curve(
    entities: &mut Entities,
    points: &mut PointIds,
    curve: &Curve,
) -> Result<u64, ExportError> {
    match curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            let origin = points.shared(entities, *origin)?;
            let direction_id = entities.add(direction(*dir)?);
            let vector = entities.add(format!(
                "VECTOR('',#{direction_id},{})",
                number(*magnitude)?
            ));
            Ok(entities.add(format!("LINE('',#{origin},#{vector})")))
        }
        Curve::Circle { frame, radius } => {
            let placement = frame_placement(entities, points, frame)?;
            Ok(entities.add(format!("CIRCLE('',#{placement},{})", number(*radius)?)))
        }
        Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => {
            let placement = frame_placement(entities, points, frame)?;
            Ok(entities.add(format!(
                "ELLIPSE('',#{placement},{},{})",
                number(*semi1)?,
                number(*semi2)?
            )))
        }
        Curve::BSpline(curve) => write_bspline_curve(entities, points, curve),
    }
}

fn write_bspline_curve(
    entities: &mut Entities,
    points: &mut PointIds,
    curve: &solidcore_record::step::model::BSplineCurve,
) -> Result<u64, ExportError> {
    let ids = curve
        .points
        .iter()
        .map(|&point| points.shared(entities, point))
        .collect::<Result<Vec<_>, _>>()?;
    let list = ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    let mults = curve
        .mults
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let knots = curve
        .knots
        .iter()
        .map(|v| number(*v))
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let text = if let Some(weights) = &curve.weights {
        let weights = weights
            .iter()
            .map(|v| number(*v))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        format!(
            "(\n BOUNDED_CURVE()\n B_SPLINE_CURVE({},({list}),.UNSPECIFIED.,.U.,.U.)\n B_SPLINE_CURVE_WITH_KNOTS(({mults}),({knots}),.UNSPECIFIED.)\n CURVE()\n GEOMETRIC_REPRESENTATION_ITEM()\n RATIONAL_B_SPLINE_CURVE(({weights}))\n REPRESENTATION_ITEM('')\n)",
            curve.degree
        )
    } else {
        format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',{},({list}),.UNSPECIFIED.,.U.,.U.,({mults}),({knots}),.UNSPECIFIED.)",
            curve.degree
        )
    };
    Ok(entities.add(text))
}

fn point2(entities: &mut Entities, uv: [f64; 2]) -> Result<u64, ExportError> {
    Ok(entities.add(format!(
        "CARTESIAN_POINT('',({},{}))",
        number(uv[0])?,
        number(uv[1])?
    )))
}

fn write_pcurve_edge(
    entities: &mut Entities,
    points: &mut PointIds,
    body: &Body,
    index: usize,
    vertex_ids: &[u64],
    context_2d: u64,
    pcurves: &[(u64, Vec<[f64; 2]>)],
) -> Result<u64, ExportError> {
    let edge = body.edges.get(index).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} references missing edge {index}",
            body.step_id
        ))
    })?;
    let start = *vertex_ids.get(edge.start).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing start vertex",
            body.step_id
        ))
    })?;
    let end = *vertex_ids.get(edge.end).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing end vertex",
            body.step_id
        ))
    })?;
    let curve_3d = write_curve(entities, points, &edge.curve)?;
    let mut pcurve_ids = Vec::with_capacity(pcurves.len());
    for (surface_id, uv) in pcurves {
        let mut uv_ids = Vec::with_capacity(uv.len());
        for point in uv {
            uv_ids.push(point2(entities, *point)?);
        }
        let count = uv.len();
        let point_refs = uv_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut knots: Vec<f64> = vec![0.0];
        for index in 1..count.saturating_sub(1) {
            knots.push(index as f64);
        }
        knots.push((count.saturating_sub(1)) as f64);
        let mut multi = vec![2u32];
        let interior = count.saturating_sub(2);
        multi.resize(multi.len() + interior, 1);
        multi.push(2);
        let knots_text = knots
            .iter()
            .map(|v| number(*v))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        let multi_text = multi
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let pcurve_curve = entities.add(format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',1,({point_refs}),.UNSPECIFIED.,.U.,.U.,({multi_text}),({knots_text}),.UNSPECIFIED.)"
        ));
        let representation = entities.add(format!(
            "DEFINITIONAL_REPRESENTATION('',(#{pcurve_curve}),#{context_2d})"
        ));
        pcurve_ids.push(entities.add(format!("PCURVE('',#{surface_id},#{representation})")));
    }
    let refs = pcurve_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    let surfaces: std::collections::BTreeSet<u64> =
        pcurves.iter().map(|(surface, _)| *surface).collect();
    let kind = if pcurve_ids.len() > 1 && surfaces.len() == 1 {
        "SEAM_CURVE"
    } else {
        "SURFACE_CURVE"
    };
    let surface_curve = entities.add(format!("{kind}('',#{curve_3d},({refs}),.PCURVE_S1.)"));
    let sense = if edge.same_sense { ".T." } else { ".F." };
    Ok(entities.add(format!(
        "EDGE_CURVE('',#{start},#{end},#{surface_curve},{sense})"
    )))
}

type EdgePcurves = BTreeMap<usize, Vec<(u64, Vec<[f64; 2]>)>>;

/// A solid body's shells: the outer `CLOSED_SHELL` first, then each void's
/// `ORIENTED_CLOSED_SHELL` (orientation `.F.`). A `MANIFOLD_SOLID_BREP` has exactly one
/// shell, the outer; a `BREP_WITH_VOIDS` has the outer and at least one void. Faces,
/// edges and vertices are written once for the body and shared by its shells.
fn solid_shells(
    entities: &mut Entities,
    body: &Body,
    pcurves: &BTreeMap<PcurveKey, Vec<Vec<[f64; 2]>>>,
) -> Result<Vec<u64>, ExportError> {
    // OCS: a shell model's shells are written here too, open or closed, each as
    // given (no outer/void roles).
    let surface_model = body.kind == BodyKind::ShellModel;
    let voids_allowed = body.kind == BodyKind::SolidWithVoids;
    let Some((outer, voids)) = body.shells.split_first() else {
        return Err(ExportError::new(format!(
            "body #{} has no shells",
            body.step_id
        )));
    };
    if !surface_model && voids_allowed == voids.is_empty() {
        return Err(ExportError::new(format!(
            "body #{} is {:?} with {} shells; a MANIFOLD_SOLID_BREP has one closed shell, a \
             BREP_WITH_VOIDS an outer shell and its voids",
            body.step_id,
            body.kind,
            body.shells.len()
        )));
    }
    if !surface_model && (!outer.orientation || voids.iter().any(|void| void.orientation)) {
        return Err(ExportError::new(format!(
            "body #{}: the outer shell must keep its orientation and every void be reversed",
            body.step_id
        )));
    }
    if !surface_model && body.shells.iter().any(|shell| !shell.closed) {
        return Err(ExportError::new(format!(
            "body #{} has an open shell; step-export writes closed solids",
            body.step_id
        )));
    }
    let mut points = PointIds::default();
    let vertex_ids = body
        .vertices
        .iter()
        .map(|vertex| {
            let point_id = points.shared(entities, vertex.point)?;
            Ok(entities.add(format!("VERTEX_POINT('',#{point_id})")))
        })
        .collect::<Result<Vec<_>, ExportError>>()?;
    let context_2d = entities.add("PARAMETRIC_REPRESENTATION_CONTEXT('','2')".to_string());
    // Emit every surface first so edge pcurves can reference their basis surfaces.
    let mut face_surfaces = Vec::new();
    for face in &body.faces {
        face_surfaces.push(write_surface(entities, &mut points, &face.surface)?);
    }
    let surface_by_step = body
        .faces
        .iter()
        .zip(&face_surfaces)
        .map(|(face, surface)| (face.step_id, *surface))
        .collect::<BTreeMap<_, _>>();
    // Group pcurves by BRep edge, not by edge use.
    let mut edge_pcurves: EdgePcurves = BTreeMap::new();
    for face in &body.faces {
        let Some(surface) = surface_by_step.get(&face.step_id).copied() else {
            continue;
        };
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            let Bound::Edges(edges) = &loop_.bound else {
                continue;
            };
            for (use_index, oriented_edge) in edges.iter().enumerate() {
                if let Some(curves) = pcurves.get(&(face.step_id, loop_index, use_index)) {
                    for uv in curves {
                        edge_pcurves
                            .entry(oriented_edge.edge)
                            .or_default()
                            .push((surface, uv.clone()));
                    }
                }
            }
        }
    }
    let mut edge_ids = vec![None; body.edges.len()];
    for (index, slot) in edge_ids.iter_mut().enumerate() {
        if let Some(curves) = edge_pcurves.get(&index) {
            *slot = Some(write_pcurve_edge(
                entities,
                &mut points,
                body,
                index,
                &vertex_ids,
                context_2d,
                curves,
            )?);
        } else {
            *slot = Some(write_edge(entities, &mut points, body, index, &vertex_ids)?);
        }
    }
    let mut shell_ids = Vec::new();
    for step_shell in &body.shells {
        let mut face_ids = Vec::new();
        for &face_index in &step_shell.faces {
            let face = body.faces.get(face_index).ok_or_else(|| {
                ExportError::new(format!(
                    "body #{} references missing face {face_index}",
                    body.step_id
                ))
            })?;
            let surface_id = *face_surfaces
                .get(
                    body.faces
                        .iter()
                        .position(|item| item.step_id == face.step_id)
                        .unwrap_or(0),
                )
                .ok_or_else(|| ExportError::new("missing face surface"))?;
            let mut bounds = Vec::new();
            for loop_ in &face.loops {
                let Bound::Edges(edges) = &loop_.bound else {
                    return Err(ExportError::new(format!(
                        "face #{} has a vertex loop; step-export needs edge loops",
                        face.step_id
                    )));
                };
                let mut oriented = Vec::new();
                for oriented_edge in edges {
                    let edge_id = edge_ids
                        .get(oriented_edge.edge)
                        .and_then(|id| *id)
                        .ok_or_else(|| {
                            ExportError::new(format!("missing edge index {}", oriented_edge.edge))
                        })?;
                    let orientation = if oriented_edge.forward { ".T." } else { ".F." };
                    oriented.push(
                        entities.add(format!("ORIENTED_EDGE('',*,*,#{edge_id},{orientation})")),
                    );
                }
                let loop_id = entities.add(format!(
                    "EDGE_LOOP('',({}))",
                    oriented
                        .iter()
                        .map(|id| format!("#{id}"))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                let orientation = if loop_.orientation { ".T." } else { ".F." };
                let kind = if loop_.outer {
                    "FACE_OUTER_BOUND"
                } else {
                    "FACE_BOUND"
                };
                bounds.push(entities.add(format!("{kind}('',#{loop_id},{orientation})")));
            }
            let bound_refs = bounds
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(",");
            let sense = if face.same_sense { ".T." } else { ".F." };
            face_ids.push(entities.add(format!(
                "ADVANCED_FACE('',({bound_refs}),#{surface_id},{sense})"
            )));
        }
        let refs = face_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        let (shell_kind, oriented_kind) = if step_shell.closed {
            ("CLOSED_SHELL", "ORIENTED_CLOSED_SHELL")
        } else {
            ("OPEN_SHELL", "ORIENTED_OPEN_SHELL")
        };
        let closed = entities.add(format!("{shell_kind}('',({refs}))"));
        shell_ids.push(if step_shell.orientation {
            closed
        } else {
            entities.add(format!("{oriented_kind}('',*,#{closed},.F.)"))
        });
    }
    Ok(shell_ids)
}

fn write_edge(
    entities: &mut Entities,
    points: &mut PointIds,
    body: &Body,
    index: usize,
    vertex_ids: &[u64],
) -> Result<u64, ExportError> {
    let edge = body.edges.get(index).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} references missing edge {index}",
            body.step_id
        ))
    })?;
    let start = *vertex_ids.get(edge.start).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing start vertex",
            body.step_id
        ))
    })?;
    let end = *vertex_ids.get(edge.end).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing end vertex",
            body.step_id
        ))
    })?;
    let curve_id = write_curve(entities, points, &edge.curve)?;
    let sense = if edge.same_sense { ".T." } else { ".F." };
    Ok(entities.add(format!(
        "EDGE_CURVE('',#{start},#{end},#{curve_id},{sense})"
    )))
}

/// A welded faceted vertex set: the unique points and the old-to-new remap.
struct Welded {
    points: Vec<[f64; 3]>,
    remap: Vec<usize>,
}

/// Welds vertices whose positions are within [`WELD_TOLERANCE_MM`].
///
/// The search is a deterministic spatial hash: a point only compares against
/// representatives in its own or the 26 neighbouring tolerance-sized cells.
fn weld_vertices(body: &Body) -> Welded {
    let tolerance = WELD_TOLERANCE_MM;
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut cells: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    let mut remap = Vec::with_capacity(body.vertices.len());
    for vertex in &body.vertices {
        let cell = [
            (vertex.point[0] / tolerance).floor() as i64,
            (vertex.point[1] / tolerance).floor() as i64,
            (vertex.point[2] / tolerance).floor() as i64,
        ];
        let mut found = None;
        'search: for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    let key = [
                        cell[0].saturating_add(dx),
                        cell[1].saturating_add(dy),
                        cell[2].saturating_add(dz),
                    ];
                    if let Some(candidates) = cells.get(&key) {
                        for &index in candidates {
                            let Some(candidate) = points.get(index) else {
                                continue;
                            };
                            let dx = vertex.point[0] - candidate[0];
                            let dy = vertex.point[1] - candidate[1];
                            let dz = vertex.point[2] - candidate[2];
                            if dx * dx + dy * dy + dz * dz <= tolerance * tolerance {
                                found = Some(index);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        remap.push(found.unwrap_or_else(|| {
            let index = points.len();
            points.push(vertex.point);
            cells.entry(cell).or_default().push(index);
            index
        }));
    }
    Welded { points, remap }
}

/// One faceted loop, already welded and expressed as point indices.
struct FacetedLoop {
    indices: Vec<usize>,
    outer: bool,
    orientation: bool,
}

/// How strictly a faceted body must be a closed two-manifold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FacetedMode {
    /// Require exactly one closed shell, with every undirected edge used twice.
    Strict,
    /// Accept a display tessellation whose shell is open or seamed.
    Display,
}

impl FacetedMode {
    /// Whether the mode enforces a closed solid.
    const fn is_strict(self) -> bool {
        matches!(self, Self::Strict)
    }
}

/// Writes a faceted body as `FACETED_BREP` with shared `POLY_LOOP` points.
///
/// [`FacetedMode::Strict`] keeps the watertight-solid contract. [`FacetedMode::Display`]
/// accepts open or seamed shells and flattens every shell of the body into the single
/// shell that `FACETED_BREP` requires.
fn faceted_shell(
    entities: &mut Entities,
    body: &Body,
    mode: FacetedMode,
) -> Result<u64, ExportError> {
    if body.shells.is_empty() {
        return Err(ExportError::new(format!(
            "body #{} has no shells",
            body.step_id
        )));
    }
    if mode.is_strict() && body.shells.len() != 1 {
        return Err(ExportError::new(format!(
            "body #{} has {} shells; step-export supports one closed faceted shell",
            body.step_id,
            body.shells.len()
        )));
    }
    let mut face_indices: Vec<usize> = Vec::new();
    for step_shell in &body.shells {
        if mode.is_strict() && !step_shell.closed {
            return Err(ExportError::new(format!(
                "body #{} has an open shell; step-export writes a closed faceted shell",
                body.step_id
            )));
        }
        face_indices.extend(step_shell.faces.iter().copied());
    }
    let welded = weld_vertices(body);
    let mut faces: Vec<Vec<FacetedLoop>> = Vec::with_capacity(face_indices.len());
    for face_index in face_indices {
        let face = body.faces.get(face_index).ok_or_else(|| {
            ExportError::new(format!(
                "body #{} references missing face {face_index}",
                body.step_id
            ))
        })?;
        let mut loops = Vec::with_capacity(face.loops.len());
        for loop_ in &face.loops {
            let Bound::Edges(edges) = &loop_.bound else {
                return Err(ExportError::new(format!(
                    "face #{} has a vertex loop; a faceted shell needs polygonal loops",
                    face.step_id
                )));
            };
            let mut indices: Vec<usize> = Vec::with_capacity(edges.len());
            for oriented_edge in edges {
                let edge = body.edges.get(oriented_edge.edge).ok_or_else(|| {
                    ExportError::new(format!(
                        "face #{} references missing edge {}",
                        face.step_id, oriented_edge.edge
                    ))
                })?;
                let start = if oriented_edge.forward {
                    edge.start
                } else {
                    edge.end
                };
                let welded_index = *welded.remap.get(start).ok_or_else(|| {
                    ExportError::new(format!(
                        "face #{} references missing vertex {start}",
                        face.step_id
                    ))
                })?;
                if indices.last().copied() != Some(welded_index) {
                    indices.push(welded_index);
                }
            }
            if indices.len() > 1 && indices.first() == indices.last() {
                indices.pop();
            }
            if indices.len() < 3 {
                return Err(ExportError::new(format!(
                    "face #{} has a loop with fewer than three distinct vertices",
                    face.step_id
                )));
            }
            loops.push(FacetedLoop {
                indices,
                outer: loop_.outer,
                orientation: loop_.orientation,
            });
        }
        faces.push(loops);
    }

    if mode.is_strict() {
        // A FACETED_BREP solid is a closed shell: every undirected edge must be
        // used twice. Display output deliberately skips this check.
        let mut uses: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for loops in &faces {
            for loop_ in loops {
                let len = loop_.indices.len();
                for (index, &a) in loop_.indices.iter().enumerate() {
                    let b = loop_.indices.get((index + 1) % len).copied().unwrap_or(a);
                    if a == b {
                        continue;
                    }
                    let key = if a < b { (a, b) } else { (b, a) };
                    *uses.entry(key).or_insert(0) += 1;
                }
            }
        }
        for ((a, b), count) in &uses {
            if *count != 2 {
                return Err(ExportError::new(format!(
                    "faceted shell is not closed: edge ({a}, {b}) is used {count} times, expected 2"
                )));
            }
        }
    }

    let mut point_ids = Vec::with_capacity(welded.points.len());
    for position in &welded.points {
        point_ids.push(entities.add(point(*position)?));
    }
    let mut face_ids = Vec::with_capacity(faces.len());
    for loops in &faces {
        let mut bounds = Vec::with_capacity(loops.len());
        for loop_ in loops {
            let refs = loop_
                .indices
                .iter()
                .map(|index| {
                    point_ids
                        .get(*index)
                        .map(|id| format!("#{id}"))
                        .ok_or_else(|| ExportError::new(format!("missing welded point {index}")))
                })
                .collect::<Result<Vec<_>, ExportError>>()?
                .join(",");
            let loop_id = entities.add(format!("POLY_LOOP('',({refs}))"));
            let orientation = if loop_.orientation { ".T." } else { ".F." };
            let kind = if loop_.outer {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(entities.add(format!("{kind}('',#{loop_id},{orientation})")));
        }
        let refs = bounds
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        face_ids.push(entities.add(format!("FACE('',({refs}))")));
    }
    let refs = face_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    // `FACETED_BREP.outer` is typed `closed_shell`, so even display output names
    // the shell `CLOSED_SHELL`. A display tessellation may be open or seamed,
    // which that entity type cannot express; it still reads as one faceted body.
    let shell_id = entities.add(format!("CLOSED_SHELL('',({refs}))"));
    let name = body.name.as_deref().unwrap_or("");
    Ok(entities.add(format!("FACETED_BREP({},#{shell_id})", quoted(name))))
}

/// Writes a model as an AP203 Part 21 exchange file with default metadata.
///
/// # Errors
///
/// A body containing anything outside the supported manifold/faceted subset, a
/// non-closed faceted shell, or a non-finite number in an emitted STEP value.
pub fn write(model: &Model) -> Result<String, ExportError> {
    write_inner(model, &ExportMetadata::default(), WriteMode::Auto)
}

/// Writes a model with explicit header, product and presentation metadata.
///
/// # Errors
///
/// The same conditions as [`write()`], plus a colour component outside `0..=1`.
pub fn write_with_metadata(
    model: &Model,
    metadata: &ExportMetadata,
) -> Result<String, ExportError> {
    write_inner(model, metadata, WriteMode::Auto)
}

/// Writes every body as a closed `FACETED_BREP`, ignoring face surfaces and body kind.
///
/// Use this for a tessellated body whose inputs are triangles or polygons: coincident
/// vertices are welded and shared `POLY_LOOP` points give every edge one identity.
///
/// # Errors
///
/// A body without exactly one closed shell, a shell that is not closed after welding,
/// or a non-finite number in an emitted STEP value.
pub fn write_faceted(model: &Model) -> Result<String, ExportError> {
    write_inner(model, &ExportMetadata::default(), WriteMode::Faceted)
}

/// [`write_faceted`] with explicit header, product and presentation metadata.
///
/// # Errors
///
/// The same conditions as [`write_faceted`], plus a colour component outside `0..=1`.
pub fn write_faceted_with_metadata(
    model: &Model,
    metadata: &ExportMetadata,
) -> Result<String, ExportError> {
    write_inner(model, metadata, WriteMode::Faceted)
}

/// Writes every body as a welded `FACETED_BREP` display mesh.
///
/// This is [`write_faceted`] for a tessellation that is only for display: faces are
/// still welded and share `POLY_LOOP` points, but the shell need not be closed. Open
/// boundaries and seams are written as given, and all shells of a body are flattened
/// into the single shell a `FACETED_BREP` requires. Nothing is dropped or rewound to
/// make the mesh watertight.
///
/// The shell is still emitted as `CLOSED_SHELL` because `FACETED_BREP`'s `outer`
/// attribute is typed `closed_shell`; use [`write_faceted`] when the output must be a
/// closed solid.
///
/// # Errors
///
/// A body without shells, a face loop with fewer than three distinct vertices after
/// welding, or a non-finite number in an emitted STEP value.
pub fn write_faceted_display(model: &Model) -> Result<String, ExportError> {
    write_inner(model, &ExportMetadata::default(), WriteMode::FacetedDisplay)
}

/// [`write_faceted_display`] with explicit header, product and presentation metadata.
///
/// # Errors
///
/// The same conditions as [`write_faceted_display`], plus a colour component outside
/// `0..=1`.
pub fn write_faceted_display_with_metadata(
    model: &Model,
    metadata: &ExportMetadata,
) -> Result<String, ExportError> {
    write_inner(model, metadata, WriteMode::FacetedDisplay)
}

/// How [`write_inner`] chooses a representation for each body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteMode {
    /// Follow [`Body::kind`].
    Auto,
    /// Force every body through the strict closed faceted writer.
    Faceted,
    /// Force every body through the display faceted writer.
    FacetedDisplay,
}

fn write_inner(
    model: &Model,
    metadata: &ExportMetadata,
    mode: WriteMode,
) -> Result<String, ExportError> {
    metadata.validate()?;
    let schema = metadata.schema.unwrap_or(Schema::Ap203);
    let product_name = metadata.product_name.as_deref().unwrap_or("part");
    let product_id = metadata.product_id.as_deref().unwrap_or(product_name);

    let mut entities = Entities::new();
    let application = entities.add(format!(
        "APPLICATION_CONTEXT({})",
        quoted(schema.application_context())
    ));
    entities.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard',{},{},#{application})",
        quoted(schema.protocol()),
        schema.protocol_year()
    ));
    let product_context = entities.add(format!("PRODUCT_CONTEXT('',#{application},'mechanical')"));
    let product = entities.add(format!(
        "PRODUCT({},{},'',(#{product_context}))",
        quoted(product_id),
        quoted(product_name)
    ));
    entities.add(format!(
        "PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}))"
    ));
    let formation = entities.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let definition_context = entities.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{application},'design')"
    ));
    let definition = entities.add(format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"
    ));

    let unit = entities.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into());
    let angle = entities.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".into());
    let solid_angle =
        entities.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())".into());
    let uncertainty = entities.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#{unit},'distance_accuracy_value','')"
    ));
    let context = entities.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) \
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit},#{angle},#{solid_angle})) REPRESENTATION_CONTEXT('',''))"
    ));

    let mut manifold_ids = Vec::new();
    let mut faceted_ids = Vec::new();
    // OCS: shell models, and each body's representation item in body order.
    let mut surface_ids = Vec::new();
    let mut body_items = Vec::new();
    for body in &model.bodies {
        let items_before = manifold_ids.len() + faceted_ids.len() + surface_ids.len();
        match mode {
            WriteMode::Auto => match body.kind {
                BodyKind::ManifoldSolid | BodyKind::SolidWithVoids => {
                    let shells = solid_shells(&mut entities, body, &model.pcurves)?;
                    let name = quoted(body.name.as_deref().unwrap_or(""));
                    let item = match shells.split_first() {
                        Some((outer, [])) => format!("MANIFOLD_SOLID_BREP({name},#{outer})"),
                        Some((outer, voids)) => format!(
                            "BREP_WITH_VOIDS({name},#{outer},({}))",
                            voids
                                .iter()
                                .map(|id| format!("#{id}"))
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        None => {
                            return Err(ExportError::new(format!(
                                "body #{} has no shells",
                                body.step_id
                            )))
                        }
                    };
                    manifold_ids.push(entities.add(item));
                }
                BodyKind::Faceted => {
                    faceted_ids.push(faceted_shell(&mut entities, body, FacetedMode::Strict)?);
                }
                BodyKind::ShellModel => {
                    let shells = solid_shells(&mut entities, body, &model.pcurves)?;
                    let name = quoted(body.name.as_deref().unwrap_or(""));
                    surface_ids.push(entities.add(format!(
                        "SHELL_BASED_SURFACE_MODEL({name},({}))",
                        refs(&shells)
                    )));
                }
            },
            WriteMode::Faceted => {
                faceted_ids.push(faceted_shell(&mut entities, body, FacetedMode::Strict)?);
            }
            WriteMode::FacetedDisplay => {
                faceted_ids.push(faceted_shell(&mut entities, body, FacetedMode::Display)?);
            }
        }
        if manifold_ids.len() + faceted_ids.len() + surface_ids.len() > items_before {
            let item = [&manifold_ids, &faceted_ids, &surface_ids]
                .into_iter()
                .filter_map(|ids| ids.last().copied())
                .max();
            body_items.extend(item);
        }
    }
    if manifold_ids.is_empty() && faceted_ids.is_empty() && surface_ids.is_empty() {
        return Err(ExportError::new("model has no bodies"));
    }

    let representation_name = metadata.product_name.as_deref().unwrap_or("");
    let mut representations = Vec::new();
    if !manifold_ids.is_empty() {
        representations.push(entities.add(format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION({},({}),#{context})",
            quoted(representation_name),
            refs(&manifold_ids)
        )));
    }
    if !faceted_ids.is_empty() {
        representations.push(entities.add(format!(
            "FACETED_BREP_SHAPE_REPRESENTATION({},({}),#{context})",
            quoted(representation_name),
            refs(&faceted_ids)
        )));
    }
    if !surface_ids.is_empty() {
        representations.push(entities.add(format!(
            "MANIFOLD_SURFACE_SHAPE_REPRESENTATION({},({}),#{context})",
            quoted(representation_name),
            refs(&surface_ids)
        )));
    }
    let shape = entities.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    for representation in &representations {
        entities.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
        ));
    }

    // OCS: layers and colours per body, falling back to the file-wide ones. Bodies
    // sharing a layer share one assignment, and bodies sharing a colour one style.
    let style = |index: usize| metadata.body_styles.get(index);
    let mut layers: Vec<(&str, Vec<u64>)> = Vec::new();
    let mut colours: Vec<([f64; 3], Vec<u64>)> = Vec::new();
    for (index, &item) in body_items.iter().enumerate() {
        let layer = style(index)
            .and_then(|s| s.layer.as_deref())
            .or(metadata.layer.as_deref());
        if let Some(layer) = layer {
            match layers.iter_mut().find(|(name, _)| *name == layer) {
                Some((_, items)) => items.push(item),
                None => layers.push((layer, vec![item])),
            }
        }
        if let Some(color) = style(index).and_then(|s| s.color).or(metadata.color) {
            match colours.iter_mut().find(|(c, _)| *c == color) {
                Some((_, items)) => items.push(item),
                None => colours.push((color, vec![item])),
            }
        }
    }
    for (layer, items) in &layers {
        entities.add(format!(
            "PRESENTATION_LAYER_ASSIGNMENT({},'',({}))",
            quoted(layer),
            refs(items)
        ));
    }
    let mut styled_items = Vec::new();
    for (color, items) in &colours {
        let colour = entities.add(format!(
            "COLOUR_RGB('',{},{},{})",
            number(color[0])?,
            number(color[1])?,
            number(color[2])?
        ));
        let fill_colour = entities.add(format!("FILL_AREA_STYLE_COLOUR('',#{colour})"));
        let fill = entities.add(format!("FILL_AREA_STYLE('',(#{fill_colour}))"));
        let surface = entities.add(format!("SURFACE_STYLE_FILL_AREA(#{fill})"));
        let side = entities.add(format!("SURFACE_SIDE_STYLE('',(#{surface}))"));
        let usage = entities.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
        let assignment = entities.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
        for body_id in items {
            styled_items.push(
                entities.add(format!("STYLED_ITEM('',(#{assignment}),#{body_id})")),
            );
        }
    }
    // OCS: readers find styled items through the presentation representation.
    if !styled_items.is_empty() {
        entities.add(format!(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{context})",
            refs(&styled_items)
        ));
    }
    if let Some(material) = metadata.material.as_deref() {
        entities.add(format!("MATERIAL_DESIGNATION({},'')", quoted(material)));
    }

    let file_name = metadata.file_name.as_deref().unwrap_or("");
    let author = metadata.author.as_deref().unwrap_or("");
    let organization = metadata.organization.as_deref().unwrap_or("");
    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('step-export'),'2;1');\n");
    // `FILE_NAME`'s fifth field is the preprocessor version in Part 21, but
    // SolidNode's header helpers store and read the organization there. Emit the
    // organization in the standard organization list as well, so both conventions
    // recover it.
    let _ = writeln!(
        out,
        "FILE_NAME({},'',({}),({}),{},'step-export','');",
        quoted(file_name),
        quoted(author),
        quoted(organization),
        quoted(organization)
    );
    let _ = writeln!(out, "FILE_SCHEMA(({}));", quoted(schema.identifier()));
    out.push_str("ENDSEC;\nDATA;\n");
    for (id, row) in &entities.rows {
        let _ = writeln!(out, "#{id}={row};");
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(out)
}

fn refs(ids: &[u64]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::step::model::{Edge, Face, Loop, OrientedEdge, Shell, Vertex};
    use solidcore_record::step::p21::Header;

    fn line(origin: [f64; 3], dir: [f64; 3], magnitude: f64) -> Curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        }
    }

    fn unit(value: [f64; 3]) -> [f64; 3] {
        let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
        value.map(|component| component / length)
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn test_frame(points: &[[f64; 3]], a: usize, b: usize, c: usize) -> Frame {
        let x = unit(std::array::from_fn(|axis| {
            points[b][axis] - points[a][axis]
        }));
        let other = std::array::from_fn(|axis| points[c][axis] - points[a][axis]);
        let z = unit(cross(x, other));
        Frame {
            origin: points[a],
            x,
            y: cross(z, x),
            z,
        }
    }

    /// A closed manifold model whose faces share one edge per undirected pair.
    fn planar_model(points: &[[f64; 3]], faces: &[Vec<usize>]) -> Model {
        let mut edges: Vec<Edge> = Vec::new();
        let mut edge_by_pair: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut face_uses = Vec::with_capacity(faces.len());
        for face_points in faces {
            let mut oriented = Vec::with_capacity(face_points.len());
            for index in 0..face_points.len() {
                let a = face_points[index];
                let b = face_points[(index + 1) % face_points.len()];
                let key = if a < b { (a, b) } else { (b, a) };
                let edge = if let Some(&edge) = edge_by_pair.get(&key) {
                    edge
                } else {
                    let start = points[key.0];
                    let end = points[key.1];
                    let delta = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
                    let magnitude =
                        (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
                    let edge_index = edges.len();
                    edges.push(Edge {
                        step_id: edge_index as u64 + 1,
                        start: key.0,
                        end: key.1,
                        curve: line(start, unit(delta), magnitude),
                        same_sense: true,
                        t_start: 0.0,
                        t_end: magnitude,
                    });
                    edge_by_pair.insert(key, edge_index);
                    edge_index
                };
                oriented.push(OrientedEdge {
                    edge,
                    forward: a < b,
                });
            }
            face_uses.push(oriented);
        }
        let faces: Vec<Face> = faces
            .iter()
            .zip(face_uses)
            .enumerate()
            .map(|(index, (face_points, uses))| {
                let frame = test_frame(points, face_points[0], face_points[1], face_points[2]);
                Face {
                    step_id: index as u64 + 1,
                    surface: Surface::Plane { frame },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(uses),
                    }],
                }
            })
            .collect();
        let vertices = points
            .iter()
            .enumerate()
            .map(|(index, &point)| Vertex {
                step_id: index as u64 + 1,
                point,
            })
            .collect();
        Model {
            header: Header::default(),
            schemas: vec!["CONFIG_CONTROL_DESIGN".into()],
            bodies: vec![Body {
                step_id: 10,
                name: Some("solid".into()),
                kind: BodyKind::ManifoldSolid,
                unit_mm: 1.0,
                uncertainty_mm: None,
                shells: vec![Shell {
                    step_id: 1,
                    closed: true,
                    orientation: true,
                    faces: (0..faces.len()).collect(),
                }],
                faces,
                edges,
                vertices,
            }],
            skipped: Vec::new(),
            diagnostics: Vec::new(),
            assembly_instances: 0,
            pcurves: BTreeMap::new(),
            file_pcurves: solidcore_record::step::model::FilePcurves::default(),
        }
    }

    fn pyramid() -> Model {
        planar_model(
            &[
                [-10.0, -10.0, -15.0],
                [10.0, -10.0, -15.0],
                [10.0, 10.0, -15.0],
                [-10.0, 10.0, -15.0],
                [0.0, 0.0, 15.0],
            ],
            &[
                vec![3, 2, 1, 0],
                vec![0, 1, 4],
                vec![1, 2, 4],
                vec![2, 3, 4],
                vec![3, 0, 4],
            ],
        )
    }

    fn triangle() -> Model {
        planar_model(
            &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            &[vec![0, 1, 2]],
        )
    }

    /// A closed cube whose every face owns its own vertices and edges (unwelded).
    fn unwelded_faceted_cube() -> Model {
        let corners = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
            [2.0, 0.0, 2.0],
            [2.0, 2.0, 2.0],
            [0.0, 2.0, 2.0],
        ];
        let face_corners = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ];
        let mut vertices = Vec::new();
        let mut edges = Vec::new();
        let mut faces = Vec::new();
        for corners_of_face in face_corners {
            let base = vertices.len();
            for &corner in &corners_of_face {
                vertices.push(Vertex {
                    step_id: vertices.len() as u64 + 1,
                    point: corners[corner],
                });
            }
            let mut uses = Vec::with_capacity(4);
            for index in 0..4 {
                let start = base + index;
                let end = base + (index + 1) % 4;
                let start_point = corners[corners_of_face[index]];
                let end_point = corners[corners_of_face[(index + 1) % 4]];
                let delta = [
                    end_point[0] - start_point[0],
                    end_point[1] - start_point[1],
                    end_point[2] - start_point[2],
                ];
                let magnitude =
                    (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
                let edge = edges.len();
                edges.push(Edge {
                    step_id: edges.len() as u64 + 1,
                    start,
                    end,
                    curve: line(start_point, unit(delta), magnitude),
                    same_sense: true,
                    t_start: 0.0,
                    t_end: magnitude,
                });
                uses.push(OrientedEdge {
                    edge,
                    forward: true,
                });
            }
            faces.push(Face {
                step_id: faces.len() as u64 + 1,
                surface: Surface::Plane {
                    frame: Frame {
                        origin: corners[corners_of_face[0]],
                        x: [1.0, 0.0, 0.0],
                        y: [0.0, 1.0, 0.0],
                        z: [0.0, 0.0, 1.0],
                    },
                },
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(uses),
                }],
            });
        }
        Model {
            header: Header::default(),
            schemas: vec!["CONFIG_CONTROL_DESIGN".into()],
            bodies: vec![Body {
                step_id: 10,
                name: Some("faceted cube".into()),
                kind: BodyKind::Faceted,
                unit_mm: 1.0,
                uncertainty_mm: None,
                shells: vec![Shell {
                    step_id: 1,
                    closed: true,
                    orientation: true,
                    faces: (0..faces.len()).collect(),
                }],
                faces,
                edges,
                vertices,
            }],
            skipped: Vec::new(),
            diagnostics: Vec::new(),
            assembly_instances: 0,
            pcurves: BTreeMap::new(),
            file_pcurves: solidcore_record::step::model::FilePcurves::default(),
        }
    }

    /// A cube with one face removed: an open, seamed tessellation whose boundary
    /// edges are used once, as a display mesh can be.
    fn cube_without_a_face() -> Model {
        let mut model = unwelded_faceted_cube();
        model.bodies[0].shells[0]
            .faces
            .pop()
            .expect("cube has faces");
        model
    }

    fn edge_uses(body: &Body) -> Vec<((usize, usize), usize)> {
        let mut uses: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for face in &body.faces {
            for loop_ in &face.loops {
                let Bound::Edges(edges) = &loop_.bound else {
                    continue;
                };
                for oriented in edges {
                    let edge = &body.edges[oriented.edge];
                    let (a, b) = if oriented.forward {
                        (edge.start, edge.end)
                    } else {
                        (edge.end, edge.start)
                    };
                    let key = if a < b { (a, b) } else { (b, a) };
                    *uses.entry(key).or_insert(0) += 1;
                }
            }
        }
        uses.into_iter().collect()
    }

    fn reimport(text: &str) -> Model {
        let model = import(text.as_bytes(), &Limits::default()).expect("re-import written STEP");
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        model
    }

    #[test]
    fn round_trip_preserves_tetrahedron_shared_edge_incidence() {
        let model = reimport(&write(&tetrahedron_model()).expect("write tetrahedron"));
        assert!(model.diagnostics.is_empty(), "{:?}", model.diagnostics);
        assert_eq!(model.bodies.len(), 1);
        let body = &model.bodies[0];
        assert_eq!(body.kind, BodyKind::ManifoldSolid);
        assert_eq!(body.shells.len(), 1);
        assert!(body.shells[0].closed);
        let expected_points = [
            [3.0, -5.0, 7.0],
            [5.0, -5.0, 7.0],
            [3.0, -2.0, 7.0],
            [3.0, -5.0, 11.0],
        ];
        let labels: Vec<_> = body
            .vertices
            .iter()
            .map(|vertex| {
                expected_points
                    .iter()
                    .position(|point| *point == vertex.point)
                    .expect("expected world-space vertex")
            })
            .collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3]);
        let mut edge_endpoints: Vec<_> = body
            .edges
            .iter()
            .map(|edge| {
                assert!(edge.same_sense);
                (labels[edge.start], labels[edge.end])
            })
            .collect();
        edge_endpoints.sort_unstable();
        assert_eq!(
            edge_endpoints,
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        );
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 6);
        assert!(uses.iter().all(|(_, count)| *count == 2), "{uses:?}");
    }

    #[test]
    fn writes_a_round_trippable_triangle() {
        let text = write(&triangle()).expect("write");
        let file = reimport(&text);
        assert_eq!(file.bodies.len(), 1);
        assert_eq!(file.bodies[0].faces.len(), 1);
        assert_eq!(file.bodies[0].edges.len(), 3);
        assert_eq!(file.bodies[0].vertices.len(), 3);
    }

    #[test]
    fn pyramid_shares_one_cartesian_point_per_vertex() {
        let text = write(&pyramid()).expect("write pyramid");
        assert_eq!(
            text.matches("CARTESIAN_POINT(").count(),
            5,
            "face placements and line origins must reuse the vertices' points"
        );
        assert_eq!(text.matches("VERTEX_POINT(").count(), 5);
        let model = reimport(&text);
        let body = &model.bodies[0];
        assert_eq!(body.kind, BodyKind::ManifoldSolid);
        assert_eq!(body.faces.len(), 5);
        assert_eq!(body.vertices.len(), 5);
        assert_eq!(body.edges.len(), 8);
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 8);
        assert!(uses.iter().all(|(_, count)| *count == 2), "{uses:?}");
    }

    #[test]
    fn metadata_lands_in_the_header_product_and_presentation() {
        let metadata = ExportMetadata::new()
            .with_file_name("bracket")
            .with_product_name("Bracket")
            .with_author("Ada Lovelace")
            .with_organization("Tangency Labs")
            .with_schema(Schema::Ap214)
            .with_layer("Parts")
            .with_color([0.125, 0.5, 0.75])
            .with_material("Steel");
        let text = write_with_metadata(&triangle(), &metadata).expect("write with metadata");
        assert!(text.contains("FILE_NAME('bracket'"), "{text}");
        assert!(text.contains("('Ada Lovelace')"), "{text}");
        assert!(text.contains("('Tangency Labs')"), "{text}");
        assert!(
            text.contains("('Tangency Labs'),'Tangency Labs'"),
            "organization must also fill SolidNode's preprocessor-version slot: {text}"
        );
        assert!(
            text.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN'))"),
            "{text}"
        );
        assert!(text.contains("PRODUCT('Bracket','Bracket',''"), "{text}");
        assert!(
            text.contains("PRESENTATION_LAYER_ASSIGNMENT('Parts','',("),
            "{text}"
        );
        assert!(text.contains("COLOUR_RGB('',0.125,0.5,0.75)"), "{text}");
        assert!(text.contains("MATERIAL_DESIGNATION('Steel'"), "{text}");
        assert!(
            text.contains(
                "APPLICATION_CONTEXT('core data for automotive mechanical design processes')"
            ),
            "{text}"
        );
        let model = reimport(&text);
        assert_eq!(model.header.file_name, "bracket");
        assert_eq!(model.header.authors, vec!["Ada Lovelace".to_string()]);
        assert_eq!(
            model.header.organizations,
            vec!["Tangency Labs".to_string()]
        );
        assert_eq!(model.schemas, vec!["AUTOMOTIVE_DESIGN".to_string()]);
        assert_eq!(model.bodies.len(), 1);
    }

    #[test]
    fn default_metadata_emits_a_renameable_product() {
        let text = write(&triangle()).expect("write");
        assert!(text.contains("=PRODUCT("), "{text}");
        assert!(text.contains("PRODUCT_DEFINITION("), "{text}");
        assert!(text.contains("SHAPE_DEFINITION_REPRESENTATION("), "{text}");
        assert!(
            text.contains("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'))"),
            "{text}"
        );
    }

    #[test]
    fn faceted_cube_welds_coincident_vertices_into_a_closed_shell() {
        let model = unwelded_faceted_cube();
        let text = write(&model).expect("write faceted cube");
        assert!(text.contains("FACETED_BREP("), "{text}");
        assert!(text.contains("POLY_LOOP("), "{text}");
        assert_eq!(
            text.matches("CARTESIAN_POINT(").count(),
            8,
            "the six unwelded faces must collapse to the cube's eight corners"
        );
        let imported = reimport(&text);
        let body = &imported.bodies[0];
        assert_eq!(body.kind, BodyKind::Faceted);
        assert!(body.shells[0].closed);
        assert_eq!(body.vertices.len(), 8);
        assert_eq!(body.edges.len(), 12);
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 12);
        assert!(
            uses.iter().all(|(_, count)| *count == 2),
            "each faceted edge must be shared by two faces: {uses:?}"
        );
    }

    #[test]
    fn write_faceted_forces_a_faceted_shell_with_shared_points() {
        let text = write_faceted(&pyramid()).expect("write pyramid as faceted");
        assert!(text.contains("FACETED_BREP("), "{text}");
        assert!(!text.contains("MANIFOLD_SOLID_BREP("), "{text}");
        assert_eq!(
            text.matches("CARTESIAN_POINT(").count(),
            5,
            "the faceted writer must share vertex points too"
        );
        let imported = reimport(&text);
        let body = &imported.bodies[0];
        assert_eq!(body.kind, BodyKind::Faceted);
        assert_eq!(body.faces.len(), 5);
        assert_eq!(body.vertices.len(), 5);
        assert_eq!(body.edges.len(), 8);
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 8);
        assert!(uses.iter().all(|(_, count)| *count == 2), "{uses:?}");
    }

    #[test]
    fn open_faceted_shell_is_rejected() {
        let error =
            write(&cube_without_a_face()).expect_err("an open faceted shell must be rejected");
        assert!(error.message.contains("expected 2"), "{}", error.message);
    }

    #[test]
    fn open_tessellation_writes_with_the_display_api() {
        let model = cube_without_a_face();
        let error = write_faceted(&model).expect_err("the strict writer rejects an open shell");
        assert!(error.message.contains("expected 2"), "{}", error.message);

        let text = write_faceted_display(&model).expect("display write of an open shell");
        assert!(text.contains("FACETED_BREP("), "{text}");
        assert!(!text.contains("MANIFOLD_SOLID_BREP("), "{text}");
        assert_eq!(
            text.matches("CARTESIAN_POINT(").count(),
            8,
            "display output must still weld coincident vertices"
        );
        let imported = reimport(&text);
        let body = &imported.bodies[0];
        assert_eq!(body.kind, BodyKind::Faceted);
        assert_eq!(body.faces.len(), 5);
        assert_eq!(body.vertices.len(), 8);
        assert_eq!(body.edges.len(), 12);
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 12);
        assert_eq!(
            uses.iter().filter(|(_, count)| *count == 1).count(),
            4,
            "the removed face must leave its four boundary edges unpaired: {uses:?}"
        );
    }

    #[test]
    fn display_api_accepts_a_shell_flagged_open() {
        let mut model = cube_without_a_face();
        model.bodies[0].shells[0].closed = false;
        assert!(write_faceted(&model).is_err());
        assert!(write_faceted_display(&model).is_ok());
    }

    #[test]
    fn closed_cube_is_unchanged_by_the_display_api() {
        let model = unwelded_faceted_cube();
        let strict = write_faceted(&model).expect("strict closed cube");
        let display = write_faceted_display(&model).expect("display closed cube");
        assert_eq!(
            strict, display,
            "a closed body has one correct faceted output"
        );
        let imported = reimport(&display);
        let body = &imported.bodies[0];
        assert_eq!(body.kind, BodyKind::Faceted);
        assert!(body.shells[0].closed);
        assert_eq!(body.vertices.len(), 8);
        assert_eq!(body.edges.len(), 12);
        let uses = edge_uses(body);
        assert_eq!(uses.len(), 12);
        assert!(uses.iter().all(|(_, count)| *count == 2), "{uses:?}");
    }

    #[test]
    fn display_flattens_multiple_shells_into_one_faceted_shell() {
        let mut model = unwelded_faceted_cube();
        let shell = model.bodies[0].shells[0].clone();
        let split = shell.faces.len() / 2;
        model.bodies[0].shells = vec![
            Shell {
                step_id: 1,
                faces: shell.faces[..split].to_vec(),
                ..shell.clone()
            },
            Shell {
                step_id: 2,
                faces: shell.faces[split..].to_vec(),
                ..shell
            },
        ];
        assert!(
            write_faceted(&model).is_err(),
            "the strict writer requires exactly one shell"
        );

        let text = write_faceted_display(&model).expect("display write of split shells");
        assert_eq!(text.matches("FACETED_BREP(").count(), 1, "{text}");
        assert_eq!(text.matches("CLOSED_SHELL(").count(), 1, "{text}");
        let imported = reimport(&text);
        assert_eq!(imported.bodies.len(), 1);
        let body = &imported.bodies[0];
        assert_eq!(body.faces.len(), 6);
        assert_eq!(body.vertices.len(), 8);
        assert_eq!(body.edges.len(), 12);
        assert!(edge_uses(body).iter().all(|(_, count)| *count == 2));
    }

    #[test]
    fn display_metadata_lands_in_the_header_and_presentation() {
        let metadata = ExportMetadata::new()
            .with_file_name("display")
            .with_product_name("Display")
            .with_schema(Schema::Ap242)
            .with_color([0.25, 0.5, 0.75]);
        let text = write_faceted_display_with_metadata(&cube_without_a_face(), &metadata)
            .expect("display write with metadata");
        assert!(text.contains("FILE_NAME('display'"), "{text}");
        assert!(text.contains("PRODUCT('Display','Display',''"), "{text}");
        assert!(
            text.contains("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'))"),
            "{text}"
        );
        assert!(text.contains("COLOUR_RGB('',0.25,0.5,0.75)"), "{text}");
        assert!(text.contains("FACETED_BREP("), "{text}");
        let imported = reimport(&text);
        assert_eq!(imported.header.file_name, "display");
        assert_eq!(imported.bodies.len(), 1);
    }

    #[test]
    fn writes_analytic_and_nurbs_geometry() {
        let unit = || Frame {
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let mut surfaces = vec![
            Surface::Cylinder {
                frame: unit(),
                radius: 2.0,
            },
            Surface::Cone {
                frame: unit(),
                radius: 2.0,
                semi_angle_deg: 30.0,
            },
            Surface::Sphere {
                frame: unit(),
                radius: 2.0,
            },
            Surface::Torus {
                frame: unit(),
                major: 4.0,
                minor: 1.0,
            },
            Surface::Revolution {
                curve: Box::new(Curve::Line {
                    origin: [1.0, 0.0, 0.0],
                    dir: [0.0, 0.0, 1.0],
                    magnitude: 1.0,
                }),
                axis_origin: [0.0; 3],
                axis_dir: [0.0, 0.0, 1.0],
            },
            Surface::Extrusion {
                curve: Box::new(Curve::Line {
                    origin: [0.0, 0.0, 0.0],
                    dir: [1.0, 0.0, 0.0],
                    magnitude: 1.0,
                }),
                dir: [0.0, 1.0, 0.0],
                magnitude: 3.0,
            },
        ];
        let bspline = solidcore_record::step::model::BSplineSurface {
            u_degree: 1,
            v_degree: 1,
            points: vec![
                vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            ],
            weights: None,
            u_knots: vec![0.0, 1.0],
            u_mults: vec![2, 2],
            v_knots: vec![0.0, 1.0],
            v_mults: vec![2, 2],
        };
        surfaces.push(Surface::BSpline(bspline));
        for surface in surfaces {
            let mut model = triangle();
            model.bodies[0].faces[0].surface = surface;
            let text = write(&model).expect("surface write");
            assert!(text.contains("SURFACE"), "{text}");
        }
        let curves = vec![
            Curve::Circle {
                frame: unit(),
                radius: 2.0,
            },
            Curve::Ellipse {
                frame: unit(),
                semi1: 2.0,
                semi2: 1.0,
            },
            Curve::BSpline(solidcore_record::step::model::BSplineCurve {
                degree: 1,
                points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
                weights: None,
                knots: vec![0.0, 1.0],
                mults: vec![2, 2],
            }),
        ];
        for curve in curves {
            let mut model = triangle();
            model.bodies[0].edges[0].curve = curve;
            let text = write(&model).expect("curve write");
            assert!(
                text.contains("CURVE(") || text.contains("CIRCLE(") || text.contains("ELLIPSE("),
                "{text}"
            );
        }
    }

    #[test]
    fn writes_zero_angle_cones_as_cylinders() {
        let mut model = triangle();
        model.bodies[0].faces[0].surface = Surface::Cone {
            frame: Frame {
                origin: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            },
            radius: 2.0,
            semi_angle_deg: 0.0,
        };
        let text = write(&model).expect("cylinder write");
        assert!(text.contains("CYLINDRICAL_SURFACE('',"), "{text}");
        assert!(!text.contains("CONICAL_SURFACE('',"), "{text}");
    }

    #[test]
    fn writes_pcurves_when_the_model_carries_them() {
        let mut model = triangle();
        let face = model.bodies[0].faces[0].step_id;
        model
            .pcurves
            .insert((face, 0, 0), vec![vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]]);
        let text = write(&model).expect("pcurve write");
        assert!(text.contains("PCURVE('',"), "{text}");
        assert!(text.contains("SURFACE_CURVE("), "{text}");
        assert!(text.contains("DEFINITIONAL_REPRESENTATION("), "{text}");
    }

    #[test]
    fn writes_seam_curves_when_an_edge_has_two_pcurves() {
        let mut model = triangle();
        let face = model.bodies[0].faces[0].step_id;
        model.pcurves.insert(
            (face, 0, 0),
            vec![
                vec![[0.0, 0.0], [1.0, 1.0]],
                vec![[360.0, 0.0], [361.0, 1.0]],
            ],
        );
        let text = write(&model).expect("seam write");
        assert!(text.contains("SEAM_CURVE("), "{text}");
        assert!(text.matches("PCURVE('',").count() >= 2, "{text}");
    }

    #[test]
    fn writes_analytic_surfaces() {
        let mut model = triangle();
        model.bodies[0].faces[0].surface = Surface::Cylinder {
            frame: Frame {
                origin: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            },
            radius: 1.0,
        };
        let text = write(&model).expect("analytic surface write");
        assert!(text.contains("CYLINDRICAL_SURFACE('',"), "{text}");
        let imported = reimport(&text);
        assert_eq!(imported.bodies.len(), 1);
    }

    #[test]
    fn rejects_open_manifold_shell() {
        let mut model = triangle();
        model.bodies[0].shells[0].closed = false;
        assert!(write(&model).is_err());
    }

    #[test]
    fn rejects_non_finite_emitted_numbers() {
        let values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        let mut accepted = Vec::new();

        // vertex point component
        for &v in &values {
            let mut model = triangle();
            model.bodies[0].vertices[0].point[0] = v;
            if write(&model).is_ok() {
                accepted.push(format!("vertex point component: {v}"));
            }
        }
        // line origin
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { origin, .. } = &mut model.bodies[0].edges[0].curve {
                origin[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line origin: {v}"));
            }
        }
        // line dir
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { dir, .. } = &mut model.bodies[0].edges[0].curve {
                dir[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line dir: {v}"));
            }
        }
        // line magnitude
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { magnitude, .. } = &mut model.bodies[0].edges[0].curve {
                *magnitude = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line magnitude: {v}"));
            }
        }
        // plane origin
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.origin[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane origin: {v}"));
            }
        }
        // plane X
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.x[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane X: {v}"));
            }
        }
        // plane Z
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.z[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane Z: {v}"));
            }
        }

        assert!(accepted.is_empty(), "wrongly accepted: {accepted:?}");
    }

    #[test]
    fn rejects_invalid_color() {
        let metadata = ExportMetadata::new().with_color([1.5, 0.0, 0.0]);
        assert!(write_with_metadata(&triangle(), &metadata).is_err());
        let metadata = ExportMetadata::new().with_color([f64::NAN, 0.0, 0.0]);
        assert!(write_with_metadata(&triangle(), &metadata).is_err());
    }

    /// The tetrahedron from the original writer test, built through `planar_model`.
    fn tetrahedron_model() -> Model {
        planar_model(
            &[
                [3.0, -5.0, 7.0],
                [5.0, -5.0, 7.0],
                [3.0, -2.0, 7.0],
                [3.0, -5.0, 11.0],
            ],
            &[vec![0, 2, 1], vec![0, 1, 3], vec![0, 3, 2], vec![1, 2, 3]],
        )
    }
}
