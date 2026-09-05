// DocApi v2 entity conversions (plan §5.1): map the crate's plain-data DTOs to
// acadrust `EntityType` and back. Lives in the host (uses the real acadrust
// types), keeping the crate's DTOs dependency-light.

use acadrust::entities::{Circle, Line, LwPolyline, LwVertex, Point};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;
use ocs_doc_api::{Aabb, ApiError, ApiResult, Curve2Spec, ObjectId};

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
        EntityType::Text(_) => "Text",
        EntityType::MText(_) => "MText",
        EntityType::Insert(_) => "Insert",
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
        _ => return Err(ApiError::Unsupported("bounds for this entity family is not yet implemented".into())),
    };
    Ok(bb)
}
