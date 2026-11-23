// Dimension geometry, frames, breaks and snaps.

use super::*;

pub(crate) fn apply_dimension_breaks(
    document: &Document,
    dimension: Handle,
    lines: &mut Vec<[f32; 3]>,
    origin: Vector3,
) {
    let references: Vec<_> = document
        .objects
        .values()
        .filter_map(|object| {
            let document::objects::ObjectType::DataObject(object) = object else {
                return None;
            };
            let document::objects::DataObjectData::BreakData(data) = &object.data else {
                return None;
            };
            (data.dimension_reference == dimension).then_some(&data.point_references)
        })
        .flatten()
        .filter(|reference| {
            let points = [reference.first_point, reference.second_point];
            points
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        })
        .collect();
    if references.is_empty() || lines.len() < 2 {
        return;
    }

    let mut output = Vec::with_capacity(lines.len() + references.len() * 3);
    for run in lines.split(|point| point[0].is_nan()) {
        for segment in run.windows(2) {
            let a = Vec3::from_array(segment[0]);
            let b = Vec3::from_array(segment[1]);
            let direction = b - a;
            let length_squared = direction.length_squared();
            if length_squared <= 1e-12 {
                continue;
            }
            let mut intervals = vec![(0.0f32, 1.0f32)];
            for reference in &references {
                let first = vec3_local(point_from_origin(reference.first_point, origin));
                let second = vec3_local(point_from_origin(reference.second_point, origin));
                let t1 = (first - a).dot(direction) / length_squared;
                let t2 = (second - a).dot(direction) / length_squared;
                let closest1 = a + direction * t1.clamp(0.0, 1.0);
                let closest2 = a + direction * t2.clamp(0.0, 1.0);
                let requested_gap = (second - first).length();
                let tolerance = (requested_gap * 0.25)
                    .max(direction.length() * 1e-5)
                    .max(1e-4);
                if (first - closest1).length() > tolerance
                    || (second - closest2).length() > tolerance
                {
                    continue;
                }
                let half_point_gap = if requested_gap <= 1e-6 {
                    (tolerance / direction.length()).min(0.1)
                } else {
                    0.0
                };
                let cut_start = (t1.min(t2) - half_point_gap).clamp(0.0, 1.0);
                let cut_end = (t1.max(t2) + half_point_gap).clamp(0.0, 1.0);
                if cut_end <= cut_start {
                    continue;
                }
                let mut remaining = Vec::new();
                for (start, end) in intervals {
                    if cut_end <= start || cut_start >= end {
                        remaining.push((start, end));
                        continue;
                    }
                    if cut_start > start {
                        remaining.push((start, cut_start));
                    }
                    if cut_end < end {
                        remaining.push((cut_end, end));
                    }
                }
                intervals = remaining;
            }
            for (start, end) in intervals {
                if end - start > 1e-6 {
                    add_segment(&mut output, a + direction * start, a + direction * end);
                }
            }
        }
    }
    *lines = output;
}

/// The jog position stored on a dimension (WCS, full f64), if any.
pub(crate) fn dimension_jog_point(dimension: &Dimension) -> Option<Vector3> {
    if !matches!(dimension, Dimension::Linear(_) | Dimension::Aligned(_)) {
        return None;
    }
    dimension
        .base()
        .common
        .extended_data
        .get_record("ACAD_DSTYLE_DIMJAG_POSITION")
        .and_then(|record| {
            record.values.iter().rev().find_map(|value| match value {
                document::xdata::XDataValue::Point3D(point) => Some(*point),
                _ => None,
            })
        })
}

pub(crate) fn apply_dimension_jog(
    lines: &mut Vec<[f32; 3]>,
    requested: Vec3,
    normal: Vec3,
    size: f32,
    angle: f32,
) {
    if lines.len() < 2 {
        return;
    }
    let segments = lines
        .windows(2)
        .enumerate()
        .filter(|(_, segment)| segment[0][0].is_finite() && segment[1][0].is_finite())
        .map(|(index, segment)| {
            (
                index,
                [segment[0].map(f64::from), segment[1].map(f64::from)],
            )
        })
        .collect::<Vec<_>>();
    let coordinates = segments
        .iter()
        .map(|(_, segment)| *segment)
        .collect::<Vec<_>>();
    let Some((candidate, center)) =
        ocs_geom::space::nearest_segment_point(&coordinates, requested.to_array().map(f64::from))
    else {
        return;
    };
    let target = segments[candidate].0;
    let Some(jog) = ocs_geom::space::dimension_jog_points(
        coordinates[candidate],
        center,
        normal.to_array().map(f64::from),
        size as f64,
        angle as f64,
    ) else {
        return;
    };
    let jog = jog.map(|point| point.map(|value| value as f32));
    let first = lines[target];
    let second = lines[target + 1];

    let mut output = Vec::with_capacity(lines.len() + 4);
    output.extend_from_slice(&lines[..target]);
    if !output.is_empty() && !output.last().is_some_and(|point| point[0].is_nan()) {
        output.push([f32::NAN; 3]);
    }
    if first != jog[0] {
        output.push(first);
        output.push(jog[0]);
        output.push([f32::NAN; 3]);
    }
    output.extend(jog);
    if jog[3] != second {
        output.push([f32::NAN; 3]);
        output.push(jog[3]);
        output.push(second);
    }
    if target + 2 < lines.len() {
        if !output.last().is_some_and(|point| point[0].is_nan()) && !lines[target + 2][0].is_nan() {
            output.push([f32::NAN; 3]);
        }
        output.extend_from_slice(&lines[target + 2..]);
    }
    *lines = output;
}

/// The ACI index a dimension colour renders with: the per-object ACAD_DSTYLE
/// override when the entity carries one, else the dimension style's value.
/// The Properties panel reads and writes these same overrides, so the renderer
/// has to consult them or an edited colour shows in the panel and nowhere else.
pub(crate) fn dim_color_index(
    xd: &document::xdata::ExtendedData,
    code: i16,
    inherited: i16,
) -> i16 {
    crate::entities::dim_override::int(xd, code).unwrap_or(inherited)
}

pub(crate) fn resolve_dim_color(idx: i16, fallback: [f32; 4]) -> [f32; 4] {
    // DIMCLR* convention: 0 = BYBLOCK, 256 = BYLAYER → entity colour wins.
    if idx == 0 || idx == 256 {
        return fallback;
    }
    aci_to_rgba(&AcadColor::from_index(idx))
}

/// Resolve a DIMLWD / DIMLWE table value (the i16 lineweight code) into a
/// pixel width. -1 (ByLayer) / -2 (ByBlock) / -3 (Default) fall through to
/// the entity's already-resolved width.
pub(crate) fn resolve_dim_lineweight_px(code: i16, fallback_px: f32) -> f32 {
    const MM_TO_PX: f32 = 96.0 / 25.4;
    if code < 0 {
        return fallback_px;
    }
    // i16 value 0..=211 represents 1/100 mm.
    let mm = code as f32 / 100.0;
    (mm * MM_TO_PX).max(1.0)
}

/// Look up a linetype in the document's line_types table by handle and
/// resolve it to a (pattern_length, pattern) pair compatible with WireModel.
/// An arrowhead of a dimension, kept apart from its dimension line: the
/// line takes DIMLTYPE's dashes, an arrowhead or tick is always drawn solid —
/// dashed, a tick broke into two stubs once the line followed its layer's
/// linetype. (#898)
pub(crate) fn append_dim_arrow(g: &mut DimGeom, tip: Vec3, dir: Vec3, arrow: &ArrowKind) {
    let mut head = DimGeom::new();
    append_arrow(&mut head, tip, dir, arrow);
    if !head.dim_lines.is_empty() {
        if g.arrow_lines.last().is_some_and(|point| !point[0].is_nan()) {
            g.arrow_lines.push([f32::NAN; 3]);
        }
        g.arrow_lines.extend(head.dim_lines);
    }
    g.arrow_fill.extend(head.arrow_fill);
}

/// A dimension line's dash pattern. A linetype the style names draws as
/// itself; none or ByBlock takes the dimension's own linetype (ByLayer: its
/// layer's), which is what the ByBlock lines of its baked `*D` block drew —
/// an edited dimension, drawn live, lost its layer's dashes. (#898)
pub(crate) fn resolve_pattern_by_handle(
    doc: &Document,
    handle: document::types::Handle,
    common: &document::entities::EntityCommon,
    viewport: Option<document::Handle>,
    scale: f32,
) -> (f32, [f32; 8]) {
    let named = (!handle.is_null())
        .then(|| doc.line_types.iter().find(|lt| lt.handle == handle))
        .flatten()
        .map(|lt| lt.name.as_str())
        .filter(|name| !name.eq_ignore_ascii_case("byblock"));
    let name = match named {
        Some(name) if name.eq_ignore_ascii_case("bylayer") => doc
            .layers
            .get(&common.layer)
            .map_or("Continuous", |layer| layer.line_type.as_str()),
        Some(name) => name,
        None => {
            crate::scene::view::render::linetype_name_for_common_viewport(doc, common, viewport)
        }
    };
    crate::scene::view::render::resolve_pattern(&doc.line_types, name, scale)
}

/// Split the combined ext-lines point list (NaN-separated segment pairs)
/// into "first" / "second" halves. `append_linear_dimension` writes ext1
/// before ext2, so the first segment is ext1 and the second is ext2.
pub(crate) fn split_ext_lines(points: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut groups: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut current: Vec<[f32; 3]> = Vec::new();
    for &p in points {
        if p[0].is_nan() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(p);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let mut iter = groups.into_iter();
    let first = iter.next().unwrap_or_default();
    let rest: Vec<[f32; 3]> = iter.flatten().collect();
    (first, rest)
}

#[derive(Clone, Copy)]
pub(crate) struct DimLineParams {
    pub(crate) dimexo: f32,
    pub(crate) dimexe: f32,
    pub(crate) dimdle: f32,
    pub(crate) dimfxl: f32,
    pub(crate) dimfxlon: bool,
    pub(crate) dimsoxd: bool,
    pub(crate) dimcen: f32,
    pub(crate) ticks: bool,
    /// Arrowhead length (DIMASZ, scaled) — used to decide arrow-outside fit.
    pub(crate) arrow_len: f32,
    pub(crate) text_width: f32,
    /// Text height (DIMTXT, scaled) — a radial leader is sized by it.
    pub(crate) text_height: f32,
    /// This dimension is a dimensional constraint's dynamic dimension, which
    /// the reference draws by its own radial rules.
    pub(crate) constraint: bool,
    pub(crate) dimatfit: i16,
    pub(crate) dimtix: bool,
    pub(crate) dimtofl: bool,
    /// The dimension's Flip Arrow flags: each arrowhead turns to the other
    /// side of its extension line from where fit puts it.
    pub(crate) flip1: bool,
    pub(crate) flip2: bool,
    pub(crate) text_position: Vec3,
    pub(crate) horizontal_text: bool,
    /// DIMTAD lifts aligned text off its dimension line.
    pub(crate) text_lifted: bool,
    pub(crate) text_movement: i16,
    pub(crate) text_break: Option<TextBreak>,
    /// DIMTMOVE 1: the point the file stores is where the leader's hook starts,
    /// so the leader is drawn to it exactly and only the text is estimated.
    pub(crate) leader_anchor: Option<Vec3>,
}
/// Which arrow (1 or 2) belongs to this grip: linear/aligned extension
/// origins and arrow tips, or the two ends of a diameter.
pub(crate) fn arrow_end(dim: &Dimension, grip_id: usize) -> Option<u8> {
    match (dim, grip_id) {
        (Dimension::Linear(_) | Dimension::Aligned(_) | Dimension::Diameter(_), 0) => Some(1),
        (Dimension::Linear(_) | Dimension::Aligned(_) | Dimension::Diameter(_), 1) => Some(2),
        (Dimension::Linear(_) | Dimension::Aligned(_), 4) => Some(1),
        (Dimension::Linear(_) | Dimension::Aligned(_), 5) => Some(2),
        _ => None,
    }
}

pub(crate) fn dimension_geometry(
    dim: &Dimension,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
) -> DimGeom {
    let lv = |v| vec3_local(v);
    let mut g = DimGeom::new();
    match dim {
        Dimension::Aligned(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = normalized_or(second - first, Vec3::X);
            append_linear_dimension(
                &mut g,
                first,
                second,
                def,
                axis,
                arrow1,
                arrow2,
                params,
                suppress,
                d.ext_line_rotation as f32,
            );
        }
        Dimension::Linear(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = Vec3::new(d.rotation.cos() as f32, d.rotation.sin() as f32, 0.0);
            append_linear_dimension(
                &mut g,
                first,
                second,
                def,
                normalized_or(axis, Vec3::X),
                arrow1,
                arrow2,
                params,
                suppress,
                d.ext_line_rotation as f32,
            );
        }
        Dimension::Radius(d) => {
            let center = lv(d.angle_vertex);
            let point = lv(d.definition_point);
            let text = params.text_position;
            // Jogged radius dimensions use a shortened zig-zag leader.
            let jogged = dim
                .base()
                .common
                .extended_data
                .get_record("OCS_JOGGED")
                .is_some();
            if jogged {
                let delta = point - center;
                let dist = delta.length();
                let u = normalized_or(delta, Vec3::X);
                let perp = Vec3::new(-u.y, u.x, 0.0);
                let half = (dist * 0.06).max(1e-3);
                let mid = center + u * (dist * 0.5);
                let a = mid - u * half + perp * half;
                let b = mid + u * half - perp * half;
                if !suppress.dim2 {
                    add_segment(&mut g.dim_lines, center, a);
                    add_segment(&mut g.dim_lines, a, b);
                    add_segment(&mut g.dim_lines, b, point);
                }
            }
            let radius = (point - center).length();
            let text_is_outside = text.distance(center) > radius + 1e-5;
            if params.constraint {
                // A dimensional constraint reads on its own dimension line,
                // which runs from the centre to the arc point and breaks
                // where the text sits on it. Text that no longer fits inside
                // is reached by a leader from the arc point, and the
                // arrowhead stays on the arc either way.
                if !jogged && !suppress.dim2 {
                    add_segment_with_text_break(&mut g.dim_lines, center, point, params.text_break);
                }
                if text_is_outside && !suppress.dim2 {
                    append_radial_leader(&mut g, point, point - center, text, &params);
                }
                if !suppress.dim2 {
                    // The arrowhead's tip is on the arc, its body inside.
                    append_dim_arrow(
                        &mut g,
                        point,
                        normalized_or(center - point, Vec3::X),
                        arrow1,
                    );
                }
                // The centre mark belongs to a radius drawn without its
                // inside line; the line itself already marks the centre.
                if jogged || suppress.dim2 {
                    append_center_mark(&mut g, center, params.dimcen, radius);
                }
            } else {
                // Text inside the arc: the dimension line runs from the arc
                // point to the centre. Text outside: only a leader from the
                // arc point to the text, unless DIMTOFL asks for the inside
                // line as well.
                let inside_line = !jogged && !suppress.dim2 && (!text_is_outside || params.dimtofl);
                if inside_line {
                    add_segment(&mut g.dim_lines, center, point);
                }
                if text_is_outside && !suppress.dim2 {
                    append_radial_leader(&mut g, point, point - center, text, &params);
                }
                if !suppress.dim2 {
                    // The arrowhead sits on the arc with its body toward the text.
                    let body = if text_is_outside {
                        point - center
                    } else {
                        center - point
                    };
                    append_dim_arrow(&mut g, point, normalized_or(body, Vec3::X), arrow1);
                }
                // The centre mark only goes with a radius whose dimension
                // line does not reach the centre.
                if text_is_outside && !inside_line {
                    append_center_mark(&mut g, center, params.dimcen, radius);
                }
            }
        }
        Dimension::Diameter(d) => {
            let chord = lv(d.angle_vertex);
            let far_chord = lv(d.definition_point);
            append_diameter_dimension(
                &mut g,
                chord,
                far_chord,
                lv(d.base.text_middle_point),
                arrow1,
                arrow2,
                params,
                suppress,
            );
        }
        Dimension::Angular2Ln(d) => {
            // A two-line angular dimension stores two LINES, not two rays:
            // `first_point`→`second_point` is one, `angle_vertex`→
            // `definition_point` the other, and the angle is between them at
            // their intersection. Reading `angle_vertex` as the centre and the
            // other two as rays measures something else entirely — an angle of
            // ten degrees came out as two hundred and seventy.
            let (p1, p2) = (lv(d.first_point), lv(d.second_point));
            let (p3, p4) = (lv(d.angle_vertex), lv(d.definition_point));
            let arc_point = lv(d.dimension_arc);
            match two_line_angle_frame(p1, p2, p3, p4, arc_point) {
                Some((vertex, start, end)) => {
                    // An extension line runs from a side's nearer end out to
                    // the arc only where the arc lies beyond that side; where
                    // the arc crosses the side itself there is nothing to add.
                    let radius = vertex.distance(arc_point);
                    let side = |angle: f32| -> (Vec3, bool) {
                        let dir = Vec3::new(angle.cos(), angle.sin(), 0.0);
                        let deviation =
                            |a: Vec3, b: Vec3| normalized_or(b - a, dir).cross(dir).length();
                        let (a, b) = if deviation(p1, p2) <= deviation(p3, p4) {
                            (p1, p2)
                        } else {
                            (p3, p4)
                        };
                        let along = |p: Vec3| (p - vertex).dot(dir);
                        let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
                        let arc_end = vertex + dir * radius;
                        let near = if a.distance(arc_end) <= b.distance(arc_end) {
                            a
                        } else {
                            b
                        };
                        (near, radius >= lo - 1e-6 && radius <= hi + 1e-6)
                    };
                    let (first, first_covered) = side(start);
                    let (second, second_covered) = side(end);
                    let suppress = SuppressFlags {
                        ext1: suppress.ext1 || first_covered,
                        ext2: suppress.ext2 || second_covered,
                        dim1: suppress.dim1,
                        dim2: suppress.dim2,
                    };
                    append_angular_dimension(
                        &mut g,
                        vertex,
                        first,
                        second,
                        arc_point,
                        arrow1,
                        arrow2,
                        Some((start, end)),
                        params,
                        suppress,
                    )
                }
                // Parallel lines have no vertex and so no angle to draw; the
                // extension lines alone say where the dimension was.
                None => {
                    add_segment(&mut g.ext_lines, p1, p2);
                    add_segment(&mut g.ext_lines, p3, p4);
                }
            }
        }
        Dimension::Angular3Pt(d) => {
            let vertex = lv(d.angle_vertex);
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let arc_point = lv(d.definition_point);
            // The sweep starts at whichever ray the arc point says; the
            // extension lines swap with it, or they would cross the angle.
            let (first, second, explicit_sweep) =
                match three_point_frame(vertex, first, second, arc_point) {
                    Some((start, end, true)) => (second, first, Some((start, end))),
                    Some((start, end, false)) => (first, second, Some((start, end))),
                    None => (first, second, None),
                };
            append_angular_dimension(
                &mut g,
                vertex,
                first,
                second,
                arc_point,
                arrow1,
                arrow2,
                explicit_sweep,
                params,
                suppress,
            );
        }
        Dimension::Ordinate(d) => {
            if !suppress.ext1 {
                let fixed_length = params.dimfxlon.then_some(params.dimfxl.max(0.0) as f64);
                let points = d.leader_polyline(
                    (params.arrow_len * 2.0) as f64,
                    params.dimexo as f64,
                    fixed_length,
                );
                for pair in points.windows(2) {
                    let start = lv(pair[0]);
                    let end = lv(pair[1]);
                    if (end - start).length_squared() > 1e-12 {
                        add_segment(&mut g.ext_lines, start, end);
                    }
                }
            }
        }
        Dimension::Arc(d) => {
            let explicit_sweep = arc_dimension_angles(d);
            append_angular_dimension(
                &mut g,
                lv(d.center_point),
                lv(d.first_extension_point),
                lv(d.second_extension_point),
                lv(d.definition_point),
                arrow1,
                arrow2,
                explicit_sweep,
                params,
                suppress,
            );
            if d.has_leader {
                add_segment(
                    &mut g.dim_lines,
                    lv(d.first_leader_point),
                    lv(d.second_leader_point),
                );
            }
        }
        Dimension::LargeRadial(d) => {
            let chord = lv(d.chord_point);
            let jog = lv(d.jog_point);
            let override_center = lv(d.override_center);
            let (near, far) = jogged_radial_break(chord, jog, override_center, d.jog_angle as f32);
            let axis = normalized_or(chord - override_center, Vec3::X);
            let available = chord.distance(override_center);
            let arrows_outside = if params.ticks || params.arrow_len <= 1.0e-6 {
                false
            } else if available < params.arrow_len {
                true
            } else if available < params.text_width + params.arrow_len {
                if params.dimtix {
                    true
                } else {
                    match params.dimatfit {
                        0 | 1 => true,
                        2 => false,
                        _ => params.text_width <= available,
                    }
                }
            } else {
                false
            };
            let true_center = lv(d.definition_point);
            let radius = chord.distance(true_center);
            let text_outside = params.text_position.distance(true_center) > radius + 1.0e-5;
            let draw_inside_line = !arrows_outside || params.dimtofl;
            if draw_inside_line && !suppress.dim2 {
                add_segment_with_text_break(&mut g.dim_lines, chord, near, params.text_break);
                add_segment_with_text_break(&mut g.dim_lines, near, far, params.text_break);
                add_segment_with_text_break(
                    &mut g.dim_lines,
                    far,
                    override_center,
                    params.text_break,
                );
            }
            if arrows_outside && !suppress.dim2 {
                add_segment(
                    &mut g.dim_lines,
                    chord,
                    chord + axis * (params.arrow_len * 2.0),
                );
            }
            if !suppress.dim2 {
                append_dim_arrow(
                    &mut g,
                    chord,
                    if arrows_outside { axis } else { -axis },
                    arrow1,
                );
            }
            if text_outside && params.text_movement == 0 && !suppress.dim2 {
                add_segment_with_text_break(
                    &mut g.dim_lines,
                    chord,
                    params.text_position,
                    params.text_break,
                );
            }
            if arrows_outside || text_outside {
                append_center_mark(&mut g, true_center, params.dimcen, radius);
            }
        }
    }
    g
}

pub(crate) fn add_segment_with_text_break(
    points: &mut Vec<[f32; 3]>,
    start: Vec3,
    end: Vec3,
    text_break: Option<TextBreak>,
) {
    let segment = end - start;
    let length = segment.length();
    if length <= 1.0e-6 {
        return;
    }
    let Some(text_break) = text_break.filter(|text_break| !text_break.through) else {
        add_segment(points, start, end);
        return;
    };
    let segment_curve = ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line {
        start: [start.x as f64, start.y as f64],
        end: [end.x as f64, end.y as f64],
    });
    let tolerance = ocs_geom::geom2d::Tolerance::new((length as f64 * 1.0e-9).max(1.0e-9));
    let inside = |padding: f32| {
        let (sin, cos) = text_break.rotation.sin_cos();
        let x_axis = Vec3::new(cos as f32, sin as f32, 0.0);
        let y_axis = Vec3::new(-sin as f32, cos as f32, 0.0);
        let half_width = text_break.half_width + padding;
        let half_height = text_break.half_height + padding;
        let corners = [
            text_break.center - x_axis * half_width - y_axis * half_height,
            text_break.center + x_axis * half_width - y_axis * half_height,
            text_break.center + x_axis * half_width + y_axis * half_height,
            text_break.center - x_axis * half_width + y_axis * half_height,
        ];
        let boundary: Vec<_> = (0..4)
            .map(|index| {
                let next = (index + 1) % 4;
                ocs_geom::geom2d::Curve::Line(ocs_geom::geom2d::Line {
                    start: [corners[index].x as f64, corners[index].y as f64],
                    end: [corners[next].x as f64, corners[next].y as f64],
                })
            })
            .collect();
        ocs_geom::geom2d::inside_spans(&boundary, &segment_curve, tolerance)
    };
    if inside(0.0).is_empty() {
        add_segment(points, start, end);
        return;
    }
    let mut cursor = 0.0_f32;
    for [cut_start, cut_end] in inside(text_break.padding.max(0.0)) {
        let cut_start = (cut_start as f32).clamp(0.0, 1.0);
        let cut_end = (cut_end as f32).clamp(0.0, 1.0);
        if cut_start - cursor > 1.0e-6 {
            add_segment(points, start.lerp(end, cursor), start.lerp(end, cut_start));
        }
        cursor = cursor.max(cut_end);
    }
    if 1.0 - cursor > 1.0e-6 {
        add_segment(points, start.lerp(end, cursor), end);
    }
}

pub(crate) fn jogged_radial_break(
    chord: Vec3,
    jog: Vec3,
    override_center: Vec3,
    jog_angle: f32,
) -> (Vec3, Vec3) {
    let radial = normalized_or(chord - override_center, Vec3::X);
    let (sin, cos) = jog_angle.sin_cos();
    let transverse = normalized_or(
        Vec3::new(
            radial.x * cos - radial.y * sin,
            radial.x * sin + radial.y * cos,
            0.0,
        ),
        Vec3::Y,
    );
    let half = ((chord - override_center).length() * 0.04).max(1e-3);
    let first = jog - transverse * half;
    let second = jog + transverse * half;
    if chord.distance_squared(first) <= chord.distance_squared(second) {
        (first, second)
    } else {
        (second, first)
    }
}

pub(crate) fn append_linear_dimension(
    g: &mut DimGeom,
    first: Vec3,
    second: Vec3,
    def: Vec3,
    axis: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
    ext_line_rotation: f32,
) {
    let perp = Vec3::new(-axis.y, axis.x, 0.0);
    let dim_line_pos = def.dot(perp);
    // ext_line_rotation (DIMEDIT "Oblique", DXF 52): the extension lines'
    // angle from the X axis; 0 means none (perpendicular). Each extension
    // line runs from its def point along that angle until it meets the
    // dimension line, so the dimension line ends where the lines cross it.
    let oblique = Vec3::new(ext_line_rotation.cos(), ext_line_rotation.sin(), 0.0);
    let ext_dir = if ext_line_rotation.abs() > 1e-6 && oblique.dot(perp).abs() > 1e-3 {
        oblique / oblique.dot(perp)
    } else {
        perp
    };
    // `ext_dir` advances one unit of perpendicular offset per step, so the
    // same offsets reach the dimension line; lengths below use its unit form.
    let offset1 = dim_line_pos - first.dot(perp);
    let offset2 = dim_line_pos - second.dot(perp);
    let d1 = first + ext_dir * offset1;
    let d2 = second + ext_dir * offset2;
    let sign1 = if offset1 >= 0.0 { 1.0_f32 } else { -1.0 };
    let sign2 = if offset2 >= 0.0 { 1.0_f32 } else { -1.0 };
    let along1 = offset1.abs() * ext_dir.length();
    let along2 = offset2.abs() * ext_dir.length();
    let ext_dir = normalized_or(ext_dir, perp);

    // DIMFXLON / DIMFXL: fixed extension-line length from the dim line back
    // toward (but not past) the definition point. Otherwise grow from the
    // def point with DIMEXO gap, extending DIMEXE past the dim line.
    // When oblique, lengths are measured along ext_dir instead of perp.
    let (ext1_start, ext1_end, ext2_start, ext2_end) = if params.dimfxlon {
        let fxl = params.dimfxl.max(0.0);
        // Never past the DIMEXO gap at the definition point: a length longer
        // than the line itself would otherwise shoot far beyond the object.
        let fxl1 = fxl.min((along1 - params.dimexo).max(0.0));
        let fxl2 = fxl.min((along2 - params.dimexo).max(0.0));
        let s1 = d1 - ext_dir * (sign1 * fxl1);
        let e1 = d1 + ext_dir * (sign1 * params.dimexe);
        let s2 = d2 - ext_dir * (sign2 * fxl2);
        let e2 = d2 + ext_dir * (sign2 * params.dimexe);
        (s1, e1, s2, e2)
    } else {
        (
            first + ext_dir * (sign1 * params.dimexo),
            d1 + ext_dir * (sign1 * params.dimexe),
            second + ext_dir * (sign2 * params.dimexo),
            d2 + ext_dir * (sign2 * params.dimexe),
        )
    };
    if !suppress.ext1 {
        add_segment(&mut g.ext_lines, ext1_start, ext1_end);
    }
    if !suppress.ext2 {
        add_segment(&mut g.ext_lines, ext2_start, ext2_end);
    }

    // DIMDLE: dim line overshoots the ext line by `dimdle` at each end,
    // but only when ticks are in use (DIMTSZ > 0). With arrowheads this
    // is ignored.
    let dle = if params.ticks { params.dimdle } else { 0.0 };
    let dir_d1_to_d2 = normalized_or(d2 - d1, axis);
    let d1_out = d1 - dir_d1_to_d2 * dle;
    let d2_out = d2 + dir_d1_to_d2 * dle;

    // When text plus arrows do not fit, DIMATFIT decides which component moves
    // first: 0=both, 1=arrows, 2=text, 3=best fit. The text is measured along
    // the dimension line where it is drawn: horizontal text on a vertical
    // dimension takes up its height, and text dragged toward one end runs into
    // that arrow long before the sum of widths says the gap is full.
    let gap = (d2 - d1).length();
    let text_span = params.text_break.map(|text_break| {
        let axis_angle = dir_d1_to_d2.y.atan2(dir_d1_to_d2.x) as f64;
        let (sin, cos) = (text_break.rotation - axis_angle).sin_cos();
        let half = (text_break.half_width as f64 * cos).abs()
            + (text_break.half_height as f64 * sin).abs()
            + text_break.padding as f64;
        let along = (text_break.center - d1).dot(dir_d1_to_d2) as f64;
        (along - half, along + half)
    });
    let text_extent = text_span
        .map(|(lo, hi)| (hi - lo) as f32)
        .unwrap_or(params.text_width);
    // Horizontal text beside a non-horizontal dimension is reached by a leg
    // and a hook instead of the dimension line itself.
    let hooks_out_horizontally =
        params.horizontal_text && !params.ticks && dir_d1_to_d2.y.abs() > 1e-3;
    let text_hits_an_arrow = text_span.is_some_and(|(lo, hi)| {
        let center = (lo + hi) * 0.5;
        center > 0.0
            && center < gap as f64
            && (lo < params.arrow_len as f64 || hi > (gap - params.arrow_len) as f64)
    });
    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if gap < 2.0 * params.arrow_len || text_hits_an_arrow {
        true
    } else if gap < text_extent + 2.0 * params.arrow_len {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => text_extent <= gap,
        }
    } else {
        false
    };

    let line_dir = normalized_or(d2_out - d1_out, axis);
    let line_len = (d2_out - d1_out).length();
    let split = params
        .text_break
        .map(|text_break| (text_break.center - d1_out).dot(line_dir))
        .filter(|along| *along > 0.0 && *along < line_len)
        .unwrap_or(line_len * 0.5);
    let split_point = d1_out + line_dir * split;
    let (out1, out2) = (
        arrows_outside != params.flip1,
        arrows_outside != params.flip2,
    );
    let draw_inside_line = !(out1 && out2) || params.dimtofl;
    if draw_inside_line && !suppress.dim1 && split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, d1_out, split_point, params.text_break);
    }
    if draw_inside_line && !suppress.dim2 && line_len - split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, split_point, d2_out, params.text_break);
    }
    // Outside the extension lines the dimension line runs two arrow lengths,
    // or on to the text when the text sits further out along the line.
    let reach =
        |default: f32, toward_text: Option<f32>| toward_text.map_or(default, |t| t.max(default));
    let stub = params.arrow_len * 2.0;
    // Text centred on the line stops the line at its near end; text that sits
    // beside it (lifted above or below) has the line run on under the whole
    // text (#3332).
    let lifted = params.text_break.is_some_and(|text_break| {
        let axis_angle = dir_d1_to_d2.y.atan2(dir_d1_to_d2.x) as f64;
        let (sin, cos) = (text_break.rotation - axis_angle).sin_cos();
        let half_across = (text_break.half_width as f64 * sin).abs()
            + (text_break.half_height as f64 * cos).abs();
        let offset = text_break.center - d1;
        let across = (offset - dir_d1_to_d2 * offset.dot(dir_d1_to_d2)).length() as f64;
        across > half_across * 0.5
    });
    let text_beyond_d1 = text_span
        .filter(|(_, hi)| *hi < 0.0 && !hooks_out_horizontally)
        .map(|(lo, hi)| -(if lifted { lo } else { hi }) as f32);
    let text_beyond_d2 = text_span
        .filter(|(lo, _)| *lo > gap as f64 && !hooks_out_horizontally)
        .map(|(lo, hi)| ((if lifted { hi } else { lo }) - gap as f64) as f32);
    // DIMTMOVE=2 leaves moved text outside the dimension without a fine
    // leader or an extension that chases the text. Keep genuine arrow-fit
    // stubs, but do not turn a user-placed text point into another segment.
    let chase_text = params.text_movement != 2;
    let beyond_d1 = chase_text.then_some(text_beyond_d1).flatten();
    let beyond_d2 = chase_text.then_some(text_beyond_d2).flatten();
    if ((out1 || out2) && !params.dimsoxd) || beyond_d1.is_some() || beyond_d2.is_some() {
        if !suppress.dim1 && (out1 || beyond_d1.is_some()) {
            add_segment(
                &mut g.dim_lines,
                d1 - dir_d1_to_d2 * reach(stub, beyond_d1),
                d1,
            );
        }
        if !suppress.dim2 && (out2 || beyond_d2.is_some()) {
            add_segment(
                &mut g.dim_lines,
                d2,
                d2 + dir_d1_to_d2 * reach(stub, beyond_d2),
            );
        }
    }

    // Outside: tip on the ext line, body pointing outward.
    let toward_d2 = normalized_or(d2 - d1, axis);
    append_dim_arrow(g, d1, if out1 { -toward_d2 } else { toward_d2 }, arrow1);
    append_dim_arrow(g, d2, if out2 { toward_d2 } else { -toward_d2 }, arrow2);

    // Horizontal text outside the extension lines of a dimension that is not
    // horizontal itself: the dimension line runs on to the text's height and
    // turns a hook one arrow long toward the text, which sits against it.
    if params.text_movement != 2
        && params.horizontal_text
        && !params.ticks
        && dir_d1_to_d2.y.abs() > 1e-3
    {
        if let Some(text_break) = params.text_break {
            let along = (text_break.center - d1).dot(dir_d1_to_d2);
            let outside = if along < 0.0 {
                Some((d1, -dir_d1_to_d2, suppress.dim1))
            } else if along > gap {
                Some((d2, dir_d1_to_d2, suppress.dim2))
            } else {
                None
            };
            if let Some((end, outward, suppressed)) = outside {
                let travel = (text_break.center.y - end.y) / outward.y;
                if travel > 0.0 && !suppressed {
                    let elbow = end + outward * travel;
                    let side = if text_break.center.x >= elbow.x {
                        1.0
                    } else {
                        -1.0
                    };
                    let hook_end = Vec3::new(elbow.x + side * params.arrow_len, elbow.y, elbow.z);
                    add_segment(&mut g.dim_lines, end, elbow);
                    add_segment(&mut g.dim_lines, elbow, hook_end);
                }
            }
        }
    }
}

/// Leader from a point on the circle to text outside it.
///
/// A dimensional constraint's dimension leaves the rim along its dimension
/// line, elbows level with the text and hooks under it. An ordinary radial
/// dimension keeps its measured layout: horizontal text gets a hook one arrow
/// long that the text sits against, and aligned text is reached by a straight
/// leader that stops a gap short of it. `text_width` already carries a gap on
/// each side.
pub(crate) fn append_radial_leader(
    g: &mut DimGeom,
    tip: Vec3,
    direction: Vec3,
    text: Vec3,
    params: &DimLineParams,
) {
    if params.constraint {
        let side = if text.x >= tip.x { 1.0 } else { -1.0 };
        let edge = match params.leader_anchor {
            Some(anchor) => anchor,
            None => Vec3::new(text.x - side * params.text_width * 0.5, text.y, text.z),
        };
        let elbow = tip
            + normalized_or(direction, Vec3::X)
                * radial_leader_run(params.text_height as f64) as f32;
        // The hook is horizontal at the text's height; the run reaches it.
        let elbow = Vec3::new(elbow.x, edge.y, edge.z);
        add_segment(&mut g.dim_lines, tip, elbow);
        add_segment(&mut g.dim_lines, elbow, edge);
        return;
    }
    if params.horizontal_text {
        let side = if text.x >= tip.x { 1.0 } else { -1.0 };
        let (hook_start, text_edge) = match params.leader_anchor {
            Some(anchor) => (anchor, anchor + Vec3::X * (side * params.arrow_len)),
            None => {
                let edge = Vec3::new(text.x - side * params.text_width * 0.5, text.y, text.z);
                (edge - Vec3::X * (side * params.arrow_len), edge)
            }
        };
        let slope = (hook_start - tip).y.abs().atan2((hook_start - tip).x.abs());
        // A leader already within 15° of horizontal runs straight to the text.
        if slope > 15.0_f32.to_radians() {
            add_segment(&mut g.dim_lines, tip, hook_start);
            add_segment(&mut g.dim_lines, hook_start, text_edge);
        } else {
            add_segment(&mut g.dim_lines, tip, text_edge);
        }
    } else if params.text_lifted {
        // Text above or below the line: the line carries on out from the
        // rim, under the whole text to its far end.
        let along = normalized_or(direction, Vec3::X);
        let reach = (text - tip).dot(along) + params.text_width * 0.5;
        if reach > 0.0 {
            add_segment(&mut g.dim_lines, tip, tip + along * reach);
        }
    } else {
        let toward = normalized_or(text - tip, Vec3::X);
        let reach = ((text - tip).length() - params.text_width * 0.5).max(0.0);
        add_segment(&mut g.dim_lines, tip, tip + toward * reach);
    }
}

pub(crate) fn append_diameter_dimension(
    g: &mut DimGeom,
    chord: Vec3,
    far_chord: Vec3,
    text_anchor: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    params: DimLineParams,
    suppress: SuppressFlags,
) {
    let axis = normalized_or(far_chord - chord, Vec3::X);
    let diameter = chord.distance(far_chord);
    if diameter <= 1e-6 {
        return;
    }
    let center = (chord + far_chord) * 0.5;
    let (tip, direction, suppressed) = if params.text_position.distance_squared(chord)
        <= params.text_position.distance_squared(far_chord)
    {
        (chord, -axis, suppress.dim1)
    } else {
        (far_chord, axis, suppress.dim2)
    };
    let text_is_outside = params.text_position.distance(center) > diameter * 0.5 + 1e-5;
    if text_is_outside && (params.constraint || !params.dimtofl) {
        // A dimensional constraint carries its text on a leader from the
        // nearer end of the dimension line, and the line and both arrowheads
        // stay inside the circle. An ordinary diameter draws no line across
        // the circle at all: just that leader, with one arrowhead on the
        // circle and its body toward the text. DIMTMOVE 2 keeps the arrowhead
        // and drops the leader.
        if !suppressed && params.text_movement != 2 {
            append_radial_leader(g, tip, tip - center, params.text_position, &params);
        }
        if !params.constraint {
            if !suppressed {
                append_dim_arrow(g, tip, normalized_or(tip - center, axis), arrow1);
            }
            return;
        }
    }
    // Text projected outside the diameter requires inward-pointing arrowheads.
    let text_along = (params.text_position - chord).dot(axis);
    let text_outside = text_along < 0.0 || text_along > diameter;

    let arrows_outside = if params.ticks || params.arrow_len <= 1e-6 {
        false
    } else if text_is_outside {
        // The leader carries the text; the arrowheads keep the circle.
        false
    } else if text_outside {
        true
    } else if diameter < 2.0 * params.arrow_len {
        true
    } else if diameter < params.text_width + 2.0 * params.arrow_len {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => params.text_width <= diameter,
        }
    } else {
        false
    };

    let extension = if params.ticks { params.dimdle } else { 0.0 };
    let first = chord - axis * extension;
    let second = far_chord + axis * extension;
    let line_length = first.distance(second);
    let split = params
        .text_break
        .map(|text_break| (text_break.center - first).dot(axis))
        .filter(|along| *along > 0.0 && *along < line_length)
        .unwrap_or(line_length * 0.5);
    let split_point = first + axis * split;

    let (out1, out2) = (
        arrows_outside != params.flip1,
        arrows_outside != params.flip2,
    );
    let draw_inside_line = text_outside || !(out1 && out2) || params.dimtofl;
    if draw_inside_line && !suppress.dim1 && split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, first, split_point, params.text_break);
    }
    if draw_inside_line && !suppress.dim2 && line_length - split > 1e-6 {
        add_segment_with_text_break(&mut g.dim_lines, split_point, second, params.text_break);
    }
    if !params.dimsoxd {
        let stub = params.arrow_len * 2.0;
        if out1 && !suppress.dim1 {
            add_segment(&mut g.dim_lines, chord - axis * stub, chord);
        }
        if out2 && !suppress.dim2 {
            add_segment(&mut g.dim_lines, far_chord, far_chord + axis * stub);
        }
    }

    // The centre mark only goes with a diameter whose dimension line does not
    // cross the circle.
    if !draw_inside_line {
        let center = (chord + far_chord) * 0.5;
        append_center_mark(g, center, params.dimcen, diameter * 0.5);
    }
    append_dim_arrow(g, chord, if out1 { -axis } else { axis }, arrow1);
    append_dim_arrow(g, far_chord, if out2 { axis } else { -axis }, arrow2);

    if params.text_movement == 1 && !suppressed && !(text_is_outside && params.constraint) {
        // The outside radial leader above already reaches the text. A diameter
        // that keeps its chord can also carry moved text on a straight leader.
        if tip.distance(params.text_position) > params.text_height {
            add_segment_with_text_break(
                &mut g.dim_lines,
                tip,
                params.text_position,
                params.text_break,
            );
        }
    } else if params.text_movement == 0 {
        if text_outside && !suppressed {
            let leader_length = (text_anchor - tip).dot(direction);

            if leader_length > 1e-6 {
                let text_extension = params.text_width * 0.5;
                add_segment(
                    &mut g.dim_lines,
                    tip,
                    tip + direction * (leader_length + text_extension),
                );
            }
        } else if text_along < 0.0 && !suppress.dim1 {
            add_segment(&mut g.dim_lines, chord, params.text_position);
        } else if text_along > diameter && !suppress.dim2 {
            add_segment(&mut g.dim_lines, far_chord, params.text_position);
        }
    }
}

/// Draw a center mark for radius/diameter dimensions.
///   DIMCEN > 0 → small "+" of half-length |DIMCEN| at the centre.
///   DIMCEN < 0 → small "+" *plus* four line segments extending from the
///                circle (radius - |DIMCEN|) outward to (radius + |DIMCEN|).
///   DIMCEN = 0 → no mark.
pub(crate) fn append_center_mark(g: &mut DimGeom, center: Vec3, dimcen: f32, radius: f32) {
    let mag = dimcen.abs();
    if mag <= 1e-6 {
        return;
    }
    // Small "+" at the centre.
    let h = mag;
    add_segment(
        &mut g.dim_lines,
        Vec3::new(center.x - h, center.y, center.z),
        Vec3::new(center.x + h, center.y, center.z),
    );
    add_segment(
        &mut g.dim_lines,
        Vec3::new(center.x, center.y - h, center.z),
        Vec3::new(center.x, center.y + h, center.z),
    );
    if dimcen < 0.0 && radius > mag + 1e-6 {
        let inner = (radius - mag).max(0.0);
        let outer = radius + mag;
        // Four short radial strokes spanning the circle edge.
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x + inner, center.y, center.z),
            Vec3::new(center.x + outer, center.y, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x - inner, center.y, center.z),
            Vec3::new(center.x - outer, center.y, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x, center.y + inner, center.z),
            Vec3::new(center.x, center.y + outer, center.z),
        );
        add_segment(
            &mut g.dim_lines,
            Vec3::new(center.x, center.y - inner, center.z),
            Vec3::new(center.x, center.y - outer, center.z),
        );
    }
}

/// Vertex and sweep of the angle between two lines, as a two-line angular
/// dimension stores them: `a1`→`a2` and `b1`→`b2`, with `arc_point` sitting on
/// the arc that shows which of the four angles at the crossing is meant.
///
/// Returns `None` for parallel lines, which cross nowhere and enclose nothing.
pub(crate) fn two_line_angle_frame(
    a1: Vec3,
    a2: Vec3,
    b1: Vec3,
    b2: Vec3,
    arc_point: Vec3,
) -> Option<(Vec3, f32, f32)> {
    let (vertex, start, end) = two_line_angle_frame_f64(
        a1.as_dvec3(),
        a2.as_dvec3(),
        b1.as_dvec3(),
        b2.as_dvec3(),
        arc_point.as_dvec3(),
    )?;
    Some((vertex.as_vec3(), start as f32, end as f32))
}

/// The box the drawing leaves open in the dimension line around the text, in
/// world coordinates: centre, half width and half height (the gap included)
/// and rotation. The saved block breaks its dimension line the same way.
pub(crate) fn dimension_text_break_world(
    dim: &Dimension,
    document: &Document,
) -> Option<(DVec3, f64, f64, f64)> {
    let origin = dimension_precision_origin(dim);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dim, origin));
    let local = shifted.as_ref().unwrap_or(dim);
    let style = dimension_source_style(document, local)
        .map(|style| resolved_dimension_style(style, local, document));
    let style = style.as_ref();
    let scale = style.map_or(1.0, |s| if s.dimscale > 1e-6 { s.dimscale } else { 1.0 });
    let text_height = style.map_or(2.5, |s| s.dimtxt * scale);
    let gap = dimension_text_layout(local, style, text_height, scale, Some(document)).break_box?;
    Some((
        gap.center.as_dvec3() + DVec3::new(origin.x, origin.y, origin.z),
        f64::from(gap.half_width + gap.padding),
        f64::from(gap.half_height + gap.padding),
        gap.rotation,
    ))
}

/// [`two_line_angle_frame`] in full precision, for the saved block.
pub(crate) fn two_line_angle_frame_f64(
    a1: DVec3,
    a2: DVec3,
    b1: DVec3,
    b2: DVec3,
    arc_point: DVec3,
) -> Option<(DVec3, f64, f64)> {
    let (u, v) = (a2 - a1, b2 - b1);
    let (t, _) = ocs_geom::geom2d::line_line([a1.x, a1.y], [u.x, u.y], [b1.x, b1.y], [v.x, v.y])?;
    let vertex = a1 + u * t;

    // Two lines cross at four angles; the arc point picks one. Each line
    // contributes its direction and its reverse, so try the four pairs and keep
    // the one whose sweep both contains the arc point and is the shorter way
    // round — the dimension marks an angle, never its reflex twin.
    let angle_of = |d: DVec3| d.y.atan2(d.x);
    let target = angle_of(arc_point - vertex);
    let mut best: Option<(f64, f64, f64)> = None;
    for su in [1.0f64, -1.0] {
        for sv in [1.0f64, -1.0] {
            let (from_u, from_v) = (angle_of(u * su), angle_of(v * sv));
            // Either line can be the one the sweep starts from; taking only
            // u→v leaves out half the angles at the crossing, and the half left
            // out is the one wanted whenever the lines are nearly parallel.
            for (start, end) in [(from_u, from_v), (from_v, from_u)] {
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sweep <= 1e-6 || sweep > std::f64::consts::PI {
                    continue;
                }
                let into_target = (target - start).rem_euclid(std::f64::consts::TAU);
                if into_target > sweep {
                    continue;
                }
                if best.is_none_or(|(_, _, known)| sweep < known) {
                    best = Some((start, start + sweep, sweep));
                }
            }
        }
    }
    let (start, end, _) = best?;
    Some((vertex, start, end))
}

/// The sweep of a three-point angular dimension: counter-clockwise from the
/// first ray to the second when the arc point lies in that sweep, otherwise
/// the other way round (`swapped`). Rays need no crossing, so a straight
/// angle draws, and a reflex one is kept as picked.
pub(crate) fn three_point_frame(
    vertex: Vec3,
    first: Vec3,
    second: Vec3,
    arc_point: Vec3,
) -> Option<(f32, f32, bool)> {
    let angle_of = |d: Vec3| d.y.atan2(d.x);
    let (r1, r2, at) = (first - vertex, second - vertex, arc_point - vertex);
    if r1.length_squared() <= 1e-12 || r2.length_squared() <= 1e-12 || at.length_squared() <= 1e-12
    {
        return None;
    }
    let tau = std::f32::consts::TAU;
    let (a1, a2) = (angle_of(r1), angle_of(r2));
    let sweep = (a2 - a1).rem_euclid(tau);
    if sweep <= 1e-6 {
        return None;
    }
    if (angle_of(at) - a1).rem_euclid(tau) <= sweep {
        Some((a1, a1 + sweep, false))
    } else {
        Some((a2, a2 + (tau - sweep), true))
    }
}

pub(crate) fn arc_dimension_angles(dimension: &DimensionArc) -> Option<(f32, f32)> {
    let raw = dimension.arc_end_parameter - dimension.arc_start_parameter;
    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
        sweep = std::f64::consts::TAU;
    }
    if sweep > 1.0e-12 {
        let start = dimension.arc_start_parameter as f32;
        return Some((start, start + sweep as f32));
    }

    let start = (dimension.first_extension_point.y - dimension.center_point.y)
        .atan2(dimension.first_extension_point.x - dimension.center_point.x);
    let end = (dimension.second_extension_point.y - dimension.center_point.y)
        .atan2(dimension.second_extension_point.x - dimension.center_point.x);
    let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
    (sweep > 1.0e-12).then_some((start as f32, (start + sweep) as f32))
}

pub(crate) fn arc_dimension_in_ocs(dimension: &DimensionArc) -> Option<(DimensionArc, Vector3)> {
    let length = dimension.base.normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base.normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    local.center_point = to_ocs(dimension.center_point);
    let elevation = local.center_point.z;
    let on_plane = |point: Vector3| {
        let mut point = to_ocs(point);
        point.z = elevation;
        point
    };
    local.definition_point = on_plane(dimension.definition_point);
    local.first_extension_point = on_plane(dimension.first_extension_point);
    local.second_extension_point = on_plane(dimension.second_extension_point);
    local.first_leader_point = on_plane(dimension.first_leader_point);
    local.second_leader_point = on_plane(dimension.second_leader_point);
    local.base.definition_point = local.definition_point;
    local.base.text_middle_point = on_plane(dimension.base.text_middle_point);
    local.base.insertion_point = on_plane(dimension.base.insertion_point);
    local.base.normal = Vector3::UNIT_Z;
    Some((local, normal))
}

pub(crate) fn large_radial_dimension_in_ocs(
    dimension: &DimensionLargeRadial,
) -> Option<(DimensionLargeRadial, Vector3)> {
    let length = dimension.base.normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base.normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    local.definition_point = to_ocs(dimension.definition_point);
    let elevation = local.definition_point.z;
    let on_plane = |point: Vector3| {
        let mut point = to_ocs(point);
        point.z = elevation;
        point
    };
    local.chord_point = on_plane(dimension.chord_point);
    local.override_center = on_plane(dimension.override_center);
    local.jog_point = on_plane(dimension.jog_point);
    local.base.definition_point = local.definition_point;
    local.base.text_middle_point = on_plane(dimension.base.text_middle_point);
    local.base.insertion_point = on_plane(dimension.base.insertion_point);
    local.base.normal = Vector3::UNIT_Z;
    Some((local, normal))
}

/// A linear or aligned dimension in a plane off world Z, moved into its own
/// OCS. Their builders read the offset and extension directions in XY, so
/// they draw it there and the result is mapped back onto the plane. `None`
/// for a dimension in world XY or of another kind.
pub(crate) fn linear_dimension_in_ocs(dimension: &Dimension) -> Option<(Dimension, Vector3)> {
    let length = dimension.base().normal.length();
    if !length.is_finite() || length <= 1.0e-12 {
        return None;
    }
    let normal = dimension.base().normal / length;
    if (normal - Vector3::UNIT_Z).length() <= 1.0e-12 {
        return None;
    }
    let normal_tuple = (normal.x, normal.y, normal.z);
    let to_ocs = |point: Vector3| {
        let point = crate::scene::view::transform::wcs_point_to_ocs(
            (point.x, point.y, point.z),
            normal_tuple,
        );
        Vector3::new(point.0, point.1, point.2)
    };
    let mut local = dimension.clone();
    match &mut local {
        Dimension::Linear(d) => {
            d.first_point = to_ocs(d.first_point);
            d.second_point = to_ocs(d.second_point);
            d.definition_point = to_ocs(d.definition_point);
            d.base.definition_point = d.definition_point;
            d.base.text_middle_point = to_ocs(d.base.text_middle_point);
            d.base.normal = Vector3::UNIT_Z;
        }
        Dimension::Aligned(d) => {
            d.first_point = to_ocs(d.first_point);
            d.second_point = to_ocs(d.second_point);
            d.definition_point = to_ocs(d.definition_point);
            d.base.definition_point = d.definition_point;
            d.base.text_middle_point = to_ocs(d.base.text_middle_point);
            d.base.normal = Vector3::UNIT_Z;
        }
        _ => return None,
    }
    Some((local, normal))
}

pub(crate) fn map_wire_ocs_to_wcs(wire: &mut WireModel, normal: Vector3) {
    let normal_tuple = (normal.x, normal.y, normal.z);
    let map = |x: f64, y: f64, z: f64| {
        crate::scene::view::transform::ocs_point_to_wcs((x, y, z), normal_tuple)
    };
    let map_split = |high: &mut Vec<[f32; 3]>, low: &mut Vec<[f32; 3]>| {
        let mut mapped_high = Vec::with_capacity(high.len());
        let mut mapped_low = Vec::with_capacity(high.len());
        for (index, point) in high.iter().enumerate() {
            if point[0].is_nan() {
                mapped_high.push(*point);
                mapped_low.push([0.0; 3]);
                continue;
            }
            let residual = low.get(index).copied().unwrap_or([0.0; 3]);
            let point = map(
                point[0] as f64 + residual[0] as f64,
                point[1] as f64 + residual[1] as f64,
                point[2] as f64 + residual[2] as f64,
            );
            let (xh, xl) = WireModel::split_ds(point.0);
            let (yh, yl) = WireModel::split_ds(point.1);
            let (zh, zl) = WireModel::split_ds(point.2);
            mapped_high.push([xh, yh, zh]);
            mapped_low.push([xl, yl, zl]);
        }
        *high = mapped_high;
        *low = mapped_low;
    };

    let geom = wire.geom_mut();
    map_split(&mut geom.points, &mut geom.points_low);
    map_split(&mut geom.fill_tris, &mut geom.fill_tris_low);
    map_split(&mut geom.pick_tris, &mut geom.pick_tris_low);
    geom.text_verts = crate::scene::model::wire_model::map_text_verts(&geom.text_verts, map);
    // OCS→WCS is rigid: unit height scale, no in-plane rotation change.
    wire.map_searchable_runs(
        &|p| {
            let mapped = map(p[0], p[1], p[2]);
            [mapped.0, mapped.1, mapped.2]
        },
        1.0,
        0.0,
    );
    for (point, _) in &mut wire.snap_pts {
        let mapped = map(point.x, point.y, point.z);
        *point = DVec3::new(mapped.0, mapped.1, mapped.2);
    }
    for point in &mut wire.key_vertices {
        let mapped = map(point[0], point[1], point[2]);
        *point = [mapped.0, mapped.1, mapped.2];
    }
    if let Some(marker) = &mut wire.point_marker {
        let origin = map(marker.origin.x, marker.origin.y, marker.origin.z);
        let vector = |value: DVec3| {
            let mapped = map(value.x, value.y, value.z);
            DVec3::new(mapped.0, mapped.1, mapped.2)
        };
        marker.origin = DVec3::new(origin.0, origin.1, origin.2);
        marker.normal = vector(marker.normal).normalize_or(DVec3::Z);
        marker.axis_x = vector(marker.axis_x).normalize_or(DVec3::X);
        marker.axis_y = vector(marker.axis_y).normalize_or(DVec3::Y);
    }
    for tangent in &mut wire.tangent_geoms {
        match tangent {
            TangentGeom::Line { p1, p2 } => {
                let first = map(p1[0] as f64, p1[1] as f64, p1[2] as f64);
                let second = map(p2[0] as f64, p2[1] as f64, p2[2] as f64);
                *p1 = [first.0 as f32, first.1 as f32, first.2 as f32];
                *p2 = [second.0 as f32, second.1 as f32, second.2 as f32];
            }
            TangentGeom::Circle { center, radius } => {
                let center = map(center[0] as f64, center[1] as f64, center[2] as f64);
                let (x_axis, y_axis) = crate::scene::view::transform::ocs_axes(normal_tuple);
                *tangent = TangentGeom::PlanarCircle {
                    center: [center.0, center.1, center.2],
                    axis_x: [x_axis.0, x_axis.1, x_axis.2],
                    axis_y: [y_axis.0, y_axis.1, y_axis.2],
                    radius: *radius as f64,
                };
            }
            TangentGeom::PlanarCircle {
                center,
                axis_x,
                axis_y,
                ..
            }
            | TangentGeom::Arc {
                center,
                axis_x,
                axis_y,
                ..
            } => {
                let mapped = map(center[0], center[1], center[2]);
                let map_vector = |axis: [f64; 3]| {
                    let mapped = map(axis[0], axis[1], axis[2]);
                    [mapped.0, mapped.1, mapped.2]
                };
                *center = [mapped.0, mapped.1, mapped.2];
                *axis_x = map_vector(*axis_x);
                *axis_y = map_vector(*axis_y);
            }
            TangentGeom::PlanarEllipse {
                center,
                major_axis,
                normal,
                ..
            } => {
                let mapped = map(center[0], center[1], center[2]);
                let map_vector = |axis: [f64; 3]| {
                    let mapped = map(axis[0], axis[1], axis[2]);
                    [mapped.0, mapped.1, mapped.2]
                };
                *center = [mapped.0, mapped.1, mapped.2];
                *major_axis = map_vector(*major_axis);
                *normal = map_vector(*normal);
            }
        }
    }
    wire.aabb = WireModel::UNBOUNDED_AABB;
}

pub(crate) fn angular_dimension_frame(dim: &Dimension) -> Option<(Vec3, f32, f32, f32)> {
    let (vertex, start, end, arc_point) = match dim {
        Dimension::Angular2Ln(value) => {
            let first_start = vec3_local(value.first_point);
            let first_end = vec3_local(value.second_point);
            let second_start = vec3_local(value.angle_vertex);
            let second_end = vec3_local(value.definition_point);
            let arc_point = vec3_local(value.dimension_arc);
            let (vertex, start, end) =
                two_line_angle_frame(first_start, first_end, second_start, second_end, arc_point)?;
            (vertex, start, end, arc_point)
        }
        Dimension::Angular3Pt(value) => {
            let vertex = vec3_local(value.angle_vertex);
            let first = vec3_local(value.first_point);
            let second = vec3_local(value.second_point);
            let arc_point = vec3_local(value.definition_point);
            let (start, end, _) = three_point_frame(vertex, first, second, arc_point)?;
            (vertex, start, end, arc_point)
        }
        Dimension::Arc(value) => {
            let vertex = vec3_local(value.center_point);
            let arc_point = vec3_local(value.definition_point);
            let (start, end) = arc_dimension_angles(value)?;
            (vertex, start, end, arc_point)
        }
        _ => return None,
    };
    let radius = vertex.distance(arc_point);
    (radius > 1.0e-6).then_some((vertex, start, end, radius))
}

pub(crate) fn append_angular_dimension(
    g: &mut DimGeom,
    vertex: Vec3,
    first: Vec3,
    second: Vec3,
    arc_point: Vec3,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
    explicit_sweep: Option<(f32, f32)>,
    params: DimLineParams,
    suppress: SuppressFlags,
) {
    let radius = vertex.distance(arc_point);
    if radius <= 1e-6 {
        add_segment(&mut g.ext_lines, vertex, first);
        add_segment(&mut g.ext_lines, vertex, second);
        return;
    }
    // Extension lines run from each measured point out to the dimension arc so
    // there is no gap between a ray and its arc endpoint. (#181 / DIM-027)
    let measured_start = (first.y - vertex.y).atan2(first.x - vertex.x);
    let measured_end = (second.y - vertex.y).atan2(second.x - vertex.x);
    let (start, mut end) = explicit_sweep.unwrap_or((measured_start, measured_end));
    let dir1 = Vec3::new(start.cos(), start.sin(), 0.0);
    let dir2 = Vec3::new(end.cos(), end.sin(), 0.0);
    let arc_start = vertex + dir1 * radius;
    let arc_end = vertex + dir2 * radius;
    let extension = |origin: Vec3, endpoint: Vec3, direction: Vec3| {
        if params.dimfxlon {
            // Never past the DIMEXO gap at the measured point.
            let free = (endpoint.distance(origin) - params.dimexo).max(0.0);
            (
                endpoint - direction * params.dimfxl.max(0.0).min(free),
                endpoint + direction * params.dimexe,
            )
        } else {
            let toward_arc = normalized_or(endpoint - origin, direction);
            (
                origin + toward_arc * params.dimexo,
                endpoint + toward_arc * params.dimexe,
            )
        }
    };
    let (ext1_start, ext1_end) = extension(first, arc_start, dir1);
    let (ext2_start, ext2_end) = extension(second, arc_end, dir2);
    if !suppress.ext1 {
        add_segment(&mut g.ext_lines, ext1_start, ext1_end);
    }
    if !suppress.ext2 {
        add_segment(&mut g.ext_lines, ext2_start, ext2_end);
    }

    let mut delta = end - start;
    // Wrap a negative sweep forwards, but leave a zero one alone: two rays that
    // point the same way enclose no angle, and turning that into a full turn
    // drew a whole circle where there was nothing to draw.
    while delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    if delta.abs() <= 1e-6 {
        return;
    }
    if explicit_sweep.is_none() && delta > std::f32::consts::PI {
        end -= std::f32::consts::TAU;
        delta = end - start;
    }

    let arc_length = radius * delta.abs();
    let arrows_outside = if params.ticks || params.arrow_len <= 1.0e-6 {
        false
    } else if arc_length < params.arrow_len * 2.0 {
        true
    } else if arc_length < params.text_width + params.arrow_len * 2.0 {
        match params.dimatfit {
            0 | 1 => true,
            2 => false,
            _ => params.text_width <= arc_length,
        }
    } else {
        false
    };
    let draw_inside_line = !arrows_outside || params.dimtofl;
    let arc_extension = if params.ticks {
        params.dimdle / radius
    } else {
        0.0
    };
    let direction = delta.signum();
    let draw_start = start - direction * arc_extension;
    let draw_delta = delta + direction * arc_extension * 2.0;
    let arc_pts = sample_angular_arc(vertex, radius, draw_start, draw_delta);
    let steps = arc_pts.len().saturating_sub(1);
    if draw_inside_line {
        for index in 0..steps {
            let t = (index as f32 + 0.5) / steps as f32;
            if (t < 0.5 && suppress.dim1) || (t >= 0.5 && suppress.dim2) {
                continue;
            }
            let a = arc_pts[index];
            let b = arc_pts[index + 1];
            add_segment_with_text_break(&mut g.dim_lines, a, b, params.text_break);
        }
    }

    if arrows_outside && !params.dimsoxd {
        let stub_angle = (params.arrow_len * 2.0 / radius).min(std::f32::consts::FRAC_PI_2);
        if !suppress.dim1 {
            append_sampled_arc(
                &mut g.dim_lines,
                vertex,
                radius,
                start - direction * stub_angle,
                start,
            );
        }
        if !suppress.dim2 {
            append_sampled_arc(
                &mut g.dim_lines,
                vertex,
                radius,
                end,
                end + direction * stub_angle,
            );
        }
    }

    if let Some(text_break) = params.text_break {
        let text_angle = (text_break.center.y - vertex.y).atan2(text_break.center.x - vertex.x);
        let into = (text_angle - start).rem_euclid(std::f32::consts::TAU);
        if into > delta.abs() + 1.0e-6 {
            let back_to_start = (start - text_angle).rem_euclid(std::f32::consts::TAU);
            let forward_from_end = (text_angle - end).rem_euclid(std::f32::consts::TAU);
            if back_to_start <= forward_from_end && !suppress.dim1 {
                append_sampled_arc_with_text_break(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    text_angle,
                    start,
                    params.text_break,
                );
            } else if !suppress.dim2 {
                append_sampled_arc_with_text_break(
                    &mut g.dim_lines,
                    vertex,
                    radius,
                    end,
                    text_angle,
                    params.text_break,
                );
            }
        }
    }

    let start_tangent = Vec3::new(-start.sin(), start.cos(), 0.0) * direction;
    let end_tangent = Vec3::new(-end.sin(), end.cos(), 0.0) * direction;
    let draw_arrows = !arrows_outside || !params.dimsoxd;
    if draw_arrows && !suppress.dim1 {
        append_dim_arrow(
            g,
            arc_start,
            if arrows_outside {
                -start_tangent
            } else {
                start_tangent
            },
            arrow1,
        );
    }
    if draw_arrows && !suppress.dim2 {
        append_dim_arrow(
            g,
            arc_end,
            if arrows_outside {
                end_tangent
            } else {
                -end_tangent
            },
            arrow2,
        );
    }
}

pub(crate) fn append_sampled_arc(
    lines: &mut Vec<[f32; 3]>,
    vertex: Vec3,
    radius: f32,
    start: f32,
    end: f32,
) {
    for pair in sample_angular_arc(vertex, radius, start, end - start).windows(2) {
        add_segment(lines, pair[0], pair[1]);
    }
}

pub(crate) fn append_sampled_arc_with_text_break(
    lines: &mut Vec<[f32; 3]>,
    vertex: Vec3,
    radius: f32,
    start: f32,
    end: f32,
    text_break: Option<TextBreak>,
) {
    for pair in sample_angular_arc(vertex, radius, start, end - start).windows(2) {
        add_segment_with_text_break(lines, pair[0], pair[1], text_break);
    }
}

pub(crate) fn sample_angular_arc(vertex: Vec3, radius: f32, start: f32, sweep: f32) -> Vec<Vec3> {
    if sweep.abs() <= 1.0e-6 {
        return vec![vertex + Vec3::new(start.cos() * radius, start.sin() * radius, 0.0)];
    }
    let end = start + sweep;
    let (from, to, reverse) = if sweep >= 0.0 {
        (start, end, false)
    } else {
        (end, start, true)
    };
    let mut points: Vec<_> = ocs_geom::geom2d::tessellate::arc(
        [vertex.x as f64, vertex.y as f64],
        radius as f64,
        from as f64,
        to as f64,
        vertex.z as f64,
        ocs_geom::geom2d::tessellate::DEFAULT_SEGMENTS_PER_RADIAN,
    )
    .into_iter()
    .map(|point| Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32))
    .collect();
    if reverse {
        points.reverse();
    }
    points
}

pub(crate) fn dimension_snap_pts(dim: &Dimension) -> Vec<(glam::DVec3, SnapHint)> {
    let lv = |v: document::types::Vector3| glam::DVec3::new(v.x, v.y, v.z);
    let node = |v: document::types::Vector3| (lv(v), SnapHint::Node);
    match dim {
        Dimension::Linear(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Aligned(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Radius(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Diameter(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Angular2Ln(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.angle_vertex),
            node(d.definition_point),
            node(d.dimension_arc),
        ],
        Dimension::Angular3Pt(d) => vec![
            node(d.angle_vertex),
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Ordinate(d) => vec![node(d.feature_location), node(d.leader_endpoint)],
        Dimension::Arc(d) => {
            let mut points = vec![
                node(d.center_point),
                node(d.first_extension_point),
                node(d.second_extension_point),
                node(d.definition_point),
            ];
            if d.has_leader {
                points.push(node(d.first_leader_point));
                points.push(node(d.second_leader_point));
            }
            points
        }
        Dimension::LargeRadial(d) => vec![
            node(d.definition_point),
            node(d.chord_point),
            node(d.override_center),
            node(d.jog_point),
        ],
    }
}
