//! The Annotate tab's Annotation Scaling panel: AIOBJECTSCALEADD,
//! AIOBJECTSCALEREMOVE, OBJECTSCALE / -OBJECTSCALE, ANNORESET, ANNOUPDATE,
//! SCALELISTEDIT / -SCALELISTEDIT and the annotation scale variables, with
//! the reference prompts and messages.
//!
//! The command-line forms keep their state in the prompt command itself:
//! every answer comes back as `_ANNOSCALEANS {state}\t{text}` and the next
//! prompt is started from the state the answer leaves.

use crate::app::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use document::Handle;
use iced::Task;
use serde::{Deserialize, Serialize};

/// A localized message without the bidi isolation marks Fluent puts round
/// its arguments; the command line shows them as stray characters.
macro_rules! trs {
    ($($arg:tt)*) => {
        crate::tr!($($arg)*).replace(['\u{2068}', '\u{2069}'], "")
    };
}

pub(crate) use crate::scene::annotative::{
    anno_monitor, selection_anno_display, set_anno_monitor, set_selection_anno_display,
};

/// The integer variables of the panel and their ranges.
const RANGED: &[(&str, i16, i16)] = &[
    ("ANNOAUTOSCALE", -4, 4),
    ("ANNOALLVISIBLE", 0, 1),
    ("SELECTIONANNODISPLAY", 0, 1),
    ("ANNOMONITOR", -2, 2),
    ("MSLTSCALE", 0, 1),
];

/// Every variable this module answers for.
pub(crate) fn is_anno_variable(name: &str) -> bool {
    name == "CANNOSCALE"
        || name == "CANNOSCALEVALUE"
        || RANGED.iter().any(|(var, _, _)| *var == name)
}

#[derive(Clone, Serialize, Deserialize)]
enum Step {
    /// -OBJECTSCALE `[Add/Delete/?]`.
    ObjOption,
    ObjAdd {
        first: bool,
    },
    ObjDelete {
        first: bool,
    },
    /// -SCALELISTEDIT `[?/Add/Delete/Reset/Exit]`.
    ListOption,
    ListName,
    ListRatio {
        name: String,
    },
    ListRedefine {
        name: String,
    },
    ListDelete,
    ListReset,
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    step: Step,
    /// Hex handles of the objects -OBJECTSCALE works on.
    #[serde(default)]
    handles: Vec<String>,
    /// The session already took its undo snapshot: the whole command is one
    /// undo step, as in the reference.
    #[serde(default)]
    snapped: bool,
}

impl Session {
    fn objects(&self) -> Vec<Handle> {
        self.handles
            .iter()
            .filter_map(|h| u64::from_str_radix(h, 16).ok().map(Handle::new))
            .collect()
    }
}

/// One prompt of a command-line session; the answer is handed back to the
/// application with the session state.
struct AnswerCommand {
    name: &'static str,
    prompt: String,
    options: Vec<CmdOption>,
    state: String,
    /// Objects are gathered first under this prompt (-OBJECTSCALE).
    select_prompt: Option<String>,
    /// The objects gathered so far.
    gathered: Vec<Handle>,
}

impl CadCommand for AnswerCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        self.select_prompt
            .clone()
            .unwrap_or_else(|| self.prompt.clone())
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.select_prompt.is_some() {
            return Vec::new();
        }
        self.options.clone()
    }

    fn input_kind(&self) -> InputKind {
        if self.select_prompt.is_some() {
            InputKind::Point
        } else {
            InputKind::SingleToken
        }
    }

    fn wants_text_input(&self) -> bool {
        self.select_prompt.is_none()
    }

    fn is_selection_gathering(&self) -> bool {
        self.select_prompt.is_some()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // Keep gathering until Enter.
        self.gathered = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(CmdResult::Dispatch(format!(
            "_ANNOSCALEANS {}\t{}",
            self.state,
            text.trim()
        )))
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.select_prompt.is_some() {
            // Enter ends the selection; nothing picked ends the command.
            if self.gathered.is_empty() {
                return CmdResult::Cancel;
            }
            if let Ok(mut session) = serde_json::from_str::<Session>(&self.state) {
                session.handles = hex(&self.gathered);
                self.state = serde_json::to_string(&session).unwrap_or_default();
            }
            self.select_prompt = None;
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("_ANNOSCALEANS {}	", self.state))
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Selection front-end: gathers the objects, then hands them to `action`.
pub(crate) struct AnnoSelectCommand {
    name: &'static str,
    prompt: String,
    action: &'static str,
    gathered: Vec<Handle>,
}

impl CadCommand for AnnoSelectCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        self.prompt.clone()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // Keep gathering until Enter.
        self.gathered = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _point: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.gathered.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::Relaunch(self.action.to_string(), std::mem::take(&mut self.gathered))
        }
    }
}

fn hex(handles: &[Handle]) -> Vec<String> {
    handles.iter().map(|h| format!("{:X}", h.value())).collect()
}

/// `1:1000                1             1             1` — the scale table
/// of `?`, numbered as the reference numbers it.
fn scale_table(rows: &[(String, f64, f64)]) -> Vec<String> {
    let number = |value: f64| {
        let text = format!("{value:.4}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text.is_empty() || text == "-0" {
            "0".to_string()
        } else {
            text.to_string()
        }
    };
    let mut lines = vec![
        "    Scale Name             Paper Units  Drawing Units  Effective Scale".to_string(),
        "    ----------             -----------  -------------  ---------------".to_string(),
    ];
    for (index, (name, paper, drawing)) in rows.iter().enumerate() {
        let effective = if *drawing > 0.0 { paper / drawing } else { 0.0 };
        lines.push(format!(
            "{:>2}: {:<23}{:>11}{:>14}{:>14}",
            index + 1,
            name,
            number(*paper),
            number(*drawing),
            number(effective)
        ));
    }
    lines
}

impl OpenCADStudio {
    /// Route the panel's commands and variables. Returns `None` for every
    /// other command.
    pub(super) fn dispatch_anno_scaling(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let trimmed = cmd.trim().trim_start_matches(['_', '.']);
        if let Some(rest) = trimmed.strip_prefix("ANNOSCALEANS ") {
            return Some(self.anno_scale_answer(i, rest));
        }
        let (verb, arg) = match trimmed.split_once(char::is_whitespace) {
            Some((verb, arg)) => (verb.to_ascii_uppercase(), Some(arg.trim())),
            None => (trimmed.to_ascii_uppercase(), None),
        };
        // `SETVAR NAME [value]` reaches the same handlers.
        if verb == "SETVAR" {
            let arg = arg?;
            let (name, value) = match arg.split_once(char::is_whitespace) {
                Some((name, value)) => (name.to_ascii_uppercase(), Some(value.trim())),
                None => (arg.to_ascii_uppercase(), None),
            };
            if !is_anno_variable(&name) {
                return None;
            }
            return Some(self.anno_variable(i, &name, value));
        }
        if is_anno_variable(&verb) {
            return Some(self.anno_variable(i, &verb, arg));
        }
        match verb.as_str() {
            "AIOBJECTSCALEADD" => {
                Some(self.anno_select(i, "AIOBJECTSCALEADD", "_ANNOSCALEADD_APPLY", true))
            }
            "AIOBJECTSCALEREMOVE" => {
                Some(self.anno_select(i, "AIOBJECTSCALEREMOVE", "_ANNOSCALEREMOVE_APPLY", true))
            }
            "ANNOSCALEADD_APPLY" => {
                let handles = self.anno_selected(i);
                Some(self.anno_add(i, &handles))
            }
            "ANNOSCALEREMOVE_APPLY" => {
                let handles = self.anno_selected(i);
                Some(self.anno_remove(i, &handles))
            }
            "-OBJECTSCALE" => {
                self.tabs[i].scene.deselect_all();
                Some(self.anno_scale_prompt_with(
                    Session {
                        step: Step::ObjOption,
                        handles: Vec::new(),
                        snapped: false,
                    },
                    i,
                    Some(trs!("anno-scale-cmd", "select-annotative")),
                ))
            }
            "OBJECTSCALE" if arg.is_some_and(|arg| arg.eq_ignore_ascii_case("ADD")) => {
                Some(self.anno_select(i, "AIOBJECTSCALEADD", "_ANNOSCALEADD_APPLY", true))
            }
            "OBJECTSCALE" if arg.is_some_and(|arg| arg.eq_ignore_ascii_case("DELETE")) => {
                Some(self.anno_select(i, "AIOBJECTSCALEREMOVE", "_ANNOSCALEREMOVE_APPLY", true))
            }
            "OBJECTSCALE" if arg.is_none() => {
                Some(self.anno_select(i, "OBJECTSCALE", "_ANNOSCALEDLG_APPLY", true))
            }
            "ANNOSCALEDLG_APPLY" => Some(Task::done(Message::AnnoObjectScaleOpen)),
            "ANNORESET" => {
                self.command_line
                    .push_output(&trs!("anno-scale-cmd", "reset-intro"));
                Some(self.anno_select(i, "ANNORESET", "_ANNORESET_APPLY", false))
            }
            "ANNORESET_APPLY" => {
                let handles = self.anno_selected(i);
                Some(self.anno_reset(i, &handles))
            }
            "ANNOUPDATE" if arg.is_none() => {
                Some(self.anno_select(i, "ANNOUPDATE", "_ANNOUPDATE_APPLY", false))
            }
            "ANNOUPDATE_APPLY" => {
                let handles = self.anno_selected(i);
                Some(self.anno_update(i, &handles))
            }
            "-SCALELISTEDIT" => Some(self.anno_scale_prompt(
                Session {
                    step: Step::ListOption,
                    handles: Vec::new(),
                    snapped: false,
                },
                i,
            )),
            "SCALELISTEDIT" if arg.is_none() => Some(Task::done(Message::ScaleManagerOpen)),
            _ => None,
        }
    }

    fn anno_selected(&mut self, i: usize) -> Vec<Handle> {
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(handle, _)| *handle)
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.tabs[i].scene.deselect_all();
        handles
    }

    /// Ask for the objects; the reference asks even with a preselection.
    fn anno_select(
        &mut self,
        i: usize,
        name: &'static str,
        action: &'static str,
        annotative_prompt: bool,
    ) -> Task<Message> {
        self.tabs[i].scene.deselect_all();
        let prompt = if annotative_prompt {
            trs!("anno-scale-cmd", "select-annotative")
        } else {
            crate::t!("Select objects:").into_owned()
        };
        let command = AnnoSelectCommand {
            name,
            prompt,
            action,
            gathered: Vec::new(),
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    fn anno_current_scale(&mut self, i: usize) -> Option<(String, Handle)> {
        let handle = self.tabs[i].scene.creation_annotation_scale_handle()?;
        let name = match self.tabs[i].scene.document.objects.get(&handle) {
            Some(document::objects::ObjectType::Scale(scale)) => scale.name.clone(),
            _ => return None,
        };
        Some((name, handle))
    }

    fn anno_snapshot(&mut self, i: usize, label: &str, session: Option<&mut Session>) {
        match session {
            Some(session) if session.snapped => {}
            Some(session) => {
                self.push_undo_snapshot(i, label);
                session.snapped = true;
            }
            None => self.push_undo_snapshot(i, label),
        }
    }

    fn anno_annotative(&self, i: usize, handles: &[Handle]) -> Vec<Handle> {
        let document = &self.tabs[i].scene.document;
        handles
            .iter()
            .copied()
            .filter(|handle| {
                document.get_entity(*handle).is_some_and(|entity| {
                    crate::scene::annotative::supports_annotation_context(entity)
                        && crate::scene::annotative::is_annotative(document, entity)
                })
            })
            .collect()
    }

    /// Give every annotative object `scale`; the count is the annotative
    /// objects, whether they had it already or not.
    fn anno_add_scale(
        &mut self,
        i: usize,
        handles: &[Handle],
        name: &str,
        scale: Handle,
        session: Option<&mut Session>,
        label: &str,
    ) {
        let targets = self.anno_annotative(i, handles);
        let missing: Vec<Handle> = targets
            .iter()
            .copied()
            .filter(|handle| {
                !crate::scene::annotative::object_scale_memberships(
                    &self.tabs[i].scene.document,
                    *handle,
                )
                .iter()
                .any(|(_, member)| *member == scale)
            })
            .collect();
        if !missing.is_empty() {
            self.anno_snapshot(i, label, session);
            for handle in &missing {
                crate::scene::annotative::create_annotation_context(
                    &mut self.tabs[i].scene.document,
                    *handle,
                    scale,
                );
            }
            self.anno_bump(i, &missing);
        }
        let message = if targets.len() == 1 {
            trs!("anno-scale-cmd", "updated-one", scale = name)
        } else {
            trs!(
                "anno-scale-cmd",
                "updated-many",
                count = targets.len(),
                scale = name
            )
        };
        self.command_line.push_output(&message);
    }

    /// Take `scale` off the annotative objects that have it. An object never
    /// loses its last scale. Without a named scale (`explicit` false) an
    /// object lacking the current scale loses the one it was given first instead, as the
    /// reference's current-scale removal does.
    #[allow(clippy::too_many_arguments)]
    fn anno_remove_scale(
        &mut self,
        i: usize,
        handles: &[Handle],
        name: &str,
        scale: Handle,
        explicit: bool,
        session: Option<&mut Session>,
        label: &str,
    ) {
        let targets = self.anno_annotative(i, handles);
        let mut removals = Vec::new();
        let mut unsupported = 0usize;
        let mut cannot = 0usize;
        for handle in &targets {
            let memberships = crate::scene::annotative::object_scale_memberships(
                &self.tabs[i].scene.document,
                *handle,
            );
            let has = memberships.iter().any(|(_, member)| *member == scale);
            if !has {
                unsupported += 1;
            }
            if memberships.len() < 2 {
                cannot += 1;
                continue;
            }
            if has {
                removals.push((*handle, scale));
            } else if !explicit {
                // Without the current scale the object loses the scale it
                // was given first (measured: 1:1, 1:2, 1:4 at 1:10 → 1:2,
                // 1:4; a scale added later never goes first).
                if let Some((_, first)) = memberships.first() {
                    removals.push((*handle, *first));
                }
            }
        }
        if !removals.is_empty() {
            self.anno_snapshot(i, label, session);
            for (handle, scale) in &removals {
                crate::scene::annotative::remove_annotation_context_for_scale(
                    &mut self.tabs[i].scene.document,
                    *handle,
                    *scale,
                );
            }
            let changed: Vec<Handle> = removals.iter().map(|(handle, _)| *handle).collect();
            self.anno_bump(i, &changed);
        }
        let removed = removals.len();
        self.command_line.push_output(&if removed == 1 {
            trs!("anno-scale-cmd", "removed-one")
        } else {
            trs!("anno-scale-cmd", "removed-many", count = removed)
        });
        if removed == 0 {
            if unsupported > 0 {
                self.command_line.push_output(&if unsupported == 1 {
                    trs!("anno-scale-cmd", "not-supported-one", scale = name)
                } else {
                    trs!(
                        "anno-scale-cmd",
                        "not-supported-many",
                        count = unsupported,
                        scale = name
                    )
                });
            }
            if cannot > 0 {
                self.command_line.push_output(&trs!(
                    "anno-scale-cmd",
                    "cannot-remove",
                    count = cannot
                ));
            }
        }
    }

    /// OK in the Annotation Object Scale window: each object gains the
    /// scales staged for it and loses the ones taken off, as one undo step.
    pub(crate) fn apply_anno_object_scale(&mut self) {
        let i = self.active_tab;
        let staged = std::mem::take(&mut self.anno_object_scale.staged);
        let mut edits: Vec<(Handle, Vec<String>, Vec<Handle>)> = Vec::new();
        for (handle, names) in staged {
            if self.tabs[i].scene.is_layer_locked(handle) {
                continue;
            }
            let current = crate::scene::annotative::object_scale_memberships(
                &self.tabs[i].scene.document,
                handle,
            );
            let add: Vec<String> = names
                .iter()
                .filter(|name| {
                    !current
                        .iter()
                        .any(|(have, _)| have.eq_ignore_ascii_case(name))
                })
                .cloned()
                .collect();
            let remove: Vec<Handle> = current
                .iter()
                .filter(|(have, _)| !names.iter().any(|name| name.eq_ignore_ascii_case(have)))
                .map(|(_, scale)| *scale)
                .collect();
            if !add.is_empty() || !remove.is_empty() {
                edits.push((handle, add, remove));
            }
        }
        if edits.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "OBJECTSCALE");
        for (handle, add, remove) in &edits {
            for name in add {
                if let Some(scale) = self.tabs[i].scene.scale_handle_ensuring(name) {
                    crate::scene::annotative::create_annotation_context(
                        &mut self.tabs[i].scene.document,
                        *handle,
                        scale,
                    );
                }
            }
            for scale in remove {
                crate::scene::annotative::remove_annotation_context_for_scale(
                    &mut self.tabs[i].scene.document,
                    *handle,
                    *scale,
                );
            }
        }
        let changed: Vec<Handle> = edits.iter().map(|(handle, _, _)| *handle).collect();
        self.anno_bump(i, &changed);
        self.refresh_properties();
    }

    fn anno_bump(&mut self, i: usize, handles: &[Handle]) {
        let changes: Vec<_> = handles
            .iter()
            .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    fn anno_add(&mut self, i: usize, handles: &[Handle]) -> Task<Message> {
        if let Some((name, scale)) = self.anno_current_scale(i) {
            self.anno_add_scale(i, handles, &name, scale, None, "AIOBJECTSCALEADD");
        }
        Task::none()
    }

    fn anno_remove(&mut self, i: usize, handles: &[Handle]) -> Task<Message> {
        if let Some((name, scale)) = self.anno_current_scale(i) {
            self.anno_remove_scale(i, handles, &name, scale, false, None, "AIOBJECTSCALEREMOVE");
        }
        Task::none()
    }

    /// ANNORESET: objects without the current scale are "not annotative"
    /// for it; the others take their other representations' positions from
    /// the current one.
    fn anno_reset(&mut self, i: usize, handles: &[Handle]) -> Task<Message> {
        let scale = self.tabs[i].scene.creation_annotation_scale_handle();
        let supported: Vec<Handle> = self
            .anno_annotative(i, handles)
            .into_iter()
            .filter(|handle| {
                crate::scene::annotative::object_scale_memberships(
                    &self.tabs[i].scene.document,
                    *handle,
                )
                .iter()
                .any(|(_, member)| Some(*member) == scale)
            })
            .collect();
        let skipped = handles.len() - supported.len();
        if skipped > 0 {
            self.command_line.push_output(&if skipped == 1 {
                trs!(
                    "anno-scale-cmd",
                    "not-annotative-one",
                    found = handles.len()
                )
            } else {
                trs!(
                    "anno-scale-cmd",
                    "not-annotative-many",
                    found = handles.len(),
                    count = skipped
                )
            });
        }
        let reset: Vec<Handle> = supported
            .into_iter()
            .filter(|handle| {
                crate::scene::annotative::object_scale_memberships(
                    &self.tabs[i].scene.document,
                    *handle,
                )
                .len()
                    > 1
            })
            .collect();
        if !reset.is_empty() {
            self.push_undo_snapshot(i, "ANNORESET");
            let mut changed = Vec::new();
            for handle in reset {
                if crate::scene::annotative::reset_annotation_context_positions(
                    &mut self.tabs[i].scene.document,
                    handle,
                    scale,
                ) {
                    changed.push(handle);
                }
            }
            if !changed.is_empty() {
                self.anno_bump(i, &changed);
            }
        }
        Task::none()
    }

    /// ANNOUPDATE: `N found M were updated`.
    fn anno_update(&mut self, i: usize, handles: &[Handle]) -> Task<Message> {
        let scale = self.tabs[i].scene.creation_annotation_scale_handle();
        let paper_to_model = self.tabs[i].scene.creation_annotation_multiplier();
        self.push_undo_snapshot(i, "ANNOUPDATE");
        let mut changed = Vec::new();
        for handle in handles {
            // Only an object the update really changed counts as updated.
            let annotative = |doc: &document::Document| {
                doc.get_entity(*handle)
                    .is_some_and(|entity| crate::scene::annotative::is_annotative(doc, entity))
            };
            let before = self.tabs[i].scene.document.get_entity(*handle).cloned();
            let was_annotative = annotative(&self.tabs[i].scene.document);
            if crate::scene::annotative::update_entity_from_annotation_style(
                &mut self.tabs[i].scene.document,
                *handle,
                scale,
                paper_to_model,
            ) && (self.tabs[i].scene.document.get_entity(*handle) != before.as_ref()
                || annotative(&self.tabs[i].scene.document) != was_annotative)
            {
                changed.push(*handle);
            }
        }
        if !changed.is_empty() {
            self.anno_bump(i, &changed);
        }
        self.command_line.push_output(&if changed.len() == 1 {
            trs!("anno-scale-cmd", "updated-found-one", found = handles.len())
        } else {
            trs!(
                "anno-scale-cmd",
                "updated-found-many",
                found = handles.len(),
                count = changed.len()
            )
        });
        Task::none()
    }

    /// The drawing's scales in list order with paper and drawing units.
    fn anno_scale_rows(&self, i: usize) -> Vec<(String, f64, f64)> {
        let scene = &self.tabs[i].scene;
        scene
            .scale_names_in_list_order()
            .into_iter()
            .filter_map(|name| {
                scene
                    .scale_paper_drawing(&name)
                    .map(|(paper, drawing)| (name, paper, drawing))
            })
            .collect()
    }

    fn anno_print_table(&mut self, rows: &[(String, f64, f64)]) {
        for line in scale_table(rows) {
            self.command_line.push_table(&line);
        }
    }

    fn anno_scale_prompt(&mut self, session: Session, i: usize) -> Task<Message> {
        self.anno_scale_prompt_with(session, i, None)
    }

    fn anno_scale_prompt_with(
        &mut self,
        session: Session,
        i: usize,
        select_prompt: Option<String>,
    ) -> Task<Message> {
        let current = self
            .anno_current_scale(i)
            .map(|(name, _)| name)
            .unwrap_or_default();
        let (name, prompt, options): (&'static str, String, Vec<CmdOption>) = match &session.step {
            Step::ObjOption => (
                "-OBJECTSCALE",
                trs!("anno-scale-cmd", "option"),
                vec![
                    CmdOption::new("Add", "A"),
                    CmdOption::new("Delete", "D"),
                    CmdOption::new("?", "?"),
                ],
            ),
            Step::ObjAdd { first } => (
                "-OBJECTSCALE",
                if *first {
                    trs!(
                        "anno-scale-cmd",
                        "add-scale-default",
                        scale = current.as_str()
                    )
                } else {
                    trs!("anno-scale-cmd", "add-scale")
                },
                vec![CmdOption::new("?", "?")],
            ),
            Step::ObjDelete { first } => (
                "-OBJECTSCALE",
                if *first {
                    trs!(
                        "anno-scale-cmd",
                        "delete-scale-default",
                        scale = current.as_str()
                    )
                } else {
                    trs!("anno-scale-cmd", "delete-scale")
                },
                vec![CmdOption::new("?", "?")],
            ),
            Step::ListOption => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-option"),
                vec![
                    CmdOption::new("?", "?"),
                    CmdOption::new("Add", "A"),
                    CmdOption::new("Delete", "D"),
                    CmdOption::new("Reset", "R"),
                    CmdOption::new("Exit", "E"),
                ],
            ),
            Step::ListName => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-name"),
                Vec::new(),
            ),
            Step::ListRatio { .. } => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-ratio"),
                Vec::new(),
            ),
            Step::ListRedefine { name } => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-exists", name = name.as_str()),
                vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            ),
            Step::ListDelete => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-delete"),
                Vec::new(),
            ),
            Step::ListReset => (
                "-SCALELISTEDIT",
                trs!("anno-scale-cmd", "list-reset"),
                vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            ),
        };
        let state = serde_json::to_string(&session).unwrap_or_default();
        let command = AnswerCommand {
            name,
            prompt,
            options,
            state,
            select_prompt,
            gathered: Vec::new(),
        };
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    fn anno_scale_answer(&mut self, i: usize, rest: &str) -> Task<Message> {
        let (state, text) = rest.split_once('\t').unwrap_or((rest, ""));
        let Ok(mut session) = serde_json::from_str::<Session>(state) else {
            return Task::none();
        };
        let text = text.trim();
        let upper = text.to_ascii_uppercase();
        let invalid = |app: &mut Self| {
            app.command_line
                .push_rejection(&trs!("anno-scale-cmd", "invalid-keyword"));
        };
        let objects = session.objects();
        let next = match session.step.clone() {
            Step::ObjOption => match upper.as_str() {
                "" | "A" | "AD" | "ADD" => Some(Step::ObjAdd { first: true }),
                "D" | "DE" | "DEL" | "DELETE" => Some(Step::ObjDelete { first: true }),
                "?" => {
                    let rows = self.anno_scale_rows(i);
                    self.anno_print_table(&rows);
                    Some(Step::ObjOption)
                }
                _ => {
                    invalid(self);
                    Some(Step::ObjOption)
                }
            },
            Step::ObjAdd { first } | Step::ObjDelete { first } => {
                let adding = matches!(session.step, Step::ObjAdd { .. });
                let label = "-OBJECTSCALE";
                let again = if adding {
                    Step::ObjAdd { first: false }
                } else {
                    Step::ObjDelete { first: false }
                };
                if text.is_empty() && !first {
                    None
                } else if text == "?" {
                    let rows = if adding {
                        self.anno_scale_rows(i)
                    } else {
                        self.anno_object_scale_rows(i, &objects)
                    };
                    self.anno_print_table(&rows);
                    Some(again)
                } else {
                    let target = if text.is_empty() {
                        self.anno_current_scale(i)
                    } else {
                        let scene = &mut self.tabs[i].scene;
                        scene.scale_object_handle(text).and_then(|handle| {
                            match scene.document.objects.get(&handle) {
                                Some(document::objects::ObjectType::Scale(scale)) => {
                                    Some((scale.name.clone(), handle))
                                }
                                _ => None,
                            }
                        })
                    };
                    match target {
                        Some((name, scale)) if adding => self.anno_add_scale(
                            i,
                            &objects,
                            &name,
                            scale,
                            Some(&mut session),
                            label,
                        ),
                        Some((name, scale)) => self.anno_remove_scale(
                            i,
                            &objects,
                            &name,
                            scale,
                            true,
                            Some(&mut session),
                            label,
                        ),
                        None => self.command_line.push_output(&trs!(
                            "anno-scale-cmd",
                            "scale-not-found",
                            name = text
                        )),
                    }
                    Some(again)
                }
            }
            Step::ListOption => match upper.as_str() {
                "?" => {
                    let rows = self.anno_scale_rows(i);
                    self.anno_print_table(&rows);
                    Some(Step::ListOption)
                }
                "" | "A" | "AD" | "ADD" => Some(Step::ListName),
                "D" | "DE" | "DEL" | "DELETE" => Some(Step::ListDelete),
                "R" | "RE" | "RES" | "RESET" => Some(Step::ListReset),
                "E" | "EX" | "EXIT" => None,
                _ => {
                    invalid(self);
                    Some(Step::ListOption)
                }
            },
            Step::ListName => {
                if text.is_empty() {
                    Some(Step::ListOption)
                } else if self.tabs[i].scene.scale_object_handle(text).is_some() {
                    Some(Step::ListRedefine {
                        name: text.to_string(),
                    })
                } else {
                    Some(Step::ListRatio {
                        name: text.to_string(),
                    })
                }
            }
            Step::ListRedefine { name } => match upper.as_str() {
                "Y" | "YES" => Some(Step::ListRatio { name }),
                "" | "N" | "NO" => Some(Step::ListOption),
                _ => {
                    invalid(self);
                    Some(Step::ListRedefine { name })
                }
            },
            Step::ListRatio { name } => match parse_ratio(text) {
                Some((paper, drawing)) => {
                    self.anno_snapshot(i, "-SCALELISTEDIT", Some(&mut session));
                    let scene = &mut self.tabs[i].scene;
                    if !scene.add_scale(&name, paper, drawing) {
                        scene.redefine_scale(&name, paper, drawing);
                    }
                    self.tabs[i].dirty = true;
                    Some(Step::ListOption)
                }
                None => {
                    self.command_line
                        .push_rejection(&trs!("anno-scale-cmd", "list-bad-ratio"));
                    Some(Step::ListRatio { name })
                }
            },
            Step::ListDelete => {
                if text == "*" {
                    let unused: Vec<String> = self
                        .anno_scale_rows(i)
                        .into_iter()
                        .map(|(name, _, _)| name)
                        .filter(|name| !self.anno_scale_referenced(i, name))
                        .collect();
                    if !unused.is_empty() {
                        self.anno_snapshot(i, "-SCALELISTEDIT", Some(&mut session));
                        for name in unused {
                            self.tabs[i].scene.remove_scale(&name);
                        }
                        self.tabs[i].dirty = true;
                    }
                } else if !text.is_empty() {
                    let found = self.tabs[i]
                        .scene
                        .scale_object_handle(text)
                        .and_then(|handle| {
                            match self.tabs[i].scene.document.objects.get(&handle) {
                                Some(document::objects::ObjectType::Scale(scale)) => {
                                    Some(scale.name.clone())
                                }
                                _ => None,
                            }
                        });
                    match found {
                        None => self.command_line.push_output(&trs!(
                            "anno-scale-cmd",
                            "scale-not-found",
                            name = text
                        )),
                        Some(name) if self.anno_scale_referenced(i, &name) => {
                            self.command_line.push_output(&trs!(
                                "anno-scale-cmd",
                                "list-referenced",
                                name = name.as_str()
                            ))
                        }
                        Some(name) => {
                            self.anno_snapshot(i, "-SCALELISTEDIT", Some(&mut session));
                            self.tabs[i].scene.remove_scale(&name);
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(&trs!(
                                "anno-scale-cmd",
                                "list-deleted",
                                name = name.as_str()
                            ));
                        }
                    }
                }
                Some(Step::ListOption)
            }
            Step::ListReset => match upper.as_str() {
                "Y" | "YES" => {
                    self.anno_snapshot(i, "-SCALELISTEDIT", Some(&mut session));
                    let keep: Vec<String> = self
                        .anno_scale_rows(i)
                        .into_iter()
                        .map(|(name, _, _)| name)
                        .filter(|name| self.anno_scale_referenced(i, name))
                        .collect();
                    self.tabs[i].scene.reset_scale_list(&keep);
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(&trs!("anno-scale-cmd", "list-reset-done"));
                    Some(Step::ListOption)
                }
                "" | "N" | "NO" => {
                    self.command_line
                        .push_output(&trs!("anno-scale-cmd", "list-reset-kept"));
                    Some(Step::ListOption)
                }
                _ => {
                    invalid(self);
                    Some(Step::ListReset)
                }
            },
        };
        match next {
            Some(step) => {
                session.step = step;
                self.anno_scale_prompt(session, i)
            }
            None => {
                self.tabs[i].active_cmd = None;
                Task::none()
            }
        }
    }

    /// The scales the objects carry, in the drawing's list order.
    fn anno_object_scale_rows(&self, i: usize, handles: &[Handle]) -> Vec<(String, f64, f64)> {
        let document = &self.tabs[i].scene.document;
        let mut names: Vec<String> = Vec::new();
        for handle in handles {
            for (name, _) in crate::scene::annotative::object_scale_memberships(document, *handle) {
                if !names.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
                    names.push(name);
                }
            }
        }
        self.anno_scale_rows(i)
            .into_iter()
            .filter(|(name, _, _)| names.iter().any(|known| known.eq_ignore_ascii_case(name)))
            .collect()
    }

    /// The current scale and the scales objects use are referenced.
    pub(crate) fn anno_scale_referenced(&self, i: usize, name: &str) -> bool {
        let scene = &self.tabs[i].scene;
        scene
            .document
            .header
            .current_annotation_scale
            .eq_ignore_ascii_case(name)
            || scene.scale_in_use_count(name) > 0
    }

    /// CANNOSCALE, CANNOSCALEVALUE and the ranged variables.
    fn anno_variable(&mut self, i: usize, name: &str, value: Option<&str>) -> Task<Message> {
        let value = value.map(str::trim).filter(|value| !value.is_empty());
        match name {
            "CANNOSCALEVALUE" => {
                let current = self.tabs[i].scene.document.header.annotation_scale_value;
                self.command_line
                    .push_output(&crate::tf!("CANNOSCALEVALUE = {current:.9} (read only)"));
            }
            "CANNOSCALE" => match value {
                None => {
                    let current = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_annotation_scale
                        .clone();
                    self.command_line
                        .push_output(&crate::ui::command_line::value_prompt(
                            "CANNOSCALE",
                            &format!("\"{current}\""),
                        ));
                    self.pending_setvar = Some("CANNOSCALE".into());
                }
                Some(value) => {
                    let previous = self.tabs[i].scene.displayed_annotation_scale_handle();
                    match self.tabs[i].scene.set_annotation_scale_named(value) {
                        Some(handle) => {
                            if self.annotation_auto_scale > 0 {
                                self.tabs[i].scene.add_annotation_scale_to_objects(
                                    handle,
                                    previous,
                                    self.annotation_auto_scale as u8,
                                );
                            }
                            self.tabs[i].dirty = true;
                        }
                        None => {
                            self.command_line
                                .push_rejection(&trs!("anno-scale-cmd", "cannoscale-invalid"));
                            self.command_line.push_rejection("*Invalid*");
                        }
                    }
                }
            },
            _ => {
                let Some(&(_, min, max)) = RANGED.iter().find(|(var, _, _)| *var == name) else {
                    return Task::none();
                };
                let current = self.anno_ranged_value(i, name);
                let ask = |app: &mut Self| {
                    app.command_line
                        .push_output(&crate::ui::command_line::value_prompt(name, &current));
                    app.pending_setvar = Some(name.to_string());
                };
                let Some(value) = value else {
                    ask(self);
                    return Task::none();
                };
                match value.parse::<i16>() {
                    Ok(parsed) if (min..=max).contains(&parsed) => {
                        self.anno_set_ranged(i, name, parsed);
                    }
                    _ => {
                        let message = if min == 0 && max == 1 {
                            crate::t!("Requires 0 or 1 only.").into_owned()
                        } else {
                            crate::tf!("Requires an integer between {min} and {max}.").into_owned()
                        };
                        self.command_line.push_rejection(&message);
                        ask(self);
                    }
                }
            }
        }
        Task::none()
    }

    fn anno_ranged_value(&self, i: usize, name: &str) -> i16 {
        match name {
            "ANNOAUTOSCALE" => self.annotation_auto_scale as i16,
            "ANNOALLVISIBLE" => i16::from(self.tabs[i].scene.annotation_all_visible()),
            "MSLTSCALE" => i16::from(self.tabs[i].scene.model_linetype_scaling()),
            "SELECTIONANNODISPLAY" => selection_anno_display(),
            "ANNOMONITOR" => anno_monitor(),
            _ => 0,
        }
    }

    fn anno_set_ranged(&mut self, i: usize, name: &str, value: i16) {
        match name {
            "ANNOAUTOSCALE" => {
                self.annotation_auto_scale = value as i8;
                self.save_config();
            }
            "ANNOALLVISIBLE" => {
                self.tabs[i].scene.set_annotation_all_visible(value == 1);
                self.tabs[i].dirty = true;
            }
            "MSLTSCALE" => {
                self.tabs[i].scene.set_model_linetype_scaling(value == 1);
                self.tabs[i].dirty = true;
            }
            "SELECTIONANNODISPLAY" => {
                set_selection_anno_display(value);
                self.save_config();
            }
            "ANNOMONITOR" => {
                set_anno_monitor(value);
                self.save_config();
            }
            _ => {}
        }
    }
}

/// `paper:drawing`; a zero side counts as 1, as the reference stores it.
fn parse_ratio(text: &str) -> Option<(f64, f64)> {
    let (paper, drawing) = text.split_once(':')?;
    let side = |value: &str| {
        crate::entities::common::parse_length(value.trim())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| if value == 0.0 { 1.0 } else { value })
    };
    Some((side(paper)?, side(drawing)?))
}
