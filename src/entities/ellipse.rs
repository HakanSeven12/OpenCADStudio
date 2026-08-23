use acadrust::entities::Ellipse;
use cadkernel::geom2d::{
    Curve as KernelCurve, EllipseArc as KernelEllipseArc, Tolerance, Vec2 as KernelVec2,
};
use cadkernel::space::PlanarCurve;

use crate::entities::curve::CurveSnap;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_prop as edit, oriented_triangle_grip, parse_f64, ro_prop as ro,
    square_grip,
};
use crate::entities::traits::RenderConvertible;
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};

fn to_render(ell: &Ellipse) -> RenderEntity {
    // ELLIPSE is one of the few WCS entities in DXF: `center` (code 10) and
    // `major_axis` (code 11) are world coordinates already — unlike ARC /
    // CIRCLE, whose centers are OCS. The converter knows that; this used to
    // run both through the arbitrary-axis OCS, which misplaced any ellipse
    // whose normal isn't Z-up — e.g. the (0,0,-1) result of a mirrored-block
    // explode.
    let curve = crate::entities::curve::ellipse_curve(ell);
    let snap = curve
        .as_ref()
        .map(crate::entities::curve::snap_from)
        .unwrap_or_default();

    // The points come from the entity's own kernel curve and angular policy.
    //
    // What does not change is the shape of the object. EXTRUDE, REVOLVE and
    // SWEEP read their profile out of `Contour` / `Curve` and have no arm for
    // a bare point list, so an ellipse handed over as `Lines` would stop
    // being usable as either.
    let empty = |snap: CurveSnap| RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(Vec::new()),
        snap_pts: snap.snap_pts,
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    };
    let Some(planar) = curve else {
        return empty(snap);
    };
    let points = crate::entities::curve::curve_points(&planar);
    if points.len() < 2 {
        return empty(snap);
    }

    let object = RenderObject::Lines(points);

    RenderEntity {
        pick_tris: Vec::new(),
        object,
        snap_pts: snap.snap_pts,
        tangent_geoms: vec![],
        key_vertices: vec![],
        fill_tris: vec![],
    }
}

fn ellipse_curves(ell: &Ellipse) -> Option<(PlanarCurve, PlanarCurve)> {
    let arc = crate::entities::curve::ellipse_curve(ell)?;
    let KernelCurve::Ellipse(piece) = &arc.curve else {
        return None;
    };
    let full = PlanarCurve::new(
        arc.plane,
        KernelCurve::Ellipse(KernelEllipseArc::full(piece.ellipse)),
    );
    Some((arc, full))
}

fn is_closed(ell: &Ellipse) -> bool {
    crate::entities::curve::ellipse_curve(ell).is_some_and(|curve| curve.curve.is_closed())
}

fn parameter_at_point(ell: &Ellipse, point: glam::DVec3) -> Option<f64> {
    let (_, full) = ellipse_curves(ell)?;
    Some(
        (full.parameter_at(point.to_array())? * std::f64::consts::TAU)
            .rem_euclid(std::f64::consts::TAU),
    )
}

fn grips(ell: &Ellipse) -> Vec<GripDef> {
    let fallback = glam::DVec3::new(ell.center.x, ell.center.y, ell.center.z);
    let Some((arc, full)) = ellipse_curves(ell) else {
        return vec![center_grip(0, fallback)];
    };
    let KernelCurve::Ellipse(piece) = &full.curve else {
        return vec![center_grip(0, fallback)];
    };
    let ctr = glam::DVec3::from_array(full.plane.point_at(piece.ellipse.centre));
    let axes = [0.0, 0.25, 0.5, 0.75].map(|t| glam::DVec3::from_array(full.point_at(t)));
    let mut grips = vec![
        center_grip(0, ctr),
        square_grip(1, axes[0]),
        square_grip(2, axes[1]),
        square_grip(3, axes[2]),
        square_grip(4, axes[3]),
    ];
    if !arc.curve.is_closed() {
        let start = glam::DVec3::from_array(arc.point_at(0.0));
        let end = glam::DVec3::from_array(arc.point_at(1.0));
        let start_outward = -glam::DVec3::from_array(arc.tangent_at(0.0));
        let end_outward = glam::DVec3::from_array(arc.tangent_at(1.0));
        grips.push(oriented_triangle_grip(5, start, start_outward));
        grips.push(oriented_triangle_grip(6, end, end_outward));
    }
    grips
}

fn properties(ell: &Ellipse) -> Vec<PropSection> {
    use crate::entities::traits::MassPropsCalc;

    let r_major = ell.major_axis_length();
    let r_minor = ell.minor_axis_length();

    // Axis frame in WCS: major-axis unit vector `u`, minor direction `v = normal × u`.
    let u = glam::DVec3::new(ell.major_axis.x, ell.major_axis.y, ell.major_axis.z);
    let u = if r_major > 1e-12 {
        u / r_major
    } else {
        glam::DVec3::X
    };
    let n = glam::DVec3::new(ell.normal.x, ell.normal.y, ell.normal.z);
    let v = n.cross(u);

    // Parametric points in WCS: center + r_major·cos(t)·u + r_minor·sin(t)·v.
    let center = glam::DVec3::new(ell.center.x, ell.center.y, ell.center.z);
    let pt_at = |t: f64| center + u * (r_major * t.cos()) + v * (r_minor * t.sin());
    let start = pt_at(ell.start_parameter);
    let end = pt_at(ell.end_parameter);

    // Axis vectors (center → axis endpoint) in WCS.
    let major_vec = u * r_major;
    let minor_vec = v * r_minor;

    let start_angle = ell.start_parameter.to_degrees().rem_euclid(360.0);
    let end_angle = if is_closed(ell) {
        360.0
    } else {
        ell.end_parameter.to_degrees().rem_euclid(360.0)
    };

    let props = ell.mass_props();

    vec![PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            ro(t!("Start X").as_ref(), "start_x", format!("{:.4}", start.x)),
            ro(t!("Start Y").as_ref(), "start_y", format!("{:.4}", start.y)),
            ro(t!("Start Z").as_ref(), "start_z", format!("{:.4}", start.z)),
            edit(t!("Center X").as_ref(), "center_x", ell.center.x),
            edit(t!("Center Y").as_ref(), "center_y", ell.center.y),
            edit(t!("Center Z").as_ref(), "center_z", ell.center.z),
            ro(t!("End X").as_ref(), "end_x", format!("{:.4}", end.x)),
            ro(t!("End Y").as_ref(), "end_y", format!("{:.4}", end.y)),
            ro(t!("End Z").as_ref(), "end_z", format!("{:.4}", end.z)),
            edit(t!("Major radius").as_ref(), "major_r", r_major),
            edit(t!("Minor radius").as_ref(), "minor_r", r_minor),
            edit(t!("Radius ratio").as_ref(), "ratio", ell.minor_axis_ratio),
            edit(t!("Start angle").as_ref(), "start_angle", start_angle),
            edit(t!("End angle").as_ref(), "end_angle", end_angle),
            ro(t!("Major axis vector X").as_ref(), "major_x", format!("{:.4}", major_vec.x)),
            ro(t!("Major axis vector Y").as_ref(), "major_y", format!("{:.4}", major_vec.y)),
            ro(t!("Major axis vector Z").as_ref(), "major_z", format!("{:.4}", major_vec.z)),
            ro(t!("Minor axis vector X").as_ref(), "minor_x", format!("{:.4}", minor_vec.x)),
            ro(t!("Minor axis vector Y").as_ref(), "minor_y", format!("{:.4}", minor_vec.y)),
            ro(t!("Minor axis vector Z").as_ref(), "minor_z", format!("{:.4}", minor_vec.z)),
            ro(t!("Area").as_ref(), "area", format!("{:.4}", props.area)),
            ro(t!("Start parameter").as_ref(), "start_param", format!("{:.4}", ell.start_parameter)),
            ro(t!("End parameter").as_ref(), "end_param", format!("{:.4}", ell.end_parameter)),
            ro(t!("Length").as_ref(), "length", format!("{:.4}", props.perimeter)),
            ro(t!("Normal X").as_ref(), "normal_x", format!("{:.4}", ell.normal.x)),
            ro(t!("Normal Y").as_ref(), "normal_y", format!("{:.4}", ell.normal.y)),
            ro(t!("Normal Z").as_ref(), "normal_z", format!("{:.4}", ell.normal.z)),
        ],
    }]
}

fn apply_geom_prop(ell: &mut Ellipse, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "center_x" => ell.center.x = v,
        "center_y" => ell.center.y = v,
        "center_z" => ell.center.z = v,
        "major_r" => {
            let cur = ell.major_axis_length();
            if cur > 1e-12 && v > 0.0 {
                let s = v / cur;
                ell.major_axis.x *= s;
                ell.major_axis.y *= s;
                ell.major_axis.z *= s;
            }
        }
        "minor_r" => {
            let major = ell.major_axis_length();
            if major > 1e-12 && v > 0.0 {
                ell.minor_axis_ratio = v / major;
            }
        }
        "ratio" if v > 0.0 => ell.minor_axis_ratio = v,
        "start_angle" => ell.start_parameter = v.to_radians(),
        "end_angle" => ell.end_parameter = v.to_radians(),
        _ => {}
    }
}

fn apply_axis_grip(ell: &mut Ellipse, grip_id: usize, point: glam::DVec3) {
    let Some((_, full)) = ellipse_curves(ell) else {
        return;
    };
    let KernelCurve::Ellipse(piece) = &full.curve else {
        return;
    };
    let Some(projected) = full.plane.project(point.to_array()) else {
        return;
    };
    let center = KernelVec2::from(piece.ellipse.centre);
    let mut dragged = KernelVec2::from(projected) - center;
    let tolerance = Tolerance::default().linear();

    if matches!(grip_id, 1 | 3) {
        if grip_id == 3 {
            dragged = -dragged;
        }
        if dragged.length() <= tolerance {
            return;
        }
        let axis = full.plane.vector_at(dragged.to_array());
        ell.major_axis.x = axis[0];
        ell.major_axis.y = axis[1];
        ell.major_axis.z = axis[2];
        return;
    }

    if grip_id == 4 {
        dragged = -dragged;
    }
    let major_dir = KernelVec2::from(piece.ellipse.major_axis);
    let minor_dir = major_dir.perpendicular();
    let major_len = piece.ellipse.major_radius.abs();
    let minor_len = piece.ellipse.minor_radius.abs();
    if major_len <= tolerance || minor_len <= tolerance {
        return;
    }

    let along_major = dragged.dot(major_dir).abs();
    let along_minor = dragged.dot(minor_dir).abs();
    let (drag_dir, dragged_len, fixed_dir, fixed_len) = if along_minor >= along_major {
        (minor_dir, along_minor, major_dir, major_len)
    } else {
        (major_dir, along_major, minor_dir, minor_len)
    };
    if fixed_len <= tolerance {
        return;
    }
    let (axis, ratio) = if dragged_len >= fixed_len {
        (
            drag_dir * dragged_len,
            (fixed_len / dragged_len).clamp(0.001, 1.0),
        )
    } else {
        (
            fixed_dir * fixed_len,
            (dragged_len / fixed_len).clamp(0.001, 1.0),
        )
    };
    let axis = full.plane.vector_at(axis.to_array());
    ell.major_axis.x = axis[0];
    ell.major_axis.y = axis[1];
    ell.major_axis.z = axis[2];
    ell.minor_axis_ratio = ratio;
}

fn apply_grip(ell: &mut Ellipse, grip_id: usize, apply: GripApply) {
    match (grip_id, apply) {
        (0, GripApply::Translate(d)) => {
            ell.center.x += d.x as f64;
            ell.center.y += d.y as f64;
            ell.center.z += d.z as f64;
        }
        (0, GripApply::Absolute(p)) => {
            ell.center.x = p.x as f64;
            ell.center.y = p.y as f64;
            ell.center.z = p.z as f64;
        }
        (1 | 2 | 3 | 4, GripApply::Absolute(p)) => apply_axis_grip(ell, grip_id, p),
        (5 | 6, GripApply::Absolute(p)) => {
            let Some(parameter) = parameter_at_point(ell, p) else {
                return;
            };
            if grip_id == 5 {
                ell.start_parameter = parameter;
            } else {
                ell.end_parameter = parameter;
            }
        }
        _ => {}
    }
}

fn apply_transform(ell: &mut Ellipse, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(ell, t, |entity, p1, p2| {
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.center.x,
            &mut entity.center.y,
            p1,
            p2,
        );
        crate::scene::view::transform::reflect_xy_point(
            &mut entity.major_axis.x,
            &mut entity.major_axis.y,
            p1,
            p2,
        );
    });
}

impl RenderConvertible for Ellipse {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(to_render(self))
    }
}

crate::impl_entity_basics!(Ellipse);

impl crate::entities::traits::MassPropsCalc for acadrust::entities::Ellipse {
    fn mass_props(&self) -> crate::entities::traits::MassProps {
        use std::f64::consts::{PI, TAU};
        let e = self;
        let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
        let b = a * e.minor_axis_ratio;
        let t0 = e.start_parameter;
        let t1 = {
            let mut t = e.end_parameter;
            if t <= t0 {
                t += TAU;
            }
            t
        };
        let span = t1 - t0;
        let is_full = (span - TAU).abs() < 1e-6;
        let area = if is_full {
            PI * a * b
        } else {
            // Sector area of ellipse approximated via 256-pt integration
            let n = 256usize;
            let mut s = 0.0f64;
            for k in 0..n {
                let t = t0 + span * (k as f64 / n as f64);
                let tp = t0 + span * ((k + 1) as f64 / n as f64);
                let nx = e.major_axis.x / a;
                let ny = e.major_axis.y / a;
                let x0 = a * t.cos() * nx - b * t.sin() * ny;
                let y0 = a * t.cos() * ny + b * t.sin() * nx;
                let x1 = a * tp.cos() * nx - b * tp.sin() * ny;
                let y1 = a * tp.cos() * ny + b * tp.sin() * nx;
                s += x0 * y1 - x1 * y0;
            }
            (s / 2.0).abs()
        };
        // Arc length via 256-pt numerical integration
        let nx = e.major_axis.x / a.max(1e-12);
        let ny = e.major_axis.y / a.max(1e-12);
        let perimeter = {
            let n = 256usize;
            let mut len = 0.0f64;
            for k in 0..n {
                let t = t0 + span * (k as f64 / n as f64);
                let tp = t0 + span * ((k + 1) as f64 / n as f64);
                let x0 = e.center.x + a * t.cos() * nx - b * t.sin() * ny;
                let y0 = e.center.y + a * t.cos() * ny + b * t.sin() * nx;
                let x1 = e.center.x + a * tp.cos() * nx - b * tp.sin() * ny;
                let y1 = e.center.y + a * tp.cos() * ny + b * tp.sin() * nx;
                len += (x1 - x0).hypot(y1 - y0);
            }
            len
        };
        crate::entities::traits::MassProps {
            area,
            perimeter,
            cx: e.center.x,
            cy: e.center.y,
        }
    }
}

#[cfg(test)]
mod grip_tests {
    use super::*;
    use glam::DVec3;

    fn ell(major_x: f64, ratio: f64) -> Ellipse {
        let mut e = Ellipse::default();
        e.center.x = 0.0;
        e.center.y = 0.0;
        e.center.z = 0.0;
        e.major_axis.x = major_x;
        e.major_axis.y = 0.0;
        e.major_axis.z = 0.0;
        e.minor_axis_ratio = ratio;
        e
    }
    fn xy_len(v: &acadrust::entities::Ellipse) -> f64 {
        (v.major_axis.x * v.major_axis.x + v.major_axis.y * v.major_axis.y).sqrt()
    }

    #[test]
    fn minor_grip_below_major_updates_ratio_only() {
        let mut e = ell(10.0, 0.5);
        apply_grip(&mut e, 2, GripApply::Absolute(DVec3::new(0.0, 8.0, 0.0)));
        assert!((xy_len(&e) - 10.0).abs() < 1e-9, "major axis unchanged");
        assert!((e.minor_axis_ratio - 0.8).abs() < 1e-9, "ratio = 8/10");
    }

    #[test]
    fn minor_grip_past_major_swaps_without_ballooning() {
        // major=10 (+X), minor=5 (+Y). Drag the minor grip to (0,11), past major.
        let mut e = ell(10.0, 0.5);
        apply_grip(&mut e, 2, GripApply::Absolute(DVec3::new(0.0, 11.0, 0.0)));
        assert!((xy_len(&e) - 11.0).abs() < 1e-9, "major follows the drag to 11");
        assert!(e.major_axis.x.abs() < 1e-9, "major points +Y after the swap");
        assert!(
            (xy_len(&e) * e.minor_axis_ratio - 10.0).abs() < 1e-9,
            "minor holds the old major length (10)"
        );

        // Keep dragging along +Y: the perpendicular axis must stay 10, not
        // balloon, and the major must not flip 90° each frame (the reported bug).
        apply_grip(&mut e, 2, GripApply::Absolute(DVec3::new(0.0, 13.0, 0.0)));
        assert!((xy_len(&e) - 13.0).abs() < 1e-9, "major grows to 13");
        assert!(e.major_axis.x.abs() < 1e-9, "major stays +Y (no flip)");
        assert!(
            (xy_len(&e) * e.minor_axis_ratio - 10.0).abs() < 1e-6,
            "minor still 10 — no ballooning"
        );
    }
}
