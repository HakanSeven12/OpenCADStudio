use acadrust::entities::Arc;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro,
    square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::TangentGeom;

const TAU: f64 = std::f64::consts::TAU;

fn to_render(arc: &Arc) -> RenderEntity {
    let cx = arc.center.x;
    let cy = arc.center.y;
    let cz = arc.center.z;
    let r = arc.radius;
    let sa = arc.start_angle;
    let ea = arc.end_angle;
    let normal = (arc.normal.x, arc.normal.y, arc.normal.z);

    // Compute OCS basis vectors for this entity's normal.
    let (ax, ay) = crate::scene::view::transform::ocs_axes(normal);

    // Arc centre in WCS.
    let (cwx, cwy, cwz) = crate::scene::view::transform::ocs_point_to_wcs((cx, cy, cz), normal);

    // Arc points in WCS: centre_wcs + r*cos(a)*Ax + r*sin(a)*Ay
    let arc_pt = |a: f64| {
        let (c, s) = (a.cos(), a.sin());
        [
            cwx + r * c * ax.0 + r * s * ay.0,
            cwy + r * c * ax.1 + r * s * ay.1,
            cwz + r * c * ax.2 + r * s * ay.2,
        ]
    };

    // Centre, ends, arc-length midpoint and the quadrants the sweep actually
    // covers, all from the entity's own curve. Circles and ellipses (closed
    // curves) deliberately emit no midpoint; see #34.
    let curve = crate::entities::curve::arc_curve(arc);
    let snap = crate::entities::curve::snap_from(&curve);
    let tangent = TangentGeom::Circle {
        center: [cwx as f32, cwy as f32, cwz as f32],
        radius: r as f32,
    };

    if arc.thickness.abs() > 1e-10 {
        let t = arc.thickness;
        let (nx, ny, nz) = normal;
        let n = 32usize;
        let ccw_end = if ea >= sa { ea } else { ea + TAU };
        let (start_a, end_a) = (sa, ccw_end);
        let base: Vec<[f64; 3]> = (0..=n)
            .map(|i| {
                let p = arc_pt(start_a + (end_a - start_a) * (i as f64 / n as f64));
                [p[0], p[1], p[2]]
            })
            .collect();
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity((n + 1) * 2 + 8);
        pts.extend_from_slice(&base);
        pts.push([f64::NAN; 3]);
        for &[x, y, z] in &base {
            pts.push([x + t * nx, y + t * ny, z + t * nz]);
        }
        pts.push([f64::NAN; 3]);
        for end in [arc_pt(sa), arc_pt(ea)] {
            pts.push(end);
            pts.push([end[0] + t * nx, end[1] + t * ny, end[2] + t * nz]);
            pts.push([f64::NAN; 3]);
        }
        pts.pop();
        return RenderEntity {
            pick_tris: extrusion_wall_tris(&base, [t * nx, t * ny, t * nz]),
            object: RenderObject::Lines(pts),
            snap_pts: snap.snap_pts.clone(),
            tangent_geoms: vec![tangent],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    // Sampled through the arc's own curve, which is the one definition of
    // its geometry. EXTRUDE, REVOLVE and SWEEP read that definition directly
    // rather than this point list, so nothing is lost by drawing from it.
    let points = crate::entities::curve::curve_points(&crate::entities::curve::arc_curve(arc));
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(points),
        snap_pts: snap.snap_pts.clone(),
        tangent_geoms: vec![tangent],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn control_points(arc: &Arc) -> [glam::DVec3; 3] {
    let center = glam::DVec3::new(arc.center.x, arc.center.y, arc.center.z);
    let sweep = (arc.end_angle - arc.start_angle).rem_euclid(TAU);
    let point = |angle: f64| {
        center
            + glam::DVec3::new(
                arc.radius * angle.cos(),
                arc.radius * angle.sin(),
                0.0,
            )
    };
    [
        point(arc.start_angle),
        point(arc.start_angle + sweep * 0.5),
        point(arc.end_angle),
    ]
}

fn circumcircle(
    a: glam::DVec3,
    b: glam::DVec3,
    c: glam::DVec3,
) -> Option<(glam::DVec3, f64)> {
    // Work relative to the first point so large drawing coordinates do not
    // lose the small differences that define the circle.
    let ab = b - a;
    let ac = c - a;
    let bc = c - b;
    let scale2 = ab
        .length_squared()
        .max(ac.length_squared())
        .max(bc.length_squared());
    if scale2 < 1.0e-18 {
        return None;
    }
    let det = 2.0 * (ab.x * ac.y - ab.y * ac.x);
    if det.abs() <= scale2 * 1.0e-12 {
        return None;
    }
    let ab2 = ab.x * ab.x + ab.y * ab.y;
    let ac2 = ac.x * ac.x + ac.y * ac.y;
    let center = a
        + glam::DVec3::new(
            (ab2 * ac.y - ac2 * ab.y) / det,
            (ab.x * ac2 - ac.x * ab2) / det,
            0.0,
        );
    let radius = center.distance(a);
    radius.is_finite().then_some((center, radius))
}

pub(crate) fn refit_grips(
    arc: &mut Arc,
    original: &Arc,
    edits: &[(usize, glam::DVec3)],
) -> bool {
    let mut points = control_points(original);
    let mut changed = false;
    for &(grip_id, point) in edits {
        let index = match grip_id {
            1 => 0,
            2 => 2,
            3 => 1,
            _ => continue,
        };
        points[index] = point;
        changed = true;
    }
    if !changed {
        return false;
    }

    let Some((center, radius)) = circumcircle(points[0], points[1], points[2]) else {
        return false;
    };
    if radius <= 1.0e-9 {
        return false;
    }

    let start = (points[0].y - center.y).atan2(points[0].x - center.x);
    let middle = (points[1].y - center.y).atan2(points[1].x - center.x);
    let end = (points[2].y - center.y).atan2(points[2].x - center.x);
    let sweep = (end - start).rem_euclid(TAU);
    let middle_sweep = (middle - start).rem_euclid(TAU);
    // Crossing the two fixed points makes the three-point definition
    // degenerate before it reverses. Keep the last valid preview instead of
    // swapping the start and end grip identities under the cursor.
    if sweep <= 1.0e-9 || middle_sweep > sweep + 1.0e-9 {
        return false;
    }

    arc.center.x = center.x;
    arc.center.y = center.y;
    arc.center.z = original.center.z;
    arc.radius = radius;
    arc.start_angle = start;
    arc.end_angle = end;
    true
}

fn grips(arc: &Arc) -> Vec<GripDef> {
    let ctr = glam::DVec3::new(arc.center.x, arc.center.y, arc.center.z);
    let [start, middle, end] = control_points(arc);
    vec![
        center_grip(0, ctr),
        square_grip(1, start),
        square_grip(2, end),
        square_grip(3, middle),
    ]
}

fn properties(arc: &Arc) -> Vec<PropSection> {
    let r = arc.radius;
    let sa = arc.start_angle;
    let ea = arc.end_angle;
    let sweep = (ea - sa).rem_euclid(TAU);
    let total_angle = sweep.to_degrees();
    let arc_length = r * sweep;
    let area = 0.5 * r * r * sweep;

    let normal = (arc.normal.x, arc.normal.y, arc.normal.z);
    let (ax, ay) = crate::scene::view::transform::ocs_axes(normal);
    let (cwx, cwy, cwz) = crate::scene::view::transform::ocs_point_to_wcs(
        (arc.center.x, arc.center.y, arc.center.z),
        normal,
    );
    let arc_pt = |a: f64| {
        let (c, s) = (a.cos(), a.sin());
        (
            cwx + r * c * ax.0 + r * s * ay.0,
            cwy + r * c * ax.1 + r * s * ay.1,
            cwz + r * c * ax.2 + r * s * ay.2,
        )
    };
    let (sx, sy, sz) = arc_pt(sa);
    let (ex, ey, ez) = arc_pt(ea);

    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            ro(t!("Start X").as_ref(), "start_x", format!("{sx:.4}")),
            ro(t!("Start Y").as_ref(), "start_y", format!("{sy:.4}")),
            ro(t!("Start Z").as_ref(), "start_z", format!("{sz:.4}")),
            edit(t!("Center X").as_ref(), "center_x", arc.center.x),
            edit(t!("Center Y").as_ref(), "center_y", arc.center.y),
            edit(t!("Center Z").as_ref(), "center_z", arc.center.z),
            ro(t!("End X").as_ref(), "end_x", format!("{ex:.4}")),
            ro(t!("End Y").as_ref(), "end_y", format!("{ey:.4}")),
            ro(t!("End Z").as_ref(), "end_z", format!("{ez:.4}")),
            edit(t!("Radius").as_ref(), "radius", arc.radius),
            edit_angle(t!("Start angle").as_ref(), "start_angle", sa.to_degrees()),
            edit_angle(t!("End angle").as_ref(), "end_angle", ea.to_degrees()),
            ro(t!("Total angle").as_ref(), "total_angle", format!("{total_angle:.2}")),
            ro(t!("Arc length").as_ref(), "arc_length", format!("{arc_length:.4}")),
            ro(t!("Area").as_ref(), "area", format!("{area:.4}")),
            ro(t!("Normal X").as_ref(), "normal_x", format!("{:.4}", arc.normal.x)),
            ro(t!("Normal Y").as_ref(), "normal_y", format!("{:.4}", arc.normal.y)),
            ro(t!("Normal Z").as_ref(), "normal_z", format!("{:.4}", arc.normal.z)),
        ],
    }]
}

fn apply_geom_prop(arc: &mut Arc, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "center_x" => arc.center.x = v,
        "center_y" => arc.center.y = v,
        "center_z" => arc.center.z = v,
        "radius" if v > 0.0 => arc.radius = v,
        "start_angle" => arc.start_angle = v.to_radians(),
        "end_angle" => arc.end_angle = v.to_radians(),
        _ => {}
    }
}

fn apply_grip(arc: &mut Arc, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Translate(d)) => {
            arc.center.x += d.x;
            arc.center.y += d.y;
            arc.center.z += d.z;
        }
        (0, GripApply::Absolute(p)) => {
            arc.center.x = p[0];
            arc.center.y = p[1];
            arc.center.z = p[2];
        }
        (1..=3, GripApply::Absolute(p)) => {
            let original = arc.clone();
            let _ = refit_grips(arc, &original, &[(grip_id, p)]);
        }
        _ => {}
    }
}

fn apply_transform(arc: &mut Arc, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(arc, t, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.center.x,
            &mut entity.center.y,
            p1,
            p2,
        );
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let line_angle = dy.atan2(dx);
        let tmp = entity.start_angle;
        entity.start_angle = 2.0 * line_angle - entity.end_angle;
        entity.end_angle = 2.0 * line_angle - tmp;
    });
}

impl RenderConvertible for Arc {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

impl crate::entities::traits::Grippable for Arc {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }
    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        match grip_id {
            0 => vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }],
            3 => vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Radius",
                    action: GripMenuAction::Radius,
                },
                GripMenuItem {
                    label: "Arc Length",
                    action: GripMenuAction::ArcLength,
                },
            ],
            _ => vec![
                GripMenuItem {
                    label: "Stretch",
                    action: GripMenuAction::Stretch,
                },
                GripMenuItem {
                    label: "Lengthen",
                    action: GripMenuAction::Lengthen,
                },
            ],
        }
    }
    fn apply_grip_menu(&mut self, _grip_id: usize, _action: crate::scene::model::object::GripMenuAction) {
        // Radius / Arc Length / Lengthen all need a follow-up prompt;
        // the actual edit happens in `apply_grip_menu_value`.
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::Radius => Some("New radius"),
            A::ArcLength => Some("New arc length"),
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
        if !matches!(action, A::Lengthen) || self.radius <= 1.0e-9 {
            return None;
        }
        let cursor_angle = (point.y - self.center.y).atan2(point.x - self.center.x);
        let current_sweep = (self.end_angle - self.start_angle).rem_euclid(TAU);
        let desired_sweep = match grip_id {
            1 => (self.end_angle - cursor_angle).rem_euclid(TAU),
            2 => (cursor_angle - self.start_angle).rem_euclid(TAU),
            _ => return None,
        };
        if desired_sweep <= 1.0e-9 {
            return None;
        }
        Some((desired_sweep - current_sweep) * self.radius)
    }

    fn apply_grip_menu_value(
        &mut self,
        grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::Radius if value > 0.0 => self.radius = value,
            A::ArcLength if value > 0.0 && self.radius > 1e-9 => {
                // Hold start_angle, derive new end_angle from arc length
                // = r * Δθ.
                let new_span = value / self.radius;
                self.end_angle = self.start_angle + new_span;
            }
            A::Lengthen => {
                // Extend either end by `value` arc-length units along
                // the arc. Positive `value` lengthens; negative
                // shortens. Grip 1 = start endpoint, grip 2 = end endpoint.
                if self.radius < 1e-9 {
                    return;
                }
                let dtheta = value / self.radius;
                match grip_id {
                    1 => self.start_angle -= dtheta,
                    2 => self.end_angle += dtheta,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl crate::entities::traits::PropertyEditable for Arc {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }
    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl crate::entities::traits::Transformable for Arc {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

impl crate::entities::traits::MassPropsCalc for acadrust::entities::Arc {
    fn mass_props(&self) -> crate::entities::traits::MassProps {
        use std::f64::consts::TAU;
        let r = self.radius;
        let span = {
            let s = (self.end_angle - self.start_angle).rem_euclid(TAU);
            if s < 1e-6 {
                TAU
            } else {
                s
            }
        };
        // Sector area (pie slice)
        let area = 0.5 * r * r * span;
        let arc_len = r * span;
        // Centroid of arc (chord midpoint direction)
        let mid_rad = self.start_angle + span / 2.0;
        crate::entities::traits::MassProps {
            area,
            perimeter: arc_len,
            cx: self.center.x + r * mid_rad.cos(),
            cy: self.center.y + r * mid_rad.sin(),
        }
    }
}
