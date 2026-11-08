// ATTEDIT command — pick a block reference with attributes, then open the
// attribute editor dialog on it.
//
// This command only performs the entity pick. Once a block is chosen it
// reports the handle through `attedit_pending_handle`; the command host
// (`command_driver`) ends the command and opens the editor dialog. When a
// suitable block is already selected, the ATTEDIT dispatch opens the dialog
// directly and never starts this command (see `app::commands::inquiry`).

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

pub struct AtteditCommand {
    /// The picked block reference, once the user clicks one.
    picked: Option<document::Handle>,
}

impl AtteditCommand {
    pub fn new() -> Self {
        Self { picked: None }
    }
}

impl CadCommand for AtteditCommand {
    fn name(&self) -> &'static str {
        "ATTEDIT"
    }

    fn prompt(&self) -> String {
        t!("ATTEDIT  Select block with attributes:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        self.picked.is_none()
    }

    fn on_entity_pick(&mut self, handle: document::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Record the pick and yield: the host reads `attedit_pending_handle`
        // and opens the editor dialog for this block.
        self.picked = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }

    fn attedit_pending_handle(&self) -> Option<document::Handle> {
        self.picked
    }
}

// ── -ATTEDIT ──────────────────────────────────────────────

/// One attribute the global branch can select: the reference's three
/// specifications match the block name, the tag and the current value.
#[derive(Clone, Debug)]
pub struct AttributeCandidate {
    pub block: String,
    pub tag: String,
    pub value: String,
    pub on_screen: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum DashStep {
    Branch,
    VisibleOnly,
    BlockSpec,
    TagSpec,
    ValueSpec,
    OldString,
    NewString,
}

fn yes_no(answer: &str, default: bool) -> Option<bool> {
    match answer.to_ascii_uppercase().as_str() {
        "" => Some(default),
        "Y" | "YES" => Some(true),
        "N" | "NO" => Some(false),
        _ => None,
    }
}

fn spec(answer: &str) -> String {
    if answer.is_empty() {
        "*".to_string()
    } else {
        answer.to_string()
    }
}

/// `-ATTEDIT`: the question flow. The global branch selects every attribute
/// the three specifications name, reports the count, and swaps a string
/// inside those values. The one-at-a-time branch asks the same questions and
/// then hands off to the attribute editor.
pub struct DashAtteditCommand {
    step: DashStep,
    one_at_a_time: bool,
    visible_only: bool,
    block_spec: String,
    tag_spec: String,
    value_spec: String,
    old: String,
    candidates: Vec<AttributeCandidate>,
}

impl DashAtteditCommand {
    pub fn new(candidates: Vec<AttributeCandidate>) -> Self {
        Self {
            step: DashStep::Branch,
            one_at_a_time: true,
            visible_only: true,
            block_spec: "*".to_string(),
            tag_spec: "*".to_string(),
            value_spec: "*".to_string(),
            old: String::new(),
            candidates,
        }
    }

    fn matches(&self, candidate: &AttributeCandidate) -> bool {
        use crate::app::control::block_ops::glob_match_ci;
        glob_match_ci(&self.block_spec, &candidate.block)
            && glob_match_ci(&self.tag_spec, &candidate.tag)
            && glob_match_ci(&self.value_spec, &candidate.value)
            && (!self.visible_only || candidate.on_screen)
    }

    fn selected(&self) -> usize {
        self.candidates.iter().filter(|c| self.matches(c)).count()
    }
}

impl CadCommand for DashAtteditCommand {
    fn name(&self) -> &'static str {
        "-ATTEDIT"
    }

    fn prompt(&self) -> String {
        match self.step {
            DashStep::Branch => "Edit attributes one at a time? [Yes/No] <Y>:".to_string(),
            DashStep::VisibleOnly => {
                "Edit only attributes visible on screen? [Yes/No] <Y>:".to_string()
            }
            DashStep::BlockSpec => "Enter block name specification <*>:".to_string(),
            DashStep::TagSpec => "Enter attribute tag specification <*>:".to_string(),
            DashStep::ValueSpec => "Enter attribute value specification <*>:".to_string(),
            DashStep::OldString => "Enter string to change:".to_string(),
            DashStep::NewString => "Enter new string:".to_string(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let answer = text.trim().to_string();
        Some(match self.step {
            DashStep::Branch => match yes_no(&answer, true) {
                Some(one_at_a_time) => {
                    self.one_at_a_time = one_at_a_time;
                    if one_at_a_time {
                        self.step = DashStep::BlockSpec;
                        CmdResult::NeedPoint
                    } else {
                        self.step = DashStep::VisibleOnly;
                        CmdResult::ReportMeasurement(
                            "Performing global editing of attribute values.".to_string(),
                        )
                    }
                }
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            DashStep::VisibleOnly => match yes_no(&answer, true) {
                Some(visible_only) => {
                    self.visible_only = visible_only;
                    self.step = DashStep::BlockSpec;
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            DashStep::BlockSpec => {
                self.block_spec = spec(&answer);
                self.step = DashStep::TagSpec;
                CmdResult::NeedPoint
            }
            DashStep::TagSpec => {
                self.tag_spec = spec(&answer);
                self.step = DashStep::ValueSpec;
                CmdResult::NeedPoint
            }
            DashStep::ValueSpec => {
                self.value_spec = spec(&answer);
                if self.one_at_a_time {
                    CmdResult::Dispatch("ATTEDIT".to_string())
                } else {
                    self.step = DashStep::OldString;
                    CmdResult::ReportMeasurement(format!(
                        "{} attributes selected.",
                        self.selected()
                    ))
                }
            }
            DashStep::OldString => {
                if answer.is_empty() {
                    return Some(CmdResult::ReportError(
                        "ATTEDIT: string to change is empty.".to_string(),
                    ));
                }
                self.old = answer;
                self.step = DashStep::NewString;
                CmdResult::NeedPoint
            }
            DashStep::NewString => {
                let visible = if self.visible_only { " VISIBLE" } else { "" };
                CmdResult::Dispatch(format!(
                    "-ATTEDIT REPLACE {} {} {} {} {}{}",
                    self.old, answer, self.block_spec, self.tag_spec, self.value_spec, visible
                ))
            }
        })
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["ATTEDIT"]
});
