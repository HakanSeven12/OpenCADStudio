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

/// The curve that Geometry Cleanup reads for an entity: a line, an arc or a
/// LWPOLYLINE that lies on the world XY plane. `None` for anything else.
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
        _ => None,
    }
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
            let n = polyline.vertices.len();
            if n < 2 {
                return None;
            }
            // The end vertex and the segment it ends.
            let (vertex, segment) = if at_start { (0, 0) } else { (n - 1, n - 2) };
            let from = polyline.vertices[segment].location;
            let next = polyline.vertices[segment + 1].location;
            let bulge = polyline.vertices[segment].bulge;
            if let Some(arc) =
                ocs_geom::geom2d::BulgeArc::from_bulge([from.x, from.y], [next.x, next.y], bulge)
            {
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
                polyline.vertices[segment].bulge = (sweep / 4.0).tan();
            }
            polyline.vertices[vertex].location.x = to[0];
            polyline.vertices[vertex].location.y = to[1];
        }
        _ => return None,
    }
    Some(result)
}

/// `entity` with its end grip at the start or the end moved to `to`: the
/// Stretch repair, which does what a drag of that grip does.
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
