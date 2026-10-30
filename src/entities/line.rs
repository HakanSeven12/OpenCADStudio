use crate::t;
use codec::{entities::Line, Entity};

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, format_direction, format_length, oriented_triangle_grip,
    parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::TangentGeom;

fn to_render(line: &Line) -> RenderEntity {
    if let Some(association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        // As the stored block lines: the cross (and the detached badge) is
        // Continuous, and each extension is a line of its own in the object's
        // linetype, running along the mark's axes and aligned at its ends as
        // a single line is.
        let axes = crate::scene::centermark::dvec(association.plane_x)
            + crate::scene::centermark::dvec(association.plane_y);
        let segments: Vec<_> = crate::scene::centermark::render_segments(&association)
            .into_iter()
            .map(|[a, b]| if (b - a).dot(axes) < 0.0 { [b, a] } else { [a, b] })
            .collect();
        let extensions = 2..crate::scene::centermark::mark_segments(&association).len();
        let mut points = Vec::with_capacity(segments.len() * 3);
        let mut solid = Vec::with_capacity(segments.len() * 3);
        let mut key_vertices = Vec::with_capacity(segments.len() * 2 + 1);
        let mut tangent_geoms = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            let target = if extensions.contains(&index) {
                &mut points
            } else {
                &mut solid
            };
            if !target.is_empty() {
                target.push([f64::NAN; 3]);
            }
            for point in segment {
                target.push([point.x, point.y, point.z]);
                key_vertices.push([point.x, point.y, point.z]);
            }
            tangent_geoms.push(TangentGeom::Line {
                p1: [
                    segment[0].x as f32,
                    segment[0].y as f32,
                    segment[0].z as f32,
                ],
                p2: [
                    segment[1].x as f32,
                    segment[1].y as f32,
                    segment[1].z as f32,
                ],
            });
        }
        let center = crate::scene::centermark::dvec(association.center);
        key_vertices.push([center.x, center.y, center.z]);
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::BoundaryLines {
                points,
                stations: Vec::new(),
                point_segments: Vec::new(),
                station_pieces: Vec::new(),
                source_length: 0.0,
                plinegen: true,
                separate_runs: true,
                solid,
            },
            snap_pts: Vec::new(),
            tangent_geoms,
            key_vertices,
            fill_tris: Vec::new(),
        };
    }
    // LINE endpoints are stored in WCS — unlike the planar OCS entities
    // (ARC/CIRCLE/LWPOLYLINE/TEXT), the extrusion normal on a LINE only
    // orients its thickness sweep. Remapping the endpoints through the
    // arbitrary-axis OCS mirrored every line carried over from a MIRROR
    // (normal 0,0,-1) to the wrong side of the drawing.
    let normal = (line.normal.x, line.normal.y, line.normal.z);
    let (sx, sy, sz) = (line.start.x, line.start.y, line.start.z);
    let (ex, ey, ez) = (line.end.x, line.end.y, line.end.z);
    let kv: Vec<[f64; 3]> = vec![[sx, sy, sz], [ex, ey, ez]];
    let tangent = TangentGeom::Line {
        p1: [kv[0][0] as f32, kv[0][1] as f32, kv[0][2] as f32],
        p2: [kv[1][0] as f32, kv[1][1] as f32, kv[1][2] as f32],
    };

    if line.thickness.abs() > 1e-10 {
        let t = line.thickness;
        let (nx, ny, nz) = normal;
        let p0t = [sx + t * nx, sy + t * ny, sz + t * nz];
        let p1t = [ex + t * nx, ey + t * ny, ez + t * nz];
        let pts: Vec<[f64; 3]> = vec![
            kv[0],
            kv[1],
            [f64::NAN; 3],
            p0t,
            p1t,
            [f64::NAN; 3],
            kv[0],
            p0t,
            [f64::NAN; 3],
            kv[1],
            p1t,
        ];
        return RenderEntity {
            pick_tris: extrusion_wall_tris(&kv, [t * nx, t * ny, t * nz]),
            object: RenderObject::Lines(pts),
            snap_pts: vec![],
            tangent_geoms: vec![tangent],
            key_vertices: kv,
            fill_tris: vec![],
        };
    }

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(kv.clone()),
        snap_pts: vec![],
        tangent_geoms: vec![tangent],
        key_vertices: kv,
        fill_tris: vec![],
    }
}

fn grips(line: &Line) -> Vec<GripDef> {
    if let Some(association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        let center = crate::scene::centermark::dvec(association.center);
        let directions = crate::scene::centermark::mark_directions(&association);
        let mut result = vec![center_grip(0, center)];
        let extension_start = association.cross_size * 0.5 + association.cross_gap;
        if !association.show_extensions || association.radius <= extension_start {
            return result;
        }
        for (index, direction) in directions.iter().enumerate() {
            let distance = (association.radius + association.length_adjustments[index]).max(0.0);
            result.push(square_grip(index + 1, center + *direction * distance));
        }
        for (index, direction) in directions.iter().enumerate() {
            let distance = (association.radius
                + association.extension_length
                + association.length_adjustments[index]
                + association.overshoots[index])
                .max(extension_start);
            result.push(oriented_triangle_grip(
                index + 5,
                center + *direction * distance,
                *direction,
            ));
        }
        return result;
    }
    let s = glam::DVec3::new(line.start.x, line.start.y, line.start.z);
    let e = glam::DVec3::new(line.end.x, line.end.y, line.end.z);
    let m = (s + e) * 0.5;
    if let Some(association) =
        codec::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        let direction = (e - s).normalize_or(glam::DVec3::X);
        let base_start = s + direction * association.start_extension;
        let base_end = e - direction * association.end_extension;
        return vec![
            square_grip(0, base_start),
            square_grip(1, base_end),
            center_grip(2, m),
            oriented_triangle_grip(3, s, -direction),
            oriented_triangle_grip(4, e, direction),
        ];
    }
    vec![square_grip(0, s), square_grip(1, e), center_grip(2, m)]
}

/// A centre mark's cross size or gap as the reference shows it: a factor
/// of the diameter (`0.1x`) or a length.
fn center_measure_text(value: f64, relative: bool, radius: f64) -> String {
    let diameter = radius * 2.0;
    if relative && diameter > 1.0e-12 {
        let factor = ((value / diameter) * 1.0e6).round() / 1.0e6;
        format!("{factor}x")
    } else {
        format_length(value)
    }
}

/// Mark directions as the reference lists them (left, right, top, bottom),
/// as indexes into the carrier's `[+x, -x, +y, -y]`.
const MARK_EXTENSIONS: [(&str, &str, usize); 4] = [
    ("Left extension", "centermark_left_extension", 1),
    ("Right extension", "centermark_right_extension", 0),
    ("Top extension", "centermark_top_extension", 2),
    ("Bottom extension", "centermark_bottom_extension", 3),
];

fn properties(line: &Line) -> Vec<PropSection> {
    use crate::scene::model::object::{PropValue, Property};
    if let Some(association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        let mut geometry = vec![
            Property {
                label: t!("Cross size").into_owned(),
                field: "centermark_cross_size",
                value: PropValue::EditText(center_measure_text(
                    association.cross_size,
                    association.cross_size_relative,
                    association.radius,
                )),
            },
            Property {
                label: t!("Cross gap").into_owned(),
                field: "centermark_cross_gap",
                value: PropValue::EditText(center_measure_text(
                    association.cross_gap,
                    association.cross_gap_relative,
                    association.radius,
                )),
            },
        ];
        for (label, field, index) in MARK_EXTENSIONS {
            geometry.push(edit(
                t!(label).as_ref(),
                field,
                association.extension_length + association.overshoots[index],
            ));
        }
        geometry.push(Property {
            label: t!("Show extension").into_owned(),
            field: "centermark_show_extension",
            value: PropValue::Choice {
                selected: if association.show_extensions { t!("Yes") } else { t!("No") }
                    .into_owned(),
                options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
            },
        });
        let x = crate::scene::centermark::dvec(association.plane_x);
        return vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: geometry,
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(t!("Type").as_ref(), "center_type", t!("Center Mark").into_owned()),
                    crate::entities::common::edit_angle_prop(
                        t!("Rotation").as_ref(),
                        "centermark_rotation",
                        x.y.atan2(x.x).to_degrees(),
                    ),
                    ro(t!("Annotative").as_ref(), "annotative", t!("No").into_owned()),
                ],
            },
        ];
    }
    if let Some(association) =
        codec::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        return vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    ro(t!("Start X").as_ref(), "start_x", format_length(line.start.x)),
                    ro(t!("Start Y").as_ref(), "start_y", format_length(line.start.y)),
                    ro(t!("Start Z").as_ref(), "start_z", format_length(line.start.z)),
                    ro(t!("End X").as_ref(), "end_x", format_length(line.end.x)),
                    ro(t!("End Y").as_ref(), "end_y", format_length(line.end.y)),
                    ro(t!("End Z").as_ref(), "end_z", format_length(line.end.z)),
                    edit(
                        t!("Start extension").as_ref(),
                        "centerline_start_extension",
                        association.start_extension,
                    ),
                    edit(
                        t!("End extension").as_ref(),
                        "centerline_end_extension",
                        association.end_extension,
                    ),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(t!("Type").as_ref(), "center_type", t!("Centerline").into_owned()),
                    ro(t!("Annotative").as_ref(), "annotative", t!("No").into_owned()),
                ],
            },
        ];
    }
    let dx = line.end.x - line.start.x;
    let dy = line.end.y - line.start.y;
    let dz = line.end.z - line.start.z;
    let angle = dy.atan2(dx);
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Start X").as_ref(), "start_x", line.start.x),
            edit(t!("Start Y").as_ref(), "start_y", line.start.y),
            edit(t!("Start Z").as_ref(), "start_z", line.start.z),
            edit(t!("End X").as_ref(), "end_x", line.end.x),
            edit(t!("End Y").as_ref(), "end_y", line.end.y),
            edit(t!("End Z").as_ref(), "end_z", line.end.z),
            ro(t!("Delta X").as_ref(), "delta_x", format_length(dx)),
            ro(t!("Delta Y").as_ref(), "delta_y", format_length(dy)),
            ro(t!("Delta Z").as_ref(), "delta_z", format_length(dz)),
            ro(
                t!("Length").as_ref(),
                "length",
                format_length(line.length()),
            ),
            ro(t!("Angle").as_ref(), "angle", format_direction(angle)),
            edit(t!("Normal X").as_ref(), "normal_x", line.normal.x),
            edit(t!("Normal Y").as_ref(), "normal_y", line.normal.y),
            edit(t!("Normal Z").as_ref(), "normal_z", line.normal.z),
        ],
    }]
}

fn apply_geom_prop(line: &mut Line, field: &str, value: &str) {
    if let Some(mut association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        match field {
            "centermark_show_extension" => {
                association.show_extensions = matches!(
                    value.trim().to_lowercase().as_str(),
                    "yes" | "on" | "true" | "1" | "evet"
                ) || value.trim() == t!("Yes").as_ref();
            }
            "centermark_cross_size" | "centermark_cross_gap" => {
                let text = value.trim();
                let diameter = association.radius * 2.0;
                let (number, relative) = match text.strip_suffix(['x', 'X']) {
                    Some(factor) => match parse_f64(factor) {
                        Some(factor) => (factor * diameter, true),
                        None => return,
                    },
                    None => match parse_f64(text) {
                        Some(length) => (length, false),
                        None => return,
                    },
                };
                if !number.is_finite() || number <= 0.0 {
                    return;
                }
                if field == "centermark_cross_size" {
                    association.cross_size = number;
                    association.cross_size_relative = relative;
                } else {
                    association.cross_gap = number;
                    association.cross_gap_relative = relative;
                }
            }
            "centermark_rotation" => {
                let Some(degrees) = parse_f64(value) else {
                    return;
                };
                let x = crate::scene::centermark::dvec(association.plane_x)
                    .normalize_or(glam::DVec3::X);
                let y = crate::scene::centermark::dvec(association.plane_y)
                    .normalize_or(glam::DVec3::Y);
                let normal = x.cross(y).normalize_or(glam::DVec3::Z);
                let turn = degrees.to_radians() - x.y.atan2(x.x);
                let rotation = glam::DQuat::from_axis_angle(normal, turn);
                let (x, y) = (rotation * x, rotation * y);
                association.plane_x = codec::types::Vector3::new(x.x, x.y, x.z);
                association.plane_y = codec::types::Vector3::new(y.x, y.y, y.z);
            }
            field => {
                let Some((_, _, index)) = MARK_EXTENSIONS
                    .iter()
                    .find(|(_, name, _)| *name == field)
                    .copied()
                else {
                    return;
                };
                let Some(number) = parse_f64(value).filter(|number| number.is_finite()) else {
                    return;
                };
                association.overshoots[index] = number - association.extension_length;
            }
        }
        crate::scene::centermark::update_carrier(line, &association);
        return;
    }
    let Some(v) = parse_f64(value) else {
        return;
    };
    if let Some(mut association) =
        codec::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        if !v.is_finite() || v < 0.0 {
            return;
        }
        let start = glam::DVec3::new(line.start.x, line.start.y, line.start.z);
        let end = glam::DVec3::new(line.end.x, line.end.y, line.end.z);
        let direction = (end - start).normalize_or(glam::DVec3::X);
        match field {
            "centerline_start_extension" => {
                let delta = v - association.start_extension;
                let moved = start - direction * delta;
                line.start = codec::types::Vector3::new(moved.x, moved.y, moved.z);
                association.start_extension = v;
            }
            "centerline_end_extension" => {
                let delta = v - association.end_extension;
                let moved = end + direction * delta;
                line.end = codec::types::Vector3::new(moved.x, moved.y, moved.z);
                association.end_extension = v;
            }
            _ => return,
        }
        association.write(&mut line.common.extended_data);
        return;
    }
    match field {
        "start_x" => line.start.x = v,
        "start_y" => line.start.y = v,
        "start_z" => line.start.z = v,
        "end_x" => line.end.x = v,
        "end_y" => line.end.y = v,
        "end_z" => line.end.z = v,
        "normal_x" | "normal_y" | "normal_z" => {
            let mut normal = glam::DVec3::new(line.normal.x, line.normal.y, line.normal.z);
            match field {
                "normal_x" => normal.x = v,
                "normal_y" => normal.y = v,
                "normal_z" => normal.z = v,
                _ => unreachable!(),
            }
            if normal.is_finite() && normal.length_squared() > 1.0e-18 {
                let normal = normal.normalize();
                line.normal = codec::types::Vector3::new(normal.x, normal.y, normal.z);
            }
        }
        _ => {}
    }
}

fn apply_grip(line: &mut Line, grip_id: usize, apply: GripApply) {
    if let Some(mut association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        let center = crate::scene::centermark::dvec(association.center);
        let directions = crate::scene::centermark::mark_directions(&association);
        match (grip_id, apply) {
            (0, GripApply::Translate(delta)) => {
                let moved = center + delta;
                association.center = codec::types::Vector3::new(moved.x, moved.y, moved.z);
                let origin = crate::scene::centermark::dvec(association.plane_origin) + delta;
                association.plane_origin = codec::types::Vector3::new(origin.x, origin.y, origin.z);
                association.associated = false;
            }
            (id @ 1..=4, GripApply::Absolute(point)) => {
                let index = id - 1;
                let distance = (point - center).dot(directions[index]).max(0.0);
                association.length_adjustments[index] = distance - association.radius;
            }
            (id @ 5..=8, GripApply::Absolute(point)) => {
                let index = id - 5;
                let distance = (point - center).dot(directions[index]).max(0.0);
                association.overshoots[index] = distance
                    - association.radius
                    - association.extension_length
                    - association.length_adjustments[index];
            }
            _ => return,
        }
        crate::scene::centermark::update_carrier(line, &association);
        return;
    }
    if let Some(mut association) =
        codec::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        let start = glam::DVec3::new(line.start.x, line.start.y, line.start.z);
        let end = glam::DVec3::new(line.end.x, line.end.y, line.end.z);
        let direction = (end - start).normalize_or(glam::DVec3::X);
        let base_start = start + direction * association.start_extension;
        let base_end = end - direction * association.end_extension;
        match (grip_id, apply) {
            (0, GripApply::Absolute(point)) => {
                let delta = (point - base_start).dot(direction);
                association.start_length_adjustment -= delta;
                let moved = start + direction * delta;
                line.start = codec::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            (1, GripApply::Absolute(point)) => {
                let delta = (point - base_end).dot(direction);
                association.end_length_adjustment += delta;
                let moved = end + direction * delta;
                line.end = codec::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            (2, GripApply::Translate(delta)) => {
                line.start.x += delta.x;
                line.start.y += delta.y;
                line.start.z += delta.z;
                line.end.x += delta.x;
                line.end.y += delta.y;
                line.end.z += delta.z;
                association.associated = false;
            }
            (3, GripApply::Absolute(point)) => {
                let total = (base_start - point).dot(direction).max(0.0);
                association.start_extension = total;
                let moved = base_start - direction * association.start_extension;
                line.start = codec::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            (4, GripApply::Absolute(point)) => {
                let total = (point - base_end).dot(direction).max(0.0);
                association.end_extension = total;
                let moved = base_end + direction * association.end_extension;
                line.end = codec::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            _ => return,
        }
        association.write(&mut line.common.extended_data);
        return;
    }
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            line.start.x = p.x as f64;
            line.start.y = p.y as f64;
            line.start.z = p.z as f64;
        }
        (1, GripApply::Absolute(p)) => {
            line.end.x = p.x as f64;
            line.end.y = p.y as f64;
            line.end.z = p.z as f64;
        }
        (2, GripApply::Translate(d)) => {
            line.start.x += d.x as f64;
            line.start.y += d.y as f64;
            line.start.z += d.z as f64;
            line.end.x += d.x as f64;
            line.end.y += d.y as f64;
            line.end.z += d.z as f64;
        }
        _ => {}
    }
}

fn apply_plain_transform(line: &mut Line, t: &EntityTransform) {
    match t {
        EntityTransform::Translate(d) => {
            line.translate(codec::types::Vector3::new(d.x, d.y, d.z));
        }
        EntityTransform::Rotate {
            center,
            axis,
            angle_rad,
        } => {
            crate::scene::view::transform::apply_standard_transform(
                line, *center, *axis, *angle_rad,
            );
        }
        EntityTransform::Scale { center, factor } => {
            crate::scene::view::transform::apply_standard_scale(line, *center, *factor);
        }
        EntityTransform::Mirror {
            p1,
            p2,
            working_normal,
        } => {
            codec::Entity::apply_transform(
                line,
                &crate::scene::view::transform::reflection_about_working_line(
                    *p1,
                    *p2,
                    *working_normal,
                ),
            );
        }
        EntityTransform::Affine(transform) => {
            codec::Entity::apply_transform(line, transform);
        }
    }
}

fn apply_transform(line: &mut Line, t: &EntityTransform) {
    if let Some(mut association) =
        codec::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        if let EntityTransform::Translate(delta) = t {
            let center = crate::scene::centermark::dvec(association.center) + *delta;
            association.center = codec::types::Vector3::new(center.x, center.y, center.z);
            let origin = crate::scene::centermark::dvec(association.plane_origin) + *delta;
            association.plane_origin = codec::types::Vector3::new(origin.x, origin.y, origin.z);
            association.associated = false;
            crate::scene::centermark::update_carrier(line, &association);
            return;
        }
        let center = crate::scene::centermark::dvec(association.center);
        let x = crate::scene::centermark::dvec(association.plane_x).normalize_or(glam::DVec3::X);
        let y = crate::scene::centermark::dvec(association.plane_y).normalize_or(glam::DVec3::Y);
        let mut x_basis = Line::from_points(
            codec::types::Vector3::new(center.x, center.y, center.z),
            codec::types::Vector3::new(center.x + x.x, center.y + x.y, center.z + x.z),
        );
        let mut y_basis = Line::from_points(
            codec::types::Vector3::new(center.x, center.y, center.z),
            codec::types::Vector3::new(center.x + y.x, center.y + y.y, center.z + y.z),
        );
        apply_plain_transform(&mut x_basis, t);
        apply_plain_transform(&mut y_basis, t);
        let moved = glam::DVec3::new(x_basis.start.x, x_basis.start.y, x_basis.start.z);
        let moved_x = glam::DVec3::new(x_basis.end.x, x_basis.end.y, x_basis.end.z) - moved;
        let moved_y = glam::DVec3::new(y_basis.end.x, y_basis.end.y, y_basis.end.z) - moved;
        let scale = ((moved_x.length() + moved_y.length()) * 0.5).max(1.0e-12);
        association.center = codec::types::Vector3::new(moved.x, moved.y, moved.z);
        association.plane_origin = association.center;
        association.plane_x = codec::types::Vector3::new(
            moved_x.normalize_or(x).x,
            moved_x.normalize_or(x).y,
            moved_x.normalize_or(x).z,
        );
        association.plane_y = codec::types::Vector3::new(
            moved_y.normalize_or(y).x,
            moved_y.normalize_or(y).y,
            moved_y.normalize_or(y).z,
        );
        association.radius *= scale;
        association.cross_size *= scale;
        association.cross_gap *= scale;
        association.extension_length *= scale;
        for value in association
            .length_adjustments
            .iter_mut()
            .chain(association.overshoots.iter_mut())
        {
            *value *= scale;
        }
        association.associated = false;
        crate::scene::centermark::update_carrier(line, &association);
        return;
    }
    if let Some(mut association) =
        codec::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        association.associated = false;
        association.write(&mut line.common.extended_data);
    }
    apply_plain_transform(line, t);
}

impl RenderConvertible for Line {
    fn to_render(&self, _document: &codec::Document) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

impl crate::entities::traits::Grippable for Line {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        if codec::entities::CenterMarkAssociation::read(&self.common.extended_data).is_some()
            || codec::entities::CenterLineAssociation::read(&self.common.extended_data).is_some()
        {
            return vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }];
        }
        if grip_id == 2 {
            vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }]
        } else {
            vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Lengthen",
                    action: GripMenuAction::Lengthen,
                },
            ]
        }
    }
    fn apply_grip_menu(
        &mut self,
        _grip_id: usize,
        _action: crate::scene::model::object::GripMenuAction,
    ) {
        // Lengthen needs a follow-up distance — handled by
        // `apply_grip_menu_value`.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::Lengthen => Some("Distance"),
            _ => None,
        }
    }

    fn grip_menu_point_value(
        &self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        point: glam::DVec3,
    ) -> Option<f64> {
        use crate::scene::model::object::GripMenuAction as A;
        if !matches!(action, A::Lengthen) {
            return None;
        }
        let direction = glam::DVec3::new(
            self.end.x - self.start.x,
            self.end.y - self.start.y,
            self.end.z - self.start.z,
        );
        let length = direction.length();
        if length < 1.0e-12 {
            return None;
        }
        let unit = direction / length;
        let value = match grip_id {
            0 => (glam::DVec3::new(self.start.x, self.start.y, self.start.z) - point).dot(unit),
            1 => (point - glam::DVec3::new(self.end.x, self.end.y, self.end.z)).dot(unit),
            _ => return None,
        };
        (length + value > 1.0e-9).then_some(value)
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        if !matches!(action, A::Lengthen) {
            return;
        }
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let dz = self.end.z - self.start.z;
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        if len < 1e-12 {
            return;
        }
        let (ux, uy, uz) = (dx / len, dy / len, dz / len);
        match grip_id {
            0 => {
                // Move start endpoint backward along the line by `value`
                // (positive = lengthen; negative = shorten).
                self.start.x -= ux * value;
                self.start.y -= uy * value;
                self.start.z -= uz * value;
            }
            1 => {
                self.end.x += ux * value;
                self.end.y += uy * value;
                self.end.z += uz * value;
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for Line {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for Line {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

impl crate::entities::traits::MassPropsCalc for codec::entities::Line {
    fn mass_props(&self) -> crate::entities::traits::MassProps {
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len = (dx * dx + dy * dy).sqrt();
        crate::entities::traits::MassProps {
            area: 0.0,
            perimeter: len,
            cx: (self.start.x + self.end.x) / 2.0,
            cy: (self.start.y + self.end.y) / 2.0,
        }
    }
}
