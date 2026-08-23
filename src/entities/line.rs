use acadrust::{entities::Line, Entity};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, oriented_triangle_grip, parse_f64, ro_prop as ro,
    square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::TangentGeom;

fn to_render(line: &Line) -> RenderEntity {
    if let Some(association) =
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        let segments = crate::scene::centermark::render_segments(&association);
        let mut points = Vec::with_capacity(segments.len() * 3);
        let mut key_vertices = Vec::with_capacity(segments.len() * 2 + 1);
        let mut tangent_geoms = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                points.push([f64::NAN; 3]);
            }
            for point in segment {
                points.push([point.x, point.y, point.z]);
                key_vertices.push([point.x, point.y, point.z]);
            }
            tangent_geoms.push(TangentGeom::Line {
                p1: [segment[0].x as f32, segment[0].y as f32, segment[0].z as f32],
                p2: [segment[1].x as f32, segment[1].y as f32, segment[1].z as f32],
            });
        }
        let center = crate::scene::centermark::dvec(association.center);
        key_vertices.push([center.x, center.y, center.z]);
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(points),
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
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
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
            result.push(oriented_triangle_grip(index + 5, center + *direction * distance, *direction));
        }
        return result;
    }
    let s = glam::DVec3::new(line.start.x, line.start.y, line.start.z);
    let e = glam::DVec3::new(line.end.x, line.end.y, line.end.z);
    let m = (s + e) * 0.5;
    if let Some(association) =
        acadrust::entities::CenterLineAssociation::read(&line.common.extended_data)
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

fn properties(line: &Line) -> Vec<PropSection> {
    if let Some(association) =
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        use crate::scene::model::object::{PropValue, Property};
        return vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                Property {
                    label: "Show extension".to_owned(),
                    field: "centermark_show_extension",
                    value: PropValue::Choice {
                        selected: if association.show_extensions { "Yes" } else { "No" }.to_owned(),
                        options: vec!["Yes".to_owned(), "No".to_owned()],
                    },
                },
                edit("Cross size", "centermark_cross_size", association.cross_size),
                edit("Cross gap", "centermark_cross_gap", association.cross_gap),
                edit(
                    "Extension length",
                    "centermark_extension_length",
                    association.extension_length,
                ),
                ro(
                    "Associative",
                    "centermark_associative",
                    if association.associated { "Yes" } else { "No" },
                ),
            ],
        }];
    }
    if let Some(association) =
        acadrust::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        return vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(
                    "Start extension",
                    "centerline_start_extension",
                    association.start_extension,
                ),
                edit(
                    "End extension",
                    "centerline_end_extension",
                    association.end_extension,
                ),
                ro(t!("Length").as_ref(), "length", format!("{:.4}", line.length())),
                ro(
                    "Associative",
                    "centerline_associative",
                    if association.associated { "Yes" } else { "No" },
                ),
            ],
        }];
    }
    let dx = line.end.x - line.start.x;
    let dy = line.end.y - line.start.y;
    let dz = line.end.z - line.start.z;
    let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Start X").as_ref(), "start_x", line.start.x),
            edit(t!("Start Y").as_ref(), "start_y", line.start.y),
            edit(t!("Start Z").as_ref(), "start_z", line.start.z),
            edit(t!("End X").as_ref(), "end_x", line.end.x),
            edit(t!("End Y").as_ref(), "end_y", line.end.y),
            edit(t!("End Z").as_ref(), "end_z", line.end.z),
            ro(t!("Delta X").as_ref(), "delta_x", format!("{dx:.4}")),
            ro(t!("Delta Y").as_ref(), "delta_y", format!("{dy:.4}")),
            ro(t!("Delta Z").as_ref(), "delta_z", format!("{dz:.4}")),
            ro(t!("Length").as_ref(), "length", format!("{:.4}", line.length())),
            ro(t!("Angle").as_ref(), "angle", format!("{angle:.2}")),
        ],
    }]
}

fn apply_geom_prop(line: &mut Line, field: &str, value: &str) {
    if let Some(mut association) =
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        match field {
            "centermark_show_extension" => {
                association.show_extensions = matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "yes" | "on" | "true" | "1"
                );
            }
            "centermark_cross_size" | "centermark_cross_gap" | "centermark_extension_length" => {
                let Some(number) = parse_f64(value) else { return; };
                if !number.is_finite() || number < 0.0 { return; }
                match field {
                    "centermark_cross_size" => association.cross_size = number,
                    "centermark_cross_gap" => association.cross_gap = number,
                    "centermark_extension_length" => association.extension_length = number,
                    _ => unreachable!(),
                }
                if field == "centermark_cross_size" {
                    association.cross_size_relative = false;
                } else if field == "centermark_cross_gap" {
                    association.cross_gap_relative = false;
                }
            }
            _ => return,
        }
        crate::scene::centermark::update_carrier(line, &association);
        return;
    }
    let Some(v) = parse_f64(value) else {
        return;
    };
    if let Some(mut association) =
        acadrust::entities::CenterLineAssociation::read(&line.common.extended_data)
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
                line.start = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
                association.start_extension = v;
            }
            "centerline_end_extension" => {
                let delta = v - association.end_extension;
                let moved = end + direction * delta;
                line.end = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
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
        _ => {}
    }
}

fn apply_grip(line: &mut Line, grip_id: usize, apply: GripApply) {
    if let Some(mut association) =
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        let center = crate::scene::centermark::dvec(association.center);
        let directions = crate::scene::centermark::mark_directions(&association);
        match (grip_id, apply) {
            (0, GripApply::Translate(delta)) => {
                let moved = center + delta;
                association.center = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
                let origin = crate::scene::centermark::dvec(association.plane_origin) + delta;
                association.plane_origin = acadrust::types::Vector3::new(origin.x, origin.y, origin.z);
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
        acadrust::entities::CenterLineAssociation::read(&line.common.extended_data)
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
                line.start = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            (1, GripApply::Absolute(point)) => {
                let delta = (point - base_end).dot(direction);
                association.end_length_adjustment += delta;
                let moved = end + direction * delta;
                line.end = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
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
                line.start = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
            }
            (4, GripApply::Absolute(point)) => {
                let total = (point - base_end).dot(direction).max(0.0);
                association.end_extension = total;
                let moved = base_end + direction * association.end_extension;
                line.end = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
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
            line.translate(acadrust::types::Vector3::new(d.x, d.y, d.z));
        }
        EntityTransform::Rotate { center, axis, angle_rad } => {
            crate::scene::view::transform::apply_standard_transform(line, *center, *axis, *angle_rad);
        }
        EntityTransform::Scale { center, factor } => {
            crate::scene::view::transform::apply_standard_scale(line, *center, *factor);
        }
        EntityTransform::Mirror { p1, p2, working_normal } => {
            acadrust::Entity::apply_transform(
                line,
                &crate::scene::view::transform::reflection_about_working_line(
                    *p1, *p2, *working_normal,
                ),
            );
        }
        EntityTransform::Affine(transform) => {
            acadrust::Entity::apply_transform(line, transform);
        }
    }
}

fn apply_transform(line: &mut Line, t: &EntityTransform) {
    if let Some(mut association) =
        acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data)
    {
        if let EntityTransform::Translate(delta) = t {
            let center = crate::scene::centermark::dvec(association.center) + *delta;
            association.center = acadrust::types::Vector3::new(center.x, center.y, center.z);
            let origin = crate::scene::centermark::dvec(association.plane_origin) + *delta;
            association.plane_origin = acadrust::types::Vector3::new(origin.x, origin.y, origin.z);
            association.associated = false;
            crate::scene::centermark::update_carrier(line, &association);
            return;
        }
        let center = crate::scene::centermark::dvec(association.center);
        let x = crate::scene::centermark::dvec(association.plane_x).normalize_or(glam::DVec3::X);
        let y = crate::scene::centermark::dvec(association.plane_y).normalize_or(glam::DVec3::Y);
        let mut x_basis = Line::from_points(
            acadrust::types::Vector3::new(center.x, center.y, center.z),
            acadrust::types::Vector3::new(center.x + x.x, center.y + x.y, center.z + x.z),
        );
        let mut y_basis = Line::from_points(
            acadrust::types::Vector3::new(center.x, center.y, center.z),
            acadrust::types::Vector3::new(center.x + y.x, center.y + y.y, center.z + y.z),
        );
        apply_plain_transform(&mut x_basis, t);
        apply_plain_transform(&mut y_basis, t);
        let moved = glam::DVec3::new(x_basis.start.x, x_basis.start.y, x_basis.start.z);
        let moved_x = glam::DVec3::new(x_basis.end.x, x_basis.end.y, x_basis.end.z) - moved;
        let moved_y = glam::DVec3::new(y_basis.end.x, y_basis.end.y, y_basis.end.z) - moved;
        let scale = ((moved_x.length() + moved_y.length()) * 0.5).max(1.0e-12);
        association.center = acadrust::types::Vector3::new(moved.x, moved.y, moved.z);
        association.plane_origin = association.center;
        association.plane_x = acadrust::types::Vector3::new(
            moved_x.normalize_or(x).x,
            moved_x.normalize_or(x).y,
            moved_x.normalize_or(x).z,
        );
        association.plane_y = acadrust::types::Vector3::new(
            moved_y.normalize_or(y).x,
            moved_y.normalize_or(y).y,
            moved_y.normalize_or(y).z,
        );
        association.radius *= scale;
        association.cross_size *= scale;
        association.cross_gap *= scale;
        association.extension_length *= scale;
        for value in association.length_adjustments.iter_mut().chain(association.overshoots.iter_mut()) {
            *value *= scale;
        }
        association.associated = false;
        crate::scene::centermark::update_carrier(line, &association);
        return;
    }
    if let Some(mut association) =
        acadrust::entities::CenterLineAssociation::read(&line.common.extended_data)
    {
        association.associated = false;
        association.write(&mut line.common.extended_data);
    }
    apply_plain_transform(line, t);
}

impl RenderConvertible for Line {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
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
        if acadrust::entities::CenterMarkAssociation::read(&self.common.extended_data).is_some()
            || acadrust::entities::CenterLineAssociation::read(&self.common.extended_data).is_some()
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
    fn apply_grip_menu(&mut self, _grip_id: usize, _action: crate::scene::model::object::GripMenuAction) {
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

impl crate::entities::traits::MassPropsCalc for acadrust::entities::Line {
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
