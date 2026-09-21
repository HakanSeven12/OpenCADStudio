// Copy tool — ribbon definition + interactive command.
//
// Command:  COPY (CO)
//   Requires at least one entity selected before starting.
//   Step 1: pick base point
//   Step 2+: each click makes another copy at (click - base); Enter to finish.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, EntityTransform};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "COPY",
        label: "Copy",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/copy.svg")),
        event: ModuleEvent::Command("COPY".to_string()),
    }
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    Base,
    Placing(DVec3),
    /// Displacement option: the next point / typed coordinate is the vector.
    Displacement,
}

pub struct CopyCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: Step,
    count: usize,
    /// Number of items for an Array copy (None = place copies one at a time).
    array_count: Option<usize>,
    /// True while the next typed value is captured as the array item count.
    awaiting_count: bool,
    /// True while the mOde prompt (Single / Multiple) is up.
    awaiting_mode: bool,
    /// COPYMODE: Single ends the command after the first copy.
    single: bool,
}

impl CopyCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: Step::Base,
            count: 0,
            array_count: None,
            awaiting_count: false,
            awaiting_mode: false,
            single: false,
        }
    }
}

impl CadCommand for CopyCommand {
    fn name(&self) -> &'static str {
        "COPY"
    }

    fn prompt(&self) -> String {
        if self.awaiting_count {
            return crate::tr!("command-copy", "array-count");
        }
        if self.awaiting_mode {
            return crate::tr!("command-copy", "mode");
        }
        match &self.step {
            Step::Base => crate::tr!(
                "command-copy", "base",
                count = (self.handles.len() as i64),
            ),
            Step::Placing(base) => {
                if let Some(n) = self.array_count {
                    crate::tr!(
                        "command-copy", "array-target",
                        count = (n as i64),
                        x = format!("{:.3}", base.x),
                        y = format!("{:.3}", base.y),
                    )
                } else {
                    crate::tr!(
                        "command-copy", "target",
                        count = (self.count as i64),
                        x = format!("{:.3}", base.x),
                        y = format!("{:.3}", base.y),
                    )
                }
            }
            Step::Displacement => crate::tr!("command-copy", "displacement"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.awaiting_count {
            return Vec::new();
        }
        if self.awaiting_mode {
            return vec![
                CmdOption::new("Single", "S"),
                CmdOption::new("Multiple", "M"),
            ];
        }
        match self.step {
            Step::Base => vec![
                CmdOption::new("Displacement", "D"),
                CmdOption::new("mOde", "O"),
            ],
            Step::Placing(_) => {
                let mut opts = vec![CmdOption::new("Array", "A")];
                if self.count > 0 {
                    opts.push(CmdOption::new("Undo", "U"));
                }
                opts.push(CmdOption::enter("Exit"));
                opts
            }
            Step::Displacement => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Base => {
                self.step = Step::Placing(pt);
                CmdResult::CopyToClipboard {
                    handles: self.handles.clone(),
                    base: pt,
                }
            }
            Step::Placing(base) => {
                let delta = pt - *base;
                if let Some(n) = self.array_count {
                    // Array: place n-1 copies at delta, 2·delta, … so the result
                    // is n evenly spaced items including the original. Ends here.
                    let transforms: Vec<EntityTransform> = (1..n)
                        .map(|k| EntityTransform::Translate(delta * k as f64))
                        .collect();
                    CmdResult::BatchCopy(self.handles.clone(), transforms)
                } else if self.single {
                    // Single mode: one copy, then done.
                    CmdResult::BatchCopy(
                        self.handles.clone(),
                        vec![EntityTransform::Translate(delta)],
                    )
                } else {
                    self.count += 1;
                    CmdResult::CopySelected(
                        self.handles.clone(),
                        EntityTransform::Translate(delta),
                    )
                }
            }
            // The point (typically typed as `dx,dy`) is the displacement itself.
            Step::Displacement => CmdResult::BatchCopy(
                self.handles.clone(),
                vec![EntityTransform::Translate(pt)],
            ),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // Cancel an in-progress Array count / mode entry; otherwise finish.
        if self.awaiting_count {
            self.awaiting_count = false;
            return CmdResult::NeedPoint;
        }
        if self.awaiting_mode {
            self.awaiting_mode = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Placing(_) | Step::Base)
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // Keywords ride along with the point picks, but a typed item count
        // or mode answer goes straight through on_text_input.
        matches!(self.step, Step::Placing(_) | Step::Base)
            && !self.awaiting_count
            && !self.awaiting_mode
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.awaiting_count {
            if let Ok(n) = text.trim().parse::<usize>() {
                if !super::array::array_items_within_limit(&[n as u64]) {
                    return Some(super::array::array_limit_error());
                }
                if n >= 2 {
                    self.array_count = Some(n);
                }
            }
            self.awaiting_count = false;
            return Some(CmdResult::NeedPoint);
        }
        let upper = text.trim().to_uppercase();
        if self.awaiting_mode {
            match upper.as_str() {
                "S" | "SINGLE" => self.single = true,
                "M" | "MULTIPLE" => self.single = false,
                _ => return Some(CmdResult::NeedPoint),
            }
            self.awaiting_mode = false;
            return Some(CmdResult::NeedPoint);
        }
        match (&self.step, upper.as_str()) {
            (Step::Base, "D" | "DISPLACEMENT") => {
                self.step = Step::Displacement;
                Some(CmdResult::NeedPoint)
            }
            (Step::Base, "O" | "MODE") => {
                self.awaiting_mode = true;
                Some(CmdResult::NeedPoint)
            }
            (Step::Placing(_), "A" | "ARRAY") => {
                self.awaiting_count = true;
                Some(CmdResult::NeedPoint)
            }
            // Undo: take back the last copy placed by this command.
            (Step::Placing(_), "U" | "UNDO") if self.count > 0 => {
                self.count -= 1;
                Some(CmdResult::UndoDocument)
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.step, Step::Placing(_)) && self.count > 0 {
            self.on_text_input("U")
        } else {
            None
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let Step::Placing(base) = &self.step else {
            return vec![];
        };
        let delta = pt - *base;
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

    fn keywords(cmd: &CopyCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    #[test]
    fn array_count_above_the_item_limit_is_rejected() {
        let mut cmd = CopyCommand::new(vec![Handle::new(1)], vec![]);
        cmd.awaiting_count = true;
        assert!(matches!(
            cmd.on_text_input("18446744073709551615"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(cmd.awaiting_count, "the count prompt stays up");
        assert_eq!(cmd.array_count, None);
    }

    #[test]
    fn mode_single_exits_after_one_copy() {
        let mut cmd = CopyCommand::new(vec![Handle::new(1)], vec![]);
        assert_eq!(keywords(&cmd), ["D", "O"]);
        assert!(matches!(cmd.on_text_input("O"), Some(CmdResult::NeedPoint)));
        assert_eq!(keywords(&cmd), ["S", "M"]);
        assert!(matches!(cmd.on_text_input("S"), Some(CmdResult::NeedPoint)));
        cmd.on_point(DVec3::ZERO);
        match cmd.on_point(DVec3::new(10.0, 0.0, 0.0)) {
            CmdResult::BatchCopy(_, transforms) => assert_eq!(transforms.len(), 1),
            _ => panic!("single mode should copy once and end"),
        }
    }

    #[test]
    fn multiple_mode_keeps_copying_and_undo_takes_one_back() {
        let mut cmd = CopyCommand::new(vec![Handle::new(1)], vec![]);
        cmd.on_point(DVec3::ZERO);
        assert_eq!(keywords(&cmd), ["A", ""], "no Undo before a copy is placed");
        assert!(matches!(
            cmd.on_point(DVec3::new(10.0, 0.0, 0.0)),
            CmdResult::CopySelected(..)
        ));
        assert_eq!(keywords(&cmd), ["A", "U", ""]);
        assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::UndoDocument)));
        assert_eq!(keywords(&cmd), ["A", ""]);
        assert!(cmd.on_text_input("U").is_none(), "nothing left to undo");
    }

    #[test]
    fn displacement_option_copies_by_vector() {
        let mut cmd = CopyCommand::new(vec![Handle::new(1)], vec![]);
        cmd.on_text_input("D");
        match cmd.on_point(DVec3::new(1.0, 2.0, 0.0)) {
            CmdResult::BatchCopy(_, transforms) => {
                assert!(matches!(transforms[0], EntityTransform::Translate(d) if d == DVec3::new(1.0, 2.0, 0.0)));
            }
            _ => panic!("expected one copy by the displacement"),
        }
    }
}
