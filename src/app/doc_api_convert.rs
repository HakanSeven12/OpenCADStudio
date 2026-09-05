// DocApi v2 entity conversions (plan §5.1): map the crate's plain-data DTOs to
// acadrust `EntityType` and back. Lives in the host (uses the real acadrust
// types), keeping the crate's DTOs dependency-light.

use acadrust::entities::{Arc, Circle, Ellipse, Line, LwPolyline, LwVertex, Point, Ray, Spline, XLine};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;
use ocs_doc_api::{Aabb, ApiError, ApiResult, Curve2Spec, ObjectId};

/// Convert a profile entity (Line/Circle/Arc/LwPolyline) to `geom2d::Curve`s for
/// sweep ops (`Extrude`/`Revolve`). Z is dropped (profiles lie on the XY plane).
pub fn entity_to_profile_curves(entity: &EntityType) -> ApiResult<Vec<cadkernel::geom2d::Curve>> {
    use cadkernel::geom2d::{Arc as KArc, Circle as KCircle, Curve as KCurve, Line as KLine};
    let curves = match entity {
        EntityType::Line(l) => vec![KCurve::Line(KLine {
            start: [l.start.x, l.start.y],
            end: [l.end.x, l.end.y],
        })],
        EntityType::Circle(c) => vec![KCurve::Circle(KCircle {
            centre: [c.center.x, c.center.y],
            radius: c.radius,
        })],
        EntityType::Arc(a) => vec![KCurve::Arc(KArc {
            centre: [a.center.x, a.center.y],
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        })],
        EntityType::LwPolyline(pl) => {
            if pl.vertices.len() < 2 {
                return Err(ApiError::validation("profile", "polyline profile needs >= 2 vertices"));
            }
            // Build a straight-segment polyline chain (bulge arcs land in a later
            // phase; straight segments cover the common rectangular profile case).
            let pts: Vec<[f64; 2]> = pl.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
            let mut segs = Vec::with_capacity(pts.len());
            for i in 0..pts.len() {
                let start = pts[i];
                let end = if i + 1 < pts.len() {
                    pts[i + 1]
                } else if pl.is_closed {
                    pts[0]
                } else {
                    break;
                };
                segs.push(KCurve::Line(KLine { start, end }));
            }
            segs
        }
        _ => {
            return Err(ApiError::Unsupported(
                "profile conversion is supported for Line/Circle/Arc/LwPolyline only".into(),
            ))
        }
    };
    Ok(curves)
}

/// Apply a rigid similarity (rotation+translation+uniform scale) to a non-solid
/// entity in place. Supports the common 2D families; `Solid3D` goes through the
/// kernel path instead. Returns the transformed entity (same handle) for the
/// caller to `update_entity`.
pub fn transform_entity_geometry(
    entity: &EntityType,
    p: &ocs_doc_api::PlacementSpec,
) -> ApiResult<EntityType> {
    // Affine point transform: out = R * (s * v) + t. Uniform scale s = |x_axis|.
    let s = (p.x_axis[0] * p.x_axis[0] + p.x_axis[1] * p.x_axis[1] + p.x_axis[2] * p.x_axis[2]).sqrt();
    let apply = |v: Vector3| -> Vector3 {
        let x = v.x * s;
        let y = v.y * s;
        let z = v.z * s;
        Vector3::new(
            p.x_axis[0] * x + p.y_axis[0] * y + p.z_axis[0] * z + p.origin[0],
            p.x_axis[1] * x + p.y_axis[1] * y + p.z_axis[1] * z + p.origin[1],
            p.x_axis[2] * x + p.y_axis[2] * y + p.z_axis[2] * z + p.origin[2],
        )
    };
    let entity = match entity {
        EntityType::Line(l) => {
            let mut l = l.clone();
            l.start = apply(l.start);
            l.end = apply(l.end);
            EntityType::Line(l)
        }
        EntityType::Circle(c) => {
            let mut c = c.clone();
            c.center = apply(c.center);
            c.radius *= s;
            EntityType::Circle(c)
        }
        EntityType::Arc(a) => {
            let mut a = a.clone();
            a.center = apply(a.center);
            a.radius *= s;
            EntityType::Arc(a)
        }
        EntityType::Ellipse(e) => {
            let mut e = e.clone();
            e.center = apply(e.center);
            // Scale the major-axis vector by the uniform scale (rigid similarity).
            e.major_axis = Vector3::new(
                e.major_axis.x * s, e.major_axis.y * s, e.major_axis.z * s,
            );
            EntityType::Ellipse(e)
        }
        EntityType::Spline(sp) => {
            let mut sp = sp.clone();
            for cp in &mut sp.control_points {
                *cp = apply(*cp);
            }
            for fp in &mut sp.fit_points {
                *fp = apply(*fp);
            }
            EntityType::Spline(sp)
        }
        EntityType::Ray(r) => {
            let mut r = r.clone();
            r.base_point = apply(r.base_point);
            EntityType::Ray(r)
        }
        EntityType::XLine(x) => {
            let mut x = x.clone();
            x.base_point = apply(x.base_point);
            EntityType::XLine(x)
        }
        EntityType::Insert(ins) => {
            let mut ins = ins.clone();
            ins.insert_point = apply(ins.insert_point);
            EntityType::Insert(ins)
        }
        EntityType::Viewport(vp) => {
            let mut vp = vp.clone();
            vp.center = apply(vp.center);
            EntityType::Viewport(vp)
        }
        EntityType::Point(pt) => {
            let mut pt = pt.clone();
            pt.location = apply(pt.location);
            EntityType::Point(pt)
        }
        EntityType::LwPolyline(pl) => {
            let mut pl = pl.clone();
            let z0 = pl.elevation;
            for v in &mut pl.vertices {
                let t = apply(Vector3::new(v.location.x, v.location.y, z0));
                v.location = Vector2::new(t.x, t.y);
                pl.elevation = t.z;
            }
            EntityType::LwPolyline(pl)
        }
        _ => {
            return Err(ApiError::Unsupported(
                "transform is supported for Line/Circle/Arc/Point/LwPolyline (and solids) in v1".into(),
            ))
        }
    };
    Ok(entity)
}

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// Build an acadrust entity for a 2D curve construction spec.
pub fn curve_spec_to_entity(spec: &Curve2Spec) -> ApiResult<EntityType> {
    let entity = match spec {
        Curve2Spec::Line { start, end } => {
            let mut e = Line::new();
            e.start = v3(*start);
            e.end = v3(*end);
            EntityType::Line(e)
        }
        Curve2Spec::Circle { centre, radius } => {
            let mut e = Circle::new();
            e.center = v3(*centre);
            e.radius = *radius;
            EntityType::Circle(e)
        }
        Curve2Spec::Polyline { points, closed } => {
            let mut e = LwPolyline::new();
            e.vertices = points
                .iter()
                .map(|p| LwVertex {
                    location: Vector2::new(p[0], p[1]),
                    bulge: 0.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 0,
                })
                .collect();
            e.is_closed = *closed;
            EntityType::LwPolyline(e)
        }
        Curve2Spec::Point { position } => {
            let mut e = Point::new();
            e.location = v3(*position);
            EntityType::Point(e)
        }
        Curve2Spec::Arc { centre, radius, start_angle, end_angle } => {
            let mut e = Arc::new();
            e.center = v3(*centre);
            e.radius = *radius;
            e.start_angle = *start_angle;
            e.end_angle = *end_angle;
            EntityType::Arc(e)
        }
        Curve2Spec::Ellipse { centre, major_axis, ratio, start, end } => {
            let mut e = Ellipse::new();
            e.center = v3(*centre);
            e.major_axis = v3(*major_axis);
            e.minor_axis_ratio = *ratio;
            e.start_parameter = *start;
            e.end_parameter = *end;
            EntityType::Ellipse(e)
        }
        Curve2Spec::Spline { degree, control_points, knots, weights } => {
            let mut e = Spline::new();
            e.degree = *degree;
            e.control_points = control_points.iter().map(|p| v3(*p)).collect();
            e.knots = knots.clone();
            e.weights = weights.clone();
            EntityType::Spline(e)
        }
        Curve2Spec::Ray { origin, direction } => {
            EntityType::Ray(Ray::new(v3(*origin), v3(*direction)))
        }
        Curve2Spec::XLine { origin, direction } => {
            EntityType::XLine(XLine::new(v3(*origin), v3(*direction)))
        }
    };
    Ok(entity)
}

/// The acadrust `EntityType` variant name (for `EntityView::kind` / downcasts).
pub fn entity_kind_name(entity: &EntityType) -> &'static str {
    match entity {
        EntityType::Solid3D(_) => "Solid3D",
        EntityType::Line(_) => "Line",
        EntityType::Circle(_) => "Circle",
        EntityType::LwPolyline(_) => "LwPolyline",
        EntityType::Polyline(_) => "Polyline",
        EntityType::Point(_) => "Point",
        EntityType::Arc(_) => "Arc",
        EntityType::Ellipse(_) => "Ellipse",
        EntityType::Spline(_) => "Spline",
        EntityType::Ray(_) => "Ray",
        EntityType::XLine(_) => "XLine",
        EntityType::Insert(_) => "Insert",
        EntityType::Viewport(_) => "Viewport",
        EntityType::Text(_) => "Text",
        EntityType::MText(_) => "MText",
        // Media & misc (phase 5, read-mostly): named so GetEntity reports a real kind.
        EntityType::RasterImage(_) => "RasterImage",
        EntityType::Table(_) => "Table",
        EntityType::Leader(_) => "Leader",
        EntityType::MultiLeader(_) => "MultiLeader",
        EntityType::MLine(_) => "MLine",
        EntityType::Mesh(_) => "Mesh",
        EntityType::Helix(_) => "Helix",
        EntityType::Region(_) => "Region",
        EntityType::Body(_) => "Body",
        EntityType::Surface(_) => "Surface",
        EntityType::Face3D(_) => "Face3D",
        EntityType::Dimension(_) => "Dimension",
        EntityType::Hatch(_) => "Hatch",
        _ => "Other",
    }
}

/// Coarse bounds for a non-solid entity (solids go through the kernel cache).
/// For v1 typed curves we compute the obvious bounds; anything else errors
/// `Unsupported` (viewport/annotation bounds land in later phases).
pub fn entity_bounds(entity: Option<&EntityType>, id: ObjectId) -> ApiResult<Aabb> {
    let entity = entity.ok_or(ApiError::UnknownId(id))?;
    let bb = match entity {
        EntityType::Line(l) => Aabb {
            min: [
                l.start.x.min(l.end.x),
                l.start.y.min(l.end.y),
                l.start.z.min(l.end.z),
            ],
            max: [
                l.start.x.max(l.end.x),
                l.start.y.max(l.end.y),
                l.start.z.max(l.end.z),
            ],
        },
        EntityType::Circle(c) => Aabb {
            min: [c.center.x - c.radius, c.center.y - c.radius, c.center.z],
            max: [c.center.x + c.radius, c.center.y + c.radius, c.center.z],
        },
        EntityType::Point(p) => Aabb {
            min: [p.location.x, p.location.y, p.location.z],
            max: [p.location.x, p.location.y, p.location.z],
        },
        EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for vtx in &pl.vertices {
                let (x, y, z) = (vtx.location.x, vtx.location.y, pl.elevation);
                min = [min[0].min(x), min[1].min(y), min[2].min(z)];
                max = [max[0].max(x), max[1].max(y), max[2].max(z)];
            }
            Aabb { min, max }
        }
        EntityType::Arc(a) => {
            // Coarse: full-circle bounds (exact arc bounds is a later refinement).
            Aabb {
                min: [a.center.x - a.radius, a.center.y - a.radius, a.center.z],
                max: [a.center.x + a.radius, a.center.y + a.radius, a.center.z],
            }
        }
        EntityType::Ellipse(e) => {
            // Coarse: use the major-axis length as the bounding radius.
            let r = (e.major_axis.x * e.major_axis.x
                + e.major_axis.y * e.major_axis.y
                + e.major_axis.z * e.major_axis.z)
                .sqrt();
            Aabb {
                min: [e.center.x - r, e.center.y - r, e.center.z - r],
                max: [e.center.x + r, e.center.y + r, e.center.z + r],
            }
        }
        EntityType::Spline(s) if !s.control_points.is_empty() => {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for p in &s.control_points {
                min = [min[0].min(p.x), min[1].min(p.y), min[2].min(p.z)];
                max = [max[0].max(p.x), max[1].max(p.y), max[2].max(p.z)];
            }
            Aabb { min, max }
        }
        EntityType::Viewport(vp) => Aabb {
            min: [
                vp.center.x - vp.width / 2.0,
                vp.center.y - vp.height / 2.0,
                vp.center.z,
            ],
            max: [
                vp.center.x + vp.width / 2.0,
                vp.center.y + vp.height / 2.0,
                vp.center.z,
            ],
        },
        EntityType::RasterImage(img) => {
            // Coarse image rect: origin + u*width + v*height (in world units).
            let w = img.u_vector.x * img.size.x + img.v_vector.x * img.size.y;
            let h = img.u_vector.y * img.size.x + img.v_vector.y * img.size.y;
            let (x0, y0) = (img.insertion_point.x, img.insertion_point.y);
            Aabb {
                min: [x0.min(x0 + w), y0.min(y0 + h), img.insertion_point.z],
                max: [x0.max(x0 + w), y0.max(y0 + h), img.insertion_point.z],
            }
        }
        // Ray/XLine are unbounded by definition — no finite bounds.
        _ => return Err(ApiError::Unsupported("bounds for this entity family is not yet implemented".into())),
    };
    Ok(bb)
}
