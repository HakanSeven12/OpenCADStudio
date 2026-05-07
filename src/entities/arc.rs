use acadrust::entities::Arc;
use glam::Vec3;
use truck_modeling::{builder, Point3};

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, parse_f64, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::{SnapHint, TangentGeom};

const TAU: f64 = std::f64::consts::TAU;

fn to_truck(arc: &Arc) -> TruckEntity {
    let cx = arc.center.x;
    let cy = arc.center.y;
    let cz = arc.center.z;
    let r = arc.radius;
    let sa = arc.start_angle;
    let ea = arc.end_angle;
    let normal = (arc.normal.x, arc.normal.y, arc.normal.z);

    // Compute OCS basis vectors for this entity's normal.
    let (ax, ay) = crate::scene::transform::ocs_axes(normal);

    let ccw_end = if ea >= sa { ea } else { ea + TAU };
    let mid_a = sa + (ccw_end - sa) * 0.5;

    // Arc centre in WCS.
    let (cwx, cwy, cwz) = crate::scene::transform::ocs_point_to_wcs((cx, cy, cz), normal);

    // Arc points in WCS: centre_wcs + r*cos(a)*Ax + r*sin(a)*Ay
    let arc_pt = |a: f64| {
        let (c, s) = (a.cos(), a.sin());
        Point3::new(
            cwx + r * c * ax.0 + r * s * ay.0,
            cwy + r * c * ax.1 + r * s * ay.1,
            cwz + r * c * ax.2 + r * s * ay.2,
        )
    };

    let cv = Vec3::new(cwx as f32, cwy as f32, cwz as f32);
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
        let mut pts: Vec<[f32; 3]> = Vec::with_capacity((n + 1) * 2 + 8);
        for i in 0..=n {
            let a = start_a + (end_a - start_a) * (i as f64 / n as f64);
            let p = arc_pt(a);
            pts.push([p.x as f32, p.y as f32, p.z as f32]);
        }
        pts.push([f32::NAN; 3]);
        for i in 0..=n {
            let a = start_a + (end_a - start_a) * (i as f64 / n as f64);
            let p = arc_pt(a);
            pts.push([(p.x + t * nx) as f32, (p.y + t * ny) as f32, (p.z + t * nz) as f32]);
        }
        pts.push([f32::NAN; 3]);
        let ps = arc_pt(sa);
        pts.push([ps.x as f32, ps.y as f32, ps.z as f32]);
        pts.push([(ps.x + t * nx) as f32, (ps.y + t * ny) as f32, (ps.z + t * nz) as f32]);
        pts.push([f32::NAN; 3]);
        let pe = arc_pt(ea);
        pts.push([pe.x as f32, pe.y as f32, pe.z as f32]);
        pts.push([(pe.x + t * nx) as f32, (pe.y + t * ny) as f32, (pe.z + t * nz) as f32]);
        return TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts: vec![(cv, SnapHint::Center)],
            tangent_geoms: vec![tangent],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let p_start = arc_pt(sa);
    let p_end = arc_pt(ea);
    let p_mid = arc_pt(mid_a);
    let v_start = builder::vertex(p_start);
    let v_end = builder::vertex(p_end);
    let edge = builder::circle_arc(&v_start, &v_end, p_mid);
    TruckEntity {
        object: TruckObject::Curve(edge),
        snap_pts: vec![(cv, SnapHint::Center)],
        tangent_geoms: vec![tangent],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn angle_span(start: f32, end: f32) -> f32 {
    let mut span = end - start;
    if span < 0.0 {
        span += std::f32::consts::TAU;
    }
    span
}

fn grips(arc: &Arc) -> Vec<GripDef> {
    let ctr = Vec3::new(
        arc.center.x as f32,
        arc.center.y as f32,
        arc.center.z as f32,
    );
    let r = arc.radius as f32;
    let sa = arc.start_angle as f32;
    let ea = arc.end_angle as f32;
    let ma = sa + angle_span(sa, ea) * 0.5;
    vec![
        diamond_grip(0, ctr),
        square_grip(1, ctr + Vec3::new(r * sa.cos(), r * sa.sin(), 0.0)),
        square_grip(2, ctr + Vec3::new(r * ea.cos(), r * ea.sin(), 0.0)),
        diamond_grip(3, ctr + Vec3::new(r * ma.cos(), r * ma.sin(), 0.0)),
    ]
}

fn properties(arc: &Arc) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            edit("Center X", "center_x", arc.center.x),
            edit("Center Y", "center_y", arc.center.y),
            edit("Center Z", "center_z", arc.center.z),
            edit("Radius", "radius", arc.radius),
            edit("Start Angle (deg)", "start_angle", arc.start_angle.to_degrees()),
            edit("End Angle (deg)", "end_angle", arc.end_angle.to_degrees()),
        ],
    }
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
            arc.center.x += d.x as f64;
            arc.center.y += d.y as f64;
            arc.center.z += d.z as f64;
        }
        (0, GripApply::Absolute(p)) => {
            arc.center.x = p.x as f64;
            arc.center.y = p.y as f64;
            arc.center.z = p.z as f64;
        }
        (1, GripApply::Absolute(p)) => {
            let dx = p.x - arc.center.x as f32;
            let dy = p.y - arc.center.y as f32;
            arc.start_angle = (dy as f64).atan2(dx as f64);
        }
        (2, GripApply::Absolute(p)) => {
            let dx = p.x - arc.center.x as f32;
            let dy = p.y - arc.center.y as f32;
            arc.end_angle = (dy as f64).atan2(dx as f64);
        }
        (3, GripApply::Translate(d)) => {
            let sa = arc.start_angle as f32;
            let ea = arc.end_angle as f32;
            let span = angle_span(sa, ea);
            let mid_a = sa + span * 0.5;
            let current_mid_x = arc.center.x as f32 + arc.radius as f32 * mid_a.cos();
            let current_mid_y = arc.center.y as f32 + arc.radius as f32 * mid_a.sin();
            let new_mid_x = current_mid_x + d.x;
            let new_mid_y = current_mid_y + d.y;
            let dx = new_mid_x - arc.center.x as f32;
            let dy = new_mid_y - arc.center.y as f32;
            let new_r = (dx * dx + dy * dy).sqrt() as f64;
            if new_r > 1e-6 {
                arc.radius = new_r;
            }
        }
        _ => {}
    }
}

fn apply_transform(arc: &mut Arc, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(arc, t, |entity, p1, p2| {
        crate::scene::transform::reflect_xy_point(
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

impl TruckConvertible for Arc {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for Arc {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Arc {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Arc {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
