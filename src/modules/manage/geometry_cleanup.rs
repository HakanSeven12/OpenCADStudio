//! Geometry Cleanup (GEOMETRYCLEANUPOPEN): the objects it reads, and the
//! edits that repair an end. The scan itself is
//! `ocs_geom::geom2d::cleanup`; the palette is `ui::window::geometry_cleanup`.

use document::types::Vector3;
use document::{Document, EntityType, Handle};
use glam::DVec3;
use ocs_geom::geom2d::Curve;

use crate::command::{CadCommand, CmdResult};
use crate::entities::traits::EntityTypeOps;
use crate::entities::transform::EntityTransform;
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/geometry_cleanup.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "GEOMETRYCLEANUPOPEN",
        label: "Geometry Cleanup",
        icon: ICON,
        event: ModuleEvent::Command("GEOMETRYCLEANUPOPEN".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GEOMETRYCLEANUPOPEN", "GEOMETRYCLEANUPCLOSE"]
});

/// Nearer than this to zero counts as zero: a Z, an elevation, the X and Y
/// of a normal.
const FLAT: f64 = 1e-9;

fn on_xy(v: Vector3) -> bool {
    v.z.abs() <= FLAT
}

fn up(normal: Vector3) -> bool {
    normal.x.abs() <= FLAT && normal.y.abs() <= FLAT && normal.z > 0.0
}

/// The curve that Geometry Cleanup reads for an entity that lies on the world
/// XY plane: a line, an arc, an elliptical arc (not a whole ellipse), a
/// LWPOLYLINE, or a 2D POLYLINE that is not curve- or spline-fit. `None` for
/// anything else.
pub fn cleanup_curve(entity: &EntityType) -> Option<Curve> {
    match entity {
        EntityType::Line(line) if on_xy(line.start) && on_xy(line.end) => {
            Some(Curve::Line(ocs_geom::geom2d::Line {
                start: [line.start.x, line.start.y],
                end: [line.end.x, line.end.y],
            }))
        }
        EntityType::Arc(arc) if up(arc.normal) && on_xy(arc.center) => {
            Some(crate::entities::curve::arc_curve(arc).curve)
        }
        EntityType::LwPolyline(polyline)
            if up(polyline.normal) && polyline.elevation.abs() <= FLAT =>
        {
            crate::entities::curve::lwpolyline_curve(polyline).map(|curve| curve.curve)
        }
        EntityType::Ellipse(ellipse) if up(ellipse.normal) && on_xy(ellipse.center) => {
            match crate::entities::curve::ellipse_curve(ellipse)?.curve {
                Curve::Ellipse(arc) if arc.sweep() < std::f64::consts::TAU - FLAT => {
                    Some(Curve::Ellipse(arc))
                }
                _ => None,
            }
        }
        EntityType::Polyline2D(polyline)
            if up(polyline.normal)
                && polyline.elevation.abs() <= FLAT
                && polyline.flags.bits() & FIT_FLAGS == 0 =>
        {
            crate::entities::curve::polyline2d_curve(polyline).map(|curve| curve.curve)
        }
        _ => None,
    }
}

/// The POLYLINE flags of a curve-fit (2) and a spline-fit (4) polyline: its
/// vertices are not the curve that it draws.
const FIT_FLAGS: u16 = 2 | 4;

/// A polyline's vertices as (position, bulge) pairs.
fn lw_points(polyline: &document::entities::LwPolyline) -> Vec<([f64; 2], f64)> {
    polyline
        .vertices
        .iter()
        .map(|v| ([v.location.x, v.location.y], v.bulge))
        .collect()
}

fn heavy_points(polyline: &document::entities::Polyline2D) -> Vec<([f64; 2], f64)> {
    polyline
        .vertices
        .iter()
        .map(|v| ([v.location.x, v.location.y], v.bulge))
        .collect()
}

/// Move the end vertex of a polyline's points to `to`, along its circle when
/// the end segment is an arc: see [`moved_along`].
fn moved_end(points: &mut [([f64; 2], f64)], at_start: bool, to: [f64; 2]) -> Option<()> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    // The end vertex and the segment it ends.
    let (vertex, segment) = if at_start { (0, 0) } else { (n - 1, n - 2) };
    let (from, bulge) = points[segment];
    let next = points[segment + 1].0;
    if let Some(arc) = ocs_geom::geom2d::BulgeArc::from_bulge(from, next, bulge) {
        // Keep the circle: the moved end goes round it the same way.
        let angle = |p: [f64; 2]| (p[1] - arc.center[1]).atan2(p[0] - arc.center[0]);
        let (a, b) = if at_start {
            (angle(to), arc.end_angle)
        } else {
            (arc.start_angle, angle(to))
        };
        let turn = arc.sweep.signum();
        let sweep = turn * (turn * (b - a)).rem_euclid(std::f64::consts::TAU);
        if sweep.abs() <= FLAT {
            return None;
        }
        points[segment].1 = (sweep / 4.0).tan();
    }
    points[vertex].0 = to;
    Some(())
}

/// Whether Geometry Cleanup may read and change `handle`: a supported curve
/// in model space, on a layer that is not locked or frozen.
pub fn is_candidate(document: &Document, handle: Handle) -> bool {
    let Some(entity) = document.get_entity(handle) else {
        return false;
    };
    let model = document.block_records.get("*Model_Space").map(|r| r.handle);
    let layer_ok = document
        .layers
        .get(&entity.common().layer)
        .is_none_or(|layer| !layer.is_locked() && !layer.is_frozen());
    model == Some(entity.common().owner_handle) && layer_ok && cleanup_curve(entity).is_some()
}

/// `entity` with the end at its start (`at_start`) or its end moved to
/// `to` along its own line or circle: the Trim and Extend repairs. `to` is
/// a point on that line or circle.
pub fn moved_along(entity: &EntityType, at_start: bool, to: [f64; 2]) -> Option<EntityType> {
    let mut result = entity.clone();
    match &mut result {
        EntityType::Line(line) => {
            let point = if at_start {
                &mut line.start
            } else {
                &mut line.end
            };
            point.x = to[0];
            point.y = to[1];
        }
        EntityType::Arc(arc) => {
            let angle = (to[1] - arc.center.y).atan2(to[0] - arc.center.x);
            if at_start {
                arc.start_angle = angle;
            } else {
                arc.end_angle = angle;
            }
        }
        EntityType::LwPolyline(polyline) => {
            let mut points = lw_points(polyline);
            moved_end(&mut points, at_start, to)?;
            for (vertex, (position, bulge)) in polyline.vertices.iter_mut().zip(points) {
                vertex.location.x = position[0];
                vertex.location.y = position[1];
                vertex.bulge = bulge;
            }
        }
        EntityType::Polyline2D(polyline) => {
            let mut points = heavy_points(polyline);
            moved_end(&mut points, at_start, to)?;
            for (vertex, (position, bulge)) in polyline.vertices.iter_mut().zip(points) {
                vertex.location.x = position[0];
                vertex.location.y = position[1];
                vertex.bulge = bulge;
            }
        }
        // The end goes round its ellipse to the point nearest `to`.
        EntityType::Ellipse(ellipse) => {
            let Some(Curve::Ellipse(arc)) = cleanup_curve(entity) else {
                return None;
            };
            let parameter = arc
                .ellipse
                .closest_parameter(to)
                .rem_euclid(std::f64::consts::TAU);
            if at_start {
                ellipse.start_parameter = parameter;
            } else {
                ellipse.end_parameter = parameter;
            }
        }
        _ => return None,
    }
    Some(result)
}

/// `entity` with its end grip at the start or the end moved to `to`: the
/// Stretch repair, which does what a drag of that grip does. An elliptical
/// arc does not get it (its end grip only goes round its ellipse).
pub fn stretched(entity: &EntityType, at_start: bool, to: [f64; 2]) -> Option<EntityType> {
    let curve = cleanup_curve(entity)?;
    let end = curve.point_at(if at_start { 0.0 } else { 1.0 });
    let grip = EntityTypeOps::grips(entity)
        .into_iter()
        .filter(|grip| !grip.is_midpoint)
        .min_by(|a, b| {
            let d = |g: &crate::scene::model::object::GripDef| {
                (g.world.x - end[0]).hypot(g.world.y - end[1])
            };
            d(a).total_cmp(&d(b))
        })?;
    let mut result = entity.clone();
    EntityTypeOps::apply_grip(
        &mut result,
        grip.id,
        crate::scene::model::object::GripApply::Absolute(DVec3::new(to[0], to[1], 0.0)),
    );
    Some(result)
}

/// `entity` moved by `by`: the Move repair.
pub fn moved(entity: &EntityType, by: [f64; 2]) -> EntityType {
    let mut result = entity.clone();
    EntityTypeOps::apply_transform(
        &mut result,
        &EntityTransform::Translate(DVec3::new(by[0], by[1], 0.0)),
    );
    result
}

/// `entity` turned by `angle` radians counter-clockwise about `centre`:
/// the Rotate repairs.
pub fn rotated(entity: &EntityType, centre: [f64; 2], angle: f64) -> EntityType {
    let mut result = entity.clone();
    EntityTypeOps::apply_transform(
        &mut result,
        &EntityTransform::Rotate {
            center: DVec3::new(centre[0], centre[1], 0.0),
            axis: DVec3::Z,
            angle_rad: angle,
        },
    );
    result
}

/// The start and the end of the straight part of `entity` that an angle
/// error is on: a LINE (`segment` is `None`), or segment `segment` of a
/// polyline, from its vertex `segment` to the next one.
pub fn straight_part(entity: &EntityType, segment: Option<usize>) -> Option<[[f64; 2]; 2]> {
    let (points, closed) = match (entity, segment) {
        (EntityType::Line(line), None) => {
            return Some([[line.start.x, line.start.y], [line.end.x, line.end.y]]);
        }
        (EntityType::LwPolyline(polyline), Some(_)) => (lw_points(polyline), polyline.is_closed),
        (EntityType::Polyline2D(polyline), Some(_)) => {
            (heavy_points(polyline), polyline.flags.is_closed())
        }
        _ => return None,
    };
    let [from, to] = segment_vertices(points.len(), closed, segment?)?;
    Some([points[from].0, points[to].0])
}

/// The two vertices of segment `segment` of a polyline of `count` vertices;
/// the last segment of a closed one ends at the first vertex.
fn segment_vertices(count: usize, closed: bool, segment: usize) -> Option<[usize; 2]> {
    let segments = if closed {
        count
    } else {
        count.saturating_sub(1)
    };
    (count >= 2 && segment < segments).then(|| [segment, (segment + 1) % count])
}

/// `entity` with its straight part (see [`straight_part`]) turned by `angle`
/// radians counter-clockwise about `centre`: the Rotate repairs. A LINE
/// turns whole. A polyline segment moves its two vertices, so the segments
/// next to it keep their other vertices: a vertex on `centre` stays.
pub fn rotated_part(
    entity: &EntityType,
    segment: Option<usize>,
    centre: [f64; 2],
    angle: f64,
) -> Option<EntityType> {
    let Some(segment) = segment else {
        return matches!(entity, EntityType::Line(_)).then(|| rotated(entity, centre, angle));
    };
    let (sin, cos) = angle.sin_cos();
    let turned = |x: &mut f64, y: &mut f64| {
        let (dx, dy) = (*x - centre[0], *y - centre[1]);
        *x = centre[0] + dx * cos - dy * sin;
        *y = centre[1] + dx * sin + dy * cos;
    };
    let mut result = entity.clone();
    match &mut result {
        EntityType::LwPolyline(polyline) => {
            let vertices = segment_vertices(polyline.vertices.len(), polyline.is_closed, segment)?;
            for index in vertices {
                let location = &mut polyline.vertices[index].location;
                turned(&mut location.x, &mut location.y);
            }
        }
        EntityType::Polyline2D(polyline) => {
            let closed = polyline.flags.is_closed();
            let vertices = segment_vertices(polyline.vertices.len(), closed, segment)?;
            for index in vertices {
                let location = &mut polyline.vertices[index].location;
                turned(&mut location.x, &mut location.y);
            }
        }
        _ => return None,
    }
    Some(result)
}

/// "Select objects" in the palette: a selection, then the scan.
pub struct SelectObjects;

impl CadCommand for SelectObjects {
    fn name(&self) -> &'static str {
        "GEOMETRYCLEANUPOPEN"
    }
    fn prompt(&self) -> String {
        "Select objects:".into()
    }
    fn is_selection_gathering(&self) -> bool {
        true
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        let list: Vec<String> = handles.iter().map(|h| format!("{:X}", h.value())).collect();
        CmdResult::Dispatch(format!("_GEOMETRYCLEANUPSCAN {}", list.join(",")))
    }
    fn on_point(&mut self, _: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
