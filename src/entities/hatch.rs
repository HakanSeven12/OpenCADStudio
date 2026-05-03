use acadrust::entities::{BoundaryEdge, Hatch};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, parse_f64, ro_prop as ro};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable};
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};

fn properties(h: &Hatch) -> PropSection {
    let pattern_type = match h.pattern_type {
        acadrust::entities::HatchPatternType::Predefined => "Predefined",
        acadrust::entities::HatchPatternType::UserDefined => "User Defined",
        acadrust::entities::HatchPatternType::Custom => "Custom",
    };
    let style = match h.style {
        acadrust::entities::HatchStyleType::Normal => "Normal",
        acadrust::entities::HatchStyleType::Outer => "Outer",
        acadrust::entities::HatchStyleType::Ignore => "Ignore",
    };
    let fill_type = if h.gradient_color.enabled {
        format!("Gradient ({})", h.gradient_color.name)
    } else if h.is_solid {
        "Solid".into()
    } else {
        format!("Pattern ({})", h.pattern.name)
    };
    let boundary_count: usize = h
        .paths
        .iter()
        .map(|p| {
            p.edges
                .iter()
                .map(|e| match e {
                    BoundaryEdge::Polyline(poly) => poly.vertices.len(),
                    _ => 1,
                })
                .sum::<usize>()
        })
        .sum();
    PropSection {
        title: "Geometry".into(),
        props: vec![
            ro("Fill Type", "fill_type", fill_type),
            Property {
                label: "Pattern Name".into(),
                field: "pattern_name",
                value: PropValue::HatchPatternChoice(h.pattern.name.clone()),
            },
            ro("Pattern Type", "pattern_type", pattern_type),
            edit(
                "Pattern Angle",
                "pattern_angle",
                h.pattern_angle.to_degrees(),
            ),
            edit("Pattern Scale", "pattern_scale", h.pattern_scale),
            ro("Style", "style", style),
            ro("Boundary Paths", "path_count", h.paths.len().to_string()),
            ro("Boundary Verts", "vert_count", boundary_count.to_string()),
            ro("Double", "double", if h.is_double { "Yes" } else { "No" }),
            ro(
                "Associative",
                "associative",
                if h.is_associative { "Yes" } else { "No" },
            ),
            edit("Elevation", "elevation", h.elevation),
        ],
    }
}

fn apply_geom_prop(h: &mut Hatch, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "pattern_angle" => h.pattern_angle = v.to_radians(),
        "pattern_scale" if v > 0.0 => h.pattern_scale = v,
        "elevation" => h.elevation = v,
        _ => {}
    }
}

fn apply_transform(h: &mut Hatch, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(h, t, |entity, p1, p2| {
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-12 {
            return;
        }
        let line_angle = dy.atan2(dx);
        for path in &mut entity.paths {
            for edge in &mut path.edges {
                match edge {
                    BoundaryEdge::Line(l) => {
                        crate::scene::transform::reflect_xy_point(
                            &mut l.start.x,
                            &mut l.start.y,
                            p1,
                            p2,
                        );
                        crate::scene::transform::reflect_xy_point(
                            &mut l.end.x,
                            &mut l.end.y,
                            p1,
                            p2,
                        );
                    }
                    BoundaryEdge::CircularArc(a) => {
                        crate::scene::transform::reflect_xy_point(
                            &mut a.center.x,
                            &mut a.center.y,
                            p1,
                            p2,
                        );
                        let tmp = a.start_angle;
                        a.start_angle = 2.0 * line_angle - a.end_angle;
                        a.end_angle = 2.0 * line_angle - tmp;
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        crate::scene::transform::reflect_xy_point(
                            &mut e.center.x,
                            &mut e.center.y,
                            p1,
                            p2,
                        );
                        let ax = dx;
                        let ay = dy;
                        let rx = e.major_axis_endpoint.x;
                        let ry = e.major_axis_endpoint.y;
                        let dot = rx * ax + ry * ay;
                        e.major_axis_endpoint.x = 2.0 * dot * ax / len2 - rx;
                        e.major_axis_endpoint.y = 2.0 * dot * ay / len2 - ry;
                        let tmp = e.start_angle;
                        e.start_angle = 2.0 * line_angle - e.end_angle;
                        e.end_angle = 2.0 * line_angle - tmp;
                    }
                    BoundaryEdge::Spline(s) => {
                        for cp in &mut s.control_points {
                            crate::scene::transform::reflect_xy_point(&mut cp.x, &mut cp.y, p1, p2);
                        }
                        for fp in &mut s.fit_points {
                            crate::scene::transform::reflect_xy_point(&mut fp.x, &mut fp.y, p1, p2);
                        }
                    }
                    BoundaryEdge::Polyline(p) => {
                        for v in &mut p.vertices {
                            crate::scene::transform::reflect_xy_point(&mut v.x, &mut v.y, p1, p2);
                        }
                    }
                }
            }
        }
    });
}

impl PropertyEditable for Hatch {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Hatch {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

// ── Grip editing ───────────────────────────────────────────────────────────

/// Assign sequential grip IDs across all boundary paths and edges.
/// Exposed control points per edge type:
///   Polyline       → each vertex (x, y)
///   Line           → start, end
///   CircularArc    → center
///   EllipticArc    → center
///   Spline         → fit points if present, else control points (x, y)
impl Grippable for Hatch {
    fn grips(&self) -> Vec<GripDef> {
        let elev = self.elevation as f32;
        let mut out = Vec::new();
        let mut id = 0usize;
        for path in &self.paths {
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Polyline(p) => {
                        for v in &p.vertices {
                            out.push(diamond_grip(id, Vec3::new(v.x as f32, v.y as f32, elev)));
                            id += 1;
                        }
                    }
                    BoundaryEdge::Line(l) => {
                        out.push(diamond_grip(id, Vec3::new(l.start.x as f32, l.start.y as f32, elev)));
                        id += 1;
                        out.push(diamond_grip(id, Vec3::new(l.end.x as f32, l.end.y as f32, elev)));
                        id += 1;
                    }
                    BoundaryEdge::CircularArc(a) => {
                        out.push(diamond_grip(id, Vec3::new(a.center.x as f32, a.center.y as f32, elev)));
                        id += 1;
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        out.push(diamond_grip(id, Vec3::new(e.center.x as f32, e.center.y as f32, elev)));
                        id += 1;
                    }
                    BoundaryEdge::Spline(s) => {
                        let pts: Vec<[f64; 2]> = if !s.fit_points.is_empty() {
                            s.fit_points.iter().map(|p| [p.x, p.y]).collect()
                        } else {
                            s.control_points.iter().map(|p| [p.x, p.y]).collect()
                        };
                        for [x, y] in pts {
                            out.push(diamond_grip(id, Vec3::new(x as f32, y as f32, elev)));
                            id += 1;
                        }
                    }
                }
            }
        }
        out
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let elev = self.elevation as f32;
        let mut id = 0usize;

        fn resolve(apply: &GripApply, cur: Vec3) -> (f64, f64) {
            let p = match apply {
                GripApply::Absolute(p) => *p,
                GripApply::Translate(d) => cur + *d,
            };
            (p.x as f64, p.y as f64)
        }

        'outer: for path in &mut self.paths {
            for edge in &mut path.edges {
                match edge {
                    BoundaryEdge::Polyline(p) => {
                        for v in &mut p.vertices {
                            if id == grip_id {
                                let (nx, ny) = resolve(&apply, Vec3::new(v.x as f32, v.y as f32, elev));
                                v.x = nx;
                                v.y = ny;
                                break 'outer;
                            }
                            id += 1;
                        }
                    }
                    BoundaryEdge::Line(l) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(&apply, Vec3::new(l.start.x as f32, l.start.y as f32, elev));
                            l.start.x = nx;
                            l.start.y = ny;
                            break 'outer;
                        }
                        id += 1;
                        if id == grip_id {
                            let (nx, ny) = resolve(&apply, Vec3::new(l.end.x as f32, l.end.y as f32, elev));
                            l.end.x = nx;
                            l.end.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::CircularArc(a) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(&apply, Vec3::new(a.center.x as f32, a.center.y as f32, elev));
                            a.center.x = nx;
                            a.center.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(&apply, Vec3::new(e.center.x as f32, e.center.y as f32, elev));
                            e.center.x = nx;
                            e.center.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::Spline(s) => {
                        if !s.fit_points.is_empty() {
                            for fp in &mut s.fit_points {
                                if id == grip_id {
                                    let (nx, ny) = resolve(&apply, Vec3::new(fp.x as f32, fp.y as f32, elev));
                                    fp.x = nx;
                                    fp.y = ny;
                                    break 'outer;
                                }
                                id += 1;
                            }
                        } else {
                            for cp in &mut s.control_points {
                                if id == grip_id {
                                    let (nx, ny) = resolve(&apply, Vec3::new(cp.x as f32, cp.y as f32, elev));
                                    cp.x = nx;
                                    cp.y = ny;
                                    break 'outer;
                                }
                                id += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}
