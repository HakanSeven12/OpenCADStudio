use acadrust::entities::Circle;
use glam::Vec3;
use truck_modeling::{builder, Point3, Wire};

use crate::command::EntityTransform;
use crate::entities::common::{
    diamond_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::{SnapHint, TangentGeom};

fn to_truck(circle: &Circle) -> TruckEntity {
    let cx = circle.center.x;
    let cy = circle.center.y;
    let cz = circle.center.z;
    let r = circle.radius;
    let normal = (circle.normal.x, circle.normal.y, circle.normal.z);

    let (ax, ay) = crate::scene::transform::ocs_axes(normal);
    let (cwx, cwy, cwz) = crate::scene::transform::ocs_point_to_wcs((cx, cy, cz), normal);

    // Circle points in WCS: centre_wcs ± r * Ax  and  centre_wcs ± r * Ay
    let pt = |da: (f64, f64, f64), db: (f64, f64, f64), s: f64| {
        Point3::new(
            cwx + s * da.0 + s * db.0,
            cwy + s * da.1 + s * db.1,
            cwz + s * da.2 + s * db.2,
        )
    };
    // right: centre + r*Ax, left: centre - r*Ax, top: centre + r*Ay, bot: centre - r*Ay
    let p_right = pt(ax, (0.0, 0.0, 0.0), r);
    let p_left = pt((ax.0 * -1.0, ax.1 * -1.0, ax.2 * -1.0), (0.0, 0.0, 0.0), r);
    let p_top = pt(ay, (0.0, 0.0, 0.0), r);
    let p_bot = pt((ay.0 * -1.0, ay.1 * -1.0, ay.2 * -1.0), (0.0, 0.0, 0.0), r);

    let cv = Vec3::new(cwx as f32, cwy as f32, cwz as f32);
    let rf = r as f32;
    let q = |d: (f64, f64, f64)| {
        Vec3::new(
            (cwx + r * d.0) as f32,
            (cwy + r * d.1) as f32,
            (cwz + r * d.2) as f32,
        )
    };
    let snap_pts = vec![
        (cv, SnapHint::Center),
        (q(ax), SnapHint::Quadrant),
        (q(ay), SnapHint::Quadrant),
        (q((-ax.0, -ax.1, -ax.2)), SnapHint::Quadrant),
        (q((-ay.0, -ay.1, -ay.2)), SnapHint::Quadrant),
    ];
    let tangent = TangentGeom::Circle {
        center: [cwx as f32, cwy as f32, cwz as f32],
        radius: rf,
    };

    if circle.thickness.abs() > 1e-10 {
        let t = circle.thickness;
        let (nx, ny, nz) = normal;
        let n = 64usize;
        let tau = std::f64::consts::TAU;
        let circ_pt = |a: f64| -> (f64, f64, f64) {
            let (c, s) = (a.cos(), a.sin());
            (
                cwx + r * (c * ax.0 + s * ay.0),
                cwy + r * (c * ax.1 + s * ay.1),
                cwz + r * (c * ax.2 + s * ay.2),
            )
        };
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity((n + 1) * 2 + 4 * 3);
        for i in 0..=n {
            let (x, y, z) = circ_pt(i as f64 * tau / n as f64);
            pts.push([x, y, z]);
        }
        pts.push([f64::NAN; 3]);
        for i in 0..=n {
            let (x, y, z) = circ_pt(i as f64 * tau / n as f64);
            pts.push([x + t * nx, y + t * ny, z + t * nz]);
        }
        pts.push([f64::NAN; 3]);
        for i in 0..4usize {
            let (x, y, z) = circ_pt(i as f64 * std::f64::consts::FRAC_PI_2);
            pts.push([x, y, z]);
            pts.push([x + t * nx, y + t * ny, z + t * nz]);
            if i < 3 {
                pts.push([f64::NAN; 3]);
            }
        }
        return TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![tangent],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let right = builder::vertex(p_right);
    let left = builder::vertex(p_left);
    let upper = builder::circle_arc(&right, &left, p_top);
    let lower = builder::circle_arc(&left, &right, p_bot);
    let wire: Wire = [upper, lower].into_iter().collect();
    TruckEntity {
        object: TruckObject::Contour(wire),
        snap_pts,
        tangent_geoms: vec![tangent],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn grips(circle: &Circle) -> Vec<GripDef> {
    let ctr = Vec3::new(
        circle.center.x as f32,
        circle.center.y as f32,
        circle.center.z as f32,
    );
    let r = circle.radius as f32;
    vec![
        diamond_grip(0, ctr),
        square_grip(1, ctr + Vec3::new(r, 0.0, 0.0)),
        square_grip(2, ctr + Vec3::new(0.0, r, 0.0)),
        square_grip(3, ctr - Vec3::new(r, 0.0, 0.0)),
        square_grip(4, ctr - Vec3::new(0.0, r, 0.0)),
    ]
}

fn properties(circle: &Circle) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            edit("Center X", "center_x", circle.center.x),
            edit("Center Y", "center_y", circle.center.y),
            edit("Center Z", "center_z", circle.center.z),
            edit("Radius", "radius", circle.radius),
            ro(
                "Diameter",
                "diameter",
                format!("{:.4}", circle.radius * 2.0),
            ),
            ro(
                "Circumference",
                "circumference",
                format!("{:.4}", circle.radius * 2.0 * std::f64::consts::PI),
            ),
        ],
    }
}

fn apply_geom_prop(circle: &mut Circle, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "center_x" => circle.center.x = v,
        "center_y" => circle.center.y = v,
        "center_z" => circle.center.z = v,
        "radius" if v > 0.0 => circle.radius = v,
        _ => {}
    }
}

fn apply_grip(circle: &mut Circle, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            circle.center.x = p.x as f64;
            circle.center.y = p.y as f64;
            circle.center.z = p.z as f64;
        }
        (0, GripApply::Translate(d)) => {
            circle.center.x += d.x as f64;
            circle.center.y += d.y as f64;
            circle.center.z += d.z as f64;
        }
        (1..=4, GripApply::Absolute(p)) => {
            let cx = circle.center.x as f32;
            let cy = circle.center.y as f32;
            let dx = p.x - cx;
            let dy = p.y - cy;
            circle.radius = ((dx * dx + dy * dy) as f64).sqrt();
        }
        _ => {}
    }
}

fn apply_transform(circle: &mut Circle, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(circle, t, |entity, p1, p2| {
        crate::scene::transform::reflect_xy_point(
            &mut entity.center.x,
            &mut entity.center.y,
            p1,
            p2,
        );
    });
}

impl TruckConvertible for Circle {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for Circle {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Circle {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Circle {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
