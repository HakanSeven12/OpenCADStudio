use acadrust::entities::Spline;
use glam::Vec3;
use truck_modeling::{
    base::{BoundedCurve, ParametricCurve, Vector4},
    builder, BSplineCurve, Curve, Edge, KnotVec, NurbsCurve, Point3, Wire,
};

use crate::command::EntityTransform;
use crate::entities::common::{ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};

fn to_truck(spl: &Spline) -> TruckEntity {
    let n = spl.control_points.len();
    if n < 2 {
        return TruckEntity {
            object: TruckObject::Point(builder::vertex(Point3::new(0.0, 0.0, 0.0))),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let knot_vec = if !spl.knots.is_empty() {
        KnotVec::from(spl.knots.clone())
    } else {
        KnotVec::uniform_knot(spl.degree as usize, n - 1)
    };

    // Use rational NURBS when weights are provided (circles/conics stored as NURBS).
    let use_nurbs = !spl.weights.is_empty() && spl.weights.len() == n;
    let (p_start, p_end, curve) = if use_nurbs {
        let homo_pts: Vec<Vector4> = spl
            .control_points
            .iter()
            .zip(spl.weights.iter())
            .map(|(p, &w)| {
                let w = if w.abs() < 1e-12 { 1.0 } else { w };
                Vector4::new(p.x * w, p.y * w, p.z * w, w)
            })
            .collect();
        let nurbs = NurbsCurve::new(BSplineCurve::new(knot_vec, homo_pts));
        let (t0, t1) = nurbs.range_tuple();
        (nurbs.subs(t0), nurbs.subs(t1), Curve::NurbsCurve(nurbs))
    } else {
        let ctrl_pts: Vec<Point3> = spl
            .control_points
            .iter()
            .map(|p| Point3::new(p.x, p.y, p.z))
            .collect();
        let bspline = BSplineCurve::new(knot_vec, ctrl_pts);
        let (t0, t1) = bspline.range_tuple();
        (
            bspline.subs(t0),
            bspline.subs(t1),
            Curve::BSplineCurve(bspline),
        )
    };

    let snap_source = if !spl.fit_points.is_empty() {
        &spl.fit_points
    } else {
        &spl.control_points
    };
    let key_vertices: Vec<[f64; 3]> = snap_source
        .iter()
        .map(|p| [p.x, p.y, p.z])
        .collect();

    let is_closed = spl.flags.closed || spl.flags.periodic;
    let gap = {
        let dx = (p_end.x - p_start.x) as f32;
        let dy = (p_end.y - p_start.y) as f32;
        let dz = (p_end.z - p_start.z) as f32;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    let object = if is_closed && gap > 1e-6 {
        let v_start = builder::vertex(p_start);
        let v_end = builder::vertex(p_end);
        let v_close = builder::vertex(p_start);
        let main_edge = Edge::new(&v_start, &v_end, curve);
        let close_edge = builder::line(&v_end, &v_close);
        let wire: Wire = [main_edge, close_edge].into_iter().collect();
        TruckObject::Contour(wire)
    } else {
        let v_start = builder::vertex(p_start);
        let v_end = builder::vertex(p_end);
        let edge = Edge::new(&v_start, &v_end, curve);
        TruckObject::Curve(edge)
    };

    TruckEntity {
        object,
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices,
        fill_tris: vec![],
    }
}

fn grips(spline: &Spline) -> Vec<GripDef> {
    spline
        .control_points
        .iter()
        .enumerate()
        .map(|(i, p)| square_grip(i, Vec3::new(p.x as f32, p.y as f32, p.z as f32)))
        .collect()
}

fn properties(spline: &Spline) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            ro("Degree", "degree", spline.degree.to_string()),
            ro(
                "Control Pts",
                "ctrl_pts",
                spline.control_points.len().to_string(),
            ),
            ro("Fit Pts", "fit_pts", spline.fit_points.len().to_string()),
        ],
    }
}

fn apply_geom_prop(_spline: &mut Spline, _field: &str, _value: &str) {}

fn apply_grip(spline: &mut Spline, grip_id: usize, apply: GripApply) {
    if let Some(cp) = spline.control_points.get_mut(grip_id) {
        match apply {
            GripApply::Absolute(p) => {
                cp.x = p.x as f64;
                cp.y = p.y as f64;
                cp.z = p.z as f64;
            }
            GripApply::Translate(d) => {
                cp.x += d.x as f64;
                cp.y += d.y as f64;
                cp.z += d.z as f64;
            }
        }
    }
}

fn apply_transform(spline: &mut Spline, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(spline, t, |entity, p1, p2| {
        for cp in &mut entity.control_points {
            crate::scene::transform::reflect_xy_point(&mut cp.x, &mut cp.y, p1, p2);
        }
        for fp in &mut entity.fit_points {
            crate::scene::transform::reflect_xy_point(&mut fp.x, &mut fp.y, p1, p2);
        }
    });
}

impl TruckConvertible for Spline {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for Spline {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for Spline {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Spline {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
