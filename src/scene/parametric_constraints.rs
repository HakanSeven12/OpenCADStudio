//! Runtime parametric-constraint data and scope management.

use super::named_parameters::DrivingValue;
use acadrust::types::{Handle, Vector3};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// One endpoint a constraint attaches to: an entity plus which sub-element
/// of it.
///
/// Reuses the GsMarker convention `AssocDimensionReference::main_gs_marker`
/// already carries for associative-dimension endpoints
/// (`src/scene/dimension_assoc.rs`), rather than inventing a second
/// sub-element addressing scheme: `marker` indexes into
/// [`dimension_assoc::source_points`](super::dimension_assoc::source_points)'s
/// ordered per-entity-type point list when non-negative (0/1 = a line's
/// start/end, ...), or names a special case when negative (-3 = a
/// circle/arc's center; -2 = a bounded curve's midpoint). Polyline segment
    /// segment-midpoint, and curved-segment-center references use private
    /// negative ranges so they stay distinct from vertex markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParametricRef {
    pub entity: Handle,
    /// `None` addresses the entity as a whole — what a Radius, Length, or
    /// whole-curve constraint (Parallel, Perpendicular, Equal, Horizontal,
    /// Vertical) needs; a point-level constraint (Coincident, Distance
    /// between two points, Angle at a shared vertex) sets `Some(marker)`.
    pub marker: Option<i32>,
}

const POLYLINE_SEGMENT_MARKER_BASE: i32 = -1_000_000;
const POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE: i32 = -2_000_000;
const POLYLINE_SEGMENT_CENTER_MARKER_BASE: i32 = -3_000_000;
const ELLIPSE_MAJOR_AXIS_MARKER: i32 = -4;
const ELLIPSE_MINOR_AXIS_MARKER: i32 = -5;
const TEXT_BASELINE_MARKER: i32 = -6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectionalAxis {
    TextBaseline,
    EllipseMajor,
    EllipseMinor,
}

impl ParametricRef {
    pub fn whole(entity: Handle) -> Self {
        Self {
            entity,
            marker: None,
        }
    }

    pub fn point(entity: Handle, marker: i32) -> Self {
        Self {
            entity,
            marker: Some(marker),
        }
    }

    /// The circle/arc-center special case (`marker == -3`), broken out as
    /// its own constructor since `-3` alone reads as a magic number
    /// everywhere it would otherwise appear.
    pub fn center(entity: Handle) -> Self {
        Self {
            entity,
            marker: Some(-3),
        }
    }

    /// Select one straight segment of a polyline.
    pub fn segment(entity: Handle, index: usize) -> Self {
        Self {
            entity,
            marker: Some(POLYLINE_SEGMENT_MARKER_BASE - index as i32),
        }
    }

    pub fn segment_index(self) -> Option<usize> {
        let marker = self.marker?;
        (marker <= POLYLINE_SEGMENT_MARKER_BASE && marker > POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE)
            .then(|| (POLYLINE_SEGMENT_MARKER_BASE - marker) as usize)
    }

    /// Select the midpoint of one straight polyline segment.
    pub fn segment_midpoint(entity: Handle, index: usize) -> Self {
        Self {
            entity,
            marker: Some(POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE - index as i32),
        }
    }

    pub fn segment_midpoint_index(self) -> Option<usize> {
        let marker = self.marker?;
        (marker <= POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE
            && marker > POLYLINE_SEGMENT_CENTER_MARKER_BASE)
            .then(|| (POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE - marker) as usize)
    }

    /// Select the center of one curved polyline segment.
    pub fn segment_center(entity: Handle, index: usize) -> Self {
        Self {
            entity,
            marker: Some(POLYLINE_SEGMENT_CENTER_MARKER_BASE - index as i32),
        }
    }

    pub fn segment_center_index(self) -> Option<usize> {
        let marker = self.marker?;
        (marker <= POLYLINE_SEGMENT_CENTER_MARKER_BASE)
            .then(|| (POLYLINE_SEGMENT_CENTER_MARKER_BASE - marker) as usize)
    }

    /// Select the displayed baseline of a Text or MText entity.
    pub fn text_baseline(entity: Handle) -> Self {
        Self {
            entity,
            marker: Some(TEXT_BASELINE_MARKER),
        }
    }

    /// Select the major axis of an ellipse or elliptical arc.
    pub fn ellipse_major_axis(entity: Handle) -> Self {
        Self {
            entity,
            marker: Some(ELLIPSE_MAJOR_AXIS_MARKER),
        }
    }

    /// Select the minor axis of an ellipse or elliptical arc.
    pub fn ellipse_minor_axis(entity: Handle) -> Self {
        Self {
            entity,
            marker: Some(ELLIPSE_MINOR_AXIS_MARKER),
        }
    }

    pub(crate) fn directional_axis(self) -> Option<DirectionalAxis> {
        match self.marker? {
            TEXT_BASELINE_MARKER => Some(DirectionalAxis::TextBaseline),
            ELLIPSE_MAJOR_AXIS_MARKER => Some(DirectionalAxis::EllipseMajor),
            ELLIPSE_MINOR_AXIS_MARKER => Some(DirectionalAxis::EllipseMinor),
            _ => None,
        }
    }
}

/// The finite guide used to pick, display and serialize a directional text or
/// ellipse reference. Constraint equations treat the guide as an infinite
/// line; its finite length only makes selection and native persistence stable.
pub(crate) fn directional_axis_endpoints(
    entity: &acadrust::EntityType,
    reference: ParametricRef,
) -> Option<[Vector3; 2]> {
    match (entity, reference.directional_axis()?) {
        (acadrust::EntityType::Text(text), DirectionalAxis::TextBaseline) => {
            use acadrust::entities::TextHorizontalAlignment as Alignment;

            if matches!(
                text.horizontal_alignment,
                Alignment::Aligned | Alignment::Fit
            ) {
                if let Some(end) = text.alignment_point.filter(|end| {
                    (*end - text.insertion_point).length_squared() > 1.0e-18
                }) {
                    return Some([text.insertion_point, end]);
                }
            }
            let length = text.height.abs().max(1.0);
            Some([
                text.insertion_point,
                text.insertion_point
                    + Vector3::new(text.rotation.cos(), text.rotation.sin(), 0.0) * length,
            ])
        }
        (acadrust::EntityType::MText(text), DirectionalAxis::TextBaseline) => {
            let length = text.height.abs().max(1.0);
            Some([
                text.insertion_point,
                text.insertion_point
                    + Vector3::new(text.rotation.cos(), text.rotation.sin(), 0.0) * length,
            ])
        }
        (acadrust::EntityType::Ellipse(ellipse), axis) => {
            let major_length = ellipse.major_axis.length();
            if major_length <= 1.0e-12 {
                return None;
            }
            let vector = match axis {
                DirectionalAxis::EllipseMajor => ellipse.major_axis,
                DirectionalAxis::EllipseMinor => {
                    let major = ellipse.major_axis / major_length;
                    let normal = ellipse.normal.normalize();
                    Vector3::new(
                        normal.y * major.z - normal.z * major.y,
                        normal.z * major.x - normal.x * major.z,
                        normal.x * major.y - normal.y * major.x,
                    ) * (major_length * ellipse.minor_axis_ratio)
                }
                DirectionalAxis::TextBaseline => return None,
            };
            (vector.length_squared() > 1.0e-24).then_some([ellipse.center, ellipse.center + vector])
        }
        _ => None,
    }
}

/// The axis a lone Horizontal/Vertical reference — or two points on one
/// entity — turns onto its datum: the entity, the anchor that stays, the
/// axis end that moves, and for a polyline the vertex to move instead of
/// turning the whole entity.
pub(crate) fn axis_alignment_target(
    document: &acadrust::CadDocument,
    refs: &[ParametricRef],
) -> Option<(Handle, Vector3, Vector3, Option<usize>)> {
    let (handle, start_marker, end_marker) = match refs {
        [reference] => {
            let entity = document.get_entity(reference.entity)?;
            if reference.directional_axis().is_some() {
                let [start, end] = directional_axis_endpoints(entity, *reference)?;
                return Some((reference.entity, start, end, None));
            }
            let index = reference.segment_index().map_or(0, |index| index as i32);
            (reference.entity, index, index + 1)
        }
        [first, second] if first.entity == second.entity => {
            (first.entity, first.marker?, second.marker?)
        }
        _ => return None,
    };
    let entity = document.get_entity(handle)?;
    let start = resolve_point(entity, start_marker)?;
    let end = resolve_point(entity, end_marker)?;
    let vertex = matches!(
        entity,
        acadrust::EntityType::LwPolyline(_) | acadrust::EntityType::Polyline2D(_)
    )
    .then(|| usize::try_from(end_marker).ok())
    .flatten();
    Some((handle, start, end, vertex))
}

/// Moves one polyline vertex in its plane; `false` for any other entity.
pub(crate) fn set_polyline_vertex(
    entity: &mut acadrust::EntityType,
    index: usize,
    x: f64,
    y: f64,
) -> bool {
    let Some(index) = polyline_vertex_index(entity, index) else {
        return false;
    };
    match entity {
        acadrust::EntityType::LwPolyline(polyline) => polyline
            .vertices
            .get_mut(index)
            .map(|vertex| {
                vertex.location.x = x;
                vertex.location.y = y;
            })
            .is_some(),
        acadrust::EntityType::Polyline2D(polyline) => polyline
            .vertices
            .get_mut(index)
            .map(|vertex| {
                vertex.location.x = x;
                vertex.location.y = y;
            })
            .is_some(),
        _ => false,
    }
}

/// The size an Equal relation copies.
#[derive(Clone, Copy)]
pub(crate) enum EqualSize {
    /// A line's or a straight polyline segment's length.
    Length(f64),
    /// A circle's or an arc's radius.
    Radius(f64),
}

pub(crate) fn equal_size(
    document: &acadrust::CadDocument,
    reference: ParametricRef,
) -> Option<EqualSize> {
    let entity = document.get_entity(reference.entity)?;
    match entity {
        acadrust::EntityType::Circle(circle) if reference.marker.is_none() => {
            Some(EqualSize::Radius(circle.radius))
        }
        acadrust::EntityType::Arc(arc) if reference.marker.is_none() => {
            Some(EqualSize::Radius(arc.radius))
        }
        acadrust::EntityType::Line(_)
        | acadrust::EntityType::LwPolyline(_)
        | acadrust::EntityType::Polyline2D(_) => {
            let index = reference.segment_index().map_or(0, |index| index as i32);
            let start = resolve_point(entity, index)?;
            let end = resolve_point(entity, index + 1)?;
            let length = (end - start).length();
            (length > 1.0e-12).then_some(EqualSize::Length(length))
        }
        _ => None,
    }
}

/// `follower` resized to `first`'s size the way the reference does it: a
/// line or segment keeps its start and direction and only its end moves,
/// a circle or arc keeps its center and takes the radius. `None` when the
/// two do not share a size kind.
pub(crate) fn equal_size_follower(
    document: &acadrust::CadDocument,
    first: ParametricRef,
    follower: ParametricRef,
) -> Option<acadrust::EntityType> {
    let size = equal_size(document, first)?;
    let original = document.get_entity(follower.entity)?;
    let mut entity = original.clone();
    match size {
        EqualSize::Radius(radius) if follower.marker.is_none() => match &mut entity {
            acadrust::EntityType::Circle(circle) => circle.radius = radius,
            acadrust::EntityType::Arc(arc) => arc.radius = radius,
            _ => return None,
        },
        EqualSize::Length(length) => {
            let index = follower.segment_index().map_or(0, |index| index as i32);
            let start = resolve_point(original, index)?;
            let end = resolve_point(original, index + 1)?;
            let current = (end - start).length();
            if current <= 1.0e-12 {
                return None;
            }
            let scale = length / current;
            let x = start.x + (end.x - start.x) * scale;
            let y = start.y + (end.y - start.y) * scale;
            if matches!(
                original,
                acadrust::EntityType::LwPolyline(_) | acadrust::EntityType::Polyline2D(_)
            ) {
                if !set_polyline_vertex(&mut entity, index as usize + 1, x, y) {
                    return None;
                }
            } else if let (acadrust::EntityType::Line(line), None) = (&mut entity, follower.marker)
            {
                line.end.x = x;
                line.end.y = y;
            } else {
                return None;
            }
        }
        _ => return None,
    }
    Some(entity)
}

/// The first free `d1`, `d2`, … name a new dimensional constraint takes.
pub(crate) fn next_dimensional_parameter_name(
    table: &super::named_parameters::ParameterTable,
) -> String {
    (1..)
        .map(|n| format!("d{n}"))
        .find(|name| !table.contains(name))
        .unwrap_or_else(|| "d1".to_string())
}

/// The first free `rad1`/`dia1`, `rad2`/`dia2`, … name a new radius or
/// diameter constraint takes.
pub(crate) fn next_radial_parameter_name(
    table: &super::named_parameters::ParameterTable,
    diameter: bool,
) -> String {
    let prefix = if diameter { "dia" } else { "rad" };
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|name| !table.contains(name))
        .unwrap_or_else(|| format!("{prefix}1"))
}

/// The first free `ang1`, `ang2`, … name a new angular constraint takes.
pub(crate) fn next_angular_parameter_name(
    table: &super::named_parameters::ParameterTable,
) -> String {
    (1..)
        .map(|n| format!("ang{n}"))
        .find(|name| !table.contains(name))
        .unwrap_or_else(|| "ang1".to_string())
}

/// An angle as the reference displays it: a negative or over-full value is
/// brought into one turn (`-30` → `330`, `400` → `40`), a full turn stays.
pub(crate) fn normalize_angle_display(degrees: f64) -> f64 {
    if !degrees.is_finite() || (degrees.abs() - 360.0).abs() < 1.0e-9 {
        degrees
    } else {
        degrees.rem_euclid(360.0)
    }
}

/// The angular precision (DIMADEC, or DIMDEC when unset) of a dimension
/// style, by name or the drawing's current one.
pub(crate) fn angle_decimals(document: &acadrust::CadDocument, style_name: Option<&str>) -> usize {
    let requested = style_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| document.header.current_dimstyle_name.trim());
    let style = document
        .dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(requested))
        .or_else(|| {
            document
                .dim_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case("Standard"))
        });
    style
        .map(|style| {
            if style.dimadec < 0 {
                style.dimdec
            } else {
                style.dimadec
            }
        })
        .unwrap_or(0)
        .clamp(0, 8) as usize
}

/// The constraint-bar label that draws a dynamic dimension's lock mark.
pub const DYNAMIC_DIMENSION_GLYPH: &str = "\u{1F512}";

/// The on-screen text height of a dynamic dimension, in pixels.
pub const DYNAMIC_DIMENSION_TEXT_PX: f32 = 12.0;

/// The layer the reference keeps dynamic dimensions on; an annotational
/// dimensional constraint sits on an ordinary layer instead.
pub(crate) const DYNAMIC_DIMENSION_LAYER: &str = "*ADSK_CONSTRAINTS";

/// The text a dynamic dimension shows: CONSTRAINTNAMEFORMAT 0 = name,
/// 1 = value, 2 = name=value, the value with its trailing zeros dropped
/// (`d9=80`, `d1=111.8034`). A parameter defined by a formula shows the
/// formula behind `fx:` (`fx: len=d1*2`); a reference (driven) constraint's
/// text sits in parentheses; an annotational one keeps dimension precision.
pub(crate) fn dynamic_dimension_text(
    name: &str,
    value: f64,
    format: u8,
    reference: bool,
    expression: Option<&str>,
    annotational: bool,
    angle_decimals: Option<usize>,
) -> String {
    let formula = expression
        .map(str::trim)
        .filter(|source| !source.is_empty() && source.parse::<f64>().is_err());
    let shown = match formula {
        Some(source) => source.to_string(),
        // An angle shows at angular precision, within one turn (`ang1=27`).
        None if angle_decimals.is_some() => {
            let decimals = angle_decimals.unwrap_or(0);
            format!("{:.decimals$}", normalize_angle_display(value))
        }
        // An annotational dimension shows the value at dimension precision
        // (`d1=100.0000`), as the reference does.
        None if annotational => format!("{value:.4}"),
        None => {
            let text = format!("{value:.4}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text.is_empty() || text == "-" {
                "0".to_string()
            } else {
                text.to_string()
            }
        }
    };
    let text = match format {
        0 => name.to_string(),
        1 => shown,
        _ => format!("{name}={shown}"),
    };
    let text = if formula.is_some() {
        format!("fx: {text}")
    } else {
        text
    };
    if reference {
        format!("({text})")
    } else {
        text
    }
}

/// The expression a measured value seeds a parameter with: twelve
/// significant digits with the trailing zeros dropped (`100`,
/// `111.803398875`), as the reference stores it.
pub(crate) fn measured_expression(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    // Solver noise (24.9999999047) reads as the round value it means.
    let snapped = (value * 1.0e4).round() / 1.0e4;
    let value = if (value - snapped).abs() < 1.0e-6 {
        snapped
    } else {
        value
    };
    let magnitude = value.abs();
    let integer_digits = if magnitude < 1.0 {
        1
    } else {
        magnitude.log10().floor() as i32 + 1
    };
    let decimals = (12 - integer_digits).clamp(0, 12) as usize;
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    if text.is_empty() || text == "-" || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// Writes a constraint point's position: a line's start (0) or end (1), a
/// polyline's vertex by index. Other entities and markers are left alone.
///
/// The inverse of [`resolve_point`], which reads polyline vertices in WCS
/// through the polyline's OCS: the world point goes back into the OCS the
/// vertex is stored in (a mirrored polyline has a -Z normal, and writing the
/// world X into its OCS X would flip the vertex), the elevation stays the
/// polyline's own, and the closing segment's end marker names vertex 0.
pub(crate) fn set_resolved_point(
    entity: &mut acadrust::EntityType,
    marker: i32,
    point: Vector3,
) -> bool {
    if marker < 0 {
        return false;
    }
    let index = polyline_vertex_index(entity, marker as usize).unwrap_or(marker as usize);
    let to_ocs = |normal: Vector3| {
        crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            (normal.x, normal.y, normal.z),
        )
    };
    match entity {
        acadrust::EntityType::Line(line) => match marker {
            0 => line.start = point,
            1 => line.end = point,
            _ => return false,
        },
        acadrust::EntityType::LwPolyline(polyline) => {
            let (x, y, _) = to_ocs(polyline.normal);
            let Some(vertex) = polyline.vertices.get_mut(index) else {
                return false;
            };
            vertex.location = acadrust::types::Vector2::new(x, y);
        }
        acadrust::EntityType::Polyline2D(polyline) => {
            let (x, y, _) = to_ocs(polyline.normal);
            let Some(vertex) = polyline.vertices.get_mut(index) else {
                return false;
            };
            vertex.location.x = x;
            vertex.location.y = y;
        }
        _ => return false,
    }
    true
}

/// Where the reference puts the second line of an Aligned 2Lines pick: it
/// turns about its start to run with the first line (keeping its heading
/// and length) and settles at the distance the first line's picked point
/// had from its original line, on the side nearer to where the turn left
/// it — the right-hand side of the first line when both are as near.
pub(crate) fn two_lines_placement(
    first_ends: [glam::DVec3; 2],
    first_pick: glam::DVec3,
    second_ends: [glam::DVec3; 2],
) -> Option<[glam::DVec3; 2]> {
    let [f0, f1] = first_ends;
    let [s0, s1] = second_ends;
    let f_dir = (f1 - f0).try_normalize()?;
    let normal = glam::DVec3::new(-f_dir.y, f_dir.x, 0.0);
    let s_dir = (s1 - s0).try_normalize()?;
    let length = (s1 - s0).length();
    let pick = f0 + f_dir * (first_pick - f0).dot(f_dir);
    let offset = pick - s0;
    let distance = (s_dir.x * offset.y - s_dir.y * offset.x).abs();
    let heading = if s_dir.dot(f_dir) >= 0.0 { f_dir } else { -f_dir };
    let turned = (s0 - f0).dot(normal);
    let target = if turned.abs() <= 1.0e-9 {
        -distance
    } else if (turned - distance).abs() <= (turned + distance).abs() {
        distance
    } else {
        -distance
    };
    let shift = normal * (target - turned);
    Some([s0 + shift, s0 + heading * length + shift])
}

/// The second line of an Aligned 2Lines pick moved to where
/// [`two_lines_placement`] puts it, or `None` when either line cannot be
/// resolved or written back.
///
/// Out of line and boxed on purpose: the caller is an arm of the command
/// driver's result dispatcher, whose debug-build frame is on the stack of
/// every click, and an `EntityType` held there is paid for on each of them.
#[inline(never)]
pub(crate) fn two_lines_placed_entity(
    document: &acadrust::CadDocument,
    first_ends: [ParametricRef; 2],
    first_pick: glam::DVec3,
    second_line: ParametricRef,
    second_ends: [ParametricRef; 2],
) -> Option<Box<acadrust::EntityType>> {
    let world = |reference: ParametricRef| {
        let point = resolve_point(document.get_entity(reference.entity)?, reference.marker?)?;
        Some(glam::DVec3::new(point.x, point.y, point.z))
    };
    let ends = |pair: [ParametricRef; 2]| Some([world(pair[0])?, world(pair[1])?]);
    let points = two_lines_placement(ends(first_ends)?, first_pick, ends(second_ends)?)?;
    let mut entity = Box::new(document.get_entity(second_line.entity)?.clone());
    let written = second_ends.iter().zip(points).all(|(reference, point)| {
        reference.marker.is_some_and(|marker| {
            set_resolved_point(&mut entity, marker, Vector3::new(point.x, point.y, point.z))
        })
    });
    written.then_some(entity)
}

/// Moves a dynamic dimension's extension origins to `first`/`second`,
/// keeping its dimension line where it was; true when anything moved.
fn dynamic_dimension_follow_points(
    dimension: &mut acadrust::entities::Dimension,
    first: Vector3,
    second: Vector3,
) -> bool {
    use acadrust::entities::Dimension;
    let same = |a: Vector3, b: Vector3| (a - b).length_squared() < 1.0e-16;
    let to_dvec = |p: Vector3| glam::DVec3::new(p.x, p.y, p.z);
    let (current_first, current_second, definition, axis) = match dimension {
        Dimension::Aligned(d) => (d.first_point, d.second_point, d.definition_point, None),
        Dimension::Linear(d) => (
            d.first_point,
            d.second_point,
            d.definition_point,
            Some(glam::DVec3::new(d.rotation.cos(), d.rotation.sin(), 0.0)),
        ),
        _ => return false,
    };
    if same(current_first, first) && same(current_second, second) {
        return false;
    }
    let text = dimension.base().user_text.clone();
    let rebuilt = match axis {
        None => crate::modules::annotate::aligned_dim::aligned_dimension_entity(
            to_dvec(first),
            to_dvec(second),
            to_dvec(definition),
            text,
        ),
        Some(axis) => crate::modules::annotate::linear_dim::linear_dimension_entity(
            to_dvec(first),
            to_dvec(second),
            to_dvec(definition),
            axis,
            text,
        ),
    };
    let acadrust::EntityType::Dimension(mut rebuilt) = rebuilt else {
        return false;
    };
    // Everything but the geometry stays: identity, layer, color, style, xdata.
    let base = dimension.base();
    let target = rebuilt.base_mut();
    target.common = base.common.clone();
    target.style_name = base.style_name.clone();
    target.text_rotation = base.text_rotation;
    target.attachment_point = base.attachment_point;
    target.line_spacing_style = base.line_spacing_style;
    target.line_spacing_factor = base.line_spacing_factor;
    *dimension = rebuilt;
    true
}

/// The end points of a line reference: a line's two ends, or the picked
/// polyline segment's vertices.
fn line_ends(entity: &acadrust::EntityType, reference: ParametricRef) -> Option<(Vector3, Vector3)> {
    let (start, end) = match reference.segment_index() {
        Some(index) => {
            let index = index as i32;
            let end = resolve_point(entity, index + 1).map_or(0, |_| index + 1);
            (index, end)
        }
        None => (0, 1),
    };
    resolve_point(entity, start).zip(resolve_point(entity, end))
}

/// Where an angular constraint's sides are now: both lines' ends for a
/// two-line angle, `[vertex, first, second]` for a three-point one.
fn angular_follow_points(
    document: &acadrust::CadDocument,
    constraint: &ParametricConstraint,
) -> Option<Vec<Vector3>> {
    match (constraint.kind, constraint.refs.as_slice()) {
        (ConstraintKind::Angle, [first, second]) => {
            let (a, b) = line_ends(document.get_entity(first.entity)?, *first)?;
            let (c, d) = line_ends(document.get_entity(second.entity)?, *second)?;
            Some(vec![a, b, c, d])
        }
        (ConstraintKind::Angle3Point, [first, vertex, second]) => {
            let world = |reference: &ParametricRef| {
                resolve_point(document.get_entity(reference.entity)?, reference.marker?)
            };
            Some(vec![world(vertex)?, world(first)?, world(second)?])
        }
        _ => None,
    }
}

/// A radial constraint's circle or arc: its centre and radius now.
fn radial_geometry(
    document: &acadrust::CadDocument,
    constraint: &ParametricConstraint,
) -> Option<(glam::DVec3, f64)> {
    let reference = constraint.refs.first()?;
    let (center, radius) = match document.get_entity(reference.entity)? {
        acadrust::EntityType::Circle(circle) => (circle.center, circle.radius),
        acadrust::EntityType::Arc(arc) => (arc.center, arc.radius),
        _ => return None,
    };
    Some((glam::DVec3::new(center.x, center.y, center.z), radius))
}

/// Rebuilds a dynamic radial dimension on its circle's current centre and
/// radius, keeping the direction its dimension line was placed in; true
/// when anything moved.
fn dynamic_dimension_follow_radial(
    dimension: &mut acadrust::entities::Dimension,
    center: glam::DVec3,
    radius: f64,
) -> bool {
    use acadrust::entities::Dimension;
    let to_dvec = |p: Vector3| glam::DVec3::new(p.x, p.y, p.z);
    let same = |a: Vector3, b: Vector3| (a - b).length_squared() < 1.0e-16;
    let text = dimension.base().user_text.clone();
    let (rebuilt, leader) = match &*dimension {
        Dimension::Radius(d) => {
            let direction = to_dvec(d.definition_point) - to_dvec(d.angle_vertex);
            let direction = direction
                .try_normalize()
                .unwrap_or(glam::DVec3::X);
            (
                crate::modules::annotate::radius_dim::radius_constraint_entity(
                    center,
                    radius,
                    center + direction * radius.max(1.0e-9),
                    text,
                ),
                d.leader_length,
            )
        }
        Dimension::Diameter(d) => {
            let direction = to_dvec(d.angle_vertex) - to_dvec(d.definition_point);
            let direction = direction
                .try_normalize()
                .unwrap_or(glam::DVec3::X);
            (
                crate::modules::annotate::diameter_dim::diameter_constraint_entity(
                    center,
                    radius,
                    center + direction * radius.max(1.0e-9),
                    text,
                ),
                d.leader_length,
            )
        }
        _ => return false,
    };
    let Some(acadrust::EntityType::Dimension(mut rebuilt)) = rebuilt else {
        return false;
    };
    let unchanged = match (&*dimension, &rebuilt) {
        (Dimension::Radius(old), Dimension::Radius(new)) => {
            same(old.angle_vertex, new.angle_vertex)
                && same(old.definition_point, new.definition_point)
        }
        (Dimension::Diameter(old), Dimension::Diameter(new)) => {
            same(old.angle_vertex, new.angle_vertex)
                && same(old.definition_point, new.definition_point)
        }
        _ => false,
    };
    if unchanged {
        return false;
    }
    match &mut rebuilt {
        Dimension::Radius(new) => new.leader_length = leader,
        Dimension::Diameter(new) => new.leader_length = leader,
        _ => {}
    }
    let base = dimension.base();
    let target = rebuilt.base_mut();
    target.common = base.common.clone();
    target.style_name = base.style_name.clone();
    target.text_rotation = base.text_rotation;
    target.attachment_point = base.attachment_point;
    target.line_spacing_style = base.line_spacing_style;
    target.line_spacing_factor = base.line_spacing_factor;
    *dimension = rebuilt;
    true
}

/// Rebuilds a dynamic angular dimension on its sides' current positions,
/// keeping its arc point; true when anything moved.
fn dynamic_dimension_follow_angle(
    dimension: &mut acadrust::entities::Dimension,
    points: &[Vector3],
) -> bool {
    use acadrust::entities::Dimension;
    let same = |a: Vector3, b: Vector3| (a - b).length_squared() < 1.0e-16;
    let to_dvec = |p: Vector3| glam::DVec3::new(p.x, p.y, p.z);
    let text = dimension.base().user_text.clone();
    let rebuilt = match (&*dimension, points) {
        (Dimension::Angular2Ln(d), [a, b, c, e]) => {
            if same(d.first_point, *a)
                && same(d.second_point, *b)
                && same(d.angle_vertex, *c)
                && same(d.definition_point, *e)
            {
                return false;
            }
            crate::modules::annotate::angular_dim::angular_two_line_entity(
                to_dvec(*a),
                to_dvec(*b),
                to_dvec(*c),
                to_dvec(*e),
                to_dvec(d.dimension_arc),
                text,
            )
        }
        (Dimension::Angular3Pt(d), [vertex, first, second]) => {
            if same(d.angle_vertex, *vertex)
                && same(d.first_point, *first)
                && same(d.second_point, *second)
            {
                return false;
            }
            crate::modules::annotate::angular_dim::angular_three_point_entity(
                to_dvec(*vertex),
                to_dvec(*first),
                to_dvec(*second),
                to_dvec(d.definition_point),
                text,
            )
        }
        _ => return false,
    };
    let Some(acadrust::EntityType::Dimension(mut rebuilt)) = rebuilt else {
        return false;
    };
    let base = dimension.base();
    let target = rebuilt.base_mut();
    target.common = base.common.clone();
    target.style_name = base.style_name.clone();
    target.text_rotation = base.text_rotation;
    target.attachment_point = base.attachment_point;
    target.line_spacing_style = base.line_spacing_style;
    target.line_spacing_factor = base.line_spacing_factor;
    *dimension = rebuilt;
    true
}

/// What stays put when a dimensional constraint's value changes: its first
/// point, and — when the distance runs perpendicular to a line that owns
/// that point (2Lines, line-first Point & line) — the whole line, so the
/// other object moves, as in the reference. An angle keeps its first side
/// (a two-line angle) or its vertex and first point (a three-point one).
pub(crate) fn dimensional_anchor_refs(
    document: &acadrust::CadDocument,
    refs: &[ParametricRef],
) -> Vec<ParametricRef> {
    let Some(&first) = refs.first() else {
        return Vec::new();
    };
    // A whole circle or arc (a radius or diameter): its centre stays and the
    // value resizes it. This is what a parameter edit through -PARAMETERS or
    // Properties goes through; the command itself passes the anchor directly.
    if refs.len() == 1
        && first.marker.is_none()
        && matches!(
            document.get_entity(first.entity),
            Some(acadrust::EntityType::Circle(_) | acadrust::EntityType::Arc(_))
        )
    {
        return vec![ParametricRef::center(first.entity)];
    }
    // Three point references: a three-point angle `[first, vertex, second]`.
    if refs.len() == 3
        && refs
            .iter()
            .all(|reference| reference.marker.is_some() && reference.segment_index().is_none())
    {
        return vec![refs[1], first];
    }
    // A two-line angle whose first side is a polyline segment: pin its ends.
    if refs.len() == 2 && first.marker.is_none() {
        return vec![first];
    }
    if let (2, Some(index)) = (refs.len(), first.segment_index()) {
        let index = index as i32;
        let end = document
            .get_entity(first.entity)
            .and_then(|entity| resolve_point(entity, index + 1))
            .map_or(0, |_| index + 1);
        return vec![
            ParametricRef::point(first.entity, index),
            ParametricRef::point(first.entity, end),
        ];
    }
    let mut anchors = vec![first];
    if let Some(&line) = refs.get(2).filter(|line| line.entity == first.entity) {
        let ends = match line.segment_index() {
            Some(index) => {
                let index = index as i32;
                let end = document
                    .get_entity(line.entity)
                    .and_then(|entity| resolve_point(entity, index + 1))
                    .map_or(0, |_| index + 1);
                [
                    ParametricRef::point(line.entity, index),
                    ParametricRef::point(line.entity, end),
                ]
            }
            None => [
                ParametricRef::point(line.entity, 0),
                ParametricRef::point(line.entity, 1),
            ],
        };
        anchors.extend(ends.into_iter().filter(|end| *end != first));
    }
    anchors
}

/// The constraint a dynamic dimension shows, with the table its parameter
/// lives in.
pub(crate) fn dynamic_dimension_constraint(
    sets: &[ParametricConstraintSet],
    handle: Handle,
) -> Option<(&ParametricConstraintSet, &ParametricConstraint)> {
    sets.iter().find_map(|set| {
        let id = set
            .dimensions
            .iter()
            .find_map(|(id, dimension)| (*dimension == handle).then_some(*id))?;
        Some((set, set.get(id)?))
    })
}

/// Grabbed points are exact kernel inputs; the solver anchors the remaining
/// endpoint coordinates according to the line's directional constraints.
pub(crate) fn grip_solve_anchor_refs(
    entity: &acadrust::EntityType,
    handle: Handle,
    grip_id: usize,
) -> Vec<ParametricRef> {
    match entity {
        acadrust::EntityType::Line(_) if grip_id <= 1 => {
            vec![ParametricRef::point(handle, grip_id as i32)]
        }
        acadrust::EntityType::LwPolyline(polyline) if grip_id < polyline.vertices.len() => {
            vec![ParametricRef::point(handle, grip_id as i32)]
        }
        acadrust::EntityType::LwPolyline(polyline) => {
            let segment = grip_id - polyline.vertices.len();
            if polyline.vertices.get(segment).is_some_and(|vertex| vertex.bulge.abs() < 1e-9)
                && (segment + 1 < polyline.vertices.len() || polyline.is_closed)
            {
                vec![ParametricRef::segment(handle, segment)]
            } else {
                Vec::new()
            }
        }
        acadrust::EntityType::Polyline2D(polyline) if grip_id < polyline.vertices.len() => {
            vec![ParametricRef::point(handle, grip_id as i32)]
        }
        acadrust::EntityType::Arc(_) => match grip_id {
            0 => vec![ParametricRef::center(handle)],
            1..=3 => vec![ParametricRef::whole(handle)],
            _ => Vec::new(),
        },
        acadrust::EntityType::Circle(_) => match grip_id {
            0 => vec![ParametricRef::center(handle)],
            1..=4 => vec![ParametricRef::whole(handle)],
            _ => Vec::new(),
        },
        acadrust::EntityType::Ellipse(_) => match grip_id {
            0 => vec![ParametricRef::center(handle)],
            1..=6 => vec![ParametricRef::whole(handle)],
            _ => Vec::new(),
        },
        acadrust::EntityType::Point(_)
        | acadrust::EntityType::Insert(_)
        | acadrust::EntityType::Text(_)
        | acadrust::EntityType::MText(_)
            if grip_id == 0 =>
        {
            vec![ParametricRef::point(handle, 0)]
        }
        _ => Vec::new(),
    }
}

/// The friendly, user-facing constraint types — the "what button did they
/// click" vocabulary, one layer above the `cadkernel_constraints` primitives each maps
/// onto (that mapping is `constraint_map`, a later stage; see the design
/// doc §2). Named and grouped the same way the existing one-shot ribbon
/// tools are (`crate::modules::parametric::tools`), plus the
/// endpoint-picking kinds (`Coincident`, `Radius`, `Tangent`) that one-shot
/// group never needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstraintKind {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    /// Distance between two points, or a single line's length, or a
    /// circle/arc's diameter — which reading applies depends on `refs`'
    /// shape, mirroring how `DistanceConstraintCommand`
    /// (`src/modules/draw/constrain/value.rs`) already infers it from the
    /// selected entity.
    Distance,
    Angle,
    /// Angle defined by two rays sharing the middle point.
    Angle3Point,
    Radius,
    Tangent,
    /// An open spline endpoint remains curvature-continuous with another bounded curve endpoint.
    Smooth,
    /// Two circles/arcs share a center — `refs`: `[center(a), center(b)]`.
    /// Solves identically to `Coincident` (`parametric_solve.rs` broadens that
    /// match arm rather than duplicating it) — only the DWG-native class
    /// name (`ACCONCENTRICCONSTRAINT` vs `ACPOINTCOINCIDENCECONSTRAINT`)
    /// and the UI entry point differ.
    Concentric,
    /// A point sits at a circle/arc's center — `refs`: `[point, center(circle)]`.
    /// Same solver math as `Coincident`/`Concentric`, different DWG class
    /// name (`ACCENTERPOINTCONSTRAINT`).
    CenterPoint,
    /// Two lines share the same infinite line — `refs`: `[whole(a), whole(b)]`.
    Colinear,
    /// A point sits at another line's midpoint — `refs`: `[point, whole(line)]`.
    Midpoint,
    /// Locks a whole entity at its current position — `refs`: `[whole(entity)]`.
    /// No `driving_param`: the target is the entity's own live geometry at
    /// solve time, not a typed value (see `parametric_solve.rs`'s `Fixed` arm).
    Fixed,
    /// A point lies anywhere along a line's or circle's curve (not
    /// restricted to an endpoint/center) — `refs`: `[point, whole(entity)]`.
    PointOnCurve,
    /// The distance between one point pair equals the distance between
    /// another — `refs`: `[p1, p2, p3, p4]` (`dist(p1,p2) == dist(p3,p4)`).
    /// For the supported entity types, equal curvature is already covered
    /// by the circle/circle radius branch of `Equal`.
    EqualDistance,
    /// Two points or compatible curves are symmetric across a line.
    /// Point mode stores two point refs plus the axis. Object mode stores two
    /// whole/segment refs plus the axis and constrains the complete relevant
    /// geometry: line direction, circular center/radius, or ellipse
    /// center/axes.
    Symmetric,
    /// A circle/arc's diameter (twice `Radius`'s target) — `refs`:
    /// `[whole(circle_or_arc)]`. Same DWG class as `Radius`
    /// (`ACRADIUSDIAMETERCONSTRAINT`), distinguished only by the
    /// `RadiusDiameterConstrType` mode byte
    /// (`dwg_native_constraints.rs`), rather than a distinct object type.
    Diameter,
    /// The X-only (resp. Y-only) component of the distance between two
    /// points — `refs`: `[p1, p2]`, same shape as `Distance`. Same DWG
    /// class as `Distance` (`ACDISTANCECONSTRAINT`) with its
    /// `DirectionType` set to a fixed `(1,0,0)`/`(0,1,0)` direction.
    DistanceX,
    DistanceY,
    /// Signed point-to-point distance along an arbitrary fixed direction,
    /// or along a direction derived from a third line reference.
    DistanceDirected,
    /// A line perpendicular to a circle/arc's tangent at their point of
    /// contact — `refs`: `[whole(a), whole(b)]`, either order. For the
    /// Line/Circle-only entity model this is equivalent to "the line
    /// passes through the circle's center" (a circle's radius is always
    /// normal to its own tangent), so it solves via the same `PointOnLine`
    /// primitive `PointOnCurve` already uses. Distinct from
    /// `Perpendicular` (line-to-line only). Line-Line has no meaning here
    /// (that's plain `Perpendicular`) and isn't buildable.
    Normal,
    /// A standard rigid geometry container. Its referenced points keep their
    /// original relative distances while the whole set may translate or rotate.
    RigidSet,
}

pub type ConstraintId = u32;

pub(crate) mod distance_direction_type {
    pub const UNDIRECTED: u8 = 0;
    pub const FIXED: u8 = 1;
    pub const PARALLEL_TO_LINE: u8 = 2;
    pub const PERPENDICULAR_TO_LINE: u8 = 3;
}

pub(crate) mod angle_sector {
    pub const PARALLEL_COUNTERCLOCKWISE: u8 = 0;
    pub const ANTIPARALLEL_CLOCKWISE: u8 = 1;
    pub const PARALLEL_CLOCKWISE: u8 = 2;
    pub const ANTIPARALLEL_COUNTERCLOCKWISE: u8 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeConstraintOrigin {
    pub(crate) group: Handle,
    pub(crate) node: i32,
}

/// One runtime constraint: a friendly [`ConstraintKind`], the
/// entities/points it relates, and — for a dimensional kind — the value
/// driving it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParametricConstraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub refs: Vec<ParametricRef>,
    /// The target for a dimensional constraint (a `Distance`'s length, an
    /// `Angle`'s degrees, a `Radius`'s radius) — a literal number or a
    /// named-parameter reference resolved through `Scene::named_parameters` at solve time by
    /// `parametric_solve::build_constraint`). `None` for every purely-geometric
    /// kind (Coincident, Horizontal, Vertical, Parallel, Perpendicular,
    /// Equal, Tangent).
    pub driving_param: Option<DrivingValue>,
    /// Lets a user suppress a constraint without losing it — a re-solve
    /// skips a disabled constraint entirely.
    pub enabled: bool,
    /// Standard graph node retained in place because its group also contains
    /// graph features that are not safe to rebuild independently.
    pub(crate) native_origin: Option<NativeConstraintOrigin>,
    /// Original point positions for a standard rigid set. This is rebuilt
    /// from the associative graph on open and never stored separately.
    pub(crate) rigid_points: Vec<(ParametricRef, Vector3)>,
    /// Standard distance direction mode and vector. A third entry in `refs`
    /// identifies the direction line for the line-relative modes.
    pub(crate) distance_direction_type: u8,
    pub(crate) distance_direction: Option<Vector3>,
    /// World-space datum direction captured for Horizontal/Vertical. Native
    /// files store the same vector on the connected constrained datum line.
    /// `None` keeps legacy world-X/world-Y behavior.
    pub(crate) axis_direction: Option<Vector3>,
    /// Which of the four directed sectors an angular constraint measures.
    pub(crate) angle_sector: u8,
}

/// A constraint scope: model space or one block definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParametricScope {
    ModelSpace,
    /// A block definition's `BlockRecord` handle — matches
    /// `BlockEditSession::br_handle`
    /// (`src/modules/draw/modify/block_edit.rs`).
    Block(Handle),
}

impl ParametricScope {
    /// The owner handle under which this scope is persisted.
    pub fn owner_handle(&self, document: &acadrust::CadDocument) -> Handle {
        match self {
            ParametricScope::ModelSpace => document.header.model_space_block_handle,
            ParametricScope::Block(handle) => *handle,
        }
    }
}

/// Every constraint decoded from one standard graph scope. Solver state is rebuilt
/// on demand and is not stored here.
#[derive(Debug, Clone)]
pub struct ParametricConstraintSet {
    pub scope: ParametricScope,
    pub constraints: Vec<ParametricConstraint>,
    next_id: ConstraintId,
    /// Parameters owned by this standard block scope. Model-space and
    /// command-created constraints continue to use the drawing parameter
    /// table; block definitions need their own namespace because identical
    /// parameter names may legitimately occur in different blocks.
    pub(crate) local_parameters: super::named_parameters::ParameterTable,
    pub(crate) retained_standard_groups: Vec<Handle>,
    /// The dynamic dimension that shows a dimensional constraint, by id.
    pub(crate) dimensions: std::collections::HashMap<ConstraintId, Handle>,
    /// Cached total remaining degrees of freedom, summed across every
    /// independent solve partition in this scope — updated by
    /// `parametric_solve::solve_scope` each time this set is resolved. `None`
    /// until the first resolve (e.g. right after loading from disk, before
    /// any edit has touched this scope yet). The status badge reads this
    /// rather than recomputing it every frame. Not persisted:
    /// it's a derived cache, not real constraint state.
    pub dof: Option<usize>,
    /// Cached redundant/conflicting constraints found by the last resolve.
    /// Also a derived cache, not
    /// persisted; a `ConflictResolverPanel` reads this rather than calling
    /// `cadkernel_constraints::diagnosis::classify_redundant` itself.
    pub conflicts: Vec<(
        ConstraintId,
        cadkernel_constraints::diagnosis::RedundancyKind,
    )>,
}

impl ParametricConstraintSet {
    pub fn new(scope: ParametricScope) -> Self {
        Self {
            scope,
            constraints: Vec::new(),
            next_id: 0,
            local_parameters: super::named_parameters::ParameterTable::new(),
            retained_standard_groups: Vec::new(),
            dimensions: std::collections::HashMap::new(),
            dof: None,
            conflicts: Vec::new(),
        }
    }

    /// Appends a constraint, assigning it a fresh id unique within this set.
    pub fn add(
        &mut self,
        kind: ConstraintKind,
        refs: Vec<ParametricRef>,
        driving_param: Option<DrivingValue>,
    ) -> ConstraintId {
        let id = self.next_id;
        self.next_id += 1;
        self.constraints.push(ParametricConstraint {
            id,
            kind,
            refs,
            driving_param,
            enabled: true,
            native_origin: None,
            rigid_points: Vec::new(),
            distance_direction_type: 0,
            distance_direction: None,
            axis_direction: None,
            angle_sector: angle_sector::PARALLEL_COUNTERCLOCKWISE,
        });
        id
    }

    /// Appends a Horizontal/Vertical constraint tied to an explicit datum
    /// direction rather than silently interpreting it in world coordinates.
    pub fn add_axis_constraint(
        &mut self,
        kind: ConstraintKind,
        refs: Vec<ParametricRef>,
        direction: Vector3,
    ) -> ConstraintId {
        let fallback = if kind == ConstraintKind::Vertical {
            Vector3::UNIT_Y
        } else {
            Vector3::UNIT_X
        };
        let direction = if direction.length_squared() > 1.0e-24 {
            direction.normalize()
        } else {
            fallback
        };
        let id = self.add(kind, refs, None);
        if let Some(constraint) = self.constraints.last_mut() {
            constraint.axis_direction = Some(direction);
        }
        id
    }

    pub fn contains_axis_constraint(
        &self,
        kind: ConstraintKind,
        refs: &[ParametricRef],
        direction: Vector3,
    ) -> bool {
        let fallback = if kind == ConstraintKind::Vertical {
            Vector3::UNIT_Y
        } else {
            Vector3::UNIT_X
        };
        let direction = if direction.length_squared() > 1.0e-24 {
            direction.normalize()
        } else {
            fallback
        };
        self.constraints.iter().any(|constraint| {
            if !constraint.enabled || constraint.kind != kind {
                return false;
            }
            let same_refs = constraint.refs == refs
                || (refs.len() == 2
                    && constraint.refs.len() == 2
                    && constraint.refs[0] == refs[1]
                    && constraint.refs[1] == refs[0]);
            if !same_refs {
                return false;
            }
            let existing = constraint.axis_direction.unwrap_or(fallback);
            existing.length_squared() > 1.0e-24
                && existing.normalize().dot(&direction).abs() >= 1.0 - 1.0e-10
        })
    }

    /// Removes a constraint by id. Returns whether one was actually removed.
    pub fn remove(&mut self, id: ConstraintId) -> bool {
        self.dimensions.remove(&id);
        let before = self.constraints.len();
        self.constraints.retain(|c| c.id != id);
        self.constraints.len() != before
    }

    pub fn get(&self, id: ConstraintId) -> Option<&ParametricConstraint> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// Every enabled constraint referencing `entity`.
    pub fn constraints_touching(
        &self,
        entity: Handle,
    ) -> impl Iterator<Item = &ParametricConstraint> {
        self.constraints
            .iter()
            .filter(move |c| c.enabled && c.refs.iter().any(|r| r.entity == entity))
    }

    /// Drops every constraint that references `entity` and returns the
    /// removed constraint ids.
    pub fn remove_all_touching(&mut self, entity: Handle) -> Vec<ConstraintId> {
        let (removed, kept): (Vec<_>, Vec<_>) = self
            .constraints
            .drain(..)
            .partition(|c| c.refs.iter().any(|r| r.entity == entity));
        self.constraints = kept;
        removed.into_iter().map(|c| c.id).collect()
    }
}

/// Resolves a [`ParametricRef`] to its current world-space point, for building
/// an `cadkernel_constraints` `ParamStore` from live document geometry — the constraint
/// endpoint's equivalent of `dimension_assoc::resolve_reference`, restricted
/// to the marker conventions constraint endpoints actually use (whole-entity
/// `None`, an ordinary `source_points()` index, the `-3` center case, or a
/// bounded curve/segment midpoint, or a curved polyline-segment center).
///
/// Solver-side registration reads raw entity fields directly. This helper is
/// for UI-side consumers that need the current world-space position.
/// The single addressable point (marker `0`) of a point-like entity: the
/// node, insertion, or table origin the solver registers as its
/// `EntityGeom::Point` / text baseline start. `None` for curve entities,
/// whose marker `0` is a `source_points()` endpoint.
pub(crate) fn insertion_point(entity: &acadrust::EntityType) -> Option<Vector3> {
    match entity {
        acadrust::EntityType::Point(point) => Some(point.location),
        acadrust::EntityType::Insert(insert) => Some(insert.insert_point),
        acadrust::EntityType::Text(text) => Some(text.insertion_point),
        acadrust::EntityType::MText(text) => Some(text.insertion_point),
        acadrust::EntityType::AttributeDefinition(attribute) => Some(attribute.insertion_point),
        acadrust::EntityType::AttributeEntity(attribute) => Some(attribute.insertion_point),
        acadrust::EntityType::Table(table) => Some(table.insertion_point),
        _ => None,
    }
}

pub(crate) fn resolve_point(entity: &acadrust::EntityType, marker: i32) -> Option<Vector3> {
    if marker == 0 {
        if let Some(point) = insertion_point(entity) {
            return Some(point);
        }
    }
    if marker == -3 {
        return match entity {
            acadrust::EntityType::Circle(circle) => Some(circle.center_wcs()),
            acadrust::EntityType::Arc(arc) => Some(arc.center_wcs()),
            acadrust::EntityType::Ellipse(ellipse) => Some(ellipse.center),
            _ => None,
        };
    }
    if marker == -2 {
        let curve = crate::entities::curve::entity_curve(entity)?;
        if curve.is_closed() {
            return None;
        }
        let point = curve.point_at(0.5);
        return Some(Vector3::new(point[0], point[1], point[2]));
    }
    let reference = ParametricRef {
        entity: Handle::NULL,
        marker: Some(marker),
    };
    if let Some(segment) = reference.segment_center_index() {
        let planar = crate::entities::curve::entity_curve(entity)?;
        let curve = planar.curve.segments().into_iter().nth(segment)?;
        let cadkernel::geom2d::Curve::Arc(arc) = curve else {
            return None;
        };
        let point = planar.plane.point_at(arc.centre);
        return Some(Vector3::new(point[0], point[1], point[2]));
    }
    if let Some(segment) = reference.segment_midpoint_index() {
        let planar = crate::entities::curve::entity_curve(entity)?;
        let curve = planar.curve.segments().into_iter().nth(segment)?;
        let point = planar.plane.point_at(curve.point_at(0.5));
        return Some(Vector3::new(point[0], point[1], point[2]));
    }
    if marker < 0 {
        return None;
    }
    let points = super::dimension_assoc::source_points(entity);
    // The closing segment of a closed polyline runs back to vertex 0, so its
    // end marker (one past the last vertex) names that vertex — the same wrap
    // the solver's `line_segment` applies.
    let index = match polyline_vertex_index(entity, marker as usize) {
        Some(index) => index,
        None => marker as usize,
    };
    points.get(index).copied()
}

/// `marker` as a vertex index of a polyline, wrapping the closing segment's
/// end (one past the last vertex) onto vertex 0 when the polyline is closed.
/// `None` for anything that is not a 2D polyline.
pub(crate) fn polyline_vertex_index(entity: &acadrust::EntityType, marker: usize) -> Option<usize> {
    let (count, closed) = match entity {
        acadrust::EntityType::LwPolyline(polyline) => (polyline.vertices.len(), polyline.is_closed),
        acadrust::EntityType::Polyline2D(polyline) => (polyline.vertices.len(), polyline.is_closed()),
        _ => return None,
    };
    Some(if closed && marker == count && count > 0 { 0 } else { marker })
}

/// Below this squared distance (1e-6 world units), two points count as
/// already coincident for [`nearest_parametric_point`]'s purposes.
const COINCIDENT_EPSILON_SQ: f64 = 1.0e-12;

/// Addressable constraint points for one entity, in the same marker space
/// used by persistent constraint references.
pub(crate) fn parametric_point_candidates(
    entity: &acadrust::EntityType,
) -> Vec<(i32, Vector3)> {
    let mut points: Vec<_> = super::dimension_assoc::source_points(entity)
        .into_iter()
        .enumerate()
        .map(|(marker, point)| (marker as i32, point))
        .collect();
    // A node / insertion point is a constraint point too (the reference
    // fixes a POINT via NODe and TEXT via INSert), and these entities have
    // no `source_points()` to collide with marker `0`.
    if let Some(point) = insertion_point(entity) {
        points.push((0, point));
    }
    match entity {
        acadrust::EntityType::Circle(circle) => points.push((-3, circle.center_wcs())),
        acadrust::EntityType::Arc(arc) => points.push((-3, arc.center_wcs())),
        acadrust::EntityType::Ellipse(ellipse) => points.push((-3, ellipse.center)),
        _ => {}
    }

    if matches!(
        entity,
        acadrust::EntityType::Line(_)
            | acadrust::EntityType::Arc(_)
            | acadrust::EntityType::Spline(_)
            | acadrust::EntityType::Ellipse(_)
    ) {
        if let Some(curve) = crate::entities::curve::entity_curve(entity) {
            if !curve.is_closed() {
                let point = curve.point_at(0.5);
                points.push((-2, Vector3::new(point[0], point[1], point[2])));
            }
        }
    }

    if matches!(
        entity,
        acadrust::EntityType::LwPolyline(_) | acadrust::EntityType::Polyline2D(_)
    ) {
        if let Some(planar) = crate::entities::curve::entity_curve(entity) {
            points.extend(planar.curve.segments().into_iter().enumerate().map(
                |(index, curve)| {
                    let point = planar.plane.point_at(curve.point_at(0.5));
                    (
                        POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE - index as i32,
                        Vector3::new(point[0], point[1], point[2]),
                    )
                },
            ));
        }
    }
    points
}

pub(crate) fn is_parametric_point_near(entity: &acadrust::EntityType, point: Vector3) -> bool {
    parametric_point_candidates(entity)
        .into_iter()
        .any(|(_, candidate)| (candidate - point).length_squared() <= COINCIDENT_EPSILON_SQ)
}

/// Finds the addressable entity point nearest a snapped world position.
/// Returns `None` when no point in the scope is within the coincidence
/// tolerance.
pub(crate) fn nearest_parametric_point(
    document: &acadrust::CadDocument,
    scope: ParametricScope,
    world_point: Vector3,
    exclude: Option<Handle>,
) -> Option<ParametricRef> {
    let owner = scope.owner_handle(document);
    let mut best: Option<(f64, ParametricRef)> = None;
    let mut consider = |handle: Handle, marker: i32, point: Vector3| {
        let dx = point.x - world_point.x;
        let dy = point.y - world_point.y;
        let dz = point.z - world_point.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq <= COINCIDENT_EPSILON_SQ && best.as_ref().is_none_or(|(d, _)| dist_sq < *d) {
            best = Some((dist_sq, ParametricRef::point(handle, marker)));
        }
    };
    for candidate in document.entities() {
        let common = candidate.common();
        if common.owner_handle != owner || Some(common.handle) == exclude {
            continue;
        }
        for (marker, point) in parametric_point_candidates(candidate) {
            consider(common.handle, marker, point);
        }
    }
    best.map(|(_, r)| r)
}

/// Resolve a point pick within one explicitly selected entity.  This keeps two
/// different endpoints at the same world coordinate distinguishable.
pub(crate) fn nearest_parametric_point_on_entity(
    document: &acadrust::CadDocument,
    scope: ParametricScope,
    handle: Handle,
    world_point: Vector3,
) -> Option<ParametricRef> {
    let entity = document.get_entity(handle)?;
    if entity.common().owner_handle != scope.owner_handle(document) {
        return None;
    }
    parametric_point_candidates(entity)
        .into_iter()
        .filter_map(|(marker, point)| {
            let distance = (point - world_point).length_squared();
            (distance <= COINCIDENT_EPSILON_SQ)
                .then_some((distance, ParametricRef::point(handle, marker)))
        })
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, reference)| reference)
}

/// Resolve the whole curve or the picked polyline segment used by a
/// point-to-curve Coincident relation.
pub(crate) fn parametric_curve_ref_for_pick(
    document: &acadrust::CadDocument,
    scope: ParametricScope,
    handle: Handle,
    world_point: Vector3,
) -> Option<ParametricRef> {
    let entity = document.get_entity(handle)?;
    if entity.common().owner_handle != scope.owner_handle(document) {
        return None;
    }
    match entity {
        acadrust::EntityType::Line(_)
        | acadrust::EntityType::Circle(_)
        | acadrust::EntityType::Arc(_)
        | acadrust::EntityType::Ellipse(_)
        | acadrust::EntityType::Spline(_) => Some(ParametricRef::whole(handle)),
        acadrust::EntityType::LwPolyline(_) | acadrust::EntityType::Polyline2D(_) => {
            let segments = crate::entities::curve::entity_curve_xy(entity)?.segments();
            cadkernel::geom2d::nearest_of(segments.iter(), [world_point.x, world_point.y])
                .map(|(index, _)| ParametricRef::segment(handle, index))
        }
        _ => None,
    }
}

impl ConstraintKind {
    /// Per-kind visibility bit used by CONSTRAINTBARMODE for the standard
    /// geometric-constraint family. Helper relations which have no standard
    /// bit stay visible under the normal display policy.
    pub const fn bar_mode_bit(self) -> Option<i16> {
        match self {
            Self::Horizontal => Some(1),
            Self::Vertical => Some(2),
            Self::Perpendicular => Some(4),
            Self::Parallel => Some(8),
            Self::Tangent => Some(16),
            Self::Smooth => Some(32),
            Self::Coincident => Some(64),
            Self::Concentric => Some(128),
            Self::Colinear => Some(256),
            Self::Symmetric => Some(512),
            Self::Equal => Some(1024),
            Self::Fixed => Some(2048),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Coincident => "Coincident",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Equal => "Equal",
            Self::Distance => "Distance",
            Self::Angle => "Angle",
            Self::Angle3Point => "3-point angle",
            Self::Radius => "Radius",
            Self::Tangent => "Tangent",
            Self::Smooth => "Smooth",
            Self::Concentric => "Concentric",
            Self::CenterPoint => "Center point",
            Self::Colinear => "Collinear",
            Self::Midpoint => "Midpoint",
            Self::Fixed => "Fixed",
            Self::PointOnCurve => "Point on curve",
            Self::EqualDistance => "Equal distance",
            Self::Symmetric => "Symmetric",
            Self::Diameter => "Diameter",
            Self::DistanceX => "Horizontal distance",
            Self::DistanceY => "Vertical distance",
            Self::DistanceDirected => "Directed distance",
            Self::Normal => "Normal",
            Self::RigidSet => "Rigid set",
        }
    }

    /// The short symbol a constraint glyph shows — matches the existing
    /// ribbon icons (`crate::modules::parametric::{tools,value}`) for
    /// the kinds that have a one-click button, so the same glyph means the
    /// same thing in both places.
    pub fn glyph_symbol(&self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "≡",
            ConstraintKind::Horizontal => "—",
            ConstraintKind::Vertical => "│",
            ConstraintKind::Parallel => "∥",
            ConstraintKind::Perpendicular => "⊥",
            ConstraintKind::Equal => "=",
            ConstraintKind::Distance => "↔",
            ConstraintKind::Angle => "∠",
            ConstraintKind::Angle3Point => "∠₃",
            ConstraintKind::Radius => "R",
            ConstraintKind::Tangent => "T",
            ConstraintKind::Smooth => "G²",
            ConstraintKind::Concentric => "◎",
            ConstraintKind::CenterPoint => "⊕",
            ConstraintKind::Colinear => "L",
            ConstraintKind::Midpoint => "M",
            ConstraintKind::Fixed => "F",
            ConstraintKind::PointOnCurve => "∈",
            ConstraintKind::EqualDistance => "≐",
            ConstraintKind::Symmetric => "S",
            ConstraintKind::Diameter => "⌀",
            ConstraintKind::DistanceX => "↔ₓ",
            ConstraintKind::DistanceY => "↔ᵧ",
            ConstraintKind::DistanceDirected => "↗",
            ConstraintKind::Normal => "⊾",
            ConstraintKind::RigidSet => "▣",
        }
    }
}

/// Fixed draws a padlock (`src/ui/overlay.rs`), the way the reference bar
/// does: the plain symbol for a held curve or segment, this variant when one
/// addressable point is held (white lock with the point marker).
pub(crate) const FIXED_POINT_GLYPH: &str = "F·";

pub(crate) fn fixed_glyph_label(constraint: &ParametricConstraint) -> &'static str {
    let point = constraint
        .refs
        .first()
        .is_some_and(|reference| reference.marker.is_some() && reference.segment_index().is_none());
    if point {
        FIXED_POINT_GLYPH
    } else {
        ConstraintKind::Fixed.glyph_symbol()
    }
}

/// Vertical draws the reference bar's axis mark (`src/ui/overlay.rs`): the
/// plain symbol for an object, this variant — with the point marker — for a
/// two-point relation.
pub(crate) const VERTICAL_POINTS_GLYPH: &str = "│·";

pub(crate) fn vertical_glyph_label(constraint: &ParametricConstraint) -> &'static str {
    if constraint.refs.len() == 2 {
        VERTICAL_POINTS_GLYPH
    } else {
        ConstraintKind::Vertical.glyph_symbol()
    }
}

/// The full glyph text for one constraint: its symbol, plus the driving
/// value for a dimensional kind (Distance/Angle/Radius).
pub(crate) fn glyph_label(constraint: &ParametricConstraint) -> String {
    match (constraint.kind, &constraint.driving_param) {
        (ConstraintKind::Angle, Some(DrivingValue::Literal(value))) => {
            format!("{} {value:.1}°", constraint.kind.glyph_symbol())
        }
        (_, Some(DrivingValue::Literal(value))) => {
            format!("{} {value:.2}", constraint.kind.glyph_symbol())
        }
        // A named reference has no single resolved number to show without
        // threading `ParameterTable` into every glyph-render call site
        // (`src/ui/overlay.rs`) — showing the name itself is enough for now;
        // stage 4's parameters panel is the natural place to reconsider this
        // once a named `driving_param` can actually be authored through the
        // UI (nothing can yet — this arm exists so the match is exhaustive
        // and correct ahead of that UI, not because it's reachable today).
        (_, Some(DrivingValue::Named(name))) => {
            format!("{} {name}", constraint.kind.glyph_symbol())
        }
        (_, None) => constraint.kind.glyph_symbol().to_string(),
    }
}

/// World-space anchor and outward direction for a constraint glyph.
pub(crate) fn glyph_placement(
    document: &acadrust::CadDocument,
    constraint: &ParametricConstraint,
) -> Option<(Vector3, Vector3)> {
    glyph_placements(document, constraint).into_iter().next()
}

fn glyph_placement_for_reference(
    document: &acadrust::CadDocument,
    r: ParametricRef,
) -> Option<(Vector3, Vector3)> {
    let entity = document.get_entity(r.entity)?;
    let line_midpoint = |line: &acadrust::entities::Line| {
        Vector3::new(
            (line.start.x + line.end.x) * 0.5,
            (line.start.y + line.end.y) * 0.5,
            (line.start.z + line.end.z) * 0.5,
        )
    };
    let segment_normal = |start: Vector3, end: Vector3| {
        let direction = Vector3::new(-(end.y - start.y), end.x - start.x, 0.0);
        (direction.length_squared() > 1e-24)
            .then_some(direction)
            .unwrap_or(Vector3::UNIT_Y)
    };
    let line_normal = |line: &acadrust::entities::Line| segment_normal(line.start, line.end);
    if r.directional_axis().is_some() {
        let [start, end] = directional_axis_endpoints(entity, r)?;
        let anchor = (start + end) * 0.5;
        return Some((anchor, segment_normal(start, end)));
    }
    if let Some(segment) = r.segment_center_index() {
        let planar = crate::entities::curve::entity_curve(entity)?;
        let curve = planar.curve.segments().into_iter().nth(segment)?;
        let on_arc = planar.plane.point_at(curve.point_at(0.5));
        let cadkernel::geom2d::Curve::Arc(arc) = curve else {
            return None;
        };
        let center = planar.plane.point_at(arc.centre);
        let center = Vector3::new(center[0], center[1], center[2]);
        let on_arc = Vector3::new(on_arc[0], on_arc[1], on_arc[2]);
        return Some((on_arc, on_arc - center));
    }
    if let Some(segment) = r.segment_index() {
        let anchor = resolve_point(
            entity,
            POLYLINE_SEGMENT_MIDPOINT_MARKER_BASE - segment as i32,
        )?;
        let [start, end] = constraint_segment_endpoints(document, r)?;
        return Some((anchor, segment_normal(start, end)));
    }
    match (entity, r.marker) {
        (acadrust::EntityType::Line(line), None) => Some((line_midpoint(line), line_normal(line))),
        (acadrust::EntityType::Circle(circle), None | Some(-3)) => {
            let center = circle.center_wcs();
            let anchor = circle.point_at_angle_wcs(0.0);
            Some((anchor, anchor - center))
        }
        (acadrust::EntityType::Arc(arc), None | Some(-3)) => {
            let center = arc.center_wcs();
            let anchor = arc.midpoint_wcs();
            Some((anchor, anchor - center))
        }
        (acadrust::EntityType::Ellipse(ellipse), None | Some(-3)) => {
            let anchor = ellipse.center + ellipse.major_axis;
            Some((anchor, ellipse.major_axis))
        }
        (acadrust::EntityType::Line(line), Some(marker)) => {
            let anchor = resolve_point(entity, marker)?;
            let direction = anchor - line_midpoint(line);
            Some((
                anchor,
                (direction.length_squared() > 1e-24)
                    .then_some(direction)
                    .unwrap_or_else(|| line_normal(line)),
            ))
        }
        (acadrust::EntityType::Arc(arc), Some(marker)) => {
            let anchor = resolve_point(entity, marker)?;
            let direction = anchor - arc.center_wcs();
            Some((
                anchor,
                (direction.length_squared() > 1e-24)
                    .then_some(direction)
                    .unwrap_or(Vector3::UNIT_Y),
            ))
        }
        (_, Some(marker)) => {
            let anchor = resolve_point(entity, marker)?;
            Some((anchor, Vector3::UNIT_Y))
        }
        _ => None,
    }
}

fn glyph_placements(
    document: &acadrust::CadDocument,
    constraint: &ParametricConstraint,
) -> Vec<(Vector3, Vector3)> {
    // Relations the reference marks on every object they join.
    if matches!(
        constraint.kind,
        ConstraintKind::Parallel | ConstraintKind::Symmetric | ConstraintKind::Equal
    ) {
        return constraint
            .refs
            .iter()
            .filter_map(|reference| glyph_placement_for_reference(document, *reference))
            .collect();
    }

    let Some(first) = constraint.refs.first().copied() else {
        return Vec::new();
    };
    let fallback = glyph_placement_for_reference(document, first);
    if matches!(
        constraint.kind,
        ConstraintKind::Perpendicular | ConstraintKind::Tangent
    ) {
        if let [first, second, ..] = constraint.refs.as_slice() {
            if let (Some(first_curve), Some(second_curve), Some((anchor, outward))) = (
                constraint_reference_curve_xy(document, *first),
                constraint_reference_curve_xy(document, *second),
                fallback,
            ) {
                if let Some(crossing) = cadkernel::geom2d::intersect(
                    &first_curve,
                    &second_curve,
                    cadkernel::geom2d::Tolerance::default(),
                )
                .into_iter()
                .next()
                {
                    return vec![(
                        Vector3::new(crossing.point[0], crossing.point[1], anchor.z),
                        outward,
                    )];
                }
            }
        }
    }

    fallback.into_iter().collect()
}

/// World-space locations that explain what a hovered constraint acts on.
/// Point constraints expose their referenced point directly; curve relations
/// expose the contact or intersection that makes the relation visible.
fn constraint_segment_endpoints(
    document: &acadrust::CadDocument,
    reference: ParametricRef,
) -> Option<[Vector3; 2]> {
    let segment = reference.segment_index()?;
    let entity = document.get_entity(reference.entity)?;
    let points = super::dimension_assoc::source_points(entity);
    let closed = match entity {
        acadrust::EntityType::LwPolyline(polyline) => polyline.is_closed,
        acadrust::EntityType::Polyline2D(polyline) => polyline.is_closed(),
        _ => return None,
    };
    let first = *points.get(segment)?;
    let second = if segment + 1 < points.len() {
        points[segment + 1]
    } else if closed {
        *points.first()?
    } else {
        return None;
    };
    Some([first, second])
}

fn constraint_reference_curve_xy(
    document: &acadrust::CadDocument,
    reference: ParametricRef,
) -> Option<cadkernel::geom2d::Curve> {
    let entity = document.get_entity(reference.entity)?;
    if reference.directional_axis().is_some() {
        let [start, end] = directional_axis_endpoints(entity, reference)?;
        return Some(cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
        }));
    }
    let curve = crate::entities::curve::entity_curve_xy(entity)?;
    reference
        .segment_index()
        .map(|index| curve.segments().into_iter().nth(index))
        .unwrap_or(Some(curve))
}

pub(crate) fn constraint_hover_points(
    document: &acadrust::CadDocument,
    constraint: &ParametricConstraint,
) -> Vec<Vector3> {
    let mut points = Vec::new();
    let push_unique = |points: &mut Vec<Vector3>, point: Vector3| {
        if point.x.is_finite()
            && point.y.is_finite()
            && point.z.is_finite()
            && !points
                .iter()
                .any(|existing| (*existing - point).length_squared() <= 1.0e-12)
        {
            points.push(point);
        }
    };

    for reference in &constraint.refs {
        let Some(marker) = reference.marker else {
            continue;
        };
        let Some(entity) = document.get_entity(reference.entity) else {
            continue;
        };
        if let Some(point) = resolve_point(entity, marker) {
            push_unique(&mut points, point);
        }
    }


    if matches!(
        constraint.kind,
        ConstraintKind::Perpendicular | ConstraintKind::Tangent
    ) {
        if let [first, second, ..] = constraint.refs.as_slice() {
            if let (Some(first_curve), Some(second_curve)) = (
                constraint_reference_curve_xy(document, *first),
                constraint_reference_curve_xy(document, *second),
            ) {
                let elevation = glyph_placement(document, constraint)
                    .map(|(anchor, _)| anchor.z)
                    .unwrap_or(0.0);
                for crossing in cadkernel::geom2d::intersect(
                    &first_curve,
                    &second_curve,
                    cadkernel::geom2d::Tolerance::default(),
                ) {
                    push_unique(
                        &mut points,
                        Vector3::new(crossing.point[0], crossing.point[1], elevation),
                    );
                }
            }
        }
    }

    points
}

/// Memoised key for `Scene::cached_glyph_placements`.
/// `sel_sig` hashes the hide sets the dynamic pills read on EVERY display
/// mode (`preview_hidden`, `command_preview_hidden`,
/// `hidden_dynamic_dimensions` — `constraint_glyph_placements_screen` filters
/// `entity_temporarily_hidden` unconditionally) PLUS, only when
/// `display_mode & 2 != 0`, the selection-gated sets (`selected`,
/// `hidden_parametric_constraints`, `shown_parametric_constraints`, and again
/// `preview_hidden` as a selection source). Hover and selection-highlight
/// state deliberately stay OUT — they only affect the highlight, never the
/// placement set. Isolation contents stay OUT — the isolation mutators bump
/// `constraints_epoch` instead, so no key input is needed for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphKey {
    pub scope: ParametricScope,
    pub c_epoch: u64,
    pub g_epoch: u64,
    pub cam_gen: u64,
    pub active_vp: Option<Handle>,
    pub layout_is_model: bool,
    pub vp_bits: (u32, u32),
    pub show_values: bool,
    pub disp: i16,
    pub bar: i16,
    pub sel_sig: u64,
    pub annotation_scale: u32,
}

/// One cached glyph placement: the `constraint_glyph_placements_screen`
/// tuple plus its precomputed overlay layout (`size`, `tangent_dx`,
/// `top_left`), so overlay consumers never re-clone labels or recompute
/// offsets. `is_selected` and the tooltip stay OUT — applied post-hoc.
#[derive(Debug, Clone)]
pub struct GlyphEntry {
    pub id: ConstraintId,
    pub anchor: iced::Point,
    pub outward: [f32; 2],
    pub label: Arc<str>,
    pub conflicting: bool,
    pub hover: Arc<[iced::Point]>,
    pub size: iced::Size,
    pub tangent_dx: f32,
    pub top_left: iced::Point,
}

// Single source of truth for constraint-glyph layout (Task 4): the overlay
// side was deleted and imports these instead, so the drawn pills, the
// hit-test, and the cached `top_left`/`tangent_dx` can never drift apart.
pub(crate) const GLYPH_SIZE: f32 = 14.0;
const GLYPH_PAD_X: f32 = 7.0;
const GLYPH_PAD_Y: f32 = 4.0;
const GLYPH_GAP: f32 = 6.0;
const GLYPH_ROW_GAP: f32 = 4.0;
const GLYPH_COINCIDENT_SIZE: f32 = 9.0;

pub(crate) fn glyph_is_compact(label: &str) -> bool {
    matches!(label, "≡" | "∈")
}

pub(crate) fn glyph_is_fixed(label: &str) -> bool {
    label == "F" || label == FIXED_POINT_GLYPH
}

pub(crate) fn glyph_is_vertical(label: &str) -> bool {
    label == "│" || label == VERTICAL_POINTS_GLYPH
}

pub(crate) fn constraint_glyph_size(label: &str) -> iced::Size {
    if glyph_is_compact(label) {
        return iced::Size::new(GLYPH_COINCIDENT_SIZE, GLYPH_COINCIDENT_SIZE);
    }
    if label == "G²"
        || glyph_is_fixed(label)
        || glyph_is_vertical(label)
        || label == DYNAMIC_DIMENSION_GLYPH
    {
        let side = GLYPH_SIZE + GLYPH_PAD_Y * 2.0;
        return iced::Size::new(side, side);
    }
    let w = label.chars().count() as f32 * GLYPH_SIZE * 0.62 + GLYPH_PAD_X * 2.0;
    let h = GLYPH_SIZE + GLYPH_PAD_Y * 2.0;
    iced::Size::new(w, h)
}

pub(crate) fn constraint_glyph_box(
    anchor: iced::Point,
    outward: [f32; 2],
    label: &str,
    tangent_offset: f32,
) -> (iced::Point, iced::Size) {
    let size = constraint_glyph_size(label);
    let gap = if glyph_is_compact(label) {
        1.0
    } else {
        GLYPH_GAP
    };
    let distance =
        outward[0].abs() * size.width * 0.5 + outward[1].abs() * size.height * 0.5 + gap;
    let tangent = [-outward[1], outward[0]];
    (
        iced::Point::new(
            anchor.x + outward[0] * distance + tangent[0] * tangent_offset - size.width * 0.5,
            anchor.y + outward[1] * distance + tangent[1] * tangent_offset - size.height * 0.5,
        ),
        size,
    )
}

pub(crate) fn constraint_glyph_offsets(glyphs: &[(iced::Point, [f32; 2], &str)]) -> Vec<f32> {
    let mut groups: rustc_hash::FxHashMap<[u32; 4], Vec<usize>> =
        rustc_hash::FxHashMap::default();
    for (index, (anchor, outward, _)) in glyphs.iter().enumerate() {
        groups
            .entry([
                anchor.x.to_bits(),
                anchor.y.to_bits(),
                outward[0].to_bits(),
                outward[1].to_bits(),
            ])
            .or_default()
            .push(index);
    }

    let mut offsets = vec![0.0; glyphs.len()];
    for indices in groups.values().filter(|indices| indices.len() > 1) {
        let half_extents: Vec<f32> = indices
            .iter()
            .map(|index| {
                let (_, outward, label) = &glyphs[*index];
                let size = constraint_glyph_size(label);
                let tangent = [-outward[1], outward[0]];
                tangent[0].abs() * size.width * 0.5 + tangent[1].abs() * size.height * 0.5
            })
            .collect();
        let total =
            half_extents.iter().sum::<f32>() * 2.0 + GLYPH_ROW_GAP * (indices.len() - 1) as f32;
        let mut cursor = -total * 0.5;
        for (index, half_extent) in indices.iter().zip(half_extents) {
            offsets[*index] = cursor + half_extent;
            cursor += half_extent * 2.0 + GLYPH_ROW_GAP;
        }
    }
    offsets
}

/// Hit-tests screen point `p` against the precomputed `top_left`/`size` boxes
/// in `entries` — the same pills `draw` renders, including the tangential
/// fan-out baked into `tangent_dx` at cache time. Returns the index of the
/// topmost (last-drawn) match. Shared by `Scene::constraint_glyph_hit` and
/// the overlay draw/hover paths so the clickable area can never drift from
/// what's drawn and no caller recomputes offsets.
pub(crate) fn glyph_hit_test_entries(entries: &[GlyphEntry], p: iced::Point) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, entry)| {
            let within = p.x >= entry.top_left.x
                && p.x <= entry.top_left.x + entry.size.width
                && p.y >= entry.top_left.y
                && p.y <= entry.top_left.y + entry.size.height;
            within.then_some(index)
        })
}

impl super::Scene {
    /// Expand through enabled constraints. Whole-object translations follow
    /// point connections; grip previews include curve relations as well.
    pub(crate) fn parametric_connected_handles(
        &self,
        scope: ParametricScope,
        seeds: &[Handle],
        include_curve_relations: bool,
    ) -> Vec<Handle> {
        let mut ordered = seeds.to_vec();
        let mut found: std::collections::HashSet<_> = seeds.iter().copied().collect();
        let Some(set) = self.parametric_constraint_set(scope) else {
            return ordered;
        };
        loop {
            let mut added = false;
            for constraint in set.constraints.iter().filter(|constraint| {
                constraint.enabled
                    && (include_curve_relations || matches!(
                        constraint.kind,
                        ConstraintKind::Coincident | ConstraintKind::PointOnCurve
                    ))
            }) {
                if !constraint
                    .refs
                    .iter()
                    .any(|reference| found.contains(&reference.entity))
                {
                    continue;
                }
                for reference in &constraint.refs {
                    if found.insert(reference.entity) {
                        ordered.push(reference.entity);
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        ordered
    }

    pub fn is_parametric_constraint_visible(
        &self,
        scope: ParametricScope,
        id: ConstraintId,
    ) -> bool {
        !self.hidden_parametric_constraints.contains(&(scope, id))
    }

    /// Rewrites every dynamic dimension's text from its constraint's
    /// parameter name and current value, in the CONSTRAINTNAMEFORMAT
    /// reading `constraint_name_format` selects, and moves its extension
    /// origins to where the solve left the constraint points.
    pub(crate) fn refresh_dynamic_dimension_texts(&mut self) {
        let format = self.constraint_name_format;
        let mut updates: Vec<(
            Handle,
            String,
            Option<(Vector3, Vector3)>,
            Option<Vec<Vector3>>,
            Option<(glam::DVec3, f64)>,
        )> = Vec::new();
        // A reference (driven) constraint's parameter follows the geometry.
        let mut followed: Vec<(String, f64)> = Vec::new();
        for set in &self.parametric_constraints {
            let model_table = set.local_parameters.is_empty();
            let table = if model_table {
                &self.named_parameters
            } else {
                &set.local_parameters
            };
            for (id, dimension) in &set.dimensions {
                let Some(constraint) = set.get(*id) else {
                    continue;
                };
                let world = |reference: &ParametricRef| {
                    let entity = self.document.get_entity(reference.entity)?;
                    resolve_point(entity, reference.marker?)
                };
                let points = match constraint.refs.as_slice() {
                    [first, second, ..] => world(first).zip(world(second)),
                    _ => None,
                };
                let reference = !constraint.enabled;
                let angular = matches!(
                    constraint.kind,
                    ConstraintKind::Angle | ConstraintKind::Angle3Point
                );
                let radial = matches!(
                    constraint.kind,
                    ConstraintKind::Radius | ConstraintKind::Diameter
                );
                let radial_now = radial
                    .then(|| radial_geometry(&self.document, constraint))
                    .flatten();
                let radial_measured = radial_now.map(|(_, radius)| {
                    if constraint.kind == ConstraintKind::Diameter {
                        radius * 2.0
                    } else {
                        radius
                    }
                });
                let measured = points.filter(|_| !angular).map(|(first, second)| {
                    let delta = second - first;
                    let axis = match self.document.get_entity(*dimension) {
                        Some(acadrust::EntityType::Dimension(
                            acadrust::entities::Dimension::Linear(linear),
                        )) => Some((linear.rotation.cos(), linear.rotation.sin())),
                        _ => None,
                    };
                    match (constraint.kind, axis) {
                        (ConstraintKind::DistanceX, _) => delta.x.abs(),
                        (ConstraintKind::DistanceY, _) => delta.y.abs(),
                        (ConstraintKind::DistanceDirected, Some((ax, ay))) => {
                            (delta.x * ax + delta.y * ay).abs()
                        }
                        _ => delta.length(),
                    }
                });
                let measured = radial_measured.or(measured);
                let (name, value, source) = match &constraint.driving_param {
                    Some(DrivingValue::Named(name)) => {
                        let value = match (reference, measured) {
                            (true, Some(measured)) => {
                                if model_table {
                                    followed.push((name.clone(), measured));
                                }
                                measured
                            }
                            _ => table.resolve(name).unwrap_or(f64::NAN),
                        };
                        let source = (!reference)
                            .then(|| table.get(name).map(|parameter| parameter.source.clone()))
                            .flatten();
                        (name.clone(), value, source)
                    }
                    Some(DrivingValue::Literal(value)) => (String::new(), *value, None),
                    None => continue,
                };
                let annotational = self.dimension_is_annotational(*dimension);
                let decimals = angular.then(|| {
                    let style = match self.document.get_entity(*dimension) {
                        Some(acadrust::EntityType::Dimension(d)) => d.base().style_name.clone(),
                        _ => String::new(),
                    };
                    angle_decimals(&self.document, Some(&style))
                });
                let angle_points = angular
                    .then(|| angular_follow_points(&self.document, constraint))
                    .flatten();
                updates.push((
                    *dimension,
                    dynamic_dimension_text(
                        &name,
                        value,
                        format,
                        reference,
                        source.as_deref(),
                        annotational,
                        decimals,
                    ),
                    // A radial dimension follows its own circle, not a pair
                    // of constraint points.
                    points.filter(|_| !radial),
                    angle_points,
                    radial_now,
                ));
            }
        }
        for (name, measured) in followed {
            let source = measured_expression(measured);
            if self.named_parameters.get(&name).map(|p| p.source.as_str()) != Some(source.as_str()) {
                let _ = self.named_parameters.set(&name, &source);
            }
        }
        for (handle, text, points, angle_points, radial) in updates {
            let Some(acadrust::EntityType::Dimension(mut dimension)) =
                self.document.get_entity(handle).cloned()
            else {
                continue;
            };
            let mut changed = false;
            if let Some((first, second)) = points {
                if dynamic_dimension_follow_points(&mut dimension, first, second) {
                    changed = true;
                }
            }
            if let Some(angle_points) = angle_points {
                if dynamic_dimension_follow_angle(&mut dimension, &angle_points) {
                    changed = true;
                }
            }
            if let Some((center, radius)) = radial {
                if dynamic_dimension_follow_radial(&mut dimension, center, radius) {
                    changed = true;
                }
            }
            if dimension.base().user_text.as_deref() != Some(text.as_str()) {
                crate::entities::dimension::set_dimension_text_override(
                    dimension.base_mut(),
                    Some(text),
                );
                changed = true;
            }
            if changed {
                self.update_entity(acadrust::EntityType::Dimension(dimension));
            }
        }
    }

    pub fn should_display_parametric_constraint(
        &self,
        scope: ParametricScope,
        id: ConstraintId,
        kind: ConstraintKind,
        related_entity_selected: bool,
        display_mode: i16,
        bar_mode: i16,
    ) -> bool {
        self.is_parametric_constraint_visible(scope, id)
            && kind
                .bar_mode_bit()
                .is_none_or(|bit| bar_mode & bit != 0)
            && (self.shown_parametric_constraints.contains(&(scope, id))
                || (display_mode & 2 != 0 && related_entity_selected))
    }

    /// Apply display bit 1 at creation; bit 2 follows the current selection.
    pub fn note_parametric_constraint_applied(
        &mut self,
        scope: ParametricScope,
        id: ConstraintId,
        display_mode: i16,
    ) {
        self.hidden_parametric_constraints.remove(&(scope, id));
        if display_mode & 1 != 0 {
            self.shown_parametric_constraints.insert((scope, id));
        } else {
            self.shown_parametric_constraints.remove(&(scope, id));
        }
        // Every production `add` flows through here, so this bump covers all
        // constraint creations (add / add_axis_constraint / AUTOCONSTRAIN /
        // DCCONVERT / Coincident / Equal / Fixed / dimensional handlers).
        self.bump_constraints_epoch();
    }

    pub fn set_parametric_constraint_visibility(
        &mut self,
        scope: ParametricScope,
        handles: Option<&[Handle]>,
        dimensional: bool,
        visible: bool,
    ) -> usize {
        let ids: Vec<_> = self
            .parametric_constraint_set(scope)
            .into_iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|constraint| constraint.driving_param.is_some() == dimensional)
            .filter(|constraint| {
                handles.is_none_or(|handles| {
                    constraint
                        .refs
                        .iter()
                        .any(|reference| handles.contains(&reference.entity))
                })
            })
            .map(|constraint| constraint.id)
            .collect();
        for id in &ids {
            if visible {
                self.hidden_parametric_constraints.remove(&(scope, *id));
                self.shown_parametric_constraints.insert((scope, *id));
            } else {
                self.hidden_parametric_constraints.insert((scope, *id));
                self.shown_parametric_constraints.remove(&(scope, *id));
            }
        }
        self.refresh_hidden_dynamic_dimensions();
        if !ids.is_empty() {
            self.bump_constraints_epoch();
        }
        ids.len()
    }

    /// Makes sure the reference's dynamic dimension layer exists: hidden
    /// from the layer lists (the `*` prefix) and never plotted.
    pub(crate) fn ensure_dynamic_dimension_layer(&mut self) {
        if self.document.layers.contains(DYNAMIC_DIMENSION_LAYER) {
            return;
        }
        let mut layer = acadrust::tables::Layer::new(DYNAMIC_DIMENSION_LAYER);
        layer.color = acadrust::types::Color::from_index(7);
        layer.is_plottable = false;
        layer.handle = self.document.allocate_handle();
        let _ = self.document.layers.add(layer);
    }

    /// Replace an entity in place inside an open undo recording, keeping the
    /// previous version for undo. Out of line so a boxed entity handed over by
    /// the command driver never lands in its frame.
    #[inline(never)]
    pub(crate) fn replace_entity_recorded(&mut self, entity: Box<acadrust::EntityType>) {
        let handle = entity.common().handle;
        let before = self.document.get_entity_arc(handle);
        self.record_undo_before(handle, before);
        self.update_entity(*entity);
    }

    /// True when an angular constraint drives with the parameter `name`.
    pub(crate) fn parameter_is_angular(&self, name: &str) -> bool {
        self.parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .any(|constraint| {
                matches!(constraint.kind, ConstraintKind::Angle | ConstraintKind::Angle3Point)
                    && matches!(&constraint.driving_param, Some(DrivingValue::Named(used)) if used == name)
            })
    }

    /// A parameter's value as -PARAMETERS prints it: an angle at angular
    /// precision within one turn, anything else with four decimals.
    pub(crate) fn parameter_value_text(&self, name: &str) -> String {
        let Ok(value) = self.named_parameters.resolve(name) else {
            return "**".to_string();
        };
        if self.parameter_is_angular(name) {
            let decimals = angle_decimals(&self.document, None);
            format!("{:.decimals$}", normalize_angle_display(value))
        } else {
            format!("{value:.4}")
        }
    }

    /// True for a dynamic dimension of a dimensional constraint (not an
    /// annotational one, which is an ordinary plotted dimension).
    pub(crate) fn is_dynamic_dimension(&self, handle: Handle) -> bool {
        self.parametric_constraints
            .iter()
            .any(|set| set.dimensions.values().any(|dimension| *dimension == handle))
            && !self.dimension_is_annotational(handle)
    }

    /// True when a dimensional constraint's dimension uses the annotational
    /// form: on an ordinary layer instead of the reference's constraints
    /// layer, drawn with its style, plotted, never rescaled to the screen.
    pub(crate) fn dimension_is_annotational(&self, handle: Handle) -> bool {
        self.document.get_entity(handle).is_some_and(|entity| {
            !entity
                .common()
                .layer
                .eq_ignore_ascii_case(DYNAMIC_DIMENSION_LAYER)
        })
    }

    /// The document as written to a file: a dynamic dimension's screen-size
    /// overrides are a display matter and stay out of the file.
    pub(crate) fn document_for_save(&self) -> acadrust::CadDocument {
        use crate::entities::dim_override as ov;
        let mut document = self.document.clone();
        for set in &self.parametric_constraints {
            for handle in set.dimensions.values().copied() {
                if self.dimension_is_annotational(handle) {
                    continue;
                }
                for code in [
                    ov::DIMSCALE,
                    ov::DIMGAP,
                    ov::DIMEXO,
                    ov::DIMEXE,
                    ov::DIMASZ,
                    ov::DIMTAD,
                    ov::DIMTIH,
                    ov::DIMTOH,
                ] {
                    ov::set(&mut document, handle, code, None);
                }
            }
        }
        document
    }

    /// Gives every dynamic dimension a DIMSCALE override that keeps its
    /// text, arrows and offsets at a screen size (the reference draws them
    /// at a constant pixel size whatever the zoom). Runs whenever the camera
    /// changed; `force` re-applies after a dimension was created or loaded.
    pub fn refresh_dynamic_dimension_scales(&mut self, force: bool) {
        let Some(wpp) = self.world_per_pixel() else {
            return;
        };
        if !force && self.dynamic_dimension_camera_gen == Some(self.camera_generation) {
            return;
        }
        self.dynamic_dimension_camera_gen = Some(self.camera_generation);
        let handles: Vec<Handle> = self
            .parametric_constraints
            .iter()
            .flat_map(|set| set.dimensions.values().copied())
            .collect();
        let mut changes = Vec::new();
        for handle in handles {
            let Some(acadrust::EntityType::Dimension(dimension)) = self.document.get_entity(handle)
            else {
                continue;
            };
            // An annotational dimension keeps its style's size.
            if self.dimension_is_annotational(handle) {
                continue;
            }
            let style_name = dimension.base().style_name.clone();
            let style = self
                .document
                .dim_styles
                .iter()
                .find(|style| {
                    style.name.eq_ignore_ascii_case(&style_name)
                        || (style_name.trim().is_empty()
                            && style.name.eq_ignore_ascii_case("Standard"))
                });
            let text_height = style
                .map(|style| style.dimtxt)
                .filter(|height| *height > 1e-9)
                .unwrap_or(0.18);
            let size_of = |value: Option<f64>, fallback: f64| {
                value.filter(|size| *size > 1e-9).unwrap_or(fallback)
            };
            let arrow_size = size_of(style.map(|style| style.dimasz), text_height);
            let extension_over = size_of(style.map(|style| style.dimexe), text_height * 0.5);
            let extension_offset = size_of(style.map(|style| style.dimexo), text_height * 0.25);
            let scale = f64::from(wpp) * f64::from(DYNAMIC_DIMENSION_TEXT_PX) / text_height;
            use crate::entities::dim_override as ov;
            let xdata = &dimension.base().common.extended_data;
            let current = ov::real(xdata, ov::DIMSCALE);
            // The gap hugs the text, so it follows the text's screen size.
            // Arrowheads and extension lines keep the style's size in
            // drawing units: dividing by the screen factor the style's
            // DIMSCALE re-applies leaves them unchanged as the view zooms.
            let sizes = [
                (ov::DIMGAP, text_height * 0.25),
                (ov::DIMEXO, extension_offset / scale),
                (ov::DIMEXE, extension_over / scale),
                (ov::DIMASZ, arrow_size / scale),
            ];
            // A radius or diameter constraint reads on its own dimension
            // line, which breaks around the text, so the text is centred on
            // it rather than lifted above it.
            let radial = matches!(
                dimension,
                acadrust::entities::Dimension::Radius(_)
                    | acadrust::entities::Dimension::Diameter(_)
            );
            let centred = !radial || ov::int(xdata, ov::DIMTAD) == Some(0);
            let sizes_set = sizes.iter().all(|(code, size)| {
                ov::real(xdata, *code).is_some_and(|value| (value - size).abs() < 1e-12)
            });
            // The reference draws dynamic distance text horizontally; an
            // angle's text keeps its style's alignment.
            let angular = matches!(
                dimension,
                acadrust::entities::Dimension::Angular2Ln(_)
                    | acadrust::entities::Dimension::Angular3Pt(_)
            );
            let horizontal = angular
                || (ov::int(xdata, ov::DIMTIH) == Some(1) && ov::int(xdata, ov::DIMTOH) == Some(1));
            if current.is_some_and(|value| (value - scale).abs() < 1e-9)
                && horizontal
                && sizes_set
                && centred
            {
                continue;
            }
            if radial && !centred {
                ov::set(
                    &mut self.document,
                    handle,
                    ov::DIMTAD,
                    Some(acadrust::xdata::XDataValue::Integer16(0)),
                );
            }
            ov::set(
                &mut self.document,
                handle,
                ov::DIMSCALE,
                Some(acadrust::xdata::XDataValue::Real(scale)),
            );
            if !sizes_set {
                for (code, size) in sizes {
                    ov::set(
                        &mut self.document,
                        handle,
                        code,
                        Some(acadrust::xdata::XDataValue::Real(size)),
                    );
                }
            }
            if !horizontal {
                for code in [ov::DIMTIH, ov::DIMTOH] {
                    ov::set(
                        &mut self.document,
                        handle,
                        code,
                        Some(acadrust::xdata::XDataValue::Integer16(1)),
                    );
                }
            }
            changes.push((handle, super::ChangeKind::Modified));
        }
        if !changes.is_empty() {
            self.bump_entities(&changes);
        }
    }

    /// Re-derives which dynamic dimensions stay off screen (DCHIDE, or
    /// DYNCONSTRAINTDISPLAY 0 for all of them) and redraws the ones that
    /// changed.
    pub(crate) fn refresh_hidden_dynamic_dimensions(&mut self) {
        let desired: rustc_hash::FxHashSet<Handle> = self
            .parametric_constraints
            .iter()
            .flat_map(|set| {
                set.dimensions.iter().filter_map(|(id, dimension)| {
                    (!self.dimension_is_annotational(*dimension)
                        && (!self.dynamic_constraint_display
                            || !self.is_parametric_constraint_visible(set.scope, *id)))
                    .then_some(*dimension)
                })
            })
            .collect();
        if desired == self.hidden_dynamic_dimensions {
            return;
        }
        let changes: Vec<_> = self
            .hidden_dynamic_dimensions
            .symmetric_difference(&desired)
            .copied()
            .map(|handle| (handle, super::ChangeKind::Modified))
            .collect();
        self.hidden_dynamic_dimensions = desired;
        // The memoised glyph placements read this set on every display mode
        // (dynamic pills filter `entity_temporarily_hidden` unconditionally),
        // so a change here must invalidate them — this covers all four
        // callers, including the DYNCONSTRAINTDISPLAY toggle that has no
        // call-site bump of its own.
        self.bump_constraints_epoch();
        self.bump_entities(&changes);
    }

    /// Enables or disables one constraint without losing it — the production
    /// path for toggling `ParametricConstraint::enabled` (a re-solve skips a
    /// disabled constraint; glyph placements filter it out). Returns whether
    /// a constraint with `id` exists in `scope`. Bumps the constraints epoch
    /// exactly when the value actually changes so future callers cannot
    /// introduce a stale-glyph path by writing the field directly.
    pub fn set_constraint_enabled(
        &mut self,
        scope: ParametricScope,
        id: ConstraintId,
        enabled: bool,
    ) -> bool {
        let mut found = false;
        let mut changed = false;
        if let Some(set) = self
            .parametric_constraints
            .iter_mut()
            .find(|set| set.scope == scope)
        {
            if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                found = true;
                if constraint.enabled != enabled {
                    constraint.enabled = enabled;
                    changed = true;
                }
            }
        }
        if changed {
            self.bump_constraints_epoch();
        }
        found
    }

    /// Infers relations already present in the selected geometry.
    pub fn inferred_parametric_constraints(
        &self,
        scope: ParametricScope,
        handles: &[Handle],
        settings: &crate::app::settings::AutoConstrainSettings,
    ) -> Vec<(ConstraintKind, Vec<ParametricRef>)> {
        use cadkernel::geom2d::{
            infer_constraints_with_settings, Arc, Circle, ConstraintEndpoint, InferenceKind,
            InferenceSettings, InferredConstraint, Line, ParametricPrimitive,
        };
        struct Source {
            handle: Handle,
            primitive: ParametricPrimitive,
            whole: ParametricRef,
            endpoints: [ParametricRef; 2],
        }
        let mut sources = Vec::new();
        for handle in handles {
            match self.document.get_entity(*handle) {
                Some(acadrust::EntityType::Line(line)) => sources.push(Source {
                    handle: *handle,
                    primitive: ParametricPrimitive::Line(Line {
                        start: [line.start.x, line.start.y],
                        end: [line.end.x, line.end.y],
                    }),
                    whole: ParametricRef::whole(*handle),
                    endpoints: [
                        ParametricRef::point(*handle, 0),
                        ParametricRef::point(*handle, 1),
                    ],
                }),
                Some(acadrust::EntityType::Circle(circle)) => sources.push(Source {
                    handle: *handle,
                    primitive: ParametricPrimitive::Circle(Circle {
                        centre: [circle.center.x, circle.center.y],
                        radius: circle.radius,
                    }),
                    whole: ParametricRef::whole(*handle),
                    endpoints: [
                        ParametricRef::center(*handle),
                        ParametricRef::center(*handle),
                    ],
                }),
                Some(acadrust::EntityType::Arc(arc)) => sources.push(Source {
                    handle: *handle,
                    primitive: ParametricPrimitive::Arc(Arc {
                        centre: [arc.center.x, arc.center.y],
                        radius: arc.radius,
                        start_angle: arc.start_angle,
                        end_angle: arc.end_angle,
                    }),
                    whole: ParametricRef::whole(*handle),
                    endpoints: [
                        ParametricRef::point(*handle, 0),
                        ParametricRef::point(*handle, 1),
                    ],
                }),
                Some(acadrust::EntityType::LwPolyline(polyline)) => {
                    let Some(world) = crate::entities::curve::lwpolyline_world_xy(polyline) else {
                        continue;
                    };
                    let count = world.vertices.len();
                    for index in 0..count {
                        let next = index + 1;
                        if next >= count && !world.is_closed {
                            break;
                        }
                        if world.vertices[index].bulge.abs() > 1e-9 {
                            continue;
                        }
                        let a = world.vertices[index].location;
                        let b = world.vertices[next % count].location;
                        sources.push(Source {
                            handle: *handle,
                            primitive: ParametricPrimitive::Line(Line {
                                start: [a.x, a.y],
                                end: [b.x, b.y],
                            }),
                            whole: ParametricRef::segment(*handle, index),
                            endpoints: [
                                ParametricRef::point(*handle, index as i32),
                                ParametricRef::point(*handle, (next % count) as i32),
                            ],
                        });
                    }
                }
                Some(acadrust::EntityType::Polyline2D(polyline)) => {
                    let count = polyline.vertices.len();
                    for index in 0..count {
                        let next = index + 1;
                        if next >= count && !polyline.is_closed() {
                            break;
                        }
                        if polyline.vertices[index].bulge.abs() > 1e-9 {
                            continue;
                        }
                        let a = polyline.vertices[index].location;
                        let b = polyline.vertices[next % count].location;
                        sources.push(Source {
                            handle: *handle,
                            primitive: ParametricPrimitive::Line(Line {
                                start: [a.x, a.y],
                                end: [b.x, b.y],
                            }),
                            whole: ParametricRef::segment(*handle, index),
                            endpoints: [
                                ParametricRef::point(*handle, index as i32),
                                ParametricRef::point(*handle, (next % count) as i32),
                            ],
                        });
                    }
                }
                _ => continue,
            }
        }
        let primitives: Vec<_> = sources.iter().map(|source| source.primitive).collect();
        let marker = |endpoint| match endpoint {
            ConstraintEndpoint::Start => 0usize,
            ConstraintEndpoint::End => 1usize,
        };
        let kind = |kind| match kind {
            crate::app::settings::AutoConstraintKind::Coincident => InferenceKind::Coincident,
            crate::app::settings::AutoConstraintKind::Collinear => InferenceKind::Collinear,
            crate::app::settings::AutoConstraintKind::Parallel => InferenceKind::Parallel,
            crate::app::settings::AutoConstraintKind::Perpendicular => {
                InferenceKind::Perpendicular
            }
            crate::app::settings::AutoConstraintKind::Tangent => InferenceKind::Tangent,
            crate::app::settings::AutoConstraintKind::Concentric => InferenceKind::Concentric,
            crate::app::settings::AutoConstraintKind::Horizontal => InferenceKind::Horizontal,
            crate::app::settings::AutoConstraintKind::Vertical => InferenceKind::Vertical,
            crate::app::settings::AutoConstraintKind::Equal => InferenceKind::Equal,
        };
        let inference_settings = InferenceSettings {
            priority: settings
                .priority
                .iter()
                .copied()
                .filter(|candidate| settings.enabled.contains(candidate))
                .map(kind)
                .collect(),
            distance_tolerance: settings.distance_tolerance,
            angle_tolerance_radians: settings.angle_tolerance_deg.to_radians(),
            tangent_must_share_point: settings.tangent_must_share_point,
            perpendicular_must_intersect: settings.perpendicular_must_intersect,
        };
        let mut mapped: Vec<_> =
            infer_constraints_with_settings(&primitives, &inference_settings)
                .into_iter()
                .filter(|relation| {
                    !matches!(relation, InferredConstraint::Coincident { first, second, .. }
                        if sources[*first].handle == sources[*second].handle)
                })
                .map(|relation| match relation {
                    InferredConstraint::Coincident {
                        first,
                        first_endpoint,
                        second,
                        second_endpoint,
                    } => (
                        ConstraintKind::Coincident,
                        vec![
                            sources[first].endpoints[marker(first_endpoint)],
                            sources[second].endpoints[marker(second_endpoint)],
                        ],
                    ),
                    InferredConstraint::Collinear { first, second } => (
                        ConstraintKind::Colinear,
                        vec![
                            sources[first].whole,
                            sources[second].whole,
                        ],
                    ),
                    InferredConstraint::Concentric { first, second } => (
                        ConstraintKind::Concentric,
                        vec![
                            ParametricRef::center(sources[first].handle),
                            ParametricRef::center(sources[second].handle),
                        ],
                    ),
                    InferredConstraint::Parallel { first, second } => (
                        ConstraintKind::Parallel,
                        vec![
                            sources[first].whole,
                            sources[second].whole,
                        ],
                    ),
                    InferredConstraint::Perpendicular { first, second } => (
                        ConstraintKind::Perpendicular,
                        vec![
                            sources[first].whole,
                            sources[second].whole,
                        ],
                    ),
                    InferredConstraint::Horizontal { entity } => (
                        ConstraintKind::Horizontal,
                        vec![sources[entity].whole],
                    ),
                    InferredConstraint::Vertical { entity } => (
                        ConstraintKind::Vertical,
                        vec![sources[entity].whole],
                    ),
                    InferredConstraint::Tangent { first, second } => (
                        ConstraintKind::Tangent,
                        vec![
                            sources[first].whole,
                            sources[second].whole,
                        ],
                    ),
                    InferredConstraint::Equal { first, second } => (
                        ConstraintKind::Equal,
                        vec![sources[first].whole, sources[second].whole],
                    ),
                })
                .collect();
        if let Some(existing) = self.parametric_constraint_set(scope) {
            mapped.retain(|(kind, refs)| {
                !existing.constraints.iter().any(|constraint| {
                    constraint.kind == *kind
                        && (constraint.refs == *refs
                            || (constraint.refs.len() == 2
                                && refs.len() == 2
                                && constraint.refs[0] == refs[1]
                                && constraint.refs[1] == refs[0]))
                })
            });
        }
        mapped.retain(|(kind, refs)| {
            self.validate_parametric_constraint(*kind, refs, None)
                .is_ok()
        });
        mapped
    }

    /// Infer only endpoint/vertex coincidences from the selected objects.
    /// Unlike the broader automatic constraint pass, this includes spline,
    /// ellipse, and polyline vertices while leaving centers and midpoints to
    /// their dedicated constraint kinds.
    pub fn inferred_coincident_constraints(
        &self,
        scope: ParametricScope,
        handles: &[Handle],
    ) -> Vec<Vec<ParametricRef>> {
        let owner = scope.owner_handle(&self.document);
        let mut sources = Vec::new();
        let mut seen_handles = std::collections::HashSet::new();
        for handle in handles.iter().copied() {
            if !seen_handles.insert(handle) {
                continue;
            }
            let Some(entity) = self.document.get_entity(handle) else {
                continue;
            };
            if entity.common().owner_handle != owner {
                continue;
            }
            for (marker, point) in super::dimension_assoc::source_points(entity)
                .into_iter()
                .enumerate()
            {
                sources.push((ParametricRef::point(handle, marker as i32), point));
            }
        }

        let existing = self.parametric_constraint_set(scope);
        let mut inferred = Vec::new();
        for first in 0..sources.len() {
            for second in first + 1..sources.len() {
                if sources[first].0.entity == sources[second].0.entity
                    || (sources[first].1 - sources[second].1).length_squared()
                        > COINCIDENT_EPSILON_SQ
                {
                    continue;
                }
                let refs = vec![sources[first].0, sources[second].0];
                let already_exists = existing.is_some_and(|set| {
                    set.constraints.iter().any(|constraint| {
                        constraint.kind == ConstraintKind::Coincident
                            && constraint.refs.len() == 2
                            && (constraint.refs == refs
                                || (constraint.refs[0] == refs[1]
                                    && constraint.refs[1] == refs[0]))
                    })
                });
                if !already_exists
                    && self
                        .validate_parametric_constraint(ConstraintKind::Coincident, &refs, None)
                        .is_ok()
                {
                    inferred.push(refs);
                }
            }
        }
        inferred
    }

    /// The constraint set for `scope`, if one has been created.
    pub fn parametric_constraint_set(
        &self,
        scope: ParametricScope,
    ) -> Option<&ParametricConstraintSet> {
        self.parametric_constraints
            .iter()
            .find(|s| s.scope == scope)
    }

    /// The constraint set for `scope`, creating an empty one on first use.
    /// The `pub(crate)` `parametric_constraints` field itself stays private so
    /// nothing outside this module can end up with two sets for the same
    /// scope — this is the one way to reach a scope's set for both reading
    /// and mutating.
    pub fn parametric_constraint_set_mut(
        &mut self,
        scope: ParametricScope,
    ) -> &mut ParametricConstraintSet {
        if let Some(index) = self
            .parametric_constraints
            .iter()
            .position(|s| s.scope == scope)
        {
            &mut self.parametric_constraints[index]
        } else {
            self.parametric_constraints
                .push(ParametricConstraintSet::new(scope));
            self.parametric_constraints.last_mut().expect("just pushed")
        }
    }

    /// Screen-projected parametric-constraint glyph placements for `scope`:
    /// `(id, anchor, outward_screen_direction, label, is_conflicting,
    /// hover_points)` for every enabled, visible constraint whose glyph
    /// projects on-screen.
    /// `vp_size` is the full canvas size (as `SelectionState::vp_size`
    /// reports it), matching what `viewport_edit_frame`/
    /// `active_model_tile_bounds` expect. Mirrors the projection
    /// `crate::app::view` builds its own render list with. The memoised
    /// [`cached_glyph_placements`](Self::cached_glyph_placements) maps these
    /// same `(anchor, outward, label)` triples through
    /// [`constraint_glyph_box`](crate::scene::parametric_constraints::constraint_glyph_box)/
    /// [`constraint_glyph_offsets`](crate::scene::parametric_constraints::constraint_glyph_offsets),
    /// so hit-testing can never drift from what's actually drawn.
    pub fn constraint_glyph_placements_screen(
        &self,
        scope: ParametricScope,
        vp_size: (f32, f32),
        show_values: bool,
        display_mode: i16,
        bar_mode: i16,
    ) -> Vec<(
        ConstraintId,
        iced::Point,
        [f32; 2],
        String,
        bool,
        Vec<iced::Point>,
    )> {
        let Some(set) = self.parametric_constraint_set(scope) else {
            return Vec::new();
        };
        if set.constraints.is_empty() {
            return Vec::new();
        }
        let edit_frame = self.viewport_edit_frame(vp_size);
        let bounds = match &edit_frame {
            Some((_, full)) => *full,
            None => self.active_model_tile_bounds(vp_size.0, vp_size.1),
        };
        let (view_rot, eye) = if let Some((cam, _)) = &edit_frame {
            (cam.view_proj_rte(bounds), cam.eye())
        } else {
            let cam = self.camera.borrow();
            (cam.view_proj_rte(bounds), cam.eye())
        };
        let mut placements = set
            .constraints
            .iter()
            // A dynamic dimension is its constraint's whole display.
            .filter(|c| c.enabled && !set.dimensions.contains_key(&c.id))
            .filter(|c| {
                let selected = c
                    .refs
                    .iter()
                    .any(|reference| self.selected.contains(&reference.entity)
                        || self.preview_hidden.contains(&reference.entity));
                self.should_display_parametric_constraint(
                    scope,
                    c.id,
                    c.kind,
                    selected,
                    display_mode,
                    bar_mode,
                )
            })
            .flat_map(|c| {
                let is_conflicting = set.conflicts.iter().any(|(id, _)| *id == c.id);
                let base_label = if c.kind == ConstraintKind::Fixed {
                    fixed_glyph_label(c).to_string()
                } else if c.kind == ConstraintKind::Vertical {
                    vertical_glyph_label(c).to_string()
                } else if show_values {
                    glyph_label(c)
                } else {
                    c.kind.glyph_symbol().to_string()
                };
                let hover_points: Vec<iced::Point> = constraint_hover_points(&self.document, c)
                    .into_iter()
                    .filter_map(|hover_point| {
                        let projected = crate::scene::pick::grip::project_rte(
                            glam::DVec3::new(hover_point.x, hover_point.y, hover_point.z),
                            view_rot,
                            eye,
                            bounds,
                        )?;
                        let point = iced::Point::new(
                            bounds.x + projected.x,
                            bounds.y + projected.y,
                        );
                        (point.x.is_finite() && point.y.is_finite()).then_some(point)
                    })
                    .collect();
                glyph_placements(&self.document, c)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, (anchor, outward))| {
                        let label = if c.kind == ConstraintKind::Symmetric {
                            if index == 2 {
                                "S│".to_string()
                            } else if c.refs.get(index).is_some_and(|reference| {
                                reference.marker.is_some()
                                    && reference.segment_index().is_none()
                            }) {
                                "S•".to_string()
                            } else {
                                "S◇".to_string()
                            }
                        } else {
                            base_label.clone()
                        };
                        let screen = crate::scene::pick::grip::project_rte(
                            glam::DVec3::new(anchor.x, anchor.y, anchor.z),
                            view_rot,
                            eye,
                            bounds,
                        )?;
                        let outward_screen = crate::scene::pick::grip::project_rte(
                            glam::DVec3::new(
                                anchor.x + outward.x,
                                anchor.y + outward.y,
                                anchor.z + outward.z,
                            ),
                            view_rot,
                            eye,
                            bounds,
                        )?;
                        let direction =
                            (outward_screen - screen).normalize_or(glam::Vec2::NEG_Y);
                        let point = iced::Point::new(bounds.x + screen.x, bounds.y + screen.y);
                        point.x.is_finite().then(|| (
                            c.id,
                            point,
                            direction.to_array(),
                            label,
                            is_conflicting,
                            hover_points.clone(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        // The reference marks an object once for Equal however many
        // relations of that kind it carries; the stacked duplicates only
        // repeated the same badge.
        let mut equal_anchors: Vec<iced::Point> = Vec::new();
        placements.retain(|(_, point, _, label, _, _)| {
            if label != "=" {
                return true;
            }
            if equal_anchors
                .iter()
                .any(|seen| (seen.x - point.x).abs() < 0.5 && (seen.y - point.y).abs() < 0.5)
            {
                return false;
            }
            equal_anchors.push(*point);
            true
        });
        // A dynamic dimension carries the reference's lock mark at the
        // start of its text instead of a constraint bar.
        for (id, dimension) in &set.dimensions {
            let Some(constraint) = set.get(*id) else {
                continue;
            };
            if !constraint.enabled || self.entity_temporarily_hidden(*dimension) {
                continue;
            }
            let Some(acadrust::EntityType::Dimension(entity)) = self.document.get_entity(*dimension)
            else {
                continue;
            };
            let Some((anchor, outward)) = crate::entities::dimension::dynamic_dimension_lock_anchor(
                &self.document,
                entity,
                self.annotation_scale as f64,
            ) else {
                continue;
            };
            let project = |point: Vector3| {
                crate::scene::pick::grip::project_rte(
                    glam::DVec3::new(point.x, point.y, point.z),
                    view_rot,
                    eye,
                    bounds,
                )
            };
            let (Some(screen), Some(outward_screen)) = (
                project(anchor),
                project(Vector3::new(
                    anchor.x + outward.x,
                    anchor.y + outward.y,
                    anchor.z + outward.z,
                )),
            ) else {
                continue;
            };
            let direction = (outward_screen - screen).normalize_or(glam::Vec2::NEG_X);
            let point = iced::Point::new(bounds.x + screen.x, bounds.y + screen.y);
            if point.x.is_finite() && point.y.is_finite() {
                placements.push((
                    *id,
                    point,
                    direction.to_array(),
                    DYNAMIC_DIMENSION_GLYPH.to_string(),
                    false,
                    Vec::new(),
                ));
            }
        }
        placements
    }

    /// Hit-tests screen point `p` (same coordinate space as `p_full` in the
    /// viewport click handler) against the memoised [`cached_glyph_placements`](Self::cached_glyph_placements)
    /// entries, so a click only registers where the pill is actually drawn —
    /// with no placement recompute on the mouse-move/click hot path.
    pub fn constraint_glyph_hit(
        &self,
        scope: ParametricScope,
        vp_size: (f32, f32),
        show_values: bool,
        display_mode: i16,
        bar_mode: i16,
        p: iced::Point,
    ) -> Option<ConstraintId> {
        let entries = self.cached_glyph_placements(scope, vp_size, show_values, display_mode, bar_mode);
        let index = glyph_hit_test_entries(&entries, p)?;
        Some(entries[index].id)
    }

    /// Order-independent hash of the visibility sets that feed
    /// [`constraint_glyph_placements_screen`](Self::constraint_glyph_placements_screen).
    /// The dynamic pills filter `entity_temporarily_hidden` on EVERY display
    /// mode, so `preview_hidden`, `command_preview_hidden` and
    /// `hidden_dynamic_dimensions` are hashed UNCONDITIONALLY; `selected` and
    /// the hidden/shown override sets only affect the `should_display` filter
    /// when `display_mode & 2 != 0`, so they stay conditional (and the key
    /// stays shared across selection changes in the other modes).
    pub(crate) fn glyph_selection_signature(&self, display_mode: i16) -> u64 {
        fn item_hash(item: &impl Hash) -> u64 {
            let mut hasher = rustc_hash::FxHasher::default();
            item.hash(&mut hasher);
            hasher.finish()
        }
        // Order-independent single pass: per-item hashes combine
        // commutatively (wrapping add), so iteration order never matters;
        // one FxHasher over (combined, count) folds in the total length, no
        // alloc/sort on the hot hit path.
        let mut combined: u64 = 0;
        let mut count: u64 = 0;
        // Always-read hide sets: the dynamic-dimension pills consult these
        // whatever the display mode, so a stale signature here would serve a
        // pill for a just-hidden dimension (or hide a just-shown one).
        for item in self.preview_hidden.iter() {
            combined = combined.wrapping_add(item_hash(item));
            count += 1;
        }
        for item in self.command_preview_hidden.iter() {
            combined = combined.wrapping_add(item_hash(item));
            count += 1;
        }
        for item in self.hidden_dynamic_dimensions.iter() {
            combined = combined.wrapping_add(item_hash(item));
            count += 1;
        }
        if display_mode & 2 != 0 {
            for item in self.selected.iter() {
                combined = combined.wrapping_add(item_hash(item));
                count += 1;
            }
            for item in self.hidden_parametric_constraints.iter() {
                combined = combined.wrapping_add(item_hash(item));
                count += 1;
            }
            for item in self.shown_parametric_constraints.iter() {
                combined = combined.wrapping_add(item_hash(item));
                count += 1;
            }
        }
        let mut hasher = rustc_hash::FxHasher::default();
        combined.hash(&mut hasher);
        count.hash(&mut hasher);
        hasher.finish()
    }

    /// Memoised [`constraint_glyph_placements_screen`](Self::constraint_glyph_placements_screen),
    /// keyed by [`GlyphKey`]: the miss path calls the existing placements fn
    /// VERBATIM (no algorithm change), maps each tuple to a [`GlyphEntry`]
    /// with precomputed layout, and stores the `Arc<[GlyphEntry]>` in the
    /// Scene-owned single-slot memo. NaN filtering is the placements fn's own and is
    /// cached as-is. Camera/layout/selection state is read from `self`, so
    /// the `(scope, vp_size, show_values, display_mode, bar_mode)` signature
    /// stays identical to the stub's.
    ///
    /// Single-slot rationale: within one frame, view + hit-test + dwell +
    /// click all share the key (hits); across camera motion each frame misses
    /// once by design; across edits the stale single entry just misses — no
    /// leak possible (an unbounded map would orphan an entry per camera frame
    /// and per edit, since `cam_gen` is in the key).
    pub fn cached_glyph_placements(
        &self,
        scope: ParametricScope,
        vp_size: (f32, f32),
        show_values: bool,
        display_mode: i16,
        bar_mode: i16,
    ) -> std::sync::Arc<[GlyphEntry]> {
        let key = GlyphKey {
            scope,
            c_epoch: self.constraints_epoch,
            g_epoch: self.geometry_epoch,
            cam_gen: self.camera_generation,
            active_vp: self.active_viewport,
            layout_is_model: self.current_layout == "Model",
            vp_bits: (vp_size.0.to_bits(), vp_size.1.to_bits()),
            show_values,
            disp: display_mode,
            bar: bar_mode,
            sel_sig: self.glyph_selection_signature(display_mode),
            // The dynamic pills anchor through `dynamic_dimension_lock_anchor`
            // with this scale — a CANNOSCALE change moves them, so it is keyed
            // (bitwise: NaN never occurs here, and distinct bit patterns must
            // miss rather than alias).
            annotation_scale: self.annotation_scale.to_bits(),
        };
        if let Some((cached_key, cached_arc)) = self.glyph_cache.borrow().as_ref() {
            if *cached_key == key {
                return std::sync::Arc::clone(cached_arc);
            }
        }
        let placements = self.constraint_glyph_placements_screen(
            scope,
            vp_size,
            show_values,
            display_mode,
            bar_mode,
        );
        let offsets = constraint_glyph_offsets(
            &placements
                .iter()
                .map(|(_, point, direction, label, _, _)| (*point, *direction, label.as_str()))
                .collect::<Vec<_>>(),
        );
        let entries: Vec<GlyphEntry> = placements
            .into_iter()
            .zip(offsets)
            .map(
                |((id, anchor, outward, label, conflicting, hover), tangent_dx)| {
                    let size = constraint_glyph_size(&label);
                    let (top_left, _) = constraint_glyph_box(anchor, outward, &label, tangent_dx);
                    GlyphEntry {
                        id,
                        anchor,
                        outward,
                        label: Arc::from(label),
                        conflicting,
                        hover: Arc::from(hover),
                        size,
                        tangent_dx,
                        top_left,
                    }
                },
            )
            .collect();
        let arc: Arc<[GlyphEntry]> = Arc::from(entries);
        *self.glyph_cache.borrow_mut() = Some((key, Arc::clone(&arc)));
        arc
    }

    /// Handle remapping lives in each command that duplicates
    /// entities — `Scene::copy_entities`' `handle_map` (COPY/ARRAY/MIRROR,
    /// `src/scene/modify.rs`) and `OpenCADStudio::finalize_paste`'s own
    /// (clipboard paste, `src/app/command_driver.rs`) — rather than in one
    /// shared table, so each call site passes its own `handle_map` here
    /// after adding the duplicated entities.
    ///
    /// For every enabled constraint whose *every* referenced entity was
    /// duplicated (a constraint straddling a duplicated and a
    /// non-duplicated entity can't sensibly follow — only one side moved),
    /// adds an equivalent constraint over the new handles to the same
    /// scope, then triggers a solve for the newly duplicated geometry the
    /// same way any other edit would. A no-op when `handle_map` is empty or
    /// nothing constrained was duplicated.
    pub fn duplicate_parametric_constraints_for(
        &mut self,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) {
        if handle_map.is_empty() {
            return;
        }
        let mut to_add: Vec<(
            usize,
            ConstraintKind,
            Vec<ParametricRef>,
            Option<DrivingValue>,
            Option<Vector3>,
        )> = Vec::new();
        for (scope_index, set) in self.parametric_constraints.iter().enumerate() {
            for c in &set.constraints {
                if !c.enabled || !c.refs.iter().all(|r| handle_map.contains_key(&r.entity)) {
                    continue;
                }
                let new_refs: Vec<ParametricRef> = c
                    .refs
                    .iter()
                    .map(|r| ParametricRef {
                        entity: handle_map[&r.entity],
                        marker: r.marker,
                    })
                    .collect();
                to_add.push((
                    scope_index,
                    c.kind,
                    new_refs,
                    c.driving_param.clone(),
                    c.axis_direction,
                ));
            }
        }
        if to_add.is_empty() {
            return;
        }
        let mut touched: Vec<Handle> = Vec::new();
        for (scope_index, kind, refs, driving_param, axis_direction) in to_add {
            touched.extend(refs.iter().map(|r| r.entity));
            self.parametric_constraints[scope_index].add(kind, refs, driving_param);
            self.parametric_constraints[scope_index]
                .constraints
                .last_mut()
                .expect("the copied constraint was just added")
                .axis_direction = axis_direction;
        }
        touched.sort();
        touched.dedup();
        self.bump_constraints_epoch();
        let changes: Vec<(Handle, super::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, super::ChangeKind::Modified))
            .collect();
        self.bump_entities(&changes);
    }

    /// Every persistent constraint, in any scope, currently driven by the
    /// named parameter `name` — what the Named Parameters panel's "used by"
    /// column shows. `entities` is the constraint's own referenced handles
    /// (deduplicated; a two-point constraint on the same entity's own two
    /// markers would otherwise list it twice), not resolved against the
    /// live document — a caller wanting an entity's current type/position
    /// still needs `Scene::document.get_entity`.
    pub fn parameter_usage(&self, name: &str) -> Vec<ParameterUsage> {
        let mut out = Vec::new();
        for set in &self.parametric_constraints {
            for c in &set.constraints {
                let Some(DrivingValue::Named(n)) = &c.driving_param else {
                    continue;
                };
                if n != name {
                    continue;
                }
                let mut entities: Vec<Handle> = c.refs.iter().map(|r| r.entity).collect();
                entities.sort();
                entities.dedup();
                out.push(ParameterUsage {
                    scope: set.scope,
                    constraint_id: c.id,
                    kind: c.kind,
                    entities,
                });
            }
        }
        out
    }
}

/// One persistent constraint driven by a named parameter — [`Scene::parameter_usage`]'s
/// result type.
#[derive(Debug, Clone)]
pub struct ParameterUsage {
    pub scope: ParametricScope,
    pub constraint_id: ConstraintId,
    pub kind: ConstraintKind,
    pub entities: Vec<Handle>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: u64) -> Handle {
        Handle::new(v)
    }

    #[test]
    fn glyph_placement_points_away_from_its_geometry() {
        let mut document = acadrust::CadDocument::new();
        let mut line = acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        );
        line.common.handle = h(1);
        document
            .add_entity(acadrust::EntityType::Line(line))
            .unwrap();
        let mut circle = acadrust::entities::Circle::from_center_radius(Vector3::ZERO, 5.0);
        circle.common.handle = h(2);
        document
            .add_entity(acadrust::EntityType::Circle(circle))
            .unwrap();
        let mut polyline = acadrust::entities::LwPolyline::from_points(vec![
            acadrust::types::Vector2::new(0.0, 0.0),
            acadrust::types::Vector2::new(10.0, 0.0),
        ]);
        polyline.common.handle = h(3);
        document
            .add_entity(acadrust::EntityType::LwPolyline(polyline))
            .unwrap();

        let constraint = |reference| ParametricConstraint {
            id: 0,
            kind: ConstraintKind::Fixed,
            refs: vec![reference],
            driving_param: None,
            enabled: true,
            native_origin: None,
            rigid_points: Vec::new(),
            distance_direction_type: 0,
            distance_direction: None,
            axis_direction: None,
            angle_sector: angle_sector::PARALLEL_COUNTERCLOCKWISE,
        };
        let (anchor, direction) =
            glyph_placement(&document, &constraint(ParametricRef::whole(h(1)))).unwrap();
        assert_eq!(anchor, Vector3::new(5.0, 0.0, 0.0));
        assert_eq!(direction, Vector3::new(0.0, 10.0, 0.0));
        let (anchor, direction) =
            glyph_placement(&document, &constraint(ParametricRef::point(h(1), 0))).unwrap();
        assert_eq!(anchor, Vector3::ZERO);
        assert_eq!(direction, Vector3::new(-5.0, 0.0, 0.0));
        let (anchor, direction) =
            glyph_placement(&document, &constraint(ParametricRef::center(h(2)))).unwrap();
        assert_eq!(anchor, Vector3::new(5.0, 0.0, 0.0));
        assert_eq!(direction, Vector3::new(5.0, 0.0, 0.0));
        let (anchor, direction) =
            glyph_placement(&document, &constraint(ParametricRef::segment(h(3), 0))).unwrap();
        assert_eq!(anchor, Vector3::new(5.0, 0.0, 0.0));
        assert_eq!(direction, Vector3::new(0.0, 10.0, 0.0));

        let mut tangent_line = acadrust::entities::Line::from_points(
            Vector3::new(-10.0, 5.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        );
        tangent_line.common.handle = h(4);
        document.add_entity(acadrust::EntityType::Line(tangent_line)).unwrap();
        let relation = |id, kind, refs| ParametricConstraint {
            id,
            kind,
            refs,
            driving_param: None,
            enabled: true,
            native_origin: None,
            rigid_points: Vec::new(),
            distance_direction_type: 0,
            distance_direction: None,
            axis_direction: None,
            angle_sector: angle_sector::PARALLEL_COUNTERCLOCKWISE,
        };
        let tangent = relation(
            1,
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(h(4)), ParametricRef::whole(h(2))],
        );
        assert_eq!(
            glyph_placement(&document, &tangent).unwrap().0,
            Vector3::new(0.0, 5.0, 0.0)
        );

        let mut rectangle = acadrust::entities::LwPolyline::from_points(vec![
            acadrust::types::Vector2::new(0.0, 0.0),
            acadrust::types::Vector2::new(4.0, 0.0),
            acadrust::types::Vector2::new(4.0, 2.0),
            acadrust::types::Vector2::new(0.0, 2.0),
        ]);
        rectangle.common.handle = h(5);
        rectangle.is_closed = true;
        document.add_entity(acadrust::EntityType::LwPolyline(rectangle)).unwrap();
        let parallel = glyph_placements(
            &document,
            &relation(
                2,
                ConstraintKind::Parallel,
                vec![ParametricRef::segment(h(5), 0), ParametricRef::segment(h(5), 2)],
            ),
        );
        assert_eq!(parallel.len(), 2);
        assert_eq!(parallel[0].0, Vector3::new(2.0, 0.0, 0.0));
        assert_eq!(parallel[1].0, Vector3::new(2.0, 2.0, 0.0));
        let perpendicular = relation(
            3,
            ConstraintKind::Perpendicular,
            vec![ParametricRef::segment(h(5), 3), ParametricRef::segment(h(5), 2)],
        );
        assert_eq!(
            glyph_placement(&document, &perpendicular).unwrap().0,
            Vector3::new(0.0, 2.0, 0.0)
        );
    }

    #[test]
    fn polyline_constraint_hover_builds_only_the_referenced_segment() {
        let mut scene = super::super::Scene::new();
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(
            acadrust::entities::LwPolyline::from_points(vec![
                acadrust::types::Vector2::new(0.0, 0.0),
                acadrust::types::Vector2::new(4.0, 0.0),
                acadrust::types::Vector2::new(4.0, 2.0),
            ]),
        ));

        scene.set_constraint_hover_highlights(&[ParametricRef::segment(handle, 1)]);

        assert!(scene.constraint_hover_highlights.is_empty());
        assert_eq!(scene.constraint_hover_wires.len(), 1);
        let wire = &scene.constraint_hover_wires[0];
        let points: Vec<_> = wire.points.iter().zip(&wire.points_low).map(|(high, low)| [
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        ]).collect();
        assert_eq!(points, vec![[4.0, 0.0, 0.0], [4.0, 2.0, 0.0]]);
    }

    #[test]
    fn add_assigns_increasing_ids_and_get_finds_them() {
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        let a = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(1))],
            None,
        );
        let b = set.add(
            ConstraintKind::Distance,
            vec![ParametricRef::whole(h(1))],
            Some(DrivingValue::Literal(25.0)),
        );
        assert_ne!(a, b);
        assert_eq!(set.get(a).unwrap().kind, ConstraintKind::Horizontal);
        assert_eq!(
            set.get(b).unwrap().driving_param,
            Some(DrivingValue::Literal(25.0))
        );
    }

    #[test]
    fn remove_drops_only_the_matching_id() {
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        let a = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(1))],
            None,
        );
        let b = set.add(
            ConstraintKind::Vertical,
            vec![ParametricRef::whole(h(2))],
            None,
        );
        assert!(set.remove(a));
        assert!(
            !set.remove(a),
            "removing twice should report nothing removed the second time"
        );
        assert!(set.get(a).is_none());
        assert!(set.get(b).is_some());
    }

    #[test]
    fn constraints_touching_finds_entity_regardless_of_marker() {
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(h(1), 0), ParametricRef::point(h(2), 1)],
            None,
        );
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(3))],
            None,
        );

        let touching_1: Vec<_> = set.constraints_touching(h(1)).collect();
        assert_eq!(touching_1.len(), 1);
        let touching_3: Vec<_> = set.constraints_touching(h(3)).collect();
        assert_eq!(touching_3.len(), 1);
        assert_eq!(set.constraints_touching(h(99)).count(), 0);
    }

    #[test]
    fn constraints_touching_skips_disabled() {
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        let id = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(1))],
            None,
        );
        set.constraints
            .iter_mut()
            .find(|c| c.id == id)
            .unwrap()
            .enabled = false;
        assert_eq!(set.constraints_touching(h(1)).count(), 0);
    }

    #[test]
    fn remove_all_touching_drops_every_constraint_referencing_the_entity() {
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        let coincident = set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(h(1), 0), ParametricRef::point(h(2), 1)],
            None,
        );
        let horizontal_other = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(3))],
            None,
        );

        let removed = set.remove_all_touching(h(1));
        assert_eq!(removed, vec![coincident]);
        assert!(set.get(coincident).is_none());
        assert!(
            set.get(horizontal_other).is_some(),
            "unrelated entity's constraint must survive"
        );
    }

    #[test]
    fn glyph_hit_test_entries_matches_precomputed_boxes() {
        let mut scene = super::super::Scene::new();
        let scope = ParametricScope::ModelSpace;
        let line = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ));
        scene.selection.borrow_mut().vp_size = (800.0, 600.0);
        let id = scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
        scene.note_parametric_constraint_applied(scope, id, 3);
        let vp = (800.0_f32, 600.0_f32);
        let entries = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(!entries.is_empty(), "fixture must yield glyphs");
        // A click at the centre of the precomputed box must hit that entry,
        // using the cached layout only (no offset recompute).
        let centre = iced::Point::new(
            entries[0].top_left.x + entries[0].size.width * 0.5,
            entries[0].top_left.y + entries[0].size.height * 0.5,
        );
        assert_eq!(super::glyph_hit_test_entries(&entries, centre), Some(0));
        // Far away from every box must miss.
        assert_eq!(
            super::glyph_hit_test_entries(&entries, iced::Point::new(-5000.0, -5000.0)),
            None
        );
        // `constraint_glyph_hit` must agree with the cached entries (it reuses
        // the cache rather than recomputing placements).
        assert_eq!(
            scene.constraint_glyph_hit(scope, vp, true, 3, 4095, centre),
            Some(entries[0].id)
        );
    }

    #[test]
    fn cached_glyph_placements_hit_returns_same_arc() {
        let mut scene = super::super::Scene::new();
        let scope = ParametricScope::ModelSpace;
        let line = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ));
        scene.selection.borrow_mut().vp_size = (800.0, 600.0);
        let id = scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
        scene.note_parametric_constraint_applied(scope, id, 3);
        let vp = (800.0_f32, 600.0_f32);
        let first = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        let second = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            !first.is_empty(),
            "fixture must yield at least one glyph placement"
        );
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "identical key inputs must return the same cached Arc"
        );
    }

    #[test]
    fn cached_glyph_placements_invalidated_by_metadata_edit() {
        let mut scene = super::super::Scene::new();
        let scope = ParametricScope::ModelSpace;
        let line = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ));
        scene.selection.borrow_mut().vp_size = (800.0, 600.0);
        let id = scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
        scene.note_parametric_constraint_applied(scope, id, 3);
        let vp = (800.0_f32, 600.0_f32);
        let before = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            !before.is_empty(),
            "fixture must yield at least one glyph placement"
        );
        // NOTE: no production set_enabled/toggle_enabled method exists (grep:
        // `enabled` is only written in `add()`, DWG import, and tests), so the
        // metadata edit goes through the REAL production Scene-level toggle
        // used by draw/update paths — hiding the geometric constraint — which
        // Task 2 must wire to an epoch bump. Not a direct field write.
        scene.set_parametric_constraint_visibility(scope, None, false, false);
        let after = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            !std::sync::Arc::ptr_eq(&before, &after),
            "metadata edit must invalidate the cache (new Arc)"
        );
        assert!(
            after.is_empty(),
            "output must reflect the edit: hidden constraint yields no glyphs"
        );
        // A selection change must also miss when `display_mode & 2 != 0`
        // (sel_sig is part of the key): restore, warm, then reselect.
        scene.set_parametric_constraint_visibility(scope, None, false, true);
        let restored = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            !restored.is_empty(),
            "restored constraint must yield glyphs again"
        );
        let reselected_warm = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            std::sync::Arc::ptr_eq(&restored, &reselected_warm),
            "identical key inputs must return the same cached Arc"
        );
        scene.selected.insert(line);
        let reselected = scene.cached_glyph_placements(scope, vp, true, 3, 4095);
        assert!(
            !std::sync::Arc::ptr_eq(&reselected_warm, &reselected),
            "selection change must invalidate the cache (new Arc)"
        );
    }

    #[test]
    fn scope_owner_handle_resolves_block_directly() {
        let block_handle = h(42);
        let scope = ParametricScope::Block(block_handle);
        let doc = acadrust::CadDocument::new();
        assert_eq!(scope.owner_handle(&doc), block_handle);
    }

    #[test]
    fn ref_center_constructor_matches_the_dash_three_convention() {
        let r = ParametricRef::center(h(7));
        assert_eq!(
            r,
            ParametricRef {
                entity: h(7),
                marker: Some(-3)
            }
        );
    }

    #[test]
    /// The center grip drives the center; the start, end and midpoint grips
    /// reshape the arc, so they drive the whole curve and a paired Equal or
    /// Symmetric follows the new radius and sweep.
    fn arc_grips_drive_center_or_the_whole_arc() {
        let handle = h(8);
        let arc = acadrust::EntityType::Arc(acadrust::entities::Arc::from_coords(
            0.0,
            0.0,
            0.0,
            5.0,
            0.0,
            std::f64::consts::PI,
        ));

        assert_eq!(
            grip_solve_anchor_refs(&arc, handle, 0),
            vec![ParametricRef::center(handle)]
        );
        for grip in 1..=3 {
            assert_eq!(
                grip_solve_anchor_refs(&arc, handle, grip),
                vec![ParametricRef::whole(handle)],
                "grip {grip}"
            );
        }
        assert_eq!(grip_solve_anchor_refs(&arc, handle, 4), Vec::new());
    }

    #[test]
    fn parameter_usage_finds_every_constraint_driven_by_the_named_parameter() {
        let mut scene = super::super::Scene::new();
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![ParametricRef::point(h(1), 0), ParametricRef::point(h(1), 1)],
                Some(DrivingValue::Named("gap".to_string())),
            );
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Radius,
                vec![ParametricRef::whole(h(2))],
                Some(DrivingValue::Named("gap".to_string())),
            );
        // Unrelated: a literal-driven constraint and one driven by a
        // different name must not show up.
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Radius,
                vec![ParametricRef::whole(h(3))],
                Some(DrivingValue::Literal(5.0)),
            );
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![ParametricRef::point(h(4), 0), ParametricRef::point(h(4), 1)],
                Some(DrivingValue::Named("other".to_string())),
            );

        let usage = scene.parameter_usage("gap");
        assert_eq!(
            usage.len(),
            2,
            "exactly the two constraints driven by 'gap', got {usage:?}"
        );
        assert!(usage
            .iter()
            .any(|u| u.kind == ConstraintKind::Distance && u.entities == vec![h(1)]));
        assert!(usage
            .iter()
            .any(|u| u.kind == ConstraintKind::Radius && u.entities == vec![h(2)]));

        assert_eq!(scene.parameter_usage("nonexistent").len(), 0);
    }

    #[test]
    fn parameter_usage_searches_every_scope_not_just_model_space() {
        let mut scene = super::super::Scene::new();
        let block = h(99);
        scene
            .parametric_constraint_set_mut(ParametricScope::Block(block))
            .add(
                ConstraintKind::Radius,
                vec![ParametricRef::whole(h(1))],
                Some(DrivingValue::Named("r".to_string())),
            );
        let usage = scene.parameter_usage("r");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].scope, ParametricScope::Block(block));
    }

    #[test]
    fn parameter_usage_deduplicates_an_entity_referenced_by_two_markers() {
        let mut scene = super::super::Scene::new();
        // A Distance constraint whose two points are both on the same
        // entity (e.g. a line's own start and end) must list that entity
        // once, not twice.
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![ParametricRef::point(h(1), 0), ParametricRef::point(h(1), 1)],
                Some(DrivingValue::Named("len".to_string())),
            );
        let usage = scene.parameter_usage("len");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].entities, vec![h(1)]);
    }

    #[test]
    fn automatic_inference_maps_relations_and_skips_existing_constraints() {
        let mut scene = super::super::Scene::new();
        let first = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(5.0, 0.0, 0.0),
            ),
        ));
        let second = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(5.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ),
        ));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(first)],
                None,
            );

        let inferred =
            scene.inferred_parametric_constraints(
                ParametricScope::ModelSpace,
                &[first, second],
                &crate::app::settings::AutoConstrainSettings::default(),
            );

        assert!(!inferred.iter().any(|(kind, refs)| {
            *kind == ConstraintKind::Horizontal && *refs == [ParametricRef::whole(first)]
        }));
        assert!(inferred.iter().any(|(kind, refs)| {
            *kind == ConstraintKind::Horizontal && *refs == [ParametricRef::whole(second)]
        }));
        assert!(inferred
            .iter()
            .any(|(kind, _)| *kind == ConstraintKind::Coincident));
        assert!(!inferred
            .iter()
            .any(|(kind, _)| *kind == ConstraintKind::Colinear));
    }

    #[test]
    fn visibility_toggles_geometric_and_dimensional_independently() {
        let mut scene = super::super::Scene::new();
        let line = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(5.0, 0.0, 0.0),
            ),
        ));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        let geometric = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
        let dimensional = set.add(
            ConstraintKind::Distance,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Literal(5.0)),
        );

        assert_eq!(
            scene.set_parametric_constraint_visibility(
                ParametricScope::ModelSpace,
                None,
                false,
                false,
            ),
            1
        );
        assert!(!scene.is_parametric_constraint_visible(ParametricScope::ModelSpace, geometric));
        assert!(scene.is_parametric_constraint_visible(ParametricScope::ModelSpace, dimensional));
    }

    #[test]
    fn constraint_display_distinguishes_loaded_created_and_explicit_visibility() {
        let mut scene = super::super::Scene::new();
        let scope = ParametricScope::ModelSpace;
        let id = scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(1))],
            None,
        );
        assert!(!scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, false, 3, 4095
        ));
        assert!(scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, true, 2, 4095
        ));
        assert!(!scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, true, 1, 4095
        ));
        scene.note_parametric_constraint_applied(scope, id, 1);
        assert!(scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, false, 0, 4095
        ));
        scene.set_parametric_constraint_visibility(scope, None, false, false);
        assert!(!scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, true, 3, 4095
        ));
        scene.set_parametric_constraint_visibility(scope, None, false, true);
        assert!(scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, false, 0, 4095
        ));
        scene.note_parametric_constraint_applied(scope, id, 0);
        assert!(!scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, false, 3, 4095
        ));
        assert!(scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, true, 2, 4095
        ));
    }

    #[test]
    fn hidden_constraint_stays_hidden_when_related_entity_is_selected() {
        let mut scene = super::super::Scene::new();
        let scope = ParametricScope::ModelSpace;
        let id = scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(h(1))],
            None,
        );
        scene.hidden_parametric_constraints.insert((scope, id));

        assert!(!scene.should_display_parametric_constraint(
            scope, id, ConstraintKind::Horizontal, true, 2, 4095
        ));
    }

    #[test]
    fn hover_geometry_keeps_a_bulged_polyline_segment_curved() {
        let mut document = acadrust::CadDocument::new();
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.common.handle = h(1);
        polyline.vertices = vec![
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(0.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
        ];
        document
            .add_entity(acadrust::EntityType::LwPolyline(polyline))
            .unwrap();

        assert!(matches!(
            constraint_reference_curve_xy(&document, ParametricRef::segment(h(1), 0)),
            Some(cadkernel::geom2d::Curve::Arc(_))
        ));
    }

    #[test]
    fn curve_pick_uses_the_bulged_segment_instead_of_its_chord() {
        let mut scene = super::super::Scene::new();
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.vertices = vec![
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(0.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
            acadrust::entities::LwVertex::from_coords(0.0, -4.0),
        ];
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(polyline));

        assert_eq!(
            parametric_curve_ref_for_pick(
                &scene.document,
                ParametricScope::ModelSpace,
                handle,
                Vector3::new(5.0, -5.0, 0.0),
            ),
            Some(ParametricRef::segment(handle, 0))
        );
    }

    #[test]
    fn move_expansion_follows_only_coincident_relations() {
        let mut scene = super::super::Scene::new();
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(h(1), 0), ParametricRef::point(h(2), 0)],
            None,
        );
        set.add(
            ConstraintKind::PointOnCurve,
            vec![ParametricRef::point(h(2), 0), ParametricRef::whole(h(3))],
            None,
        );
        set.add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(h(3)), ParametricRef::whole(h(4))],
            None,
        );

        assert_eq!(
            scene.parametric_connected_handles(ParametricScope::ModelSpace, &[h(1)], false),
            vec![h(1), h(2), h(3)]
        );
    }

    #[test]
    fn duplicating_an_axis_constraint_keeps_its_direction() {
        let mut scene = super::super::Scene::new();
        let line = |y| {
            acadrust::EntityType::Line(acadrust::entities::Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(4.0, y + 1.0, 0.0),
            ))
        };
        let source = scene.add_entity(line(0.0));
        let copied = scene.add_entity(line(10.0));
        let direction = Vector3::new(3.0, 4.0, 0.0);
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add_axis_constraint(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(source)],
                direction,
            );
        let mut handle_map = rustc_hash::FxHashMap::default();
        handle_map.insert(source, copied);

        scene.duplicate_parametric_constraints_for(&handle_map);

        let constraints = &scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints;
        assert_eq!(constraints.len(), 2);
        assert_eq!(constraints[1].refs, vec![ParametricRef::whole(copied)]);
        assert_eq!(constraints[1].axis_direction, Some(direction.normalize()));
    }
}

#[cfg(test)]
mod two_lines_placement_tests {
    use super::two_lines_placement;
    use glam::DVec3;

    fn p(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1.0e-2
    }

    // Measured on the reference: first line (200,0)-(300,0) picked at its
    // midpoint, the second line as listed, and where it ended up.
    #[test]
    fn follows_the_reference_measurements() {
        let first = [p(200.0, 0.0), p(300.0, 0.0)];
        let pick = p(250.0, 0.0);
        let cases = [
            ([p(0.0, 0.0), p(100.0, 50.0)], [p(0.0, -111.803), p(111.803, -111.803)]),
            ([p(0.0, 40.0), p(100.0, 90.0)], [p(0.0, 147.58), p(111.803, 147.58)]),
            ([p(100.0, 0.0), p(200.0, 50.0)], [p(100.0, -67.08), p(211.803, -67.08)]),
            ([p(-100.0, 0.0), p(0.0, 50.0)], [p(-100.0, -156.52), p(11.803, -156.52)]),
            ([p(0.0, 0.0), p(200.0, 100.0)], [p(0.0, -111.8), p(223.607, -111.8)]),
            ([p(0.0, 0.0), p(79.0569, 79.0569)], [p(0.0, -176.78), p(111.803, -176.78)]),
            ([p(0.0, 0.0), p(100.0, -50.0)], [p(0.0, -111.8), p(111.803, -111.8)]),
            ([p(100.0, 50.0), p(0.0, 0.0)], [p(100.0, 111.8), p(-11.803, 111.8)]),
            ([p(0.0, 20.0), p(100.0, 70.0)], [p(0.0, 129.69), p(111.803, 129.69)]),
        ];
        for (second, expected) in cases {
            let placed = two_lines_placement(first, pick, second).expect("a placement");
            assert!(close(placed[0], expected[0]) && close(placed[1], expected[1]), "{second:?} -> {placed:?}, expected {expected:?}");
        }
        // The first line picked near its end: the distance grows with it.
        let placed = two_lines_placement(first, p(290.0, 0.0), [p(0.0, 0.0), p(100.0, 50.0)]).unwrap();
        assert!(close(placed[0], p(0.0, -129.69)));
        // A slanted first line: the second line's start keeps its place along it.
        let placed = two_lines_placement(
            [p(500.0, 300.0), p(600.0, 350.0)],
            p(550.0, 325.0),
            [p(400.0, 300.0), p(500.0, 300.0)],
        )
        .unwrap();
        assert!(close(placed[0], p(408.82, 282.36)) && close(placed[1], p(498.26, 327.08)), "{placed:?}");
    }
}
