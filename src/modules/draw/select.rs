// SelectObjectsCommand — generic "select objects then run command" gather phase.
//
// Used when a modify command is invoked with nothing pre-selected.
// The user may single-click, box-select, or polygon-select any number of objects.
// Picks accumulate into the selection (Shift removes); the set is applied when
// the user presses Enter or right-clicks (standard "Select objects:" behaviour).
//
// Single-object commands (e.g. LAYMCUR) use `instant()` instead: the first
// completed selection action fires straight away, no Enter required.

use crate::t;
use codec::Handle;
use glam::DVec3;

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
    /// Add the command-specific Settings keyword used by Auto Constrain.
    auto_constrain_settings: bool,
    /// A command-specific prompt replacing the generic "Select objects:".
    prompt_text: Option<&'static str>,
    /// The command works on associative dimensions only; the host drops
    /// everything else from each completed selection.
    associative_dimensions_only: bool,
    /// The kinds of object the command takes; the host drops the rest of
    /// each completed selection silently, and Enter on nothing ends quietly.
    keep: Option<fn(&codec::EntityType) -> bool>,
    /// A SEttings keyword at the prompt, and what it does.
    settings: Option<fn() -> CmdResult>,
    /// A command's own keyword at the prompt (its label, the keyword and
    /// what it does).
    keyword: Option<(&'static str, &'static str, fn() -> CmdResult)>,
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
            auto_constrain_settings: false,
            prompt_text: None,
            associative_dimensions_only: false,
            keep: None,
            settings: None,
            keyword: None,
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
            auto_constrain_settings: false,
            prompt_text: None,
            associative_dimensions_only: false,
            keep: None,
            settings: None,
            keyword: None,
        }
    }

    /// Standard selection behavior routed to a private apply command while
    /// retaining the public command name in the visible prompt.
    pub fn routed(prompt_cmd: &str, pending_cmd: &str) -> Self {
        Self {
            prompt_cmd: prompt_cmd.to_string(),
            pending_cmd: pending_cmd.to_string(),
            handles: Vec::new(),
            commit_on_enter: true,
            show_options: true,
            pick: None,
            pick_crossing: true,
            auto_constrain_settings: false,
            prompt_text: None,
            associative_dimensions_only: false,
            keep: None,
            settings: None,
            keyword: None,
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
            auto_constrain_settings: false,
            prompt_text: None,
            associative_dimensions_only: false,
            keep: None,
            settings: None,
            keyword: None,
        }
    }

    /// Gather associative dimensions with the command's own wording; the host
    /// drops every other object from each completed selection.
    pub fn associative_dimensions(
        prompt_cmd: &str,
        pending_cmd: &str,
        prompt_text: &'static str,
    ) -> Self {
        let mut command = Self::plain(prompt_cmd, pending_cmd);
        command.prompt_text = Some(prompt_text);
        command.associative_dimensions_only = true;
        command
    }

    /// The standard gather with the command's own wording.
    pub fn with_prompt(prompt_cmd: &str, pending_cmd: &str, prompt_text: &'static str) -> Self {
        let mut command = Self::plain(prompt_cmd, pending_cmd);
        command.prompt_text = Some(prompt_text);
        command
    }

    /// The command's own wording, keeping only what `keep` accepts.
    pub fn filtered(
        prompt_cmd: &str,
        pending_cmd: &str,
        prompt_text: &'static str,
        keep: fn(&codec::EntityType) -> bool,
    ) -> Self {
        let mut command = Self::with_prompt(prompt_cmd, pending_cmd, prompt_text);
        command.keep = Some(keep);
        command
    }

    /// The command's own wording with one keyword of its own (`label`, typed
    /// as `keyword` or its capitals).
    pub fn with_keyword(
        prompt_cmd: &str,
        pending_cmd: &str,
        prompt_text: &'static str,
        label: &'static str,
        keyword: &'static str,
        action: fn() -> CmdResult,
    ) -> Self {
        let mut command = Self::with_prompt(prompt_cmd, pending_cmd, prompt_text);
        command.keyword = Some((label, keyword, action));
        command
    }

    /// The command's own wording with a SEttings keyword; Enter on nothing
    /// ends quietly.
    pub fn with_settings(
        prompt_cmd: &str,
        pending_cmd: &str,
        prompt_text: &'static str,
        settings: fn() -> CmdResult,
    ) -> Self {
        let mut command = Self::filtered(prompt_cmd, pending_cmd, prompt_text, |_| true);
        command.settings = Some(settings);
        command
    }

    pub fn auto_constrain(pending_cmd: &str) -> Self {
        let mut command = Self::new(pending_cmd);
        command.auto_constrain_settings = true;
        command
    }
}

impl CadCommand for SelectObjectsCommand {
    fn reveals_hidden_block_objects(&self) -> bool {
        self.pending_cmd == "_BVSHOW"
    }

    fn name(&self) -> &'static str {
        "SELECT"
    }

    fn prompt(&self) -> String {
        // A command with its own wording keeps it whatever is selected, the
        // way the reference repeats its prompt after every selection.
        if let (Some(text), None) = (self.prompt_text, self.pick.as_ref()) {
            return format!("{}  {}", self.prompt_cmd, t!(text));
        }
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
            if self.auto_constrain_settings {
                return t!(
                    "%{cmd}  Select objects or [Settings] (%{count} selected, Enter to apply):",
                    cmd = self.prompt_cmd,
                    count = self.handles.len()
                )
                .into_owned();
            }
            t!(
                "%{cmd}  Select objects (%{count} selected, Enter to apply):",
                cmd = self.prompt_cmd,
                count = self.handles.len()
            )
            .into_owned()
        } else {
            if self.auto_constrain_settings {
                return t!(
                    "%{cmd}  Select objects or [Settings]:",
                    cmd = self.prompt_cmd
                )
                .into_owned();
            }
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

    fn entity_pick_highlights_hover(&self) -> bool {
        self.pick.is_none()
    }

    fn selection_keeps_associative_dimensions(&self) -> bool {
        self.associative_dimensions_only
    }

    fn selection_entity_filter(&self) -> Option<fn(&codec::EntityType) -> bool> {
        self.keep
    }

    fn selection_drops_locked_point_clouds(&self) -> bool {
        matches!(
            self.pending_cmd.as_str(),
            "MOVE" | "3DMOVE" | "ROTATE" | "3DROTATE" | "SCALE" | "MIRROR" | "MIRROR3D" | "STRETCH"
        )
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
        if self.settings.is_some() && self.pick.is_none() {
            return vec![CmdOption::new(t!("Settings").as_ref(), "SE")];
        }
        if let (Some((label, keyword, _)), None) = (self.keyword, self.pick.as_ref()) {
            return vec![CmdOption::new(label, keyword)];
        }
        if self.commit_on_enter && self.show_options {
            if self.auto_constrain_settings {
                return vec![CmdOption::new(t!("Settings").as_ref(), "S")];
            }
            let options = vec![
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
            ];
            options
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

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if self.commit_on_enter {
            if !self.handles.contains(&handle) {
                self.handles.push(handle);
            }
            CmdResult::NeedPoint
        } else {
            CmdResult::Relaunch(std::mem::take(&mut self.pending_cmd), vec![handle])
        }
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
        let word = text.trim().trim_start_matches('_').to_uppercase();
        if let Some(settings) = self.settings {
            if word.len() >= 2 && "SETTINGS".starts_with(word.as_str()) {
                return Some(settings());
            }
        }
        if let Some((label, keyword, action)) = self.keyword {
            let capitals: String = label.chars().filter(char::is_ascii_uppercase).collect();
            if word == keyword || word == capitals || word == label.to_ascii_uppercase() {
                return Some(action());
            }
        }
        match text.trim().to_uppercase().as_str() {
            "S" | "SETTINGS" if self.auto_constrain_settings => {
                Some(CmdResult::OpenAutoConstrainSettings)
            }
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
        if self.keep.is_some() && self.handles.is_empty() {
            return CmdResult::Relaunch(std::mem::take(&mut self.pending_cmd), Vec::new());
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
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "ARRAY",
        "ARRAYPATH",
        "ARRAYPOLAR",
        "ARRAYRECT",
        "BLOCK",
        "COPY",
        "COPYCLIP",
        "CUTCLIP",
        "DCDISPLAY",
        "DIMDISASSOCIATE",
        "DIMREASSOCIATE",
        "ERASE",
        "EXPLODE",
        "GROUP",
        "LAYFRZ",
        "LAYLCK",
        "LAYCUR",
        "LAYMCUR",
        "LAYOFF",
        "LAYULK",
        "MIRROR",
        "MOVE",
        "ROTATE",
        "SCALE",
        "STRETCH",
        "TXTEXP"
    ]
}); // SelectObjectsCommand

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_objects_command_entity_pick_and_relaunch() {
        let mut cmd = SelectObjectsCommand::new("MOVE");
        assert!(cmd.is_selection_gathering());
        assert_eq!(cmd.prompt(), "MOVE  Select objects:");

        let h1 = Handle::new(42);
        let h2 = Handle::new(43);

        // Pick first entity
        let res1 = cmd.on_entity_pick(h1, DVec3::ZERO);
        assert!(matches!(res1, CmdResult::NeedPoint));
        assert_eq!(cmd.handles, vec![h1]);

        // Duplicate pick is ignored
        let res_dup = cmd.on_entity_pick(h1, DVec3::ZERO);
        assert!(matches!(res_dup, CmdResult::NeedPoint));
        assert_eq!(cmd.handles, vec![h1]);

        // Pick second entity
        let res2 = cmd.on_entity_pick(h2, DVec3::ZERO);
        assert!(matches!(res2, CmdResult::NeedPoint));
        assert_eq!(cmd.handles, vec![h1, h2]);

        // Enter finishes selection and relaunches pending command with accumulated handles
        let enter_res = cmd.on_enter();
        match enter_res {
            CmdResult::Relaunch(relaunch_cmd, handles) => {
                assert_eq!(relaunch_cmd, "MOVE");
                assert_eq!(handles, vec![h1, h2]);
            }
            _ => panic!("expected CmdResult::Relaunch"),
        }
    }

    #[test]
    fn test_select_objects_command_single_pick_variant() {
        let mut cmd = SelectObjectsCommand::instant("CHAMFER");
        let h = Handle::new(99);

        // Single-object variant relaunches immediately on pick
        let res = cmd.on_entity_pick(h, DVec3::ZERO);
        match res {
            CmdResult::Relaunch(relaunch_cmd, handles) => {
                assert_eq!(relaunch_cmd, "CHAMFER");
                assert_eq!(handles, vec![h]);
            }
            _ => panic!("expected immediate CmdResult::Relaunch"),
        }
    }
}
