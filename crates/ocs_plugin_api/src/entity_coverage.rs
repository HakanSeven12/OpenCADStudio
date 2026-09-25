//! Host-owned coverage generated from the traced entity registry and an
//! explicit policy. `unmapped` fields remain accessible through the typed
//! `CadDocument` snapshot but are not promised by the Python document model.

pub use crate::entity_coverage_types::*;

pub const EMBEDDED_ENTITY_COVERAGE_JSON: &str =
    include_str!(concat!(env!("OUT_DIR"), "/entity_coverage.json"));

pub fn get_embedded_entity_coverage_json() -> &'static str {
    EMBEDDED_ENTITY_COVERAGE_JSON
}

/// Read-only JSON serialization of one typed entity. The outer object
/// has one key: the `EntityType` variant name used by the coverage catalog.
/// JSON does not preserve non-finite floating-point values, so this is an
/// inspection format and must remain separate from the typed entity write path.
#[cfg(feature = "host")]
pub fn entity_snapshot(
    entity: &crate::host::EntityType,
) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(entity)
}

#[cfg(feature = "host")]
fn finite_vector(name: &str, value: &crate::host::codec::types::Vector3) -> Result<(), String> {
    if value.x.is_finite() && value.y.is_finite() && value.z.is_finite() {
        Ok(())
    } else {
        Err(format!("{name} must contain finite coordinates"))
    }
}

#[cfg(feature = "host")]
fn unit_direction(name: &str, value: &crate::host::codec::types::Vector3) -> Result<(), String> {
    finite_vector(name, value)?;
    let length = value.x.hypot(value.y).hypot(value.z);
    if (length - 1.0).abs() <= 1e-6 {
        Ok(())
    } else {
        Err(format!("{name} must be a unit vector"))
    }
}

#[cfg(feature = "host")]
fn solid_normal(name: &str, value: &crate::host::codec::types::Vector3) -> Result<(), String> {
    finite_vector(name, value)?;
    if value.x.hypot(value.y).hypot(value.z) > 0.0 {
        Ok(())
    } else {
        Err(format!("{name} must be nonzero"))
    }
}

#[cfg(feature = "host")]
fn insert_scale(name: &str, value: f64) -> Result<(), String> {
    if value.is_finite() && value.abs() >= 1e-12 {
        Ok(())
    } else {
        Err(format!("{name} must be finite and nonzero"))
    }
}

#[cfg(feature = "host")]
fn validate_hatch_paths(paths: &[crate::host::codec::entities::BoundaryPath]) -> Result<(), String> {
    use crate::host::codec::entities::BoundaryEdge;
    use crate::host::codec::types::Vector2;
    let close = |a: Vector2, b: Vector2| (a.x - b.x).hypot(a.y - b.y) <= 1e-8;
    let ellipse_point = |center: Vector2, major: Vector2, ratio: f64, angle: f64| {
        Vector2::new(
            center.x + major.x * angle.cos() - major.y * ratio * angle.sin(),
            center.y + major.y * angle.cos() + major.x * ratio * angle.sin(),
        )
    };
    if paths.is_empty() {
        return Err("Hatch requires at least one boundary path".into());
    }
    for (pi, path) in paths.iter().enumerate() {
        if path.edges.is_empty() {
            return Err(format!("Hatch boundary path {pi} has no edges"));
        }
        if path.flags.is_not_closed() {
            return Err(format!("Hatch boundary path {pi} is marked open"));
        }
        if path.flags.bits() & !0x1ff != 0 {
            return Err(format!("Hatch boundary path {pi} has unknown flag bits"));
        }
        let mut endpoints = Vec::with_capacity(path.edges.len());
        for (ei, edge) in path.edges.iter().enumerate() {
            let pair = match edge {
                BoundaryEdge::Line(line) => {
                    if !line.start.x.is_finite() || !line.start.y.is_finite() || !line.end.x.is_finite() || !line.end.y.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} line has non-finite coordinates"));
                    }
                    if (line.start.x - line.end.x).hypot(line.start.y - line.end.y) < 1e-12 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} line has zero length"));
                    }
                    (line.start, line.end)
                }
                BoundaryEdge::CircularArc(arc) => {
                    if !arc.center.x.is_finite() || !arc.center.y.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} arc center has non-finite coordinates"));
                    }
                    if !arc.radius.is_finite() || arc.radius <= 0.0 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} arc radius must be finite and positive"));
                    }
                    if !arc.start_angle.is_finite() || !arc.end_angle.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} arc angles must be finite"));
                    }
                    let point = |angle: f64| Vector2::new(
                        arc.center.x + arc.radius * angle.cos(),
                        arc.center.y + arc.radius * angle.sin(),
                    );
                    let (start, end) = (point(arc.start_angle), point(arc.end_angle));
                    if arc.counter_clockwise { (start, end) } else { (end, start) }
                }
                BoundaryEdge::EllipticArc(arc) => {
                    if !arc.center.x.is_finite() || !arc.center.y.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} ellipse center has non-finite coordinates"));
                    }
                    if !arc.major_axis_endpoint.x.is_finite() || !arc.major_axis_endpoint.y.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} ellipse major axis has non-finite coordinates"));
                    }
                    if arc.major_axis_endpoint.x.hypot(arc.major_axis_endpoint.y) < 1e-12 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} ellipse major axis has zero length"));
                    }
                    if !arc.minor_axis_ratio.is_finite() || !(0.0..=1.0).contains(&arc.minor_axis_ratio) || arc.minor_axis_ratio == 0.0 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} ellipse ratio must be within (0, 1]"));
                    }
                    if !arc.start_angle.is_finite() || !arc.end_angle.is_finite() {
                        return Err(format!("Hatch boundary path {pi} edge {ei} ellipse angles must be finite"));
                    }
                    let start = ellipse_point(arc.center, arc.major_axis_endpoint, arc.minor_axis_ratio, arc.start_angle);
                    let end = ellipse_point(arc.center, arc.major_axis_endpoint, arc.minor_axis_ratio, arc.end_angle);
                    if arc.counter_clockwise { (start, end) } else { (end, start) }
                }
                BoundaryEdge::Polyline(poly) => {
                    if poly.vertices.len() < 3 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} polyline must have at least 3 vertices"));
                    }
                    for (vi, v) in poly.vertices.iter().enumerate() {
                        if !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite() {
                            return Err(format!("Hatch boundary path {pi} edge {ei} vertex {vi} has non-finite coordinates"));
                        }
                    }
                    if !poly.is_closed {
                        return Err(format!("Hatch boundary path {pi} edge {ei} polyline is open"));
                    }
                    let first = poly.vertices.first().unwrap();
                    (Vector2::new(first.x, first.y), Vector2::new(first.x, first.y))
                }
                BoundaryEdge::Spline(spline) => {
                    if spline.degree < 1 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} spline degree must be at least 1"));
                    }
                    if spline.control_points.len() < spline.degree as usize + 1 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} spline has too few control points for its degree"));
                    }
                    if spline.knots.iter().any(|knot| !knot.is_finite())
                        || spline.knots.windows(2).any(|pair| pair[0] > pair[1])
                    {
                        return Err(format!("Hatch boundary path {pi} edge {ei} spline knots must be finite and nondecreasing"));
                    }
                    for (ci, cp) in spline.control_points.iter().enumerate() {
                        if !cp.x.is_finite() || !cp.y.is_finite() || !cp.z.is_finite() {
                            return Err(format!("Hatch boundary path {pi} edge {ei} control point {ci} has non-finite coordinates"));
                        }
                    }
                    for (fi, fp) in spline.fit_points.iter().enumerate() {
                        if !fp.x.is_finite() || !fp.y.is_finite() {
                            return Err(format!("Hatch boundary path {pi} edge {ei} fit point {fi} has non-finite coordinates"));
                        }
                    }
                    for (name, tangent) in [("start", spline.start_tangent), ("end", spline.end_tangent)] {
                        if !tangent.x.is_finite() || !tangent.y.is_finite() {
                            return Err(format!("Hatch boundary path {pi} edge {ei} spline {name} tangent is non-finite"));
                        }
                    }
                    let points = if spline.fit_points.len() >= 2 {
                        spline.fit_points.clone()
                    } else {
                        spline.control_points.iter().map(|p| Vector2::new(p.x, p.y)).collect()
                    };
                    if points.len() < 2 {
                        return Err(format!("Hatch boundary path {pi} edge {ei} spline has too few points"));
                    }
                    (*points.first().unwrap(), *points.last().unwrap())
                }
            };
            endpoints.push(pair);
        }
        for ei in 0..endpoints.len() {
            let next = (ei + 1) % endpoints.len();
            if !close(endpoints[ei].1, endpoints[next].0) {
                return Err(format!("Hatch boundary path {pi} is not closed between edges {ei} and {next}"));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "host")]
fn validate_hatch_pattern(value: &crate::host::codec::entities::Hatch) -> Result<(), String> {
    if value.pattern.name.trim().is_empty() {
        return Err("Hatch.pattern.name is empty".into());
    }
    if value.is_solid {
        if !value.pattern.name.eq_ignore_ascii_case("SOLID") || !value.pattern.lines.is_empty() {
            return Err("solid Hatch requires the SOLID pattern without pattern lines".into());
        }
        return Ok(());
    }
    if !value.pattern_scale.is_finite() || value.pattern_scale <= 0.0 {
        return Err("Hatch.pattern_scale must be finite and greater than zero".into());
    }
    if value.pattern.lines.is_empty() {
        return Err("patterned Hatch requires at least one pattern line".into());
    }
    for (index, line) in value.pattern.lines.iter().enumerate() {
        if !line.angle.is_finite()
            || !line.base_point.x.is_finite() || !line.base_point.y.is_finite()
            || !line.offset.x.is_finite() || !line.offset.y.is_finite()
            || line.dash_lengths.iter().any(|dash| !dash.is_finite())
        {
            return Err(format!("Hatch.pattern.lines[{index}] contains non-finite values"));
        }
        if line.offset.x.hypot(line.offset.y) < 1e-12 {
            return Err(format!("Hatch.pattern.lines[{index}].offset must be nonzero"));
        }
    }
    Ok(())
}

/// Field checks shared by Leader creation and mutation. `old` limits the
/// checks to fields the edit touched so legacy DWG leaders stay editable.
#[cfg(feature = "host")]
fn validate_leader(
    old: Option<&crate::host::codec::entities::Leader>,
    new: &crate::host::codec::entities::Leader,
) -> Result<(), String> {
    use crate::host::codec::entities::LeaderCreationType;
    let vec_changed = |a: &crate::host::codec::types::Vector3,
                       b: &crate::host::codec::types::Vector3| {
        old.is_none()
            || a.x.to_bits() != b.x.to_bits()
            || a.y.to_bits() != b.y.to_bits()
            || a.z.to_bits() != b.z.to_bits()
    };
    if old.is_none_or(|old| old.vertices != new.vertices) {
        if new.vertices.len() < 2 {
            return Err("Leader.vertices requires at least 2 points".into());
        }
        for (index, vertex) in new.vertices.iter().enumerate() {
            finite_vector(&format!("Leader.vertices[{index}]"), vertex)?;
        }
        if new.vertices.windows(2).all(|pair| pair[0] == pair[1]) {
            return Err("Leader.vertices must not all coincide".into());
        }
    }
    if let Some(old_leader) = old {
        if vec_changed(&old_leader.normal, &new.normal) {
            solid_normal("Leader.normal", &new.normal)?;
        }
    } else {
        solid_normal("Leader.normal", &new.normal)?;
    }
    if vec_changed(
        old.map_or(&new.horizontal_direction, |o| &o.horizontal_direction),
        &new.horizontal_direction,
    ) {
        solid_normal("Leader.horizontal_direction", &new.horizontal_direction)?;
    }
    for (name, before, after) in [
        (
            "block_offset",
            old.map_or(&new.block_offset, |o| &o.block_offset),
            &new.block_offset,
        ),
        (
            "annotation_offset",
            old.map_or(&new.annotation_offset, |o| &o.annotation_offset),
            &new.annotation_offset,
        ),
    ] {
        if vec_changed(before, after) {
            finite_vector(&format!("Leader.{name}"), after)?;
        }
    }
    for (name, before, after) in [
        (
            "text_height",
            old.map_or(new.text_height, |o| o.text_height),
            new.text_height,
        ),
        (
            "text_width",
            old.map_or(new.text_width, |o| o.text_width),
            new.text_width,
        ),
    ] {
        if (old.is_none() || before.to_bits() != after.to_bits())
            && (!after.is_finite() || after < 0.0 || (name == "text_height" && after == 0.0))
        {
            return Err(format!(
                "Leader.{name} must be finite and {}",
                if name == "text_height" { "greater than zero" } else { "non-negative" }
            ));
        }
    }
    if old.is_none_or(|o| o.dimension_style != new.dimension_style)
        && new.dimension_style.trim().is_empty()
    {
        return Err("Leader.dimension_style is empty".into());
    }
    if new.creation_type == LeaderCreationType::NoAnnotation && !new.annotation_handle.is_null() {
        return Err("Leader with no annotation cannot carry an annotation handle".into());
    }
    Ok(())
}

/// Field checks shared by MLine creation and mutation. Vertex `direction`,
/// `miter` and segment cut parameters are derived by the host, so only their
/// finiteness is checked here.
#[cfg(feature = "host")]
fn validate_mline(
    old: Option<&crate::host::codec::entities::MLine>,
    new: &crate::host::codec::entities::MLine,
) -> Result<(), String> {
    if old.is_none_or(|old| old.flags != new.flags) && new.flags.bits() & !0x0f != 0 {
        return Err("MLine.flags has unknown bits".into());
    }
    if old.is_none_or(|old| old.scale_factor.to_bits() != new.scale_factor.to_bits())
        && (!new.scale_factor.is_finite() || new.scale_factor == 0.0)
    {
        return Err("MLine.scale_factor must be finite and nonzero".into());
    }
    if old.is_none_or(|old| old.normal != new.normal) {
        solid_normal("MLine.normal", &new.normal)?;
    }
    if old.is_none_or(|old| old.style_name != new.style_name) && new.style_name.trim().is_empty() {
        return Err("MLine.style_name is empty".into());
    }
    if old.is_none_or(|old| old.vertices != new.vertices) {
        if new.vertices.len() < 2 {
            return Err("MLine.vertices requires at least 2 points".into());
        }
        for (vi, vertex) in new.vertices.iter().enumerate() {
            finite_vector(&format!("MLine.vertices[{vi}].position"), &vertex.position)?;
            finite_vector(&format!("MLine.vertices[{vi}].direction"), &vertex.direction)?;
            finite_vector(&format!("MLine.vertices[{vi}].miter"), &vertex.miter)?;
            for (si, segment) in vertex.segments.iter().enumerate() {
                if segment
                    .parameters
                    .iter()
                    .chain(&segment.area_fill_parameters)
                    .any(|value| !value.is_finite())
                {
                    return Err(format!(
                        "MLine.vertices[{vi}].segments[{si}] contains non-finite parameters"
                    ));
                }
            }
        }
        if new
            .vertices
            .windows(2)
            .all(|pair| pair[0].position == pair[1].position)
        {
            return Err("MLine.vertices must not all coincide".into());
        }
    }
    if new.flags.contains(crate::host::codec::entities::MLineFlags::CLOSED)
        && new.vertices.len() < 3
    {
        return Err("closed MLine requires at least 3 vertices".into());
    }
    Ok(())
}

/// Field checks for every Dimension subtype. Derived fields (base definition
/// point, `actual_measurement`) are recomputed by the host, so they are not
/// validated as inputs; the measurement the geometry implies must be finite.
#[cfg(feature = "host")]
fn validate_dimension(
    old: Option<&crate::host::codec::entities::Dimension>,
    new: &crate::host::codec::entities::Dimension,
) -> Result<(), String> {
    use crate::host::codec::entities::Dimension as D;
    if old == Some(new) {
        return Ok(());
    }
    let base = new.base();
    solid_normal("Dimension.normal", &base.normal)?;
    finite_vector("Dimension.text_middle_point", &base.text_middle_point)?;
    if !base.text_rotation.is_finite() || !base.horizontal_direction.is_finite() {
        return Err("Dimension.text_rotation and horizontal_direction must be finite".into());
    }
    if base.style_name.trim().is_empty() {
        return Err("Dimension.style_name is empty".into());
    }
    let (points, scalars, distinct): (Vec<(&str, &crate::host::codec::types::Vector3)>, Vec<(&str, f64)>, Vec<(&str, &str)>) = match new {
        D::Aligned(d) => (
            vec![("definition_point", &d.definition_point), ("first_point", &d.first_point), ("second_point", &d.second_point)],
            vec![("ext_line_rotation", d.ext_line_rotation)],
            vec![("first_point", "second_point")],
        ),
        D::Linear(d) => (
            vec![("definition_point", &d.definition_point), ("first_point", &d.first_point), ("second_point", &d.second_point)],
            vec![("rotation", d.rotation), ("ext_line_rotation", d.ext_line_rotation)],
            vec![("first_point", "second_point")],
        ),
        D::Radius(d) => (
            vec![("definition_point", &d.definition_point), ("angle_vertex", &d.angle_vertex)],
            vec![("leader_length", d.leader_length)],
            vec![("definition_point", "angle_vertex")],
        ),
        D::Diameter(d) => (
            vec![("definition_point", &d.definition_point), ("angle_vertex", &d.angle_vertex)],
            vec![("leader_length", d.leader_length)],
            vec![("definition_point", "angle_vertex")],
        ),
        D::Angular2Ln(d) => (
            vec![("definition_point", &d.definition_point), ("dimension_arc", &d.dimension_arc), ("first_point", &d.first_point), ("second_point", &d.second_point), ("angle_vertex", &d.angle_vertex)],
            vec![],
            vec![],
        ),
        D::Angular3Pt(d) => (
            vec![("definition_point", &d.definition_point), ("first_point", &d.first_point), ("second_point", &d.second_point), ("angle_vertex", &d.angle_vertex)],
            vec![],
            vec![("first_point", "angle_vertex"), ("second_point", "angle_vertex")],
        ),
        D::Ordinate(d) => (
            vec![("definition_point", &d.definition_point), ("feature_location", &d.feature_location), ("leader_endpoint", &d.leader_endpoint)],
            vec![],
            vec![],
        ),
        D::Arc(d) => (
            vec![("definition_point", &d.definition_point), ("first_extension_point", &d.first_extension_point), ("second_extension_point", &d.second_extension_point), ("center_point", &d.center_point), ("first_leader_point", &d.first_leader_point), ("second_leader_point", &d.second_leader_point)],
            vec![("arc_start_parameter", d.arc_start_parameter), ("arc_end_parameter", d.arc_end_parameter)],
            vec![("first_extension_point", "second_extension_point")],
        ),
        D::LargeRadial(d) => (
            vec![("definition_point", &d.definition_point), ("chord_point", &d.chord_point), ("override_center", &d.override_center), ("jog_point", &d.jog_point)],
            vec![("jog_angle", d.jog_angle)],
            vec![("definition_point", "chord_point")],
        ),
    };
    for (name, point) in &points {
        finite_vector(&format!("Dimension.{name}"), point)?;
    }
    for (name, value) in &scalars {
        if !value.is_finite() {
            return Err(format!("Dimension.{name} must be finite"));
        }
    }
    let find = |name: &str| points.iter().find(|(n, _)| *n == name).map(|(_, p)| **p);
    for (a, b) in distinct {
        if find(a) == find(b) {
            return Err(format!("Dimension.{a} and {b} must not coincide"));
        }
    }
    if !new.measurement().is_finite() {
        return Err("Dimension geometry does not define a finite measurement".into());
    }
    Ok(())
}

/// Field checks shared by MultiLeader creation and mutation.
#[cfg(feature = "host")]
fn validate_multileader(
    old: Option<&crate::host::codec::entities::MultiLeader>,
    new: &crate::host::codec::entities::MultiLeader,
) -> Result<(), String> {
    use crate::host::codec::entities::LeaderContentType;
    if old == Some(new) {
        return Ok(());
    }
    for (name, value, allow_zero) in [
        ("dogleg_length", new.dogleg_length, true),
        ("arrowhead_size", new.arrowhead_size, true),
        ("text_height", new.text_height, false),
        ("scale_factor", new.scale_factor, false),
    ] {
        if !value.is_finite() || value < 0.0 || (!allow_zero && value == 0.0) {
            return Err(format!(
                "MultiLeader.{name} must be finite and {}",
                if allow_zero { "non-negative" } else { "greater than zero" }
            ));
        }
    }
    if !new.block_rotation.is_finite() {
        return Err("MultiLeader.block_rotation must be finite".into());
    }
    finite_vector("MultiLeader.block_scale", &new.block_scale)?;
    let context = &new.context;
    if !context.scale_factor.is_finite() || context.scale_factor <= 0.0 {
        return Err("MultiLeader.context.scale_factor must be finite and greater than zero".into());
    }
    for (name, value) in [
        ("text_height", context.text_height),
        ("text_width", context.text_width),
        ("text_rotation", context.text_rotation),
        ("landing_gap", context.landing_gap),
        ("arrowhead_size", context.arrowhead_size),
    ] {
        if !value.is_finite() {
            return Err(format!("MultiLeader.context.{name} must be finite"));
        }
    }
    for (name, point) in [
        ("content_base_point", &context.content_base_point),
        ("text_location", &context.text_location),
        ("block_content_location", &context.block_content_location),
        ("base_point", &context.base_point),
    ] {
        finite_vector(&format!("MultiLeader.context.{name}"), point)?;
    }
    if context.transform_matrix.iter().any(|value| !value.is_finite()) {
        return Err("MultiLeader.context.transform_matrix must be finite".into());
    }
    for (ri, root) in context.leader_roots.iter().enumerate() {
        finite_vector(&format!("MultiLeader.context.leader_roots[{ri}].connection_point"), &root.connection_point)?;
        finite_vector(&format!("MultiLeader.context.leader_roots[{ri}].direction"), &root.direction)?;
        if !root.landing_distance.is_finite() {
            return Err(format!("MultiLeader.context.leader_roots[{ri}].landing_distance must be finite"));
        }
        for (li, line) in root.lines.iter().enumerate() {
            if line.points.is_empty() {
                return Err(format!("MultiLeader.context.leader_roots[{ri}].lines[{li}] has no points"));
            }
            for (pi, point) in line.points.iter().enumerate() {
                finite_vector(&format!("MultiLeader.context.leader_roots[{ri}].lines[{li}].points[{pi}]"), point)?;
            }
        }
    }
    match new.content_type {
        LeaderContentType::MText if !context.has_text_contents => {
            return Err("MText MultiLeader requires context.has_text_contents".into());
        }
        LeaderContentType::Block if !context.has_block_contents => {
            return Err("Block MultiLeader requires context.has_block_contents".into());
        }
        LeaderContentType::None if context.has_text_contents || context.has_block_contents => {
            return Err("MultiLeader without content cannot carry text or block contents".into());
        }
        _ => {}
    }
    Ok(())
}

/// Field checks shared by Table creation and mutation.
#[cfg(feature = "host")]
fn validate_table(
    old: Option<&crate::host::codec::entities::Table>,
    new: &crate::host::codec::entities::Table,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    finite_vector("Table.insertion_point", &new.insertion_point)?;
    solid_normal("Table.normal", &new.normal)?;
    solid_normal("Table.horizontal_direction", &new.horizontal_direction)?;
    if !new.break_spacing.is_finite() || new.break_spacing < 0.0 {
        return Err("Table.break_spacing must be finite and non-negative".into());
    }
    if new.rows.is_empty() || new.columns.is_empty() {
        return Err("Table requires at least one row and one column".into());
    }
    for (ci, column) in new.columns.iter().enumerate() {
        if !column.width.is_finite() || column.width <= 0.0 {
            return Err(format!("Table.columns[{ci}].width must be finite and greater than zero"));
        }
    }
    for (ri, row) in new.rows.iter().enumerate() {
        if !row.height.is_finite() || row.height <= 0.0 {
            return Err(format!("Table.rows[{ri}].height must be finite and greater than zero"));
        }
        if row.cells.len() != new.columns.len() {
            return Err(format!(
                "Table.rows[{ri}] has {} cells but the table has {} columns",
                row.cells.len(),
                new.columns.len()
            ));
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            if !cell.rotation.is_finite() || !cell.geometry_scale.is_finite() || !cell.block_scale.is_finite() {
                return Err(format!("Table.rows[{ri}].cells[{ci}] has non-finite rotation or scale"));
            }
            for (ki, content) in cell.contents.iter().enumerate() {
                if !content.rotation.is_finite() || !content.scale.is_finite() || !content.text_height.is_finite() || content.text_height < 0.0 {
                    return Err(format!(
                        "Table.rows[{ri}].cells[{ci}].contents[{ki}] has invalid rotation, scale or text height"
                    ));
                }
                if !content.value.numeric_value.is_finite() {
                    return Err(format!(
                        "Table.rows[{ri}].cells[{ci}].contents[{ki}].value.numeric_value must be finite"
                    ));
                }
            }
        }
    }
    let (rows, columns) = (new.rows.len(), new.columns.len());
    for (mi, range) in new.merged_ranges.iter().enumerate() {
        if range.top_row > range.bottom_row
            || range.left_col > range.right_col
            || range.bottom_row >= rows
            || range.right_col >= columns
        {
            return Err(format!("Table.merged_ranges[{mi}] lies outside the {rows}x{columns} grid"));
        }
        for (mj, other) in new.merged_ranges.iter().enumerate().skip(mi + 1) {
            let separate = range.bottom_row < other.top_row
                || other.bottom_row < range.top_row
                || range.right_col < other.left_col
                || other.right_col < range.left_col;
            if !separate {
                return Err(format!("Table.merged_ranges[{mi}] overlaps merged_ranges[{mj}]"));
            }
        }
    }
    Ok(())
}

/// PolygonMesh: an M x N grid; the vertex list must match exactly.
#[cfg(feature = "host")]
fn validate_polygon_mesh(
    old: Option<&crate::host::codec::entities::PolygonMeshEntity>,
    new: &crate::host::codec::entities::PolygonMeshEntity,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if new.m_vertex_count < 2 || new.n_vertex_count < 2 {
        return Err("PolygonMesh.m_vertex_count and n_vertex_count must be at least 2".into());
    }
    let expected = new.m_vertex_count as usize * new.n_vertex_count as usize;
    if new.vertices.len() != expected {
        return Err(format!(
            "PolygonMesh has {} vertices but {}x{} requires {expected}",
            new.vertices.len(),
            new.m_vertex_count,
            new.n_vertex_count
        ));
    }
    if new.m_smooth_density < 0 || new.n_smooth_density < 0 {
        return Err("PolygonMesh smooth densities must be non-negative".into());
    }
    if !new.elevation.is_finite() {
        return Err("PolygonMesh.elevation must be finite".into());
    }
    solid_normal("PolygonMesh.normal", &new.normal)?;
    for (index, vertex) in new.vertices.iter().enumerate() {
        finite_vector(&format!("PolygonMesh.vertices[{index}].location"), &vertex.location)?;
    }
    Ok(())
}

/// PolyfaceMesh: faces index the vertex list (1-based; a negative index hides
/// that edge; 0 marks an unused corner).
#[cfg(feature = "host")]
fn validate_polyface_mesh(
    old: Option<&crate::host::codec::entities::PolyfaceMesh>,
    new: &crate::host::codec::entities::PolyfaceMesh,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if !new.elevation.is_finite() || !new.thickness.is_finite() || !new.start_width.is_finite() || !new.end_width.is_finite() {
        return Err("PolyfaceMesh elevation, thickness and widths must be finite".into());
    }
    solid_normal("PolyfaceMesh.normal", &new.normal)?;
    if new.vertices.len() < 3 {
        return Err("PolyfaceMesh.vertices requires at least 3 vertices".into());
    }
    for (index, vertex) in new.vertices.iter().enumerate() {
        finite_vector(&format!("PolyfaceMesh.vertices[{index}].location"), &vertex.location)?;
        if !vertex.bulge.is_finite() || !vertex.start_width.is_finite() || !vertex.end_width.is_finite() || !vertex.curve_tangent.is_finite() {
            return Err(format!("PolyfaceMesh.vertices[{index}] has non-finite bulge, width or tangent"));
        }
    }
    if new.faces.is_empty() {
        return Err("PolyfaceMesh.faces requires at least 1 face".into());
    }
    let count = new.vertices.len() as i64;
    for (index, face) in new.faces.iter().enumerate() {
        let indices = [face.index1, face.index2, face.index3, face.index4];
        for value in indices {
            if i64::from(value).abs() > count {
                return Err(format!(
                    "PolyfaceMesh.faces[{index}] references vertex {} but only {count} exist",
                    value.abs()
                ));
            }
        }
        if indices.iter().filter(|value| **value != 0).count() < 3 {
            return Err(format!("PolyfaceMesh.faces[{index}] needs at least 3 vertex indices"));
        }
    }
    Ok(())
}

/// Mesh (subdivision): faces and edges index the vertex list (0-based).
#[cfg(feature = "host")]
fn validate_mesh(
    old: Option<&crate::host::codec::entities::Mesh>,
    new: &crate::host::codec::entities::Mesh,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if new.subdivision_level < 0 {
        return Err("Mesh.subdivision_level must be non-negative".into());
    }
    if new.vertices.len() < 3 {
        return Err("Mesh.vertices requires at least 3 vertices".into());
    }
    for (index, vertex) in new.vertices.iter().enumerate() {
        finite_vector(&format!("Mesh.vertices[{index}]"), vertex)?;
    }
    for (index, face) in new.faces.iter().enumerate() {
        if face.vertices.len() < 3 {
            return Err(format!("Mesh.faces[{index}] needs at least 3 vertices"));
        }
        if let Some(bad) = face.vertices.iter().find(|vertex| **vertex >= new.vertices.len()) {
            return Err(format!(
                "Mesh.faces[{index}] references vertex {bad} but only {} exist",
                new.vertices.len()
            ));
        }
    }
    for (index, edge) in new.edges.iter().enumerate() {
        if edge.start >= new.vertices.len() || edge.end >= new.vertices.len() {
            return Err(format!("Mesh.edges[{index}] references a vertex out of range"));
        }
        if edge.start == edge.end {
            return Err(format!("Mesh.edges[{index}] has identical endpoints"));
        }
        if edge.crease.is_some_and(|crease| !crease.is_finite() || crease < 0.0) {
            return Err(format!("Mesh.edges[{index}].crease must be finite and non-negative"));
        }
    }
    Ok(())
}

/// Helix generating parameters. The spline is derived by the host, so only
/// the parameters are validated.
#[cfg(feature = "host")]
fn validate_helix(
    old: Option<&crate::host::codec::entities::Helix>,
    new: &crate::host::codec::entities::Helix,
) -> Result<(), String> {
    if old.is_some_and(|old| {
        old.axis_base_point == new.axis_base_point
            && old.start_point == new.start_point
            && old.axis_vector == new.axis_vector
            && old.radius.to_bits() == new.radius.to_bits()
            && old.turns.to_bits() == new.turns.to_bits()
            && old.turn_height.to_bits() == new.turn_height.to_bits()
            && old.handedness == new.handedness
            && old.constraint == new.constraint
    }) {
        return Ok(());
    }
    finite_vector("Helix.axis_base_point", &new.axis_base_point)?;
    finite_vector("Helix.start_point", &new.start_point)?;
    solid_normal("Helix.axis_vector", &new.axis_vector)?;
    for (name, value) in [("radius", new.radius), ("turns", new.turns), ("turn_height", new.turn_height)] {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!("Helix.{name} must be finite and greater than zero"));
        }
    }
    let axis = new.axis_vector;
    let length = axis.x.hypot(axis.y).hypot(axis.z);
    let delta = new.start_point - new.axis_base_point;
    let along = (delta.x * axis.x + delta.y * axis.y + delta.z * axis.z) / length;
    let radial = ((delta.x * delta.x + delta.y * delta.y + delta.z * delta.z) - along * along).max(0.0).sqrt();
    if radial <= 1.0e-9 {
        return Err("Helix.start_point must not lie on the axis".into());
    }
    Ok(())
}

/// RasterImage display parameters. The image definition is created and owned
/// by the host; a script supplies the file and the placement.
#[cfg(feature = "host")]
fn validate_raster_image(
    old: Option<&crate::host::codec::entities::RasterImage>,
    new: &crate::host::codec::entities::RasterImage,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if let Some(old) = old {
        if old.file_path != new.file_path {
            return Err("RasterImage.file_path is fixed at creation; the image definition owns it".into());
        }
    } else if new.file_path.trim().is_empty() {
        return Err("RasterImage.file_path is empty".into());
    } else if new.definition_handle.is_none_or(|handle| handle.is_null())
        && !std::path::Path::new(&new.file_path).is_file()
    {
        return Err(format!("RasterImage.file_path {:?} is not a readable file", new.file_path));
    }
    finite_vector("RasterImage.insertion_point", &new.insertion_point)?;
    finite_vector("RasterImage.u_vector", &new.u_vector)?;
    finite_vector("RasterImage.v_vector", &new.v_vector)?;
    let cross = new.u_vector.cross(&new.v_vector);
    if cross.x.hypot(cross.y).hypot(cross.z) <= 1.0e-12 {
        return Err("RasterImage.u_vector and v_vector must be nonzero and not parallel".into());
    }
    if !new.size.x.is_finite() || !new.size.y.is_finite() || new.size.x < 0.0 || new.size.y < 0.0 {
        return Err("RasterImage.size must be finite and non-negative".into());
    }
    if new.brightness > 100 || new.contrast > 100 || new.fade > 100 {
        return Err("RasterImage brightness, contrast and fade must be within 0..=100".into());
    }
    if new.flags.bits() & !0x0f != 0 {
        return Err("RasterImage.flags has unknown bits".into());
    }
    for (index, vertex) in new.clip_boundary.vertices.iter().enumerate() {
        if !vertex.x.is_finite() || !vertex.y.is_finite() {
            return Err(format!("RasterImage.clip_boundary.vertices[{index}] must be finite"));
        }
    }
    if new.clipping_enabled && new.clip_boundary.vertices.len() < 2 {
        return Err("RasterImage clipping requires at least 2 clip boundary vertices".into());
    }
    Ok(())
}

/// Wipeout: a masking region placed like an image; the clip boundary (in
/// normalized image coordinates) defines the masked area.
#[cfg(feature = "host")]
fn validate_wipeout(
    old: Option<&crate::host::codec::entities::Wipeout>,
    new: &crate::host::codec::entities::Wipeout,
) -> Result<(), String> {
    use crate::host::codec::entities::WipeoutClipType;
    if old == Some(new) {
        return Ok(());
    }
    finite_vector("Wipeout.insertion_point", &new.insertion_point)?;
    finite_vector("Wipeout.u_vector", &new.u_vector)?;
    finite_vector("Wipeout.v_vector", &new.v_vector)?;
    let cross = new.u_vector.cross(&new.v_vector);
    if cross.x.hypot(cross.y).hypot(cross.z) <= 1.0e-12 {
        return Err("Wipeout.u_vector and v_vector must be nonzero and not parallel".into());
    }
    if !new.size.x.is_finite() || !new.size.y.is_finite() || new.size.x <= 0.0 || new.size.y <= 0.0 {
        return Err("Wipeout.size must be finite and greater than zero".into());
    }
    if new.brightness > 100 || new.contrast > 100 || new.fade > 100 {
        return Err("Wipeout brightness, contrast and fade must be within 0..=100".into());
    }
    if new.flags.bits() & !0x0f != 0 {
        return Err("Wipeout.flags has unknown bits".into());
    }
    for (index, vertex) in new.clip_boundary_vertices.iter().enumerate() {
        if !vertex.x.is_finite() || !vertex.y.is_finite() {
            return Err(format!("Wipeout.clip_boundary_vertices[{index}] must be finite"));
        }
    }
    let count = new.clip_boundary_vertices.len();
    match new.clip_type {
        WipeoutClipType::Rectangular if count != 2 => {
            return Err("a rectangular Wipeout needs exactly 2 clip boundary vertices".into());
        }
        WipeoutClipType::Polygonal if count < 3 => {
            return Err("a polygonal Wipeout needs at least 3 clip boundary vertices".into());
        }
        _ => {}
    }
    Ok(())
}

/// Underlay placement and display. The file and page live on the referenced
/// `UnderlayDefinition`, which a script names by handle at creation.
#[cfg(feature = "host")]
fn validate_underlay(
    old: Option<&crate::host::codec::entities::Underlay>,
    new: &crate::host::codec::entities::Underlay,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if let Some(old) = old {
        if old.definition_handle != new.definition_handle {
            return Err("Underlay.definition_handle is fixed at creation".into());
        }
        if old.underlay_type != new.underlay_type {
            return Err("Underlay.underlay_type is fixed at creation".into());
        }
    }
    finite_vector("Underlay.insertion_point", &new.insertion_point)?;
    solid_normal("Underlay.normal", &new.normal)?;
    for (name, value) in [("x_scale", new.x_scale), ("y_scale", new.y_scale), ("z_scale", new.z_scale)] {
        if !value.is_finite() || value == 0.0 {
            return Err(format!("Underlay.{name} must be finite and nonzero"));
        }
    }
    if !new.rotation.is_finite() {
        return Err("Underlay.rotation must be finite".into());
    }
    if new.contrast > 100 || new.fade > 100 {
        return Err("Underlay contrast and fade must be within 0..=100".into());
    }
    if new.flags.bits() & !0x0f != 0 {
        return Err("Underlay.flags has unknown bits".into());
    }
    for (index, vertex) in new.clip_boundary_vertices.iter().enumerate() {
        if !vertex.x.is_finite() || !vertex.y.is_finite() {
            return Err(format!("Underlay.clip_boundary_vertices[{index}] must be finite"));
        }
    }
    if !new.clip_boundary_vertices.is_empty() && new.clip_boundary_vertices.len() < 2 {
        return Err("Underlay clip boundary needs at least 2 vertices".into());
    }
    Ok(())
}

/// Viewport view state. Owner and layer-handle references are checked with the
/// other references; this validates the numbers.
#[cfg(feature = "host")]
fn validate_viewport(
    old: Option<&crate::host::codec::entities::Viewport>,
    new: &crate::host::codec::entities::Viewport,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    finite_vector("Viewport.center", &new.center)?;
    finite_vector("Viewport.view_center", &new.view_center)?;
    finite_vector("Viewport.view_target", &new.view_target)?;
    solid_normal("Viewport.view_direction", &new.view_direction)?;
    for (name, value) in [("width", new.width), ("height", new.height), ("view_height", new.view_height), ("lens_length", new.lens_length), ("custom_scale", new.custom_scale)] {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!("Viewport.{name} must be finite and greater than zero"));
        }
    }
    for (name, value) in [("front_clip_z", new.front_clip_z), ("back_clip_z", new.back_clip_z), ("twist_angle", new.twist_angle)] {
        if !value.is_finite() {
            return Err(format!("Viewport.{name} must be finite"));
        }
    }
    if new.circle_sides < 3 {
        return Err("Viewport.circle_sides must be at least 3".into());
    }
    Ok(())
}

/// ViewBorder: the frame of a drawing view, tied to a viewport and a scale.
#[cfg(feature = "host")]
fn validate_view_border(
    old: Option<&crate::host::codec::entities::ViewBorder>,
    new: &crate::host::codec::entities::ViewBorder,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    for (name, pair) in [("min", new.min), ("max", new.max), ("center", new.center)] {
        if !pair[0].is_finite() || !pair[1].is_finite() {
            return Err(format!("ViewBorder.{name} must be finite"));
        }
    }
    if new.min[0] >= new.max[0] || new.min[1] >= new.max[1] {
        return Err("ViewBorder.min must be below max on both axes".into());
    }
    if !new.scale.is_finite() || new.scale <= 0.0 {
        return Err("ViewBorder.scale must be finite and greater than zero".into());
    }
    if !new.rotation_angle.is_finite() {
        return Err("ViewBorder.rotation_angle must be finite".into());
    }
    Ok(())
}

/// Light: type, placement, intensity, attenuation and shadow settings.
#[cfg(feature = "host")]
fn validate_light(
    old: Option<&crate::host::codec::entities::Light>,
    new: &crate::host::codec::entities::Light,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    if new.name.trim().is_empty() {
        return Err("Light.name is empty".into());
    }
    if !(1..=3).contains(&new.light_type) {
        return Err("Light.light_type must be 1 (distant), 2 (point) or 3 (spot)".into());
    }
    finite_vector("Light.position", &new.position)?;
    finite_vector("Light.target", &new.target)?;
    if new.light_type != 2 && new.position == new.target {
        return Err("a distant or spot Light needs a target different from its position".into());
    }
    if !new.intensity.is_finite() || new.intensity < 0.0 {
        return Err("Light.intensity must be finite and non-negative".into());
    }
    for (name, value) in [
        ("attenuation_start_limit", new.attenuation_start_limit),
        ("attenuation_end_limit", new.attenuation_end_limit),
        ("hotspot_angle", new.hotspot_angle),
        ("falloff_angle", new.falloff_angle),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(format!("Light.{name} must be finite and non-negative"));
        }
    }
    if new.use_attenuation_limits && new.attenuation_start_limit > new.attenuation_end_limit {
        return Err("Light attenuation start limit must not exceed the end limit".into());
    }
    if new.light_type == 3 && new.hotspot_angle > new.falloff_angle {
        return Err("Light.hotspot_angle must not exceed falloff_angle".into());
    }
    if new.shadow_map_size < 0 {
        return Err("Light.shadow_map_size must be non-negative".into());
    }
    Ok(())
}

/// Every geometry problem of a first-generation mapped kind. Messages name
/// the field, so an edit can be compared against the entity it replaces.
#[cfg(feature = "host")]
fn basic_entity_errors(new: &crate::host::EntityType) -> Vec<String> {
    use crate::host::EntityType;
    let mut errs: Vec<String> = Vec::new();
    macro_rules! chk {
        ($e:expr) => {
            if let Err(error) = $e {
                errs.push(error);
            }
        };
    }
    let finite = |name: &str, value: f64| {
        if value.is_finite() {
            Ok(())
        } else {
            Err(format!("{name} must be finite"))
        }
    };
    let positive = |name: &str, value: f64| {
        if value.is_finite() && value > 0.0 {
            Ok(())
        } else {
            Err(format!("{name} must be finite and greater than zero"))
        }
    };
    match new {
        EntityType::Point(v) => {
            chk!(finite_vector("Point.location", &v.location));
            chk!(finite("Point.thickness", v.thickness));
            chk!(finite("Point.x_axis_angle", v.x_axis_angle));
            chk!(solid_normal("Point.normal", &v.normal));
        }
        EntityType::Line(v) => {
            chk!(finite_vector("Line.start", &v.start));
            chk!(finite_vector("Line.end", &v.end));
            chk!(finite("Line.thickness", v.thickness));
            chk!(solid_normal("Line.normal", &v.normal));
        }
        EntityType::Circle(v) => {
            chk!(finite_vector("Circle.center", &v.center));
            chk!(positive("Circle.radius", v.radius));
            chk!(finite("Circle.thickness", v.thickness));
            chk!(solid_normal("Circle.normal", &v.normal));
        }
        EntityType::Arc(v) => {
            chk!(finite_vector("Arc.center", &v.center));
            chk!(positive("Arc.radius", v.radius));
            chk!(finite("Arc.start_angle", v.start_angle));
            chk!(finite("Arc.end_angle", v.end_angle));
            chk!(finite("Arc.thickness", v.thickness));
            chk!(solid_normal("Arc.normal", &v.normal));
        }
        EntityType::Ellipse(v) => {
            chk!(finite_vector("Ellipse.center", &v.center));
            chk!(solid_normal("Ellipse.major_axis", &v.major_axis));
            if !v.minor_axis_ratio.is_finite() || v.minor_axis_ratio <= 0.0 || v.minor_axis_ratio > 1.0 {
                errs.push("Ellipse.minor_axis_ratio must be within (0, 1]".into());
            }
            chk!(finite("Ellipse.start_parameter", v.start_parameter));
            chk!(finite("Ellipse.end_parameter", v.end_parameter));
            chk!(solid_normal("Ellipse.normal", &v.normal));
        }
        EntityType::Text(v) => {
            chk!(finite_vector("Text.insertion_point", &v.insertion_point));
            if let Some(alignment) = &v.alignment_point {
                chk!(finite_vector("Text.alignment_point", alignment));
            }
            chk!(positive("Text.height", v.height));
            chk!(finite("Text.rotation", v.rotation));
            chk!(finite("Text.oblique_angle", v.oblique_angle));
            if !v.width_factor.is_finite() || v.width_factor <= 0.0 {
                errs.push("Text.width_factor must be finite and greater than zero".into());
            }
            chk!(finite("Text.thickness", v.thickness));
            chk!(solid_normal("Text.normal", &v.normal));
        }
        EntityType::MText(v) => {
            chk!(finite_vector("MText.insertion_point", &v.insertion_point));
            chk!(positive("MText.height", v.height));
            chk!(finite("MText.rotation", v.rotation));
            if !v.rectangle_width.is_finite() || v.rectangle_width < 0.0 {
                errs.push("MText.rectangle_width must be finite and non-negative".into());
            }
            if let Some(height) = v.rectangle_height {
                chk!(finite("MText.rectangle_height", height));
            }
            chk!(positive("MText.line_spacing_factor", v.line_spacing_factor));
            chk!(solid_normal("MText.normal", &v.normal));
        }
        EntityType::LwPolyline(v) => {
            if v.vertices.len() < 2 {
                errs.push("LwPolyline.vertices requires at least 2 vertices".into());
            }
            for (i, vertex) in v.vertices.iter().enumerate() {
                if !vertex.location.x.is_finite() || !vertex.location.y.is_finite() {
                    errs.push(format!("LwPolyline.vertices[{i}].location must be finite"));
                }
                if !vertex.bulge.is_finite() || !vertex.start_width.is_finite() || !vertex.end_width.is_finite() {
                    errs.push(format!("LwPolyline.vertices[{i}] has a non-finite bulge or width"));
                }
            }
            if !v.constant_width.is_finite() || v.constant_width < 0.0 {
                errs.push("LwPolyline.constant_width must be finite and non-negative".into());
            }
            chk!(finite("LwPolyline.elevation", v.elevation));
            chk!(finite("LwPolyline.thickness", v.thickness));
            chk!(solid_normal("LwPolyline.normal", &v.normal));
        }
        EntityType::Polyline2D(v) => {
            if v.vertices.len() < 2 {
                errs.push("Polyline2D.vertices requires at least 2 vertices".into());
            }
            for (i, vertex) in v.vertices.iter().enumerate() {
                chk!(finite_vector(&format!("Polyline2D.vertices[{i}].location"), &vertex.location));
                if !vertex.bulge.is_finite() || !vertex.start_width.is_finite() || !vertex.end_width.is_finite() {
                    errs.push(format!("Polyline2D.vertices[{i}] has a non-finite bulge or width"));
                }
            }
            for (name, value) in [("start_width", v.start_width), ("end_width", v.end_width), ("thickness", v.thickness), ("elevation", v.elevation)] {
                chk!(finite(&format!("Polyline2D.{name}"), value));
            }
            chk!(solid_normal("Polyline2D.normal", &v.normal));
        }
        EntityType::Polyline(v) => {
            if v.vertices.len() < 2 {
                errs.push("Polyline.vertices requires at least 2 vertices".into());
            }
            for (i, vertex) in v.vertices.iter().enumerate() {
                chk!(finite_vector(&format!("Polyline.vertices[{i}].location"), &vertex.location));
            }
        }
        EntityType::Polyline3D(v) => {
            if v.vertices.len() < 2 {
                errs.push("Polyline3D.vertices requires at least 2 vertices".into());
            }
            for (i, vertex) in v.vertices.iter().enumerate() {
                chk!(finite_vector(&format!("Polyline3D.vertices[{i}].position"), &vertex.position));
            }
            for (name, value) in [("default_start_width", v.default_start_width), ("default_end_width", v.default_end_width), ("elevation", v.elevation)] {
                chk!(finite(&format!("Polyline3D.{name}"), value));
            }
            chk!(solid_normal("Polyline3D.normal", &v.normal));
        }
        EntityType::Spline(v) => {
            if v.degree < 1 {
                errs.push("Spline.degree must be at least 1".into());
            }
            let degree = v.degree as usize;
            if v.control_points.len() <= degree {
                errs.push("Spline needs more control points than its degree".into());
            }
            for (i, point) in v.control_points.iter().enumerate() {
                chk!(finite_vector(&format!("Spline.control_points[{i}]"), point));
            }
            for (i, point) in v.fit_points.iter().enumerate() {
                chk!(finite_vector(&format!("Spline.fit_points[{i}]"), point));
            }
            if v.knots.len() != v.control_points.len() + degree + 1 {
                errs.push(format!(
                    "Spline.knots must hold {} values for {} control points of degree {degree}",
                    v.control_points.len() + degree + 1,
                    v.control_points.len()
                ));
            }
            if v.knots.iter().any(|k| !k.is_finite()) || v.knots.windows(2).any(|w| w[1] < w[0]) {
                errs.push("Spline.knots must be finite and nondecreasing".into());
            }
            if !v.weights.is_empty() {
                if v.weights.len() != v.control_points.len() {
                    errs.push("Spline.weights must match the control points".into());
                }
                if v.weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
                    errs.push("Spline.weights must be finite and greater than zero".into());
                }
            }
            chk!(solid_normal("Spline.normal", &v.normal));
        }
        _ => {}
    }
    errs
}

/// Creation must be fully valid. An edit is refused only for problems it
/// introduces: a legacy value that was already invalid and is untouched
/// keeps the entity editable.
#[cfg(feature = "host")]
fn validate_basic_entity(
    old: Option<&crate::host::EntityType>,
    new: &crate::host::EntityType,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    let errors = basic_entity_errors(new);
    let existing = old.map(basic_entity_errors).unwrap_or_default();
    match errors.into_iter().find(|error| !existing.contains(error)) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Ole2Frame: only the frame placement is scriptable; the embedded object
/// storage is never rewritten.
#[cfg(feature = "host")]
fn validate_ole_frame(
    old: Option<&crate::host::codec::entities::Ole2Frame>,
    new: &crate::host::codec::entities::Ole2Frame,
) -> Result<(), String> {
    if old == Some(new) {
        return Ok(());
    }
    finite_vector("Ole2Frame.upper_left_corner", &new.upper_left_corner)?;
    finite_vector("Ole2Frame.lower_right_corner", &new.lower_right_corner)?;
    let (a, b) = (new.upper_left_corner, new.lower_right_corner);
    if (a.x - b.x).abs() <= 1.0e-12 || (a.y - b.y).abs() <= 1.0e-12 {
        return Err("Ole2Frame corners must span a nonzero width and height".into());
    }
    if new.lock_aspect > 1 {
        return Err("Ole2Frame.lock_aspect must be 0 or 1".into());
    }
    Ok(())
}

/// Surface: only the display isoline density is scriptable; the geometry lives
/// in the ACIS payload that the geometry kernel owns.
#[cfg(feature = "host")]
fn validate_surface(
    old: Option<&crate::host::codec::entities::Surface>,
    new: &crate::host::codec::entities::Surface,
) -> Result<(), String> {
    if old.is_some_and(|old| old.u_isolines == new.u_isolines && old.v_isolines == new.v_isolines) {
        return Ok(());
    }
    for (name, value) in [("u_isolines", new.u_isolines), ("v_isolines", new.v_isolines)] {
        if !(0..=200).contains(&value) {
            return Err(format!("Surface.{name} must be between 0 and 200"));
        }
    }
    Ok(())
}

/// SectionSymbol: the cut mark drawn on a Model-Documentation view. `points`
/// is the canonical geometry; the counts and the end/tick/label projections are
/// derived by the host, so only the inputs are validated.
#[cfg(feature = "host")]
fn validate_section_symbol(
    old: Option<&crate::host::codec::entities::SectionSymbol>,
    new: &crate::host::codec::entities::SectionSymbol,
) -> Result<(), String> {
    if old.is_some_and(|old| old.points == new.points && old.symbol_scale.to_bits() == new.symbol_scale.to_bits()) {
        return Ok(());
    }
    if !new.symbol_scale.is_finite() || new.symbol_scale <= 0.0 {
        return Err("SectionSymbol.symbol_scale must be finite and greater than zero".into());
    }
    if new.points.len() < 2 {
        return Err("SectionSymbol.points requires at least 2 points".into());
    }
    for (index, point) in new.points.iter().enumerate() {
        finite_vector(&format!("SectionSymbol.points[{index}].point"), &point.point)?;
        finite_vector(&format!("SectionSymbol.points[{index}].label_offset"), &point.label_offset)?;
        if !point.bulge.is_finite() {
            return Err(format!("SectionSymbol.points[{index}].bulge must be finite"));
        }
    }
    Ok(())
}

/// Validate newly created geometry for the kinds whose mapped fields have
/// host checks. Called by the Python add path before an entity enters the document.
#[cfg(feature = "host")]
pub fn validate_new_canvas_entity(entity: &crate::host::EntityType) -> Result<(), String> {
    use crate::host::EntityType;
    if entity.common().layer.trim().is_empty() {
        return Err("layer name is empty".into());
    }
    validate_basic_entity(None, entity)?;
    match entity {
        EntityType::Ray(value) => {
            finite_vector("Ray.base_point", &value.base_point)?;
            unit_direction("Ray.direction", &value.direction)?;
        }
        EntityType::XLine(value) => {
            finite_vector("XLine.base_point", &value.base_point)?;
            unit_direction("XLine.direction", &value.direction)?;
        }
        EntityType::Solid(value) => {
            for (name, corner) in [
                ("first_corner", &value.first_corner),
                ("second_corner", &value.second_corner),
                ("third_corner", &value.third_corner),
                ("fourth_corner", &value.fourth_corner),
            ] {
                finite_vector(&format!("Solid.{name}"), corner)?;
            }
            solid_normal("Solid.normal", &value.normal)?;
            if !value.thickness.is_finite() {
                return Err("Solid.thickness must be finite".into());
            }
        }
        EntityType::Face3D(value) => {
            for (name, corner) in [
                ("first_corner", &value.first_corner),
                ("second_corner", &value.second_corner),
                ("third_corner", &value.third_corner),
                ("fourth_corner", &value.fourth_corner),
            ] {
                finite_vector(&format!("Face3D.{name}"), corner)?;
            }
            if value.invisible_edges.bits() & !0x0f != 0 {
                return Err("Face3D.invisible_edges has unknown bits".into());
            }
        }
        EntityType::Tolerance(value) => {
            finite_vector("Tolerance.insertion_point", &value.insertion_point)?;
            unit_direction("Tolerance.direction", &value.direction)?;
            solid_normal("Tolerance.normal", &value.normal)?;
            if value.text.trim().is_empty() {
                return Err("Tolerance.text is empty".into());
            }
            if value.dimension_style_name.trim().is_empty() {
                return Err("Tolerance.dimension_style_name is empty".into());
            }
            if !value.text_height.is_finite() || value.text_height <= 0.0 {
                return Err("Tolerance.text_height must be finite and greater than zero".into());
            }
            if !value.dimension_gap.is_finite() {
                return Err("Tolerance.dimension_gap must be finite".into());
            }
        }
        EntityType::Shape(value) => {
            finite_vector("Shape.insertion_point", &value.insertion_point)?;
            solid_normal("Shape.normal", &value.normal)?;
            if !value.size.is_finite() || value.size <= 0.0 {
                return Err("Shape.size must be finite and greater than zero".into());
            }
            if value.shape_name.trim().is_empty() {
                return Err("Shape.shape_name is empty".into());
            }
            if !(1..=i16::MAX as i32).contains(&value.shape_number) {
                return Err("Shape.shape_number must be between 1 and 32767".into());
            }
            if !value.rotation.is_finite() {
                return Err("Shape.rotation must be finite".into());
            }
            if !value.relative_x_scale.is_finite() || value.relative_x_scale.abs() < 1e-12 {
                return Err("Shape.relative_x_scale must be finite and nonzero".into());
            }
            if !value.oblique_angle.is_finite()
                || value.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return Err("Shape.oblique_angle must be finite and between -PI/2 and PI/2".into());
            }
            if !value.thickness.is_finite() {
                return Err("Shape.thickness must be finite".into());
            }
            if value.style_name.trim().is_empty() {
                return Err("Shape.style_name is empty".into());
            }
        }
        EntityType::Leader(value) => validate_leader(None, value)?,
        EntityType::MLine(value) => validate_mline(None, value)?,
        EntityType::Dimension(value) => validate_dimension(None, value)?,
        EntityType::MultiLeader(value) => validate_multileader(None, value)?,
        EntityType::Table(value) => validate_table(None, value)?,
        EntityType::PolygonMesh(value) => validate_polygon_mesh(None, value)?,
        EntityType::PolyfaceMesh(value) => validate_polyface_mesh(None, value)?,
        EntityType::Mesh(value) => validate_mesh(None, value)?,
        EntityType::Helix(value) => validate_helix(None, value)?,
        EntityType::RasterImage(value) => validate_raster_image(None, value)?,
        EntityType::Wipeout(value) => validate_wipeout(None, value)?,
        EntityType::Underlay(value) => validate_underlay(None, value)?,
        EntityType::Viewport(value) => validate_viewport(None, value)?,
        EntityType::ViewBorder(value) => validate_view_border(None, value)?,
        EntityType::Light(value) => validate_light(None, value)?,
        EntityType::Insert(value) => {
            if value.block_name.trim().is_empty() {
                return Err("Insert.block_name is empty".into());
            }
            finite_vector("Insert.insert_point", &value.insert_point)?;
            solid_normal("Insert.normal", &value.normal)?;
            for (name, scale) in [("x_scale", value.x_scale()), ("y_scale", value.y_scale()), ("z_scale", value.z_scale())] {
                insert_scale(&format!("Insert.{name}"), scale)?;
            }
            for (name, number) in [("rotation", value.rotation), ("column_spacing", value.column_spacing), ("row_spacing", value.row_spacing)] {
                if !number.is_finite() {
                    return Err(format!("Insert.{name} must be finite"));
                }
            }
            if value.column_count == 0 || value.row_count == 0 {
                return Err("Insert array counts must be greater than zero".into());
            }
        }
        EntityType::Ole2Frame(value) => validate_ole_frame(None, value)?,
        EntityType::Surface(value) => validate_surface(None, value)?,
        EntityType::SectionSymbol(value) => validate_section_symbol(None, value)?,
        EntityType::AttributeDefinition(value) => {
            finite_vector(
                "AttributeDefinition.insertion_point",
                &value.insertion_point,
            )?;
            finite_vector(
                "AttributeDefinition.alignment_point",
                &value.alignment_point,
            )?;
            solid_normal("AttributeDefinition.normal", &value.normal)?;
            if value.tag.trim().is_empty() || value.tag.chars().any(char::is_whitespace) {
                return Err(
                    "AttributeDefinition.tag must be nonempty and contain no whitespace".into(),
                );
            }
            if !value.height.is_finite() || value.height <= 0.0 {
                return Err(
                    "AttributeDefinition.height must be finite and greater than zero".into(),
                );
            }
            if !value.rotation.is_finite() {
                return Err("AttributeDefinition.rotation must be finite".into());
            }
            if !value.width_factor.is_finite() || value.width_factor.abs() < 1e-12 {
                return Err("AttributeDefinition.width_factor must be finite and nonzero".into());
            }
            if !value.oblique_angle.is_finite()
                || value.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return Err(
                    "AttributeDefinition.oblique_angle must be finite and between -PI/2 and PI/2"
                        .into(),
                );
            }
            if value.text_style.trim().is_empty() {
                return Err("AttributeDefinition.text_style is empty".into());
            }
            if value.field_length < 0 {
                return Err("AttributeDefinition.field_length must be nonnegative".into());
            }
            if value.line_count < 1 {
                return Err("AttributeDefinition.line_count must be greater than zero".into());
            }
        }
        EntityType::AttributeEntity(value) => {
            finite_vector("AttributeEntity.insertion_point", &value.insertion_point)?;
            finite_vector("AttributeEntity.alignment_point", &value.alignment_point)?;
            solid_normal("AttributeEntity.normal", &value.normal)?;
            if value.tag.trim().is_empty() || value.tag.chars().any(char::is_whitespace) {
                return Err(
                    "AttributeEntity.tag must be nonempty and contain no whitespace".into(),
                );
            }
            if !value.height.is_finite() || value.height <= 0.0 {
                return Err("AttributeEntity.height must be finite and greater than zero".into());
            }
            if !value.rotation.is_finite() {
                return Err("AttributeEntity.rotation must be finite".into());
            }
            if !value.width_factor.is_finite() || value.width_factor.abs() < 1e-12 {
                return Err("AttributeEntity.width_factor must be finite and nonzero".into());
            }
            if !value.oblique_angle.is_finite()
                || value.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return Err(
                    "AttributeEntity.oblique_angle must be finite and between -PI/2 and PI/2"
                        .into(),
                );
            }
            if value.text_style.trim().is_empty() {
                return Err("AttributeEntity.text_style is empty".into());
            }
            if value.field_length < 0 {
                return Err("AttributeEntity.field_length must be nonnegative".into());
            }
            if value.line_count < 1 {
                return Err("AttributeEntity.line_count must be greater than zero".into());
            }
        }
        EntityType::Hatch(value) => {
            solid_normal("Hatch.normal", &value.normal)?;
            if !value.elevation.is_finite() {
                return Err("Hatch.elevation must be finite".into());
            }
            validate_hatch_pattern(value)?;
            if !value.pattern_angle.is_finite() {
                return Err("Hatch.pattern_angle must be finite".into());
            }
            if !value.pixel_size.is_finite() || value.pixel_size < 0.0 {
                return Err("Hatch.pixel_size must be finite and non-negative".into());
            }
            for (si, sp) in value.seed_points.iter().enumerate() {
                if !sp.x.is_finite() || !sp.y.is_finite() {
                    return Err(format!("Hatch.seed_points[{si}] has non-finite coordinates"));
                }
            }
            validate_hatch_paths(&value.paths)?;
        }
        _ => {}
    }
    Ok(())
}

/// Validate geometry changed by a typed replacement before an undoable
/// transaction commits. Legacy invalid values in untouched fields remain
/// untouched, so editing a layer does not unexpectedly reject an old drawing.
#[cfg(feature = "host")]
pub fn validate_entity_mutation(
    before: &crate::host::EntityType,
    after: &crate::host::EntityType,
) -> Result<(), String> {
    use crate::host::EntityType;
    if before.common().layer != after.common().layer {
        if after.common().layer.trim().is_empty() {
            return Err("layer name is empty".into());
        }
        if matches!(
            after,
            EntityType::Block(_)
                | EntityType::BlockEnd(_)
                | EntityType::Seqend(_)
                | EntityType::Extended(_)
                | EntityType::Unknown(_)
        ) {
            return Err("entity kind does not support canvas layer edits".into());
        }
    }
    let finite3 = |name: &str, x: f64, y: f64, z: f64| {
        if x.is_finite() && y.is_finite() && z.is_finite() {
            Ok(())
        } else {
            Err(format!("{name} must contain finite coordinates"))
        }
    };
    let same3 = |a: &crate::host::codec::types::Vector3,
                 b: &crate::host::codec::types::Vector3| {
        a.x.to_bits() == b.x.to_bits()
            && a.y.to_bits() == b.y.to_bits()
            && a.z.to_bits() == b.z.to_bits()
    };
    let radius = |name: &str, value: f64| {
        if value.is_finite() && value > 0.0 {
            Ok(())
        } else {
            Err(format!("{name} must be finite and greater than zero"))
        }
    };
    let changed3 = |name: &str,
                    old: &crate::host::codec::types::Vector3,
                    new: &crate::host::codec::types::Vector3| {
        if !same3(old, new) {
            finite_vector(name, new)
        } else {
            Ok(())
        }
    };
    validate_basic_entity(Some(before), after)?;
    match (before, after) {
        (EntityType::Point(old), EntityType::Point(new)) => {
            if !same3(&old.location, &new.location) {
                finite3(
                    "Point.location",
                    new.location.x,
                    new.location.y,
                    new.location.z,
                )?;
            }
        }
        (EntityType::Line(old), EntityType::Line(new)) => {
            if !same3(&old.start, &new.start) {
                finite3("Line.start", new.start.x, new.start.y, new.start.z)?;
            }
            if !same3(&old.end, &new.end) {
                finite3("Line.end", new.end.x, new.end.y, new.end.z)?;
            }
        }
        (EntityType::Circle(old), EntityType::Circle(new)) => {
            if !same3(&old.center, &new.center) {
                finite3("Circle.center", new.center.x, new.center.y, new.center.z)?;
            }
            if old.radius.to_bits() != new.radius.to_bits() {
                radius("Circle.radius", new.radius)?;
            }
        }
        (EntityType::Arc(old), EntityType::Arc(new)) => {
            if !same3(&old.center, &new.center) {
                finite3("Arc.center", new.center.x, new.center.y, new.center.z)?;
            }
            if old.radius.to_bits() != new.radius.to_bits() {
                radius("Arc.radius", new.radius)?;
            }
        }
        (EntityType::Ray(old), EntityType::Ray(new)) => {
            changed3("Ray.base_point", &old.base_point, &new.base_point)?;
            if !same3(&old.direction, &new.direction) {
                unit_direction("Ray.direction", &new.direction)?;
            }
        }
        (EntityType::XLine(old), EntityType::XLine(new)) => {
            changed3("XLine.base_point", &old.base_point, &new.base_point)?;
            if !same3(&old.direction, &new.direction) {
                unit_direction("XLine.direction", &new.direction)?;
            }
        }
        (EntityType::Solid(old), EntityType::Solid(new)) => {
            for (name, old_corner, new_corner) in [
                ("first_corner", &old.first_corner, &new.first_corner),
                ("second_corner", &old.second_corner, &new.second_corner),
                ("third_corner", &old.third_corner, &new.third_corner),
                ("fourth_corner", &old.fourth_corner, &new.fourth_corner),
            ] {
                changed3(&format!("Solid.{name}"), old_corner, new_corner)?;
            }
            if !same3(&old.normal, &new.normal) {
                solid_normal("Solid.normal", &new.normal)?;
            }
            if old.thickness.to_bits() != new.thickness.to_bits() && !new.thickness.is_finite() {
                return Err("Solid.thickness must be finite".into());
            }
        }
        (EntityType::Face3D(old), EntityType::Face3D(new)) => {
            for (name, old_corner, new_corner) in [
                ("first_corner", &old.first_corner, &new.first_corner),
                ("second_corner", &old.second_corner, &new.second_corner),
                ("third_corner", &old.third_corner, &new.third_corner),
                ("fourth_corner", &old.fourth_corner, &new.fourth_corner),
            ] {
                changed3(&format!("Face3D.{name}"), old_corner, new_corner)?;
            }
            if old.invisible_edges != new.invisible_edges && new.invisible_edges.bits() & !0x0f != 0
            {
                return Err("Face3D.invisible_edges has unknown bits".into());
            }
        }
        (EntityType::Insert(old), EntityType::Insert(new)) => {
            if old.block_name != new.block_name
                || old.attributes != new.attributes
                || old.view_rep_handle != new.view_rep_handle
                || old.seqend_handle != new.seqend_handle
            {
                return Err("Insert block identity and attached records cannot change in a geometry transaction".into());
            }
            changed3("Insert.insert_point", &old.insert_point, &new.insert_point)?;
            if !same3(&old.normal, &new.normal) {
                solid_normal("Insert.normal", &new.normal)?;
            }
            for (name, before, after) in [
                ("x_scale", old.x_scale(), new.x_scale()),
                ("y_scale", old.y_scale(), new.y_scale()),
                ("z_scale", old.z_scale(), new.z_scale()),
            ] {
                if before.to_bits() != after.to_bits() {
                    insert_scale(&format!("Insert.{name}"), after)?;
                }
            }
            for (name, before, after) in [
                ("rotation", old.rotation, new.rotation),
                ("column_spacing", old.column_spacing, new.column_spacing),
                ("row_spacing", old.row_spacing, new.row_spacing),
            ] {
                if before.to_bits() != after.to_bits() && !after.is_finite() {
                    return Err(format!("Insert.{name} must be finite"));
                }
            }
            if (old.column_count != new.column_count && new.column_count == 0)
                || (old.row_count != new.row_count && new.row_count == 0)
            {
                return Err("Insert array counts must be greater than zero".into());
            }
        }
        (EntityType::Tolerance(old), EntityType::Tolerance(new)) => {
            changed3(
                "Tolerance.insertion_point",
                &old.insertion_point,
                &new.insertion_point,
            )?;
            if !same3(&old.direction, &new.direction) {
                unit_direction("Tolerance.direction", &new.direction)?;
            }
            if !same3(&old.normal, &new.normal) {
                solid_normal("Tolerance.normal", &new.normal)?;
            }
            if old.text != new.text && new.text.trim().is_empty() {
                return Err("Tolerance.text is empty".into());
            }
            if old.dimension_style_name != new.dimension_style_name
                && new.dimension_style_name.trim().is_empty()
            {
                return Err("Tolerance.dimension_style_name is empty".into());
            }
            if old.text_height.to_bits() != new.text_height.to_bits()
                && (!new.text_height.is_finite() || new.text_height <= 0.0)
            {
                return Err("Tolerance.text_height must be finite and greater than zero".into());
            }
            if old.dimension_gap.to_bits() != new.dimension_gap.to_bits()
                && !new.dimension_gap.is_finite()
            {
                return Err("Tolerance.dimension_gap must be finite".into());
            }
            if old.dimension_style_handle != new.dimension_style_handle
                && old.dimension_style_name == new.dimension_style_name
            {
                return Err("Tolerance.dimension_style_handle is read-only".into());
            }
        }
        (EntityType::Shape(old), EntityType::Shape(new)) => {
            changed3(
                "Shape.insertion_point",
                &old.insertion_point,
                &new.insertion_point,
            )?;
            if !same3(&old.normal, &new.normal) {
                solid_normal("Shape.normal", &new.normal)?;
            }
            if old.size.to_bits() != new.size.to_bits()
                && (!new.size.is_finite() || new.size <= 0.0)
            {
                return Err("Shape.size must be finite and greater than zero".into());
            }
            if old.shape_name != new.shape_name && new.shape_name.trim().is_empty() {
                return Err("Shape.shape_name is empty".into());
            }
            if old.shape_number != new.shape_number
                && !(1..=i16::MAX as i32).contains(&new.shape_number)
            {
                return Err("Shape.shape_number must be between 1 and 32767".into());
            }
            if old.rotation.to_bits() != new.rotation.to_bits() && !new.rotation.is_finite() {
                return Err("Shape.rotation must be finite".into());
            }
            if old.relative_x_scale.to_bits() != new.relative_x_scale.to_bits()
                && (!new.relative_x_scale.is_finite() || new.relative_x_scale.abs() < 1e-12)
            {
                return Err("Shape.relative_x_scale must be finite and nonzero".into());
            }
            if old.oblique_angle.to_bits() != new.oblique_angle.to_bits()
                && (!new.oblique_angle.is_finite()
                    || new.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2)
            {
                return Err("Shape.oblique_angle must be finite and between -PI/2 and PI/2".into());
            }
            if old.thickness.to_bits() != new.thickness.to_bits() && !new.thickness.is_finite() {
                return Err("Shape.thickness must be finite".into());
            }
            if old.style_name != new.style_name && new.style_name.trim().is_empty() {
                return Err("Shape.style_name is empty".into());
            }
            if old.style_handle != new.style_handle
                && !(old.style_name != new.style_name && new.style_handle.is_some())
            {
                return Err("Shape.style_handle is read-only".into());
            }
        }
        (EntityType::Dimension(old), EntityType::Dimension(new)) => validate_dimension(Some(old), new)?,
        (EntityType::MultiLeader(old), EntityType::MultiLeader(new)) => validate_multileader(Some(old), new)?,
        (EntityType::Table(old), EntityType::Table(new)) => validate_table(Some(old), new)?,
        (EntityType::PolygonMesh(old), EntityType::PolygonMesh(new)) => validate_polygon_mesh(Some(old), new)?,
        (EntityType::PolyfaceMesh(old), EntityType::PolyfaceMesh(new)) => validate_polyface_mesh(Some(old), new)?,
        (EntityType::Mesh(old), EntityType::Mesh(new)) => validate_mesh(Some(old), new)?,
        (EntityType::Helix(old), EntityType::Helix(new)) => validate_helix(Some(old), new)?,
        (EntityType::RasterImage(old), EntityType::RasterImage(new)) => validate_raster_image(Some(old), new)?,
        (EntityType::Wipeout(old), EntityType::Wipeout(new)) => validate_wipeout(Some(old), new)?,
        (EntityType::Underlay(old), EntityType::Underlay(new)) => validate_underlay(Some(old), new)?,
        (EntityType::Viewport(old), EntityType::Viewport(new)) => validate_viewport(Some(old), new)?,
        (EntityType::ViewBorder(old), EntityType::ViewBorder(new)) => validate_view_border(Some(old), new)?,
        (EntityType::Light(old), EntityType::Light(new)) => validate_light(Some(old), new)?,
        (EntityType::Ole2Frame(old), EntityType::Ole2Frame(new)) => {
            validate_ole_frame(Some(old), new)?;
            if old.storage != new.storage || old.envelope != new.envelope {
                return Err("Ole2Frame embedded storage cannot change".into());
            }
        }
        (EntityType::Surface(old), EntityType::Surface(new)) => validate_surface(Some(old), new)?,
        (EntityType::SectionSymbol(old), EntityType::SectionSymbol(new)) => validate_section_symbol(Some(old), new)?,
        (EntityType::MLine(old), EntityType::MLine(new)) => validate_mline(Some(old), new)?,
        (EntityType::Leader(old), EntityType::Leader(new)) => validate_leader(Some(old), new)?,
        (EntityType::AttributeDefinition(old), EntityType::AttributeDefinition(new)) => {
            changed3(
                "AttributeDefinition.insertion_point",
                &old.insertion_point,
                &new.insertion_point,
            )?;
            changed3(
                "AttributeDefinition.alignment_point",
                &old.alignment_point,
                &new.alignment_point,
            )?;
            if !same3(&old.normal, &new.normal) {
                solid_normal("AttributeDefinition.normal", &new.normal)?;
            }
            if old.tag != new.tag
                && (new.tag.trim().is_empty() || new.tag.chars().any(char::is_whitespace))
            {
                return Err(
                    "AttributeDefinition.tag must be nonempty and contain no whitespace".into(),
                );
            }
            if old.height.to_bits() != new.height.to_bits()
                && (!new.height.is_finite() || new.height <= 0.0)
            {
                return Err(
                    "AttributeDefinition.height must be finite and greater than zero".into(),
                );
            }
            for (name, before, after) in [
                ("rotation", old.rotation, new.rotation),
                ("oblique_angle", old.oblique_angle, new.oblique_angle),
            ] {
                if before.to_bits() != after.to_bits() && !after.is_finite() {
                    return Err(format!("AttributeDefinition.{name} must be finite"));
                }
            }
            if old.width_factor.to_bits() != new.width_factor.to_bits()
                && (!new.width_factor.is_finite() || new.width_factor.abs() < 1e-12)
            {
                return Err("AttributeDefinition.width_factor must be finite and nonzero".into());
            }
            if old.oblique_angle.to_bits() != new.oblique_angle.to_bits()
                && new.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return Err(
                    "AttributeDefinition.oblique_angle must be between -PI/2 and PI/2".into(),
                );
            }
            if old.text_style != new.text_style && new.text_style.trim().is_empty() {
                return Err("AttributeDefinition.text_style is empty".into());
            }
            if old.field_length != new.field_length && new.field_length < 0 {
                return Err("AttributeDefinition.field_length must be nonnegative".into());
            }
            if old.line_count != new.line_count && new.line_count < 1 {
                return Err("AttributeDefinition.line_count must be greater than zero".into());
            }
            if old.embedded_mtext != new.embedded_mtext {
                return Err("AttributeDefinition.embedded_mtext is unmapped and read-only".into());
            }
        }
        (EntityType::AttributeEntity(old), EntityType::AttributeEntity(new)) => {
            changed3(
                "AttributeEntity.insertion_point",
                &old.insertion_point,
                &new.insertion_point,
            )?;
            changed3(
                "AttributeEntity.alignment_point",
                &old.alignment_point,
                &new.alignment_point,
            )?;
            if !same3(&old.normal, &new.normal) {
                solid_normal("AttributeEntity.normal", &new.normal)?;
            }
            if old.tag != new.tag
                && (new.tag.trim().is_empty() || new.tag.chars().any(char::is_whitespace))
            {
                return Err(
                    "AttributeEntity.tag must be nonempty and contain no whitespace".into(),
                );
            }
            if old.height.to_bits() != new.height.to_bits()
                && (!new.height.is_finite() || new.height <= 0.0)
            {
                return Err("AttributeEntity.height must be finite and greater than zero".into());
            }
            for (name, before, after) in [
                ("rotation", old.rotation, new.rotation),
                ("oblique_angle", old.oblique_angle, new.oblique_angle),
            ] {
                if before.to_bits() != after.to_bits() && !after.is_finite() {
                    return Err(format!("AttributeEntity.{name} must be finite"));
                }
            }
            if old.width_factor.to_bits() != new.width_factor.to_bits()
                && (!new.width_factor.is_finite() || new.width_factor.abs() < 1e-12)
            {
                return Err("AttributeEntity.width_factor must be finite and nonzero".into());
            }
            if old.oblique_angle.to_bits() != new.oblique_angle.to_bits()
                && new.oblique_angle.abs() >= std::f64::consts::FRAC_PI_2
            {
                return Err("AttributeEntity.oblique_angle must be between -PI/2 and PI/2".into());
            }
            if old.text_style != new.text_style && new.text_style.trim().is_empty() {
                return Err("AttributeEntity.text_style is empty".into());
            }
            if old.field_length != new.field_length && new.field_length < 0 {
                return Err("AttributeEntity.field_length must be nonnegative".into());
            }
            if old.line_count != new.line_count && new.line_count < 1 {
                return Err("AttributeEntity.line_count must be greater than zero".into());
            }
            if old.embedded_mtext != new.embedded_mtext {
                return Err("AttributeEntity.embedded_mtext is unmapped and read-only".into());
            }
            if old.attdef_handle != new.attdef_handle
                && !(old.tag != new.tag && !new.attdef_handle.is_null())
            {
                return Err("AttributeEntity.attdef_handle is read-only".into());
            }
        }
        (EntityType::Hatch(old), EntityType::Hatch(new)) => {
            if !same3(&old.normal, &new.normal) {
                solid_normal("Hatch.normal", &new.normal)?;
            }
            if old.elevation.to_bits() != new.elevation.to_bits() && !new.elevation.is_finite() {
                return Err("Hatch.elevation must be finite".into());
            }
            if old.pattern != new.pattern
                || old.is_solid != new.is_solid
                || old.pattern_scale.to_bits() != new.pattern_scale.to_bits()
            {
                validate_hatch_pattern(new)?;
            }
            if old.pattern_angle.to_bits() != new.pattern_angle.to_bits()
                && !new.pattern_angle.is_finite()
            {
                return Err("Hatch.pattern_angle must be finite".into());
            }
            if old.pixel_size.to_bits() != new.pixel_size.to_bits()
                && (!new.pixel_size.is_finite() || new.pixel_size < 0.0)
            {
                return Err("Hatch.pixel_size must be finite and non-negative".into());
            }
            if old.paths != new.paths {
                validate_hatch_paths(&new.paths)?;
            }
            if old.seed_points != new.seed_points {
                for (si, sp) in new.seed_points.iter().enumerate() {
                    if !sp.x.is_finite() || !sp.y.is_finite() {
                        return Err(format!("Hatch.seed_points[{si}] has non-finite coordinates"));
                    }
                }
            }
            if old.gradient_color != new.gradient_color {
                return Err("Hatch.gradient_color is unmapped and read-only".into());
            }
            if old.is_mpolygon != new.is_mpolygon
                || old.mpolygon_hatch_color != new.mpolygon_hatch_color
                || old.mpolygon_x_direction != new.mpolygon_x_direction
                || old.mpolygon_boundary_handle_count != new.mpolygon_boundary_handle_count
            {
                return Err("Hatch MPOLYGON fields are unmapped and read-only".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validate references which require the surrounding document. Keeping this
/// in the host API gives every scripting adapter the same reference rules.
#[cfg(feature = "host")]
pub fn validate_canvas_entity_references(
    document: &crate::host::CadDocument,
    entity: &crate::host::EntityType,
) -> Result<(), String> {
    use crate::host::EntityType;
    let owner = entity.common().owner_handle;
    if !owner.is_null() {
        let block_owner = document
            .block_records
            .iter()
            .any(|record| record.handle == owner);
        let insert_owner = matches!(entity, EntityType::AttributeEntity(_))
            && matches!(document.get_entity(owner), Some(EntityType::Insert(_)));
        if !block_owner && !insert_owner {
            return Err(format!("entity owner {owner:?} does not exist"));
        }
    }
    if let EntityType::Hatch(value) = entity {
        let handles = value.paths.iter().flat_map(|path| &path.boundary_handles);
        let mut count = 0usize;
        for handle in handles {
            count += 1;
            if handle.is_null() || document.get_entity(*handle).is_none() {
                return Err(format!("Hatch boundary handle {handle:?} does not exist"));
            }
        }
        if value.is_associative && count == 0 {
            return Err("associative Hatch requires boundary handles".into());
        }
        if !value.is_associative && count != 0 {
            return Err("non-associative Hatch cannot carry boundary handles".into());
        }
    }
    if let EntityType::Tolerance(value) = entity {
        let found = value
            .dimension_style_handle
            .filter(|handle| !handle.is_null())
            .map_or_else(
                || {
                    document.dim_styles.iter().any(|style| {
                        style
                            .name
                            .eq_ignore_ascii_case(value.dimension_style_name.trim())
                    })
                },
                |handle| {
                    document
                        .dim_styles
                        .iter()
                        .any(|style| style.handle == handle)
                },
        );
        if !found {
            return Err(format!(
                "Tolerance dimension style {:?} does not exist",
                value.dimension_style_name
            ));
        }
    }
    if let EntityType::Shape(value) = entity {
        let style = value
            .style_handle
            .filter(|handle| !handle.is_null())
            .and_then(|handle| {
                document
                    .text_styles
                    .iter()
                    .find(|style| style.handle == handle)
            })
            .or_else(|| {
                document
                    .text_styles
                    .iter()
                    .find(|style| style.name.eq_ignore_ascii_case(value.style_name.trim()))
            })
            .ok_or_else(|| format!("Shape text style {:?} does not exist", value.style_name))?;
        if !style.is_shape_file {
            return Err(format!(
                "Shape text style {:?} is not a shape-file style",
                style.name
            ));
        }
        if style.font_file.trim().is_empty() {
            return Err(format!("Shape text style {:?} has no SHX file", style.name));
        }
    }
    // An edit that keeps the block name is left alone so a legacy insert of a
    // missing block stays editable; creation and renames must name a real,
    // ordinary block that does not create a cycle.
    let insert_is_unchanged_edit = |value: &crate::host::codec::entities::Insert| {
        matches!(document.get_entity(value.common.handle),
            Some(EntityType::Insert(existing)) if existing.block_name == value.block_name)
    };
    if let Some(value) = match entity {
        EntityType::Insert(value) if !insert_is_unchanged_edit(value) => Some(value),
        _ => None,
    } {
        let name = value.block_name.trim();
        let record = document.block_records.get(name).ok_or_else(|| {
            format!("Insert block {:?} does not exist", value.block_name)
        })?;
        if record.is_model_space() || record.is_paper_space() {
            return Err("Insert cannot reference a model-space or paper-space block".into());
        }
        // A block cannot (transitively) contain an insert of itself.
        let owner_name = document
            .block_records
            .iter()
            .find(|candidate| candidate.handle == value.common.owner_handle)
            .map(|candidate| candidate.name.clone());
        if let Some(owner_name) = owner_name {
            let mut pending = vec![record.name.clone()];
            let mut seen = std::collections::HashSet::new();
            while let Some(current) = pending.pop() {
                if current.eq_ignore_ascii_case(&owner_name) {
                    return Err(format!(
                        "inserting block {:?} into {owner_name:?} would make the block contain itself",
                        value.block_name
                    ));
                }
                if !seen.insert(current.to_ascii_lowercase()) {
                    continue;
                }
                if let Some(block) = document.block_records.get(&current) {
                    for handle in &block.entity_handles {
                        if let Some(EntityType::Insert(nested)) = document.get_entity(*handle) {
                            pending.push(nested.block_name.clone());
                        }
                    }
                }
            }
        }
    }
    if let EntityType::SectionSymbol(value) = entity {
        use crate::host::codec::objects::{ClassObjectData, ObjectType};
        for (name, handle, wanted) in [
            ("style", value.style_handle, "SectionViewStyle"),
            ("view representation", value.view_rep_handle, "ViewRep"),
        ] {
            if handle.is_null() {
                continue;
            }
            let matches = match document.objects.get(&handle) {
                Some(ObjectType::ClassObject(object)) => match (&object.data, wanted) {
                    (ClassObjectData::SectionViewStyle(_), "SectionViewStyle") => true,
                    (ClassObjectData::ViewRep(_), "ViewRep") => true,
                    _ => false,
                },
                _ => false,
            };
            if !matches {
                return Err(format!("SectionSymbol {name} {handle:?} is not an existing {wanted} object"));
            }
        }
    }
    if let EntityType::ViewBorder(value) = entity {
        if !matches!(document.get_entity(value.active_viewport), Some(EntityType::Viewport(_))) {
            return Err(format!("ViewBorder viewport {:?} does not exist", value.active_viewport));
        }
        if !matches!(
            document.objects.get(&value.scale_handle),
            Some(crate::host::codec::objects::ObjectType::Scale(_))
        ) {
            return Err(format!("ViewBorder scale {:?} does not exist", value.scale_handle));
        }
    }
    if let EntityType::Viewport(value) = entity {
        let owner = value.common.owner_handle;
        let in_paper_space = !owner.is_null()
            && document
                .block_records
                .iter()
                .any(|record| record.handle == owner && record.is_paper_space());
        if !in_paper_space {
            return Err("Viewport must be owned by a paper-space layout block".into());
        }
        for handle in &value.frozen_layers {
            if !document.layers.iter().any(|layer| layer.handle == *handle) {
                return Err(format!("Viewport frozen layer {handle:?} does not exist"));
            }
        }
    }
    if let EntityType::Underlay(value) = entity {
        match document.objects.get(&value.definition_handle) {
            Some(crate::host::codec::objects::ObjectType::UnderlayDefinition(definition)) => {
                if definition.underlay_type != value.underlay_type {
                    return Err(format!(
                        "Underlay type {:?} does not match its definition ({:?})",
                        value.underlay_type, definition.underlay_type
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "Underlay definition {:?} does not exist",
                    value.definition_handle
                ));
            }
        }
    }
    if let EntityType::RasterImage(value) = entity {
        if let Some(handle) = value.definition_handle.filter(|handle| !handle.is_null()) {
            if !matches!(
                document.objects.get(&handle),
                Some(crate::host::codec::objects::ObjectType::ImageDefinition(_))
            ) {
                return Err(format!("RasterImage image definition {handle:?} does not exist"));
            }
        }
    }
    if let EntityType::Table(value) = entity {
        if let Some(handle) = value.table_style_handle.filter(|handle| !handle.is_null()) {
            if !matches!(
                document.objects.get(&handle),
                Some(crate::host::codec::objects::ObjectType::TableStyle(_))
            ) {
                return Err(format!("Table style handle {handle:?} does not exist"));
            }
        }
        for (ri, row) in value.rows.iter().enumerate() {
            for (ci, cell) in row.cells.iter().enumerate() {
                for content in &cell.contents {
                    if let Some(handle) = content.text_style_handle.filter(|handle| !handle.is_null()) {
                        if !document.text_styles.iter().any(|style| style.handle == handle) {
                            return Err(format!("Table.rows[{ri}].cells[{ci}] text style handle {handle:?} does not exist"));
                        }
                    }
                    if let Some(handle) = content.block_handle.filter(|handle| !handle.is_null()) {
                        if !document.block_records.iter().any(|record| record.handle == handle) {
                            return Err(format!("Table.rows[{ri}].cells[{ci}] block handle {handle:?} does not exist"));
                        }
                    }
                }
            }
        }
    }
    if let EntityType::MultiLeader(value) = entity {
        use crate::host::codec::objects::ObjectType;
        let handle_exists = |handle: Option<crate::host::codec::Handle>, what: &str, ok: &dyn Fn(crate::host::codec::Handle) -> bool| {
            match handle.filter(|handle| !handle.is_null()) {
                Some(handle) if !ok(handle) => Err(format!("MultiLeader {what} handle {handle:?} does not exist")),
                _ => Ok(()),
            }
        };
        let is_block = |h| document.block_records.iter().any(|record| record.handle == h);
        let is_text_style = |h| document.text_styles.iter().any(|style| style.handle == h);
        let is_linetype = |h| document.line_types.iter().any(|ltype| ltype.handle == h);
        let is_mleader_style = |h| matches!(document.objects.get(&h), Some(ObjectType::MultiLeaderStyle(_)));
        handle_exists(value.style_handle, "style", &is_mleader_style)?;
        handle_exists(value.text_style_handle, "text style", &is_text_style)?;
        handle_exists(value.context.text_style_handle, "context text style", &is_text_style)?;
        handle_exists(value.line_type_handle, "line type", &is_linetype)?;
        handle_exists(value.arrowhead_handle, "arrowhead block", &is_block)?;
        handle_exists(value.block_content_handle, "block content", &is_block)?;
        handle_exists(value.context.block_content_handle, "context block content", &is_block)?;
    }
    if let EntityType::Dimension(value) = entity {
        let name = value.base().style_name.trim();
        if !document.dim_styles.iter().any(|style| style.name.eq_ignore_ascii_case(name)) {
            return Err(format!("Dimension style {:?} does not exist", value.base().style_name));
        }
    }
    if let EntityType::MLine(value) = entity {
        let style = resolve_mline_style(document, value).ok_or_else(|| {
            format!("MLine style {:?} does not exist", value.style_name)
        })?;
        if style.elements.is_empty() {
            return Err(format!("MLine style {:?} has no elements", style.name));
        }
        for (index, vertex) in value.vertices.iter().enumerate() {
            if !vertex.segments.is_empty() && vertex.segments.len() != style.elements.len() {
                return Err(format!(
                    "MLine.vertices[{index}] has {} segments but style {:?} has {} elements",
                    vertex.segments.len(),
                    style.name,
                    style.elements.len()
                ));
            }
        }
    }
    if let EntityType::Leader(value) = entity {
        use crate::host::codec::entities::LeaderCreationType;
        if !document
            .dim_styles
            .iter()
            .any(|style| style.name.eq_ignore_ascii_case(value.dimension_style.trim()))
        {
            return Err(format!(
                "Leader dimension style {:?} does not exist",
                value.dimension_style
            ));
        }
        if !value.annotation_handle.is_null() {
            if value.creation_type == LeaderCreationType::NoAnnotation {
                return Err("Leader with no annotation cannot carry an annotation handle".into());
            }
            let annotation = document
                .get_entity(value.annotation_handle)
                .filter(|_| value.annotation_handle != value.common.handle)
                .ok_or_else(|| {
                    format!(
                        "Leader annotation handle {:?} does not exist",
                        value.annotation_handle
                    )
                })?;
            let expected = match value.creation_type {
                LeaderCreationType::WithText => matches!(
                    annotation,
                    EntityType::Text(_) | EntityType::MText(_)
                ),
                LeaderCreationType::WithTolerance => matches!(annotation, EntityType::Tolerance(_)),
                LeaderCreationType::WithBlock => matches!(annotation, EntityType::Insert(_)),
                LeaderCreationType::NoAnnotation => unreachable!(),
            };
            if !expected {
                return Err(format!(
                    "Leader annotation {:?} does not match creation_type {:?}",
                    value.annotation_handle, value.creation_type
                ));
            }
        }
    }
    if let EntityType::AttributeDefinition(value) = entity {
        let owner = entity.common().owner_handle;
        let block = document
            .block_records
            .iter()
            .find(|record| record.handle == owner)
            .ok_or_else(|| {
                "AttributeDefinition requires an existing block-record owner".to_owned()
            })?;
        if block.is_model_space() || block.is_paper_space() {
            return Err("AttributeDefinition owner must be a block definition".into());
        }
        let style = document
            .text_styles
            .get(value.text_style.trim())
            .ok_or_else(|| {
                format!(
                    "AttributeDefinition text style {:?} does not exist",
                    value.text_style
                )
            })?;
        if style.is_shape_file {
            return Err(format!(
                "AttributeDefinition text style {:?} is a shape-file style",
                style.name
            ));
        }
    }
    if let EntityType::AttributeEntity(value) = entity {
        let owner = entity.common().owner_handle;
        let insert = match document.get_entity(owner) {
            Some(EntityType::Insert(insert)) => insert,
            _ => return Err("AttributeEntity requires an existing Insert owner".into()),
        };
        let block = document
            .block_records
            .get(insert.block_name.trim())
            .ok_or_else(|| {
                format!(
                    "AttributeEntity owner references missing block {:?}",
                    insert.block_name
                )
            })?;
        let definition = block
            .entity_handles
            .iter()
            .find_map(|handle| match document.get_entity(*handle) {
                Some(EntityType::AttributeDefinition(definition))
                    if definition.common.handle == value.attdef_handle =>
                {
                    Some(definition)
                }
                _ => None,
            })
            .ok_or_else(|| {
                "AttributeEntity attdef_handle does not belong to the owner's block".to_owned()
            })?;
        if !definition.tag.eq_ignore_ascii_case(value.tag.trim()) {
            return Err("AttributeEntity tag does not match its attribute definition".into());
        }
        let style = document
            .text_styles
            .get(value.text_style.trim())
            .ok_or_else(|| {
                format!(
                    "AttributeEntity text style {:?} does not exist",
                    value.text_style
                )
            })?;
        if style.is_shape_file {
            return Err(format!(
                "AttributeEntity text style {:?} is a shape-file style",
                style.name
            ));
        }
    }
    Ok(())
}

/// Stable handle wins; the name is the fallback for legacy entities.
#[cfg(feature = "host")]
fn resolve_mline_style<'a>(
    document: &'a crate::host::CadDocument,
    mline: &crate::host::codec::entities::MLine,
) -> Option<&'a crate::host::codec::objects::MLineStyle> {
    use crate::host::codec::objects::ObjectType;
    mline
        .style_handle
        .filter(|handle| !handle.is_null())
        .and_then(|handle| match document.objects.get(&handle) {
            Some(ObjectType::MLineStyle(style)) => Some(style),
            _ => None,
        })
        .or_else(|| {
            document.objects.values().find_map(|object| match object {
                ObjectType::MLineStyle(style)
                    if style.name.eq_ignore_ascii_case(mline.style_name.trim()) =>
                {
                    Some(style)
                }
                _ => None,
            })
        })
}

/// Fill name-based table references with stable handles before a new entity or
/// replacement is committed. Existing non-null handles remain authoritative
/// for legacy DWG entities whose fallback name can be stale.
#[cfg(feature = "host")]
pub fn bind_canvas_entity_references(
    document: &crate::host::CadDocument,
    entity: &mut crate::host::EntityType,
) -> Result<(), String> {
    use crate::host::EntityType;
    match entity {
        EntityType::Tolerance(value)
            if value
                .dimension_style_handle
                .filter(|h| !h.is_null())
                .is_none() =>
        {
            value.dimension_style_handle = document
                .dim_styles
                .iter()
                .find(|style| {
                    style
                        .name
                        .eq_ignore_ascii_case(value.dimension_style_name.trim())
                })
                .map(|style| style.handle);
        }
        EntityType::MLine(value) if value.style_handle.filter(|h| !h.is_null()).is_none() => {
            value.style_handle = resolve_mline_style(document, value).map(|style| style.handle);
        }
        EntityType::Shape(value) if value.style_handle.filter(|h| !h.is_null()).is_none() => {
            value.style_handle = document
                .text_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case(value.style_name.trim()))
                .map(|style| style.handle);
        }
        EntityType::AttributeEntity(value) => {
            let insert = match document.get_entity(value.common.owner_handle) {
                Some(EntityType::Insert(insert)) => insert,
                _ => return Err("AttributeEntity requires an existing Insert owner".into()),
            };
            let block = document
                .block_records
                .get(insert.block_name.trim())
                .ok_or_else(|| {
                    format!(
                        "AttributeEntity owner references missing block {:?}",
                        insert.block_name
                    )
                })?;
            let current_matches = block.entity_handles.iter().any(|handle| {
                *handle == value.attdef_handle
                    && matches!(document.get_entity(*handle),
                    Some(EntityType::AttributeDefinition(definition))
                        if definition.tag.eq_ignore_ascii_case(value.tag.trim()))
            });
            if !current_matches {
                value.attdef_handle = block
                    .entity_handles
                    .iter()
                    .find_map(|handle| match document.get_entity(*handle) {
                        Some(EntityType::AttributeDefinition(definition))
                            if definition.tag.eq_ignore_ascii_case(value.tag.trim()) =>
                        {
                            Some(*handle)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        format!(
                            "AttributeEntity tag {:?} has no definition in block {:?}",
                            value.tag, insert.block_name
                        )
                    })?;
            }
        }
        _ => {}
    }
    validate_canvas_entity_references(document, entity)
}

/// Clone a canvas entity with a new layer, preserving its other snapshot
/// fields. Internal records and opaque fallbacks are excluded.
#[cfg(feature = "host")]
pub fn patch_canvas_layer(
    entity: &crate::host::EntityType,
    layer: &str,
) -> Result<crate::host::EntityType, String> {
    use crate::host::EntityType;
    if layer.trim().is_empty() {
        return Err("layer name is empty".into());
    }
    if matches!(
        entity,
        EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::Seqend(_)
            | EntityType::Extended(_)
            | EntityType::Unknown(_)
    ) {
        return Err("entity kind does not support canvas layer edits".into());
    }
    let mut changed = entity.clone();
    changed.common_mut().layer = layer.to_owned();
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{get_embedded_type_registry_json, TypeId, TypeRegistry};

    #[test]
    fn every_entity_variant_has_exactly_one_classification() {
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        let registry: TypeRegistry =
            serde_json::from_str(get_embedded_type_registry_json()).unwrap();
        let variants = &registry.types[&TypeId::new("EntityType")].variants;
        assert_eq!(catalog.entity_kinds.len(), variants.len());
        let mut names = std::collections::HashSet::new();
        for entry in &catalog.entity_kinds {
            assert!(names.insert(&entry.kind), "duplicate kind {}", entry.kind);
            assert!(variants.iter().any(|v| v.name == entry.kind));
            assert!(entry
                .properties
                .iter()
                .any(|p| p.name == "handle" && p.model_access == ModelAccess::ReadOnly));
            assert!(entry
                .properties
                .iter()
                .any(|p| p.name == "kind" && p.model_access == ModelAccess::ReadOnly));
            assert!(entry
                .properties
                .iter()
                .any(|p| p.name == "owner_handle" && p.model_access == ModelAccess::ReadOnly));
            let mut source_paths = std::collections::HashSet::new();
            for property in &entry.properties {
                assert!(
                    source_paths.insert(&property.source_path),
                    "duplicate source path in {}: {}",
                    entry.kind,
                    property.source_path
                );
                assert!(
                    property.snapshot_readable,
                    "unreadable traced property in {}: {}",
                    entry.kind, property.name
                );
                if property.model_access == ModelAccess::ReadWrite {
                    assert!(matches!(
                        property.validation.as_str(),
                        "type_conversion_only" | "transaction_geometry" | "transaction_nonempty"
                    ));
                }
            }
        }
    }

    #[test]
    fn exposed_property_names_are_unique_per_kind() {
        // A field named like a base key (`kind`, `handle`, `layer`, ...) would
        // overwrite it in every Python dict; such fields must be renamed.
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        for entry in &catalog.entity_kinds {
            let mut seen = std::collections::HashSet::new();
            for property in entry.properties.iter().filter(|p| p.model_access != ModelAccess::Unmapped) {
                assert!(seen.insert(property.name.clone()), "{}: duplicate exposed name {}", entry.kind, property.name);
            }
        }
    }

    #[test]
    fn unsupported_kinds_have_no_editable_properties() {
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        for entry in &catalog.entity_kinds {
            if entry.scope != EntityScope::Canvas
                || !matches!(
                    entry.kind.as_str(),
                    "Point"
                        | "Line"
                        | "Circle"
                        | "Arc"
                        | "Ellipse"
                        | "Polyline"
                        | "Polyline2D"
                        | "Polyline3D"
                        | "LwPolyline"
                        | "Spline"
                        | "Text"
                        | "MText"
                        | "Ray"
                        | "XLine"
                        | "Solid"
                        | "Face3D"
                        | "Insert"
                        | "Tolerance"
                        | "Shape"
                        | "AttributeDefinition"
                        | "AttributeEntity"
                        | "Hatch"
                        | "Leader"
                        | "MLine"
                        | "Dimension"
                        | "MultiLeader"
                        | "Table"
                        | "PolygonMesh"
                        | "PolyfaceMesh"
                        | "Mesh"
                        | "Helix"
                        | "RasterImage"
                        | "Wipeout"
                        | "Underlay"
                        | "Viewport"
                        | "ViewBorder"
                        | "Light"
                        | "Ole2Frame"
                        | "Solid3D"
                        | "Body"
                        | "Region"
                        | "Surface"
                        | "SectionSymbol"
                )
            {
                assert!(
                    entry
                        .properties
                        .iter()
                        .all(|p| p.model_access != ModelAccess::ReadWrite
                            || (entry.scope == EntityScope::Canvas && p.name == "layer")),
                    "{}",
                    entry.kind
                );
            }
        }
    }

    #[test]
    fn catalog_names_the_fields_with_transaction_geometry_checks() {
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        let checked: std::collections::HashSet<_> = catalog
            .entity_kinds
            .iter()
            .flat_map(|entry| {
                entry
                    .properties
                    .iter()
                .filter(|property| property.validation == "transaction_geometry")
                    .map(|property| format!("{}.{}", entry.kind, property.name))
            })
            .collect();
        assert_eq!(
            checked,
            std::collections::HashSet::from([
                "Point.location".to_owned(),
                "Line.start".to_owned(),
                "Line.end".to_owned(),
                "Circle.center".to_owned(),
                "Circle.radius".to_owned(),
                "Arc.center".to_owned(),
                "Arc.radius".to_owned(),
                "Ray.base_point".to_owned(),
                "Ray.direction".to_owned(),
                "XLine.base_point".to_owned(),
                "XLine.direction".to_owned(),
                "Solid.first_corner".to_owned(),
                "Solid.second_corner".to_owned(),
                "Solid.third_corner".to_owned(),
                "Solid.fourth_corner".to_owned(),
                "Solid.normal".to_owned(),
                "Solid.thickness".to_owned(),
                "Face3D.first_corner".to_owned(),
                "Face3D.second_corner".to_owned(),
                "Face3D.third_corner".to_owned(),
                "Face3D.fourth_corner".to_owned(),
            "Face3D.invisible_edges".to_owned(),
                "Insert.insert_point".to_owned(),
                "Insert.x_scale".to_owned(),
                "Insert.y_scale".to_owned(),
                "Insert.z_scale".to_owned(),
                "Insert.rotation".to_owned(),
                "Insert.normal".to_owned(),
                "Insert.column_count".to_owned(),
                "Insert.row_count".to_owned(),
                "Insert.column_spacing".to_owned(),
                "Insert.row_spacing".to_owned(),
                "Tolerance.insertion_point".to_owned(),
                "Tolerance.direction".to_owned(),
                "Tolerance.normal".to_owned(),
                "Tolerance.text".to_owned(),
                "Tolerance.dimension_style_name".to_owned(),
                "Tolerance.text_height".to_owned(),
            "Tolerance.dimension_gap".to_owned(),
                "Shape.insertion_point".to_owned(),
                "Shape.size".to_owned(),
                "Shape.shape_name".to_owned(),
                "Shape.shape_number".to_owned(),
                "Shape.rotation".to_owned(),
                "Shape.relative_x_scale".to_owned(),
                "Shape.oblique_angle".to_owned(),
                "Shape.normal".to_owned(),
                "Shape.thickness".to_owned(),
                "Shape.style_name".to_owned(),
                "AttributeDefinition.tag".to_owned(),
                "AttributeDefinition.prompt".to_owned(),
            "AttributeDefinition.default_value".to_owned(),
            "AttributeDefinition.insertion_point".to_owned(),
            "AttributeDefinition.alignment_point".to_owned(),
                "AttributeDefinition.height".to_owned(),
                "AttributeDefinition.rotation".to_owned(),
            "AttributeDefinition.width_factor".to_owned(),
            "AttributeDefinition.oblique_angle".to_owned(),
            "AttributeDefinition.text_style".to_owned(),
            "AttributeDefinition.text_generation_flags".to_owned(),
            "AttributeDefinition.horizontal_alignment".to_owned(),
            "AttributeDefinition.vertical_alignment".to_owned(),
                "AttributeDefinition.flags".to_owned(),
                "AttributeDefinition.field_length".to_owned(),
                "AttributeDefinition.normal".to_owned(),
                "AttributeDefinition.mtext_flag".to_owned(),
                "AttributeDefinition.is_multiline".to_owned(),
                "AttributeDefinition.line_count".to_owned(),
            "AttributeDefinition.lock_position".to_owned(),
                "AttributeEntity.tag".to_owned(),
                "AttributeEntity.value".to_owned(),
                "AttributeEntity.insertion_point".to_owned(),
                "AttributeEntity.alignment_point".to_owned(),
                "AttributeEntity.height".to_owned(),
                "AttributeEntity.rotation".to_owned(),
                "AttributeEntity.width_factor".to_owned(),
                "AttributeEntity.oblique_angle".to_owned(),
                "AttributeEntity.text_style".to_owned(),
                "AttributeEntity.text_generation_flags".to_owned(),
                "AttributeEntity.horizontal_alignment".to_owned(),
                "AttributeEntity.vertical_alignment".to_owned(),
                "AttributeEntity.flags".to_owned(),
                "AttributeEntity.field_length".to_owned(),
                "AttributeEntity.normal".to_owned(),
                "AttributeEntity.mtext_flag".to_owned(),
                "AttributeEntity.is_multiline".to_owned(),
                "AttributeEntity.line_count".to_owned(),
                "AttributeEntity.lock_position".to_owned(),
                "Hatch.elevation".to_owned(),
                "Hatch.normal".to_owned(),
                "Hatch.is_solid".to_owned(),
                "Hatch.pattern".to_owned(),
                "Hatch.pattern_angle".to_owned(),
                "Hatch.pattern_scale".to_owned(),
                "Hatch.pattern_type".to_owned(),
                "Hatch.is_double".to_owned(),
                "Hatch.style".to_owned(),
                "Hatch.is_associative".to_owned(),
                "Hatch.pixel_size".to_owned(),
                "Hatch.paths".to_owned(),
                "Hatch.seed_points".to_owned(),
                "Leader.dimension_style".to_owned(),
                "Leader.arrow_enabled".to_owned(),
                "Leader.path_type".to_owned(),
                "Leader.creation_type".to_owned(),
                "Leader.hookline_direction".to_owned(),
                "Leader.hookline_enabled".to_owned(),
                "Leader.text_height".to_owned(),
                "Leader.text_width".to_owned(),
                "Leader.vertices".to_owned(),
                "Leader.annotation_handle".to_owned(),
                "Leader.override_color".to_owned(),
                "Leader.normal".to_owned(),
                "Leader.horizontal_direction".to_owned(),
                "Leader.block_offset".to_owned(),
                "Leader.annotation_offset".to_owned(),
                "MLine.flags".to_owned(),
                "MLine.justification".to_owned(),
                "MLine.normal".to_owned(),
                "MLine.scale_factor".to_owned(),
                "MLine.style_name".to_owned(),
                "MLine.vertices".to_owned(),
                "Dimension.text".to_owned(),
                "Dimension.style_name".to_owned(),
                "Dimension.normal".to_owned(),
                "Dimension.text_middle_point".to_owned(),
                "Dimension.attachment_point".to_owned(),
                "Dimension.text_rotation".to_owned(),
                "Dimension.horizontal_direction".to_owned(),
                "Dimension.flip_arrow1".to_owned(),
                "Dimension.flip_arrow2".to_owned(),
                "Dimension.text_user_positioned".to_owned(),
                "Dimension.definition_point".to_owned(),
                "Dimension.first_point".to_owned(),
                "Dimension.second_point".to_owned(),
                "Dimension.angle_vertex".to_owned(),
                "Dimension.dimension_arc".to_owned(),
                "Dimension.feature_location".to_owned(),
                "Dimension.leader_endpoint".to_owned(),
                "Dimension.first_extension_point".to_owned(),
                "Dimension.second_extension_point".to_owned(),
                "Dimension.center_point".to_owned(),
                "Dimension.first_leader_point".to_owned(),
                "Dimension.second_leader_point".to_owned(),
                "Dimension.chord_point".to_owned(),
                "Dimension.override_center".to_owned(),
                "Dimension.jog_point".to_owned(),
                "Dimension.rotation".to_owned(),
                "Dimension.ext_line_rotation".to_owned(),
                "Dimension.leader_length".to_owned(),
                "Dimension.arc_start_parameter".to_owned(),
                "Dimension.arc_end_parameter".to_owned(),
                "Dimension.jog_angle".to_owned(),
                "Dimension.is_ordinate_type_x".to_owned(),
                "Dimension.is_partial".to_owned(),
                "Dimension.has_leader".to_owned(),
                "MultiLeader.content_type".to_owned(),
                "MultiLeader.path_type".to_owned(),
                "MultiLeader.line_color".to_owned(),
                "MultiLeader.line_weight".to_owned(),
                "MultiLeader.enable_landing".to_owned(),
                "MultiLeader.enable_dogleg".to_owned(),
                "MultiLeader.dogleg_length".to_owned(),
                "MultiLeader.arrowhead_size".to_owned(),
                "MultiLeader.text_color".to_owned(),
                "MultiLeader.text_frame".to_owned(),
                "MultiLeader.text_left_attachment".to_owned(),
                "MultiLeader.text_right_attachment".to_owned(),
                "MultiLeader.text_top_attachment".to_owned(),
                "MultiLeader.text_bottom_attachment".to_owned(),
                "MultiLeader.text_attachment_direction".to_owned(),
                "MultiLeader.text_attachment_point".to_owned(),
                "MultiLeader.text_alignment".to_owned(),
                "MultiLeader.text_angle_type".to_owned(),
                "MultiLeader.text_direction_negative".to_owned(),
                "MultiLeader.scale_factor".to_owned(),
                "MultiLeader.enable_annotation_scale".to_owned(),
                "MultiLeader.extend_leader_to_text".to_owned(),
                "MultiLeader.block_content_color".to_owned(),
                "MultiLeader.block_connection_type".to_owned(),
                "MultiLeader.block_rotation".to_owned(),
                "MultiLeader.block_scale".to_owned(),
                "MultiLeader.context".to_owned(),
                "MultiLeader.style_handle".to_owned(),
                "MultiLeader.text_style_handle".to_owned(),
                "MultiLeader.arrowhead_handle".to_owned(),
                "MultiLeader.line_type_handle".to_owned(),
                "MultiLeader.block_content_handle".to_owned(),
                "Table.insertion_point".to_owned(),
                "Table.horizontal_direction".to_owned(),
                "Table.normal".to_owned(),
                "Table.table_style_handle".to_owned(),
                "Table.rows".to_owned(),
                "Table.columns".to_owned(),
                "Table.merged_ranges".to_owned(),
                "Table.break_spacing".to_owned(),
                "Table.break_flow_direction".to_owned(),
                "Table.break_options".to_owned(),
                "PolygonMesh.flags".to_owned(),
                "PolygonMesh.m_vertex_count".to_owned(),
                "PolygonMesh.n_vertex_count".to_owned(),
                "PolygonMesh.m_smooth_density".to_owned(),
                "PolygonMesh.n_smooth_density".to_owned(),
                "PolygonMesh.smooth_type".to_owned(),
                "PolygonMesh.elevation".to_owned(),
                "PolygonMesh.normal".to_owned(),
                "PolygonMesh.vertices".to_owned(),
                "PolyfaceMesh.elevation".to_owned(),
                "PolyfaceMesh.flags".to_owned(),
                "PolyfaceMesh.normal".to_owned(),
                "PolyfaceMesh.start_width".to_owned(),
                "PolyfaceMesh.end_width".to_owned(),
                "PolyfaceMesh.smooth_surface".to_owned(),
                "PolyfaceMesh.thickness".to_owned(),
                "PolyfaceMesh.vertices".to_owned(),
                "PolyfaceMesh.faces".to_owned(),
                "Mesh.blend_crease".to_owned(),
                "Mesh.subdivision_level".to_owned(),
                "Mesh.vertices".to_owned(),
                "Mesh.faces".to_owned(),
                "Mesh.edges".to_owned(),
                "Mesh.override_option".to_owned(),
                "Helix.axis_base_point".to_owned(),
                "Helix.start_point".to_owned(),
                "Helix.axis_vector".to_owned(),
                "Helix.radius".to_owned(),
                "Helix.turns".to_owned(),
                "Helix.turn_height".to_owned(),
                "Helix.handedness".to_owned(),
                "Helix.constraint".to_owned(),
                "RasterImage.insertion_point".to_owned(),
                "RasterImage.u_vector".to_owned(),
                "RasterImage.v_vector".to_owned(),
                "RasterImage.flags".to_owned(),
                "RasterImage.clipping_enabled".to_owned(),
                "RasterImage.brightness".to_owned(),
                "RasterImage.contrast".to_owned(),
                "RasterImage.fade".to_owned(),
                "RasterImage.clip_boundary".to_owned(),
                "RasterImage.file_path".to_owned(),
                "Wipeout.insertion_point".to_owned(),
                "Wipeout.u_vector".to_owned(),
                "Wipeout.v_vector".to_owned(),
                "Wipeout.size".to_owned(),
                "Wipeout.flags".to_owned(),
                "Wipeout.clipping_enabled".to_owned(),
                "Wipeout.brightness".to_owned(),
                "Wipeout.contrast".to_owned(),
                "Wipeout.fade".to_owned(),
                "Wipeout.clip_mode".to_owned(),
                "Wipeout.clip_type".to_owned(),
                "Wipeout.clip_boundary_vertices".to_owned(),
                "Ole2Frame.upper_left_corner".to_owned(),
                "Ole2Frame.lower_right_corner".to_owned(),
                "Ole2Frame.lock_aspect".to_owned(),
                "Ole2Frame.is_paper_space".to_owned(),
                "Surface.u_isolines".to_owned(),
                "Surface.v_isolines".to_owned(),
                "SectionSymbol.symbol_scale".to_owned(),
                "SectionSymbol.points".to_owned(),
                "SectionSymbol.style_handle".to_owned(),
                "SectionSymbol.view_rep_handle".to_owned(),
                "Underlay.underlay_type".to_owned(),
                "Underlay.definition_handle".to_owned(),
                "Underlay.insertion_point".to_owned(),
                "Underlay.x_scale".to_owned(),
                "Underlay.y_scale".to_owned(),
                "Underlay.z_scale".to_owned(),
                "Underlay.rotation".to_owned(),
                "Underlay.normal".to_owned(),
                "Underlay.flags".to_owned(),
                "Underlay.contrast".to_owned(),
                "Underlay.fade".to_owned(),
                "Underlay.clip_boundary_vertices".to_owned(),
                "Underlay.clip_inverted".to_owned(),
                "Viewport.center".to_owned(),
                "Viewport.width".to_owned(),
                "Viewport.height".to_owned(),
                "Viewport.status".to_owned(),
                "Viewport.view_center".to_owned(),
                "Viewport.view_direction".to_owned(),
                "Viewport.view_target".to_owned(),
                "Viewport.lens_length".to_owned(),
                "Viewport.front_clip_z".to_owned(),
                "Viewport.back_clip_z".to_owned(),
                "Viewport.view_height".to_owned(),
                "Viewport.twist_angle".to_owned(),
                "Viewport.frozen_layers".to_owned(),
                "Viewport.render_mode".to_owned(),
                "Viewport.circle_sides".to_owned(),
                "Viewport.ucs_icon_visible".to_owned(),
                "ViewBorder.min".to_owned(),
                "ViewBorder.max".to_owned(),
                "ViewBorder.center".to_owned(),
                "ViewBorder.scale".to_owned(),
                "ViewBorder.rotation_angle".to_owned(),
                "ViewBorder.active_viewport".to_owned(),
                "ViewBorder.scale_handle".to_owned(),
                "Light.name".to_owned(),
                "Light.light_type".to_owned(),
                "Light.position".to_owned(),
                "Light.target".to_owned(),
                "Light.status".to_owned(),
                "Light.light_color".to_owned(),
                "Light.plot_glyph".to_owned(),
                "Light.intensity".to_owned(),
                "Light.attenuation_type".to_owned(),
                "Light.use_attenuation_limits".to_owned(),
                "Light.attenuation_start_limit".to_owned(),
                "Light.attenuation_end_limit".to_owned(),
                "Light.hotspot_angle".to_owned(),
                "Light.falloff_angle".to_owned(),
                "Light.cast_shadows".to_owned(),
                "Light.shadow_type".to_owned(),
                "Light.shadow_map_size".to_owned(),
                "Light.shadow_map_softness".to_owned(),
                "Light.photometric_mode".to_owned(),
                "Light.photometric_data".to_owned(),
            ])
        );
    }

    #[cfg(feature = "host")]
    #[test]
    fn tolerance_geometry_and_style_references_are_validated() {
        use crate::host::codec::{self, types::Vector3};
        let document = codec::CadDocument::new();
        let mut tolerance = codec::entities::Tolerance::with_text(
            Vector3::new(1.0, 2.0, 0.0),
            "{\\Fgdt;p}%%v0.1",
        );
        let entity = codec::EntityType::Tolerance(tolerance.clone());
        validate_new_canvas_entity(&entity).unwrap();
        validate_canvas_entity_references(&document, &entity).unwrap();
        tolerance.direction = Vector3::new(2.0, 0.0, 0.0);
        assert!(
            validate_new_canvas_entity(&codec::EntityType::Tolerance(tolerance.clone()))
                .unwrap_err()
                .contains("unit vector")
        );
        tolerance.direction = Vector3::UNIT_X;
        tolerance.dimension_style_name = "Missing".into();
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::Tolerance(tolerance)
        )
        .unwrap_err()
        .contains("does not exist"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn leader_geometry_and_annotation_references_are_validated() {
        use crate::host::codec::{self, entities::LeaderCreationType, types::Vector3};
        let mut document = codec::CadDocument::new();
        let mut text = codec::entities::Text::new();
        text.common.handle = document.allocate_handle();
        let text_handle = text.common.handle;
        document.add_entity(codec::EntityType::Text(text)).unwrap();
        let mut leader = codec::entities::Leader::two_point(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        );
        leader.annotation_handle = text_handle;
        let entity = codec::EntityType::Leader(leader.clone());
        validate_new_canvas_entity(&entity).unwrap();
        validate_canvas_entity_references(&document, &entity).unwrap();

        let mut wrong_kind = leader.clone();
        wrong_kind.creation_type = LeaderCreationType::WithTolerance;
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::Leader(wrong_kind)
        )
        .unwrap_err()
        .contains("does not match"));
        let mut missing = leader.clone();
        missing.annotation_handle = codec::Handle::new(0xdead);
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::Leader(missing)
        )
        .unwrap_err()
        .contains("does not exist"));
        let mut none_with_handle = leader.clone();
        none_with_handle.creation_type = LeaderCreationType::NoAnnotation;
        assert!(validate_new_canvas_entity(&codec::EntityType::Leader(none_with_handle))
            .unwrap_err()
            .contains("no annotation"));
        let mut one_point = leader.clone();
        one_point.vertices.truncate(1);
        assert!(validate_new_canvas_entity(&codec::EntityType::Leader(one_point))
            .unwrap_err()
            .contains("at least 2"));
        let mut bad_style = leader.clone();
        bad_style.dimension_style = "Missing".into();
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::Leader(bad_style)
        )
        .unwrap_err()
        .contains("dimension style"));

        let mut edited = leader.clone();
        edited.vertices[1] = Vector3::new(f64::NAN, 0.0, 0.0);
        assert!(validate_entity_mutation(
            &codec::EntityType::Leader(leader.clone()),
            &codec::EntityType::Leader(edited)
        )
        .unwrap_err()
        .contains("finite"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn mline_geometry_and_style_references_are_validated_and_bound() {
        use crate::host::codec::{self, entities::MLineVertex, types::Vector3};
        let document = codec::CadDocument::new();
        let mut mline = codec::entities::MLine::new();
        for x in [0.0, 5.0] {
            mline.vertices.push(MLineVertex::new(Vector3::new(x, 0.0, 0.0)));
        }
        let mut entity = codec::EntityType::MLine(mline.clone());
        validate_new_canvas_entity(&entity).unwrap();
        bind_canvas_entity_references(&document, &mut entity).unwrap();
        assert!(matches!(&entity, codec::EntityType::MLine(value) if value.style_handle.is_some()));

        let mut missing = mline.clone();
        missing.style_name = "Missing".into();
        assert!(bind_canvas_entity_references(&document, &mut codec::EntityType::MLine(missing))
            .unwrap_err().contains("does not exist"));
        let mut wrong_segments = mline.clone();
        wrong_segments.vertices[0].init_segments(3);
        assert!(bind_canvas_entity_references(&document, &mut codec::EntityType::MLine(wrong_segments))
            .unwrap_err().contains("segments but style"));
        let mut one = mline.clone();
        one.vertices.truncate(1);
        assert!(validate_new_canvas_entity(&codec::EntityType::MLine(one)).unwrap_err().contains("at least 2"));
        let mut closed = mline.clone();
        closed.flags |= codec::entities::MLineFlags::CLOSED;
        assert!(validate_new_canvas_entity(&codec::EntityType::MLine(closed)).unwrap_err().contains("at least 3"));
        let mut scale = mline.clone();
        scale.scale_factor = 0.0;
        assert!(validate_new_canvas_entity(&codec::EntityType::MLine(scale)).unwrap_err().contains("nonzero"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn shape_geometry_and_shape_file_style_are_validated_and_bound() {
        use crate::host::codec::{self, tables::TableEntry, types::Vector3};
        let mut document = codec::CadDocument::new();
        let mut style = codec::tables::TextStyle::new("Symbols");
        style.set_handle(document.allocate_handle());
        style.is_shape_file = true;
        style.font_file = "symbols.shx".into();
        let style_handle = style.handle;
        document.text_styles.add(style).unwrap();
        let mut shape = codec::entities::Shape::with_style(
            Vector3::new(1.0, 2.0, 0.0),
            "ARROW",
            "Symbols",
            2.0,
            0.5,
        );
        shape.shape_number = 1;
        let mut entity = codec::EntityType::Shape(shape);
        validate_new_canvas_entity(&entity).unwrap();
        bind_canvas_entity_references(&document, &mut entity).unwrap();
        assert!(matches!(entity, codec::EntityType::Shape(ref value)
            if value.style_handle == Some(style_handle)));
        let codec::EntityType::Shape(mut bad) = entity else {
            unreachable!()
        };
        bad.size = 0.0;
        assert!(
            validate_new_canvas_entity(&codec::EntityType::Shape(bad))
                .unwrap_err()
                .contains("greater than zero")
        );
    }

    #[test]
    fn layer_write_coverage_matches_canvas_scope() {
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        assert_eq!(
            catalog
                .entity_kinds
                .iter()
                .filter(|entry| entry.scope == EntityScope::Canvas)
                .count(),
            43
        );
        for entry in &catalog.entity_kinds {
            let layer = entry
                .properties
                .iter()
                .find(|property| property.name == "layer")
                .unwrap();
            assert_eq!(
                layer.model_access == ModelAccess::ReadWrite,
                entry.scope == EntityScope::Canvas
            );
            if entry.scope == EntityScope::Canvas {
                assert_eq!(layer.validation, "transaction_nonempty");
            }
        }
    }

    #[test]
    fn insert_catalog_separates_transform_writes_from_block_identity() {
        let catalog: EntityCoverageCatalog =
            serde_json::from_str(get_embedded_entity_coverage_json()).unwrap();
        let insert = catalog
            .entity_kinds
            .iter()
            .find(|entry| entry.kind == "Insert")
            .unwrap();
        let block_name = insert
            .properties
            .iter()
            .find(|property| property.name == "block_name")
            .unwrap();
        // Named at creation; a geometry transaction can never change it (the
        // mutation check rejects a different block).
        assert_eq!(block_name.model_access, ModelAccess::ReadWrite);
        assert!(
            insert
                .properties
                .iter()
                .find(|property| property.name == "attributes")
                .unwrap()
                .model_access
                == ModelAccess::Unmapped
        );
        assert!(
            insert
                .properties
                .iter()
                .find(|property| property.name == "insert_point")
                .unwrap()
                .model_access
                == ModelAccess::ReadWrite
        );
    }

    #[cfg(feature = "host")]
    #[test]
    fn unsupported_canvas_kind_has_typed_snapshot_fields() {
        use crate::host::codec;
        let hatch = codec::EntityType::Hatch(codec::entities::Hatch::default());
        let snapshot = entity_snapshot(&hatch).unwrap();
        let fields = snapshot
            .get("Hatch")
            .and_then(|value| value.as_object())
            .unwrap();
        assert!(fields.contains_key("common"));
        assert!(fields.len() > 1);
    }

    #[cfg(feature = "host")]
    #[test]
    fn changed_geometry_is_validated_without_rejecting_untouched_legacy_values() {
        use crate::host::codec;
        let mut before = codec::entities::Circle::default();
        before.radius = f64::NAN;
        let mut layer_only = before.clone();
        layer_only.common.layer = "OTHER".into();
        assert!(validate_entity_mutation(
            &codec::EntityType::Circle(before.clone()),
            &codec::EntityType::Circle(layer_only),
        )
        .is_ok());
        let mut bad_radius = before.clone();
        bad_radius.radius = -1.0;
        assert!(validate_entity_mutation(
            &codec::EntityType::Circle(before),
            &codec::EntityType::Circle(bad_radius),
        )
        .unwrap_err()
        .contains("Circle.radius"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn new_phase_seven_shapes_reject_invalid_geometry_and_keep_legacy_fields() {
        use crate::host::codec::{self, types::Vector3};
        let mut ray = codec::entities::Ray::default();
        ray.direction = Vector3::new(2.0, 0.0, 0.0);
        assert!(
            validate_new_canvas_entity(&codec::EntityType::Ray(ray.clone()))
                .unwrap_err()
                .contains("Ray.direction")
        );
        let before = codec::EntityType::Ray(ray.clone());
        ray.common.layer = "CONSTRUCTION".into();
        assert!(validate_entity_mutation(&before, &codec::EntityType::Ray(ray.clone())).is_ok());
        ray.direction = Vector3::new(0.0, 1.0, 0.0);
        assert!(validate_entity_mutation(&before, &codec::EntityType::Ray(ray)).is_ok());

        let mut xline = codec::entities::XLine::default();
        xline.base_point.x = f64::NAN;
        assert!(
            validate_new_canvas_entity(&codec::EntityType::XLine(xline))
                .unwrap_err()
                .contains("XLine.base_point")
        );

        let solid = codec::entities::Solid::new(
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::UNIT_Y,
            Vector3::ZERO,
        );
        let mut changed = solid.clone();
        changed.thickness = f64::INFINITY;
        assert!(validate_entity_mutation(
            &codec::EntityType::Solid(solid),
            &codec::EntityType::Solid(changed)
        )
        .unwrap_err()
        .contains("Solid.thickness"));

        let face = codec::entities::Face3D::new(
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::UNIT_Y,
            Vector3::ZERO,
        );
        let mut changed = face.clone();
        changed.invisible_edges = codec::entities::InvisibleEdgeFlags::from_bits(0x10);
        assert!(validate_entity_mutation(
            &codec::EntityType::Face3D(face),
            &codec::EntityType::Face3D(changed)
        )
        .unwrap_err()
        .contains("Face3D.invisible_edges"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn insert_transaction_checks_transforms_and_protects_references() {
        use crate::host::codec::{self, types::Vector3};
        let before = codec::entities::Insert::new("DOOR", Vector3::new(1.0, 2.0, 0.0));
        let mut moved = before.clone();
        moved.insert_point = Vector3::new(5.0, 6.0, 0.0);
        moved.set_x_scale(2.0);
        assert!(validate_entity_mutation(
            &codec::EntityType::Insert(before.clone()),
            &codec::EntityType::Insert(moved)
        )
        .is_ok());
        let mut bad = before.clone();
        bad.set_y_scale(f64::NAN);
        assert!(validate_entity_mutation(
            &codec::EntityType::Insert(before.clone()),
            &codec::EntityType::Insert(bad)
        )
        .unwrap_err()
        .contains("Insert.y_scale"));
        let mut bad = before.clone();
        bad.column_count = 0;
        assert!(validate_entity_mutation(
            &codec::EntityType::Insert(before.clone()),
            &codec::EntityType::Insert(bad)
        )
        .unwrap_err()
        .contains("array counts"));
        let mut bad = before.clone();
        bad.block_name = "OTHER".into();
        assert!(validate_entity_mutation(
            &codec::EntityType::Insert(before),
            &codec::EntityType::Insert(bad)
        )
        .unwrap_err()
        .contains("block identity"));
    }

    #[cfg(feature = "host")]
    #[test]
    fn canvas_layer_patch_preserves_unmapped_hatch_fields() {
        use crate::host::codec;
        let hatch = codec::EntityType::Hatch(codec::entities::Hatch::default());
        let patched = patch_canvas_layer(&hatch, "HATCHES").unwrap();
        assert_eq!(patched.common().layer, "HATCHES");
        assert_eq!(hatch.common().layer, "0");
        let mut expected = hatch.clone();
        expected.common_mut().layer = "HATCHES".into();
        assert_eq!(patched, expected);
        assert!(patch_canvas_layer(&hatch, " ").is_err());
    }

    #[cfg(feature = "host")]
    #[test]
    fn attribute_definition_requires_a_block_owner_and_text_style() {
        use crate::host::codec::{
            self,
            entities::{Block, BlockEnd},
            types::{Handle, Vector3},
        };
        let mut document = codec::CadDocument::new();
        let next = document.next_handle();
        let record_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);
        let mut record = codec::tables::BlockRecord::new("TAGBLOCK");
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = end_handle;
        document.block_records.add(record).unwrap();
        let mut block = Block::new("TAGBLOCK", Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        document
            .add_entity(codec::EntityType::Block(block))
            .unwrap();
        let mut end = BlockEnd::new();
        end.common.handle = end_handle;
        end.common.owner_handle = record_handle;
        document
            .add_entity(codec::EntityType::BlockEnd(end))
            .unwrap();

        let mut definition = codec::entities::AttributeDefinition::new(
            "PART_NO".into(),
            "Part number".into(),
            "PN-001".into(),
        );
        definition.common.owner_handle = record_handle;
        definition.insertion_point = Vector3::new(1.0, 2.0, 0.0);
        let entity = codec::EntityType::AttributeDefinition(definition.clone());
        validate_new_canvas_entity(&entity).unwrap();
        validate_canvas_entity_references(&document, &entity).unwrap();

        definition.common.owner_handle = Handle::NULL;
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::AttributeDefinition(definition.clone())
        )
        .unwrap_err()
        .contains("block-record owner"));
        definition.common.owner_handle = record_handle;
        definition.text_style = "Missing".into();
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::AttributeDefinition(definition.clone())
        )
        .unwrap_err()
        .contains("does not exist"));
        definition.text_style = "Standard".into();
        definition.tag = "BAD TAG".into();
        assert!(
            validate_new_canvas_entity(&codec::EntityType::AttributeDefinition(definition))
                .unwrap_err()
                .contains("no whitespace")
        );
    }
    #[test]
    fn hatch_geometry_and_boundaries_are_validated() {
        use codec::entities::hatch::{BoundaryEdge, BoundaryPath, LineEdge};
        let mut hatch = codec::entities::Hatch::new();
        let mut path = BoundaryPath::new();
        for (start, end) in [
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ] {
            path.edges.push(BoundaryEdge::Line(LineEdge {
                start: codec::types::Vector2::new(start.0, start.1),
                end: codec::types::Vector2::new(end.0, end.1),
            }));
        }
        hatch.paths.push(path);
        let entity = codec::EntityType::Hatch(hatch.clone());
        validate_new_canvas_entity(&entity).unwrap();

        let mut invalid = hatch.clone();
        invalid.paths.clear();
        assert!(validate_new_canvas_entity(&codec::EntityType::Hatch(invalid))
            .unwrap_err()
            .contains("at least one boundary path"));

        let mut invalid = hatch.clone();
        invalid.is_solid = false;
        invalid.pattern_scale = 0.0;
        assert!(validate_new_canvas_entity(&codec::EntityType::Hatch(invalid))
            .unwrap_err()
            .contains("greater than zero"));

        let mut document = codec::CadDocument::new();
        let boundary = document.add_entity(codec::EntityType::Line(
            codec::entities::Line::from_points(
                codec::types::Vector3::ZERO,
                codec::types::Vector3::UNIT_X,
            ),
        )).unwrap();
        let mut associative = hatch.clone();
        associative.is_associative = true;
        associative.paths[0].boundary_handles.push(boundary);
        validate_canvas_entity_references(
            &document,
            &codec::EntityType::Hatch(associative.clone()),
        ).unwrap();
        associative.paths[0].boundary_handles[0] = codec::Handle::new(u64::MAX);
        assert!(validate_canvas_entity_references(
            &document,
            &codec::EntityType::Hatch(associative),
        ).unwrap_err().contains("does not exist"));
    }
}
