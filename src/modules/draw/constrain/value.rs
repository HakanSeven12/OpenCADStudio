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
use crate::scene::named_parameters::DrivingValue;
use crate::scene::sketch_constraints::{ConstraintKind, SketchRef};
use crate::scene::Scene;

/// Recognizes a typed prompt token as either a numeric literal or a
/// reference to an existing named parameter (`docs/
/// named_parameters_design.md` stage 4's "a way to pick a named parameter
/// instead of typing a literal" — the same prompt takes either, mirroring
/// how AutoCAD's own dimensional-constraint prompts accept an expression in
/// place of a bare value). `known_names` is a one-time snapshot taken at
/// command construction (`DistanceConstraintCommand`/`AngleConstraintCommand
/// ::new`, which already has `&Scene`) since `on_text_input` itself has no
/// document access — the same constraint `default_value` already works
/// around. Only *existence* is checked here; the actual value is resolved
/// fresh at solve time (`sketch_solve::build_constraint`), consistent with
/// this project's "no incremental/cached resolution" approach throughout.
fn parse_driving_value(text: &str, known_names: &[String]) -> Option<DrivingValue> {
    let text = text.trim();
    if let Ok(value) = text.parse::<f64>() {
        return Some(DrivingValue::Literal(value));
    }
    known_names.iter().find(|n| n.as_str() == text).map(|n| DrivingValue::Named(n.clone()))
}

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
    /// Snapshot of `Scene::named_parameters`' names at construction time —
    /// see `parse_driving_value`'s doc comment for why a snapshot.
    known_param_names: Vec<String>,
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
        let known_param_names = scene.named_parameters().iter().map(|p| p.name.clone()).collect();
        Some(Self { handle, is_circle, default_value, known_param_names })
    }

    fn build(&self, target: DrivingValue) -> Option<CmdResult> {
        if let DrivingValue::Literal(v) = target {
            if v <= 0.0 {
                return None;
            }
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
        self.build(DrivingValue::Literal(self.default_value)).unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = parse_driving_value(text, &self.known_param_names)?;
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
    /// Snapshot of `Scene::named_parameters`' names at construction time —
    /// see `parse_driving_value`'s doc comment for why a snapshot.
    known_param_names: Vec<String>,
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
        let known_param_names = scene.named_parameters().iter().map(|p| p.name.clone()).collect();
        Some(Self { fixed_handle: fixed, moving_handle: moving, default_value, known_param_names })
    }

    fn build(&self, target: DrivingValue) -> Option<CmdResult> {
        Some(CmdResult::AddSketchConstraint {
            kind: ConstraintKind::Angle,
            refs: vec![SketchRef::whole(self.fixed_handle), SketchRef::whole(self.moving_handle)],
            driving_param: Some(target),
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
        self.build(DrivingValue::Literal(self.default_value)).unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = parse_driving_value(text, &self.known_param_names)?;
        self.build(value)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_driving_value_prefers_a_numeric_literal() {
        let known = vec!["hole_dia".to_string()];
        assert_eq!(parse_driving_value("12.5", &known), Some(DrivingValue::Literal(12.5)));
        // Distance's own `build` rejects a non-positive target later; the
        // parse step itself accepts any number, negative included.
        assert_eq!(parse_driving_value("-3", &known), Some(DrivingValue::Literal(-3.0)));
    }

    #[test]
    fn parse_driving_value_recognizes_a_known_parameter_name() {
        let known = vec!["hole_dia".to_string(), "plate_len".to_string()];
        assert_eq!(parse_driving_value("hole_dia", &known), Some(DrivingValue::Named("hole_dia".to_string())));
    }

    #[test]
    fn parse_driving_value_rejects_an_unknown_token() {
        let known = vec!["hole_dia".to_string()];
        assert_eq!(parse_driving_value("bogus", &known), None);
        assert_eq!(parse_driving_value("", &known), None);
    }
}
