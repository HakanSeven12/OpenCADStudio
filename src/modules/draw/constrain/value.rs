//! DCONSTRAINT / ACONSTRAINT — constraints that need a typed target value
//! (a distance or an angle), unlike the plain select-and-click constraints
//! in `mod.rs`.
//!
//! Persistent (design doc `docs/parametric_system_design.md` §6.1): these
//! don't solve anything themselves. A `CadCommand`'s `on_text_input` only
//! gets `&mut self` — no document access — so it can't add a
//! `SketchConstraint` to the scene directly; instead these commands hand the
//! typed value back as `CmdResult::AddSketchConstraint`, and the host (which
//! does have `&mut Scene`) adds the record and solves it via the same
//! `Scene::bump_entities` path any later edit to these entities will use too.
//! `default_value` (what the prompt shows in `<...>`, and what a bare Enter
//! submits) is still computed from a one-time read of current geometry —
//! that part doesn't change.

use acadrust::entities::EntityType;
use acadrust::types::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::sketch_constraints::{ConstraintKind, SketchRef};
use crate::scene::Scene;

pub mod distance_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "DCONSTRAINT",
            label: "Distance",
            icon: IconKind::Glyph("↔"),
            event: ModuleEvent::Command("DCONSTRAINT".to_string()),
        }
    }
}

pub mod angle_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "ACONSTRAINT",
            label: "Angle",
            icon: IconKind::Glyph("∠"),
            event: ModuleEvent::Command("ACONSTRAINT".to_string()),
        }
    }
}

/// Constrains a single line's length (between its two endpoints) or a
/// circle's radius, to a typed target value.
pub struct DistanceConstraintCommand {
    handle: Handle,
    /// Which kind this becomes once a value comes in — decided once at
    /// construction from the entity's type, matching `ConstraintKind`'s own
    /// split between line-length (`Distance`) and circle-radius (`Radius`).
    is_circle: bool,
    default_value: f64,
}

impl DistanceConstraintCommand {
    /// `None` if `handle` isn't a Line or Circle.
    pub fn new(scene: &Scene, handle: Handle) -> Option<Self> {
        let entity = scene.document.get_entity(handle)?;
        let (is_circle, default_value) = match entity {
            EntityType::Line(l) => {
                let dx = l.end.x - l.start.x;
                let dy = l.end.y - l.start.y;
                (false, (dx * dx + dy * dy).sqrt())
            }
            EntityType::Circle(c) => (true, c.radius),
            _ => return None,
        };
        Some(Self { handle, is_circle, default_value })
    }

    fn build(&self, target: f64) -> Option<CmdResult> {
        if target <= 0.0 {
            return None;
        }
        let (kind, refs, label) = if self.is_circle {
            (ConstraintKind::Radius, vec![SketchRef::whole(self.handle)], "Radius constraint")
        } else {
            (
                ConstraintKind::Distance,
                vec![SketchRef::point(self.handle, 0), SketchRef::point(self.handle, 1)],
                "Distance constraint",
            )
        };
        Some(CmdResult::AddSketchConstraint { kind, refs, driving_param: Some(target), label })
    }
}

impl CadCommand for DistanceConstraintCommand {
    fn name(&self) -> &'static str {
        "DCONSTRAINT"
    }

    fn prompt(&self) -> String {
        format!("Specify distance <{:.4}>: ", self.default_value)
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.build(self.default_value).unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value: f64 = text.trim().parse().ok()?;
        self.build(value)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Constrains the angle (in degrees) from `fixed`'s direction to `moving`'s.
pub struct AngleConstraintCommand {
    fixed_handle: Handle,
    moving_handle: Handle,
    default_value: f64,
}

impl AngleConstraintCommand {
    /// `None` unless both `fixed` and `moving` are lines.
    pub fn new(scene: &Scene, fixed: Handle, moving: Handle) -> Option<Self> {
        let fixed_entity = scene.document.get_entity(fixed)?;
        let moving_entity = scene.document.get_entity(moving)?;
        let (EntityType::Line(f), EntityType::Line(m)) = (fixed_entity, moving_entity) else {
            return None;
        };
        let a1 = (f.end.y - f.start.y).atan2(f.end.x - f.start.x);
        let a2 = (m.end.y - m.start.y).atan2(m.end.x - m.start.x);
        let default_value = (a2 - a1).to_degrees();
        Some(Self { fixed_handle: fixed, moving_handle: moving, default_value })
    }

    fn build(&self, target_degrees: f64) -> Option<CmdResult> {
        Some(CmdResult::AddSketchConstraint {
            kind: ConstraintKind::Angle,
            refs: vec![SketchRef::whole(self.fixed_handle), SketchRef::whole(self.moving_handle)],
            driving_param: Some(target_degrees),
            label: "Angle constraint",
        })
    }
}

impl CadCommand for AngleConstraintCommand {
    fn name(&self) -> &'static str {
        "ACONSTRAINT"
    }

    fn prompt(&self) -> String {
        format!("Specify angle in degrees <{:.4}>: ", self.default_value)
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.build(self.default_value).unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value: f64 = text.trim().parse().ok()?;
        self.build(value)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
