// Move tool — ribbon definition + interactive command.
//
// Command:  MOVE (M)
//   Requires at least one entity selected before starting.
//   Step 1: pick base point, or D = type the displacement vector directly
//   Step 2: pick destination → translates all selected entities by (dest - base);
//           Enter uses the base point as the displacement (commercial solutions).

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, EntityTransform};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MOVE",
        label: "Move",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/move.svg")),
        event: ModuleEvent::Command("MOVE".to_string()),
    }
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    Base,
    Target(DVec3),
    /// Displacement option: the next point / typed coordinate is the vector.
    Displacement,
}

pub struct MoveCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: Step,
}

impl MoveCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        // The actual edit/commit uses CAD handles, never this visual cache.
        // For extreme 2-D selections keep only the whole group's live
        // bounding box: no per-frame expansion of complex repeated INSERTs.
        let wire_models = crate::scene::model::instance_model::
            cached_command_preview_for_large_2d_selection(handles.len(), wire_models);
        Self {
            handles,
            wire_models,
            step: Step::Base,
        }
    }
}

impl CadCommand for MoveCommand {
    fn name(&self) -> &'static str {
        "MOVE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Base => crate::tr!("command-move", "base", count = (self.handles.len() as i64),),
            Step::Target(base) => crate::tr!(
                "command-move",
                "target",
                x = format!("{:.3}", base.x),
                y = format!("{:.3}", base.y),
            ),
            Step::Displacement => crate::tr!("command-move", "displacement"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            Step::Base => vec![CmdOption::new("Displacement", "D")],
            Step::Target(_) => vec![CmdOption::enter("Use first point as displacement")],
            Step::Displacement => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Base => {
                self.step = Step::Target(pt);
                CmdResult::NeedPoint
            }
            Step::Target(base) => {
                let delta = pt - *base;
                CmdResult::TransformSelected(
                    self.handles.clone(),
                    EntityTransform::Translate(delta),
                )
            }
            // The point (typically typed as `dx,dy`) is the displacement itself.
            Step::Displacement => {
                CmdResult::TransformSelected(self.handles.clone(), EntityTransform::Translate(pt))
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Base)
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Base)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match (&self.step, text.trim().to_uppercase().as_str()) {
            (Step::Base, "D" | "DISPLACEMENT") => {
                self.step = Step::Displacement;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Commercial solutions: Enter at the second point uses the base point as the
            // displacement from the origin.
            Step::Target(base) => {
                CmdResult::TransformSelected(self.handles.clone(), EntityTransform::Translate(base))
            }
            _ => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let Step::Target(base) = &self.step else {
            return vec![];
        };
        let delta = pt - *base;
        // Translated ghost of each selected object + rubber-band line.
        let mut out: Vec<WireModel> = self
            .wire_models
            .iter()
            .map(|w| w.translated(delta.as_vec3()))
            .collect();
        out.push(WireModel::solid(
            "rubber_band".into(),
            vec![
                [base.x as f32, base.y as f32, base.z as f32],
                [pt.x as f32, pt.y as f32, pt.z as f32],
            ],
            WireModel::CYAN,
            false,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keywords(cmd: &MoveCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    #[test]
    fn displacement_option_translates_by_vector() {
        let mut cmd = MoveCommand::new(vec![Handle::new(1)], vec![]);
        assert_eq!(keywords(&cmd), ["D"]);
        assert!(matches!(cmd.on_text_input("D"), Some(CmdResult::NeedPoint)));
        assert!(keywords(&cmd).is_empty());
        match cmd.on_point(DVec3::new(3.0, 4.0, 0.0)) {
            CmdResult::TransformSelected(handles, EntityTransform::Translate(d)) => {
                assert_eq!(handles, vec![Handle::new(1)]);
                assert_eq!(d, DVec3::new(3.0, 4.0, 0.0));
            }
            _ => panic!("expected a translation"),
        }
    }

    #[test]
    fn huge_move_uses_one_live_box_but_commits_every_original_handle() {
        let handles: Vec<Handle> = (1..=600).map(Handle::new).collect();
        let mut wires = Vec::new();
        for h in &handles {
            for _ in 0..4 {
                let x = h.value() as f64;
                let mut w = WireModel::solid_f64(
                    h.value().to_string(),
                    vec![[x, 0.0, 0.0], [x+1.0, 2.0, 0.0]],
                    WireModel::WHITE, false,
                );
                w.aabb = [x as f32, 0.0, x as f32+1.0, 2.0];
                wires.push(w);
            }
        }
        let mut cmd = MoveCommand::new(handles.clone(), wires);
        // The full 2400 preview children are discarded at command creation.
        assert_eq!(cmd.wire_models.len(), 1);
        assert_eq!(cmd.wire_models[0].points.len(), 5);
        assert!(matches!(cmd.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        let preview = cmd.on_preview_wires(DVec3::new(10.0, 5.0, 0.0));
        assert_eq!(preview.len(), 2, "group bounding box + rubber band");
        assert!((preview[0].points[0][0] - 11.0).abs() < 0.001);
        match cmd.on_point(DVec3::new(10.0, 5.0, 0.0)) {
            CmdResult::TransformSelected(committed, EntityTransform::Translate(delta)) => {
                assert_eq!(committed, handles);
                assert_eq!(delta, DVec3::new(10.0, 5.0, 0.0));
            }
            _ => panic!("expected exact MOVE of every selected handle"),
        }
    }

    #[test]
    fn enter_at_second_point_uses_base_as_displacement() {
        let mut cmd = MoveCommand::new(vec![Handle::new(1)], vec![]);
        cmd.on_point(DVec3::new(5.0, -2.0, 0.0));
        assert_eq!(
            keywords(&cmd),
            [""],
            "Enter is the only option at the second point"
        );
        match cmd.on_enter() {
            CmdResult::TransformSelected(_, EntityTransform::Translate(d)) => {
                assert_eq!(d, DVec3::new(5.0, -2.0, 0.0));
            }
            _ => panic!("expected a translation by the base point"),
        }
    }
}
