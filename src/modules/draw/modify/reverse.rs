// REVERSE command — reverse the direction (vertex / point order) of a single
// picked curve.
//
// The user picks one open or closed curve; the command builds a reversed copy
// and swaps it in via `CmdResult::ReplaceEntity`. Supported curve types:
//
//   * Line        — swap start / end.
//   * LwPolyline / Polyline2D — reverse the vertex list. The bulge stored at a
//                   vertex
//                   describes the arc on the segment *starting* at that vertex,
//                   so a plain `vertices.reverse()` would attach each bulge to
//                   the wrong segment (and with the wrong sense). We therefore
//                   recompute every bulge from the original segment bulges (see
//                   `reverse_lwpolyline`).
//   * Polyline / Polyline3D — reverse the vertex list (no bulges to reconcile).
//   * Spline      — reverse control points and fit points, then regenerate the
//                   clamped knot vector (mirrors the REVERSE branch of
//                   `splinedit::apply_spline_op`).
//
// Any other entity type is left untouched: the command returns
// `CmdResult::NeedPoint` and keeps prompting so nothing is corrupted.

use crate::t;
use document::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub(crate) use crate::entities::reverse::reverse_spline;

// ── Ribbon definition ───────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "REVERSE",
        label: "Reverse",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("REVERSE".to_string()),
    }
}

// ── Command implementation ──────────────────────────────────────────────────

pub struct ReverseCommand {
    /// The picked entity, injected by the host before `on_entity_pick` runs.
    /// `None` until the host injects it.
    picked: Option<EntityType>,
}

impl Default for ReverseCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl ReverseCommand {
    pub fn new() -> Self {
        Self { picked: None }
    }

    /// Build a reversed copy of `entity`, or `None` for an unsupported type.
    pub(crate) fn reversed(entity: &EntityType) -> Option<EntityType> {
        crate::entities::reverse::reversed(entity)
    }
}

impl CadCommand for ReverseCommand {
    fn name(&self) -> &'static str {
        "REVERSE"
    }

    fn prompt(&self) -> String {
        t!("REVERSE  Select line, polyline, spline or helix to reverse:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.picked.as_ref().and_then(Self::reversed) {
            Some(reversed) => CmdResult::ReplaceEntity(handle, vec![reversed]),
            // Unsupported entity type — keep prompting, do not corrupt it.
            None => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ───────────────────────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["REVERSE"]
}); // ReverseCommand

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::Spline;
    use document::types::Vector3;

    #[test]
    fn spline_reversal_preserves_knot_spacing_weights_and_tangents() {
        let mut spline = Spline::new();
        spline.degree = 2;
        spline.control_points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 2.0, 0.0),
            Vector3::new(3.0, 1.0, 1.0),
            Vector3::new(4.0, 0.0, 2.0),
        ];
        spline.knots = vec![0.0, 0.0, 0.0, 0.25, 1.0, 1.0, 1.0];
        spline.weights = vec![1.0, 2.0, 3.0, 4.0];
        spline.begin_tangent = Vector3::new(1.0, 2.0, 3.0);
        spline.end_tangent = Vector3::new(4.0, 5.0, 6.0);

        let reversed = reverse_spline(&spline);
        assert_eq!(
            reversed.control_points,
            spline
                .control_points
                .iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(reversed.weights, vec![4.0, 3.0, 2.0, 1.0]);
        assert_eq!(reversed.knots, vec![0.0, 0.0, 0.0, 0.75, 1.0, 1.0, 1.0]);
        assert_eq!(reversed.begin_tangent, Vector3::new(-4.0, -5.0, -6.0));
        assert_eq!(reversed.end_tangent, Vector3::new(-1.0, -2.0, -3.0));

        let restored = reverse_spline(&reversed);
        assert_eq!(restored.control_points, spline.control_points);
        assert_eq!(restored.weights, spline.weights);
        assert_eq!(restored.knots, spline.knots);
        assert_eq!(restored.begin_tangent, spline.begin_tangent);
        assert_eq!(restored.end_tangent, spline.end_tangent);
    }
}
