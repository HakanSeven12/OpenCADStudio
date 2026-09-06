//! Entity conversions: map the crate's plain-data DTOs to acadrust
//! `EntityType` and back. Host feature only (uses the real acadrust types). This
//! module is host-side-capable but host-independent — it operates purely on
//! acadrust entities and crate DTOs, so any host can reuse it.

use acadrust::entities::{
    Arc, Circle, Ellipse, Line, LwPolyline, LwVertex, Point, Ray, Spline, XLine,
};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;

use crate::{Aabb, ApiError, ApiResult, Curve2Spec, GeometryErrorKind, ObjectId};

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
                return Err(ApiError::validation(
                    "profile",
                    "polyline profile needs at least two vertices",
                ));
            }
            KCurve::Polyline(cadkernel::geom2d::Polyline {
                vertices: pl
                    .vertices
                    .iter()
                    .map(|v| cadkernel::geom2d::PolylineVertex {
                        position: [v.location.x, v.location.y],
                        bulge: v.bulge,
                    })
                    .collect(),
                closed: pl.is_closed,
            })
            .segments()
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
    p: &crate::PlacementSpec,
) -> ApiResult<EntityType> {
    crate::geom::validate_placement(p)?;
    let matrix = acadrust::types::Matrix4 {
        m: [
            [p.x_axis[0], p.y_axis[0], p.z_axis[0], p.origin[0]],
            [p.x_axis[1], p.y_axis[1], p.z_axis[1], p.origin[1]],
            [p.x_axis[2], p.y_axis[2], p.z_axis[2], p.origin[2]],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let mut moved = entity.clone();
    moved.apply_transform(&acadrust::types::Transform::from_matrix(matrix));
    Ok(moved)
}

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// Convert a polyline bulge segment (start → end, bulge = tan(θ/4)) to a
/// `geom2d::Curve::Arc`. Positive bulge = counter-clockwise arc; negative = clockwise.
pub fn bulge_arc_segment(
    start: [f64; 2],
    end: [f64; 2],
    bulge: f64,
) -> ApiResult<cadkernel::geom2d::Curve> {
    use cadkernel::geom2d::{Arc as KArc, Curve as KCurve};
    let arc = cadkernel::geom2d::BulgeArc::from_bulge(start, end, bulge)
        .ok_or_else(|| ApiError::validation("profile", "invalid bulge segment"))?;
    // The kernel's Arc is counter-clockwise; reverse clockwise endpoints.
    let (start_angle, end_angle) = if arc.sweep < 0.0 {
        (arc.end_angle, arc.start_angle)
    } else {
        (arc.start_angle, arc.end_angle)
    };
    Ok(KCurve::Arc(KArc {
        centre: arc.center,
        radius: arc.radius,
        start_angle,
        end_angle,
    }))
}

/// Build an acadrust entity for a 2D curve construction spec.
pub fn curve_spec_to_entity(spec: &Curve2Spec) -> ApiResult<EntityType> {
    crate::validation::curve(spec)?;
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
            e.elevation = points.first().map_or(0.0, |p| p[2]);
            if points.iter().any(|p| p[2] != e.elevation) {
                return Err(ApiError::validation(
                    "CreateCurve",
                    "polyline points must share an elevation",
                ));
            }
            e.is_closed = *closed;
            EntityType::LwPolyline(e)
        }
        Curve2Spec::Point { position } => {
            let mut e = Point::new();
            e.location = v3(*position);
            EntityType::Point(e)
        }
        Curve2Spec::Arc {
            centre,
            radius,
            start_angle,
            end_angle,
        } => {
            let mut e = Arc::new();
            e.center = v3(*centre);
            e.radius = *radius;
            e.start_angle = *start_angle;
            e.end_angle = *end_angle;
            EntityType::Arc(e)
        }
        Curve2Spec::Ellipse {
            centre,
            major_axis,
            ratio,
            start,
            end,
        } => {
            // Validate the minor/major ratio so the (major-axis) bounds never
            // under-bound: ratio must be in (0, 1].
            if !(*ratio > 0.0 && *ratio <= 1.0) {
                return Err(ApiError::validation(
                    "CreateCurve",
                    format!("ellipse minor_axis_ratio must be in (0, 1], got {ratio}"),
                ));
            }
            let mut e = Ellipse::new();
            e.center = v3(*centre);
            e.major_axis = v3(*major_axis);
            e.minor_axis_ratio = *ratio;
            e.start_parameter = *start;
            e.end_parameter = *end;
            EntityType::Ellipse(e)
        }
        Curve2Spec::Spline {
            degree,
            control_points,
            knots,
            weights,
        } => {
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
        EntityType::AttributeDefinition(_) => "AttributeDefinition",
        EntityType::AttributeEntity(_) => "AttributeEntity",
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

/// Existing entity bounds; unbounded or empty geometry returns an error.
pub fn entity_bounds(entity: Option<&EntityType>, id: ObjectId) -> ApiResult<Aabb> {
    let entity = entity.ok_or(ApiError::UnknownId(id))?;
    if matches!(entity, EntityType::Ray(_) | EntityType::XLine(_)) {
        return Err(ApiError::Unsupported("unbounded entity".into()));
    }
    let bounds = entity.as_entity().bounding_box();
    let min = [bounds.min.x, bounds.min.y, bounds.min.z];
    let max = [bounds.max.x, bounds.max.y, bounds.max.z];
    if min.iter().chain(&max).any(|v| !v.is_finite()) {
        return Err(ApiError::geometry(
            GeometryErrorKind::Empty,
            "entity has no finite bounds",
        ));
    }
    Ok(Aabb { min, max })
}
