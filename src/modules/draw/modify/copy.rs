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
                } else {
                    self.count += 1;
                    CmdResult::CopySelected(
                        self.handles.clone(),
                        EntityTransform::Translate(delta),
                    )
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // Cancel an in-progress Array count entry; otherwise finish.
        if self.awaiting_count {
            self.awaiting_count = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Placing(_))
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // Accept the Array keyword while placing, but route the typed item
        // count straight through on_text_input.
        matches!(self.step, Step::Placing(_)) && !self.awaiting_count
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.awaiting_count {
            if let Ok(n) = text.trim().parse::<usize>() {
                if n >= 2 {
                    self.array_count = Some(n);
                }
            }
            self.awaiting_count = false;
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_uppercase().as_str() {
            "A" | "ARRAY" => {
                self.awaiting_count = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
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
