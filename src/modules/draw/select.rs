// SelectObjectsCommand — generic "select objects then run command" gather phase.
//
// Used when a modify command is invoked with nothing pre-selected.
// The user may single-click, box-select, or polygon-select any number of objects.
// Picks accumulate into the selection (Shift removes); the set is applied when
// the user presses Enter or right-clicks (standard "Select objects:" behaviour).
//
// Single-object commands (e.g. LAYMCUR) use `instant()` instead: the first
// completed selection action fires straight away, no Enter required.

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::draw::fence::FencePick;
use crate::scene::model::wire_model::WireModel;

pub struct SelectObjectsCommand {
    prompt_cmd: String,
    pending_cmd: String,
    /// Selection accumulated so far (kept in sync with the scene selection by
    /// each `on_selection_complete` call). Applied on Enter when `commit_on_enter`.
    handles: Vec<Handle>,
    /// When true, gathering continues until Enter / right-click commits the set.
    /// When false, the first completed selection action fires immediately.
    commit_on_enter: bool,
    /// Whether the generic selection methods are shown as command buttons.
    show_options: bool,
    /// A Fence / WPolygon / CPolygon path being picked point by point. While
    /// one is under way the host must send clicks here as points rather than
    /// treating them as picks, which is what `is_selection_gathering` reports.
    pick: Option<FencePick>,
    /// Whether the polygon being picked takes what it merely touches. Ignored
    /// by a fence, which has no inside to speak of.
    pick_crossing: bool,
}

impl SelectObjectsCommand {
    /// Standard selection set: accumulate picks, apply on Enter / right-click.
    pub fn new(pending_cmd: &str) -> Self {
        Self {
            prompt_cmd: pending_cmd.to_string(),
            pending_cmd: pending_cmd.to_string(),
            handles: Vec::new(),
            commit_on_enter: true,
            show_options: true,
            pick: None,
            pick_crossing: true,
        }
    }

    /// Plain gather prompt with no generic selection-method buttons. The
    /// visible command name may differ from the private apply command.
    pub fn plain(prompt_cmd: &str, pending_cmd: &str) -> Self {
        Self {
            prompt_cmd: prompt_cmd.to_string(),
            pending_cmd: pending_cmd.to_string(),
            handles: Vec::new(),
            commit_on_enter: true,
            show_options: false,
            pick: None,
            pick_crossing: true,
        }
    }

    /// Single-object variant: the first completed selection action applies
    /// immediately, with no Enter (used by commands that act on one object).
    pub fn instant(pending_cmd: &str) -> Self {
        Self {
            prompt_cmd: pending_cmd.to_string(),
            pending_cmd: pending_cmd.to_string(),
            handles: Vec::new(),
            commit_on_enter: false,
            show_options: false,
            pick: None,
            pick_crossing: true,
        }
    }
}

impl CadCommand for SelectObjectsCommand {
    fn name(&self) -> &'static str {
        "SELECT"
    }

    fn prompt(&self) -> String {
        if let Some(pick) = &self.pick {
            let label = if !pick.closed {
                t!("Fence")
            } else if self.pick_crossing {
                t!("Crossing polygon")
            } else {
                t!("Window polygon")
            };
            return t!(
                "%{mode}: pick points (%{count} placed, Enter to apply):",
                mode = label,
                count = pick.len()
            )
            .into_owned();
        }
        if self.commit_on_enter && !self.handles.is_empty() {
            t!(
                "%{cmd}  Select objects (%{count} selected, Enter to apply):",
                cmd = self.prompt_cmd,
                count = self.handles.len()
            )
            .into_owned()
        } else {
            t!("%{cmd}  Select objects:", cmd = self.prompt_cmd).into_owned()
        }
    }

    // Opt into the selection-gathering path; host routes clicks through
    // the normal selection system and calls on_selection_complete after each action.
    fn is_selection_gathering(&self) -> bool {
        // A path being picked needs the clicks as points; handing them to the
        // selection system instead would pick objects under each vertex.
        self.pick.is_none()
    }

    // Clickable selection keywords (#426, #596). Window and Crossing fix the
    // sense of the next box, which dragging would otherwise take from the
    // direction the corner travels; All takes the whole space; Add and Remove
    // decide what a pick does for the rest of the selection; Previous
    // re-selects the set the last command worked on, Last the most recently
    // created object. All are consumed centrally in `try_selection_keyword`, so
    // they work typed as well — these buttons only surface them, which is what
    // the on-screen keyboard-less case needs.
    fn options(&self) -> Vec<CmdOption> {
        if self.commit_on_enter && self.show_options {
            vec![
                CmdOption::new(t!("Window").as_ref(), "W"),
                CmdOption::new(t!("Crossing").as_ref(), "C"),
                CmdOption::new(t!("Fence").as_ref(), "F"),
                CmdOption::new(t!("WPolygon").as_ref(), "WP"),
                CmdOption::new(t!("CPolygon").as_ref(), "CP"),
                CmdOption::new(t!("All").as_ref(), "ALL"),
                CmdOption::new(t!("Add").as_ref(), "A"),
                CmdOption::new(t!("Remove").as_ref(), "R"),
                CmdOption::new(t!("Previous").as_ref(), "P"),
                CmdOption::new(t!("Last").as_ref(), "L"),
            ]
        } else {
            Vec::new()
        }
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if self.commit_on_enter {
            // Keep gathering: remember the running set and update the prompt.
            // The command is applied when the user presses Enter / right-clicks.
            self.handles = handles;
            return CmdResult::NeedPoint;
        }
        // Single-object variant: apply on the first non-empty selection.
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Relaunch(std::mem::take(&mut self.pending_cmd), handles)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(pick) = self.pick.as_mut() {
            pick.push([pt.x, pt.y]);
        }
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    // Fence and the two polygons are picked here rather than centrally: they
    // need somewhere to accumulate points, and the command is what outlives the
    // individual clicks. The rest of the keywords act at once and are consumed
    // in `try_selection_keyword`, which sees the token first. (#596)
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !self.commit_on_enter {
            return None;
        }
        match text.trim().to_uppercase().as_str() {
            "F" | "FENCE" => {
                self.pick = Some(FencePick::fence());
                Some(CmdResult::NeedPoint)
            }
            "CP" | "CPOLYGON" => {
                self.pick = Some(FencePick::polygon());
                self.pick_crossing = true;
                Some(CmdResult::NeedPoint)
            }
            "WP" | "WPOLYGON" => {
                self.pick = Some(FencePick::polygon());
                self.pick_crossing = false;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.pick {
            Some(pick) if !pick.is_empty() => {
                vec![pick.preview([pt.x, pt.y], "select_fence")]
            }
            _ => vec![],
        }
    }

    // Enter / right-click ends the gather phase and fires the pending command
    // with the accumulated selection. Nothing selected → cancel.
    fn on_enter(&mut self) -> CmdResult {
        // Finishing a path applies it and returns to picking objects; the set
        // it produced joins whatever was already gathered.
        if let Some(pick) = self.pick.take() {
            if pick.is_usable() {
                return CmdResult::SelectByPath {
                    path: pick.path(),
                    closed: pick.closed,
                    crossing: self.pick_crossing,
                };
            }
            return CmdResult::NeedPoint;
        }
        if !self.commit_on_enter || self.handles.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::Relaunch(
            std::mem::take(&mut self.pending_cmd),
            std::mem::take(&mut self.handles),
        )
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ARRAY", "ARRAYPATH", "ARRAYPOLAR", "ARRAYRECT", "BLOCK", "COPY", "COPYCLIP", "CUTCLIP", "ERASE", "EXPLODE", "GROUP", "LAYFRZ", "LAYLCK", "LAYMCUR", "LAYOFF", "LAYULK", "MIRROR", "MOVE", "ROTATE", "SCALE", "STRETCH"] });  // SelectObjectsCommand
