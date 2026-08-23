use acadrust::entities::Circle;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::TangentGeom;

fn to_render(circle: &Circle) -> RenderEntity {
    let cx = circle.center.x;
    let cy = circle.center.y;
    let cz = circle.center.z;
    let r = circle.radius;
    let normal = (circle.normal.x, circle.normal.y, circle.normal.z);

    let (ax, ay) = crate::scene::view::transform::ocs_axes(normal);
    let (cwx, cwy, cwz) = crate::scene::view::transform::ocs_point_to_wcs((cx, cy, cz), normal);

    // Centre and quadrants come from the entity's own curve, so the same
    // definition answers here, in the tessellation and in a trim.
    let curve = crate::entities::curve::circle_curve(circle);
    let snap_pts = crate::entities::curve::snap_from(&curve).snap_pts;
    let tangent = TangentGeom::PlanarCircle {
        center: [cwx, cwy, cwz],
        axis_x: [ax.0, ax.1, ax.2],
        axis_y: [ay.0, ay.1, ay.2],
        radius: r,
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
        let base: Vec<[f64; 3]> = (0..=n)
            .map(|i| {
                let (x, y, z) = circ_pt(i as f64 * tau / n as f64);
                [x, y, z]
            })
            .collect();
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity((n + 1) * 2 + 4 * 3);
        pts.extend_from_slice(&base);
        pts.push([f64::NAN; 3]);
        for &[x, y, z] in &base {
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
        return RenderEntity {
            pick_tris: extrusion_wall_tris(&base, [t * nx, t * ny, t * nz]),
            object: RenderObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![tangent],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    // Keep tessellation on the entity curve for large-coordinate precision.
    let pts = crate::entities::curve::curve_points(&curve);

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(pts),
        snap_pts,
        tangent_geoms: vec![tangent],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn grips(circle: &Circle) -> Vec<GripDef> {
    let center = circle.center_wcs();
    let (axis_x, axis_y) = circle.axes_wcs();
    let ctr = glam::DVec3::new(center.x, center.y, center.z);
    let axis_x = glam::DVec3::new(axis_x.x, axis_x.y, axis_x.z);
    let axis_y = glam::DVec3::new(axis_y.x, axis_y.y, axis_y.z);
    let r = circle.radius;
    vec![
        center_grip(0, ctr),
        square_grip(1, ctr + axis_x * r),
        square_grip(2, ctr + axis_y * r),
        square_grip(3, ctr - axis_x * r),
        square_grip(4, ctr - axis_y * r),
    ]
}

fn properties(circle: &Circle) -> Vec<PropSection> {
    use std::f64::consts::PI;
    let r = circle.radius;
    let center = circle.center_wcs();
    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Center X").as_ref(), "center_x", center.x),
            edit(t!("Center Y").as_ref(), "center_y", center.y),
            edit(t!("Center Z").as_ref(), "center_z", center.z),
            edit(t!("Radius").as_ref(), "radius", r),
            edit(t!("Diameter").as_ref(), "diameter", r * 2.0),
            edit(t!("Circumference").as_ref(), "circumference", 2.0 * PI * r),
            edit(t!("Area").as_ref(), "area", PI * r * r),
            ro(t!("Normal X").as_ref(), "normal_x", format!("{:.4}", circle.normal.x)),
            ro(t!("Normal Y").as_ref(), "normal_y", format!("{:.4}", circle.normal.y)),
            ro(t!("Normal Z").as_ref(), "normal_z", format!("{:.4}", circle.normal.z)),
        ],
    }]
}

fn apply_geom_prop(circle: &mut Circle, field: &str, value: &str) {
    use std::f64::consts::PI;
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "center_x" | "center_y" | "center_z" => {
            let normal = (circle.normal.x, circle.normal.y, circle.normal.z);
            let center = circle.center_wcs();
            let (mut x, mut y, mut z) = (center.x, center.y, center.z);
            match field {
                "center_x" => x = v,
                "center_y" => y = v,
                "center_z" => z = v,
                _ => {}
            }
            let (ox, oy, oz) =
                crate::scene::view::transform::wcs_point_to_ocs((x, y, z), normal);
            circle.center.x = ox;
            circle.center.y = oy;
            circle.center.z = oz;
        }
        "radius" if v > 0.0 => circle.radius = v,
        "diameter" if v > 0.0 => circle.radius = v / 2.0,
        "circumference" if v > 0.0 => circle.radius = v / (2.0 * PI),
        "area" if v > 0.0 => circle.radius = (v / PI).sqrt(),
        _ => {}
    }
}

fn apply_grip(circle: &mut Circle, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Absolute(p)) => {
            let (x, y, z) = crate::scene::view::transform::wcs_point_to_ocs(
                (p.x, p.y, p.z),
                (circle.normal.x, circle.normal.y, circle.normal.z),
            );
            circle.center.x = x;
            circle.center.y = y;
            circle.center.z = z;
        }
        (0, GripApply::Translate(d)) => {
            let (x, y, z) = crate::scene::view::transform::wcs_point_to_ocs(
                (d.x, d.y, d.z),
                (circle.normal.x, circle.normal.y, circle.normal.z),
            );
            circle.center.x += x;
            circle.center.y += y;
            circle.center.z += z;
        }
        (1..=4, GripApply::Absolute(p)) => {
            let plane = crate::entities::curve::circle_curve(circle).plane;
            if let Some(point) = plane.project([p.x, p.y, p.z]) {
                let radius = (point[0] - circle.center.x).hypot(point[1] - circle.center.y);
                if radius > 1.0e-9 {
                    circle.radius = radius;
                }
            }
        }
        _ => {}
    }
}

fn apply_transform(circle: &mut Circle, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(circle, t, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.center.x,
            &mut entity.center.y,
            p1,
            p2,
        );
    });
}

impl RenderConvertible for Circle {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

crate::impl_entity_basics!(Circle);

impl crate::entities::traits::MassPropsCalc for Circle {
    fn mass_props(&self) -> crate::entities::traits::MassProps {
        use std::f64::consts::{PI, TAU};
        let r = self.radius;
        let center = self.center_wcs();
        crate::entities::traits::MassProps {
            area: PI * r * r,
            perimeter: TAU * r,
            cx: center.x,
            cy: center.y,
        }
    }
}
