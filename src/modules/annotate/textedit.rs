// TEXTEDIT — edit multiline text, single-line text, or dimension text in-place.
//
// Workflow:
//   1. Enters a loop prompting to select an annotation object.
//   2. Accepts keyword options: Undo (to revert the last edit) and Mode (to switch Single/Multiple).
//   3. In Multiple mode (default), editing an object suspends the command, opens the editor,
//      and when closed, resumes the command loop.
//   4. In Single mode, editing an object exits the command immediately.

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::t;

/// Parse a TEXTEDITMODE value. Accepts `0`/`m`/`multiple`/`false` (Multiple → false)
/// and `1`/`s`/`single`/`true` (Single → true), case-insensitive. Returns `None`
/// for anything else so callers can re-prompt or report the error.
pub fn parse_texteditmode(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "0" | "m" | "multiple" | "false" => Some(false),
        "1" | "s" | "single" | "true" => Some(true),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    PickObject,
    EnterMode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextEditMode {
    Single,
    Multiple,
}

pub struct TexteditCommand {
    mode: TextEditMode,
    edit_count: usize,
    step: Step,
    /// Set when the last Mode entry was unrecognized, so the next prompt
    /// leads with the error instead of silently re-asking.
    invalid_mode: bool,
}

impl TexteditCommand {
    pub fn new(texteditmode: bool) -> Self {
        let mode = if texteditmode {
            TextEditMode::Single
        } else {
            TextEditMode::Multiple
        };
        Self {
            mode,
            edit_count: 0,
            step: Step::PickObject,
            invalid_mode: false,
        }
    }
}

impl CadCommand for TexteditCommand {
    fn name(&self) -> &'static str {
        "TEXTEDIT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::PickObject => {
                if self.edit_count == 0 {
                    t!("TEXTEDIT Select an annotation object or [Undo Mode]:").into_owned()
                } else {
                    t!("TEXTEDIT Select an annotation object or [Undo Mode] <exit>:").into_owned()
                }
            }
            Step::EnterMode => {
                let prefix = if self.invalid_mode {
                    format!("{} ", t!("Requires Single or Multiple."))
                } else {
                    String::new()
                };
                let mode = match self.mode {
                    TextEditMode::Single => t!("Single"),
                    TextEditMode::Multiple => t!("Multiple"),
                };
                t!(
                    "%{prefix}TEXTEDIT Enter text edit mode [Single/Multiple] <%{mode}>:",
                    prefix = prefix,
                    mode = mode
                )
                .into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::PickObject
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::SuspendForTextEdit { handle }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        match self.step {
            Step::PickObject => {
                if text.is_empty() {
                    if self.edit_count > 0 {
                        return Some(CmdResult::Cancel);
                    } else {
                        return Some(CmdResult::NeedPoint);
                    }
                }

                let lower = text.to_lowercase();
                if lower == "u" || lower == "undo" {
                    if self.edit_count > 0 {
                        self.edit_count -= 1;
                        return Some(CmdResult::UndoDocument);
                    } else {
                        return Some(CmdResult::NeedPoint);
                    }
                } else if lower == "m" || lower == "mode" {
                    self.step = Step::EnterMode;
                    return Some(CmdResult::NeedPoint);
                }

                Some(CmdResult::NeedPoint)
            }
            Step::EnterMode => {
                if text.is_empty() {
                    self.invalid_mode = false;
                    self.step = Step::PickObject;
                    return Some(CmdResult::NeedPoint);
                }

                match parse_texteditmode(text) {
                    Some(single) => {
                        self.mode = if single {
                            TextEditMode::Single
                        } else {
                            TextEditMode::Multiple
                        };
                        self.invalid_mode = false;
                        self.step = Step::PickObject;
                    }
                    // Unrecognized value: stay on the Mode step and re-prompt
                    // with the error rather than silently ignoring it.
                    None => self.invalid_mode = true,
                }

                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::PickObject => {
                if self.edit_count > 0 {
                    CmdResult::Cancel
                } else {
                    CmdResult::NeedPoint
                }
            }
            Step::EnterMode => {
                self.step = Step::PickObject;
                CmdResult::NeedPoint
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_editor_closed(&mut self, committed: bool) -> CmdResult {
        if committed {
            self.edit_count += 1;
        }

        match self.mode {
            TextEditMode::Single => CmdResult::Cancel,
            TextEditMode::Multiple => CmdResult::NeedPoint,
        }
    }
}

pub struct TexteditmodeCommand {
    current: bool,
    /// Set when the last value entry was unrecognized, so the next prompt
    /// leads with the error instead of silently re-asking.
    invalid: bool,
}

impl TexteditmodeCommand {
    pub fn new(current: bool) -> Self {
        Self {
            current,
            invalid: false,
        }
    }
}

impl CadCommand for TexteditmodeCommand {
    fn name(&self) -> &'static str {
        "TEXTEDITMODE"
    }

    fn prompt(&self) -> String {
        let v = if self.current { 1 } else { 0 };
        let prefix = if self.invalid {
            format!("{} ", t!("Requires 0 OR 1 OR MULTIPLE OR SINGLE."))
        } else {
            String::new()
        };
        t!(
            "%{prefix}TEXTEDITMODE Enter new value for TEXTEDITMODE <%{v}>:",
            prefix = prefix,
            v = v
        )
        .into_owned()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return Some(CmdResult::Cancel);
        }

        match parse_texteditmode(text) {
            Some(v) => Some(CmdResult::SetTexteditMode(v)),
            // Unrecognized value: stay in the command and re-prompt with the
            // error, rather than aborting via the Measurement arm.
            None => {
                self.invalid = true;
                Some(CmdResult::NeedPoint)
            }
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

    fn dyn_field(&self) -> crate::command::DynField {
        crate::command::DynField::Scalar
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TEXTEDIT", "TEDIT", "TEXTEDITMODE"] });
