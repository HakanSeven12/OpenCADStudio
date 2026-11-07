//! Read-only object sampling for the selection-filter dialog.

use crate::app::{OpenCADStudio, QSelectCondition, QSelectState};
use crate::command::{CadCommand, CmdResult};
use document::Handle;
use glam::DVec3;
use iced::Task;

/// The command owns the suspended dialog, so Escape can restore the exact
/// draft, committed conditions, scope and result mode without global pick state.
struct QSelectSampleCommand {
    state: Option<Box<QSelectState>>,
    tab_id: u64,
}

impl QSelectSampleCommand {
    fn finish(&mut self, handle: Option<Handle>) -> CmdResult {
        match self.state.take() {
            Some(state) => CmdResult::QSelectSample {
                state,
                tab_id: self.tab_id,
                handle,
            },
            None => CmdResult::Cancel,
        }
    }
}

impl CadCommand for QSelectSampleCommand {
    fn name(&self) -> &'static str {
        "FILTER"
    }

    fn prompt(&self) -> String {
        format!("FILTER  {}", crate::t!("Select object:"))
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }
    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        if handle.is_null() {
            CmdResult::NeedPoint
        } else {
            self.finish(Some(handle))
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        self.finish(None)
    }
    fn on_escape(&mut self) -> CmdResult {
        self.finish(None)
    }
}

fn append_unique_conditions(target: &mut Vec<QSelectCondition>, incoming: Vec<QSelectCondition>) {
    for condition in incoming {
        let duplicate = target.iter().any(|existing| {
            existing.type_filter == condition.type_filter
                && existing.property == condition.property
                && existing.operator == condition.operator
                && existing.value.eq_ignore_ascii_case(&condition.value)
        });
        if !duplicate {
            target.push(condition);
        }
    }
}

impl OpenCADStudio {
    pub(super) fn on_qselect_pick_object(&mut self) -> Task<crate::app::Message> {
        // Finish or cancel a condition edit first. Sampling only appends rows.
        if self
            .qselect
            .as_ref()
            .is_none_or(|state| state.editing_condition.is_some())
        {
            return Task::none();
        }
        let state = self.qselect.take().expect("checked open filter");
        // Keep a recoverable copy if another command or a document switch
        // interrupts picking instead of ending it through Enter/Escape.
        self.qselect_settings = Some((&state).into());
        let i = self.active_tab;
        let command = QSelectSampleCommand {
            state: Some(Box::new(state)),
            tab_id: self.tabs[i].id,
        };
        self.active_modal = None;
        self.command_line.input.clear();
        self.command_line.set_step_options(Vec::new());
        self.command_line.push_info(&command.prompt());
        self.tabs[i].scene.selection.borrow_mut().menu.close();
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// The picked object's properties, offered ticked or not; only what
    /// describes it, not where it sits, is ticked (#2611). Quick Select and
    /// FILTER's From object share it.
    pub(in crate::app) fn sample_from_entity(
        &self,
        i: usize,
        handle: Handle,
    ) -> crate::app::QSelectSample {
        let conditions = self.tabs[i].scene.qselect_conditions_from_entity(handle);
        let type_name = conditions
            .first()
            .and_then(|condition| condition.type_filter.clone())
            .unwrap_or_default();
        let rows = conditions
            .into_iter()
            .map(|condition| {
                let ticked = condition.property.as_ref().is_none_or(|property| {
                    SAMPLE_TICKED_FIELDS.contains(&property.field.as_str())
                });
                (condition, ticked)
            })
            .collect();
        crate::app::QSelectSample { type_name, rows }
    }

    pub(in crate::app) fn finish_qselect_sample(
        &mut self,
        mut state: QSelectState,
        tab_id: u64,
        handle: Option<Handle>,
    ) {
        let i = self.active_tab;
        // A handle is only meaningful in its original document.
        if self.tabs[i].id != tab_id {
            return;
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        if let Some(handle) = handle {
            state.sample = Some(self.sample_from_entity(i, handle));
            state.error = None;
        }
        self.qselect = Some(state);
        self.qselect_refresh_live();
    }

    /// Add the picked object's ticked properties as conditions.
    pub(super) fn on_qselect_sample_add(&mut self) {
        if let Some(state) = self.qselect.as_mut() {
            if let Some(sample) = state.sample.take() {
                let ticked = sample
                    .rows
                    .into_iter()
                    .filter_map(|(condition, ticked)| ticked.then_some(condition))
                    .collect();
                append_unique_conditions(&mut state.conditions, ticked);
            }
        }
        self.qselect_refresh_live();
    }

    /// Count (and, with Show on, highlight) what the conditions find now.
    pub(in crate::app) fn qselect_refresh_live(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.qselect.as_ref() else {
            self.tabs[i].scene.set_filter_preview_highlights(&[]);
            return;
        };
        let matches = state
            .effective_conditions()
            .map_or_else(Vec::new, |conditions| {
                self.tabs[i]
                    .scene
                    .qselect_matches(state.scope, &conditions, state.mode)
            });
        let show = state.show;
        self.tabs[i]
            .scene
            .set_filter_preview_highlights(if show { &matches } else { &[] });
        let mut by_type = std::collections::BTreeMap::<String, usize>::new();
        for h in &matches {
            if let Some(entity) = self.tabs[i].scene.document.get_entity(*h) {
                *by_type
                    .entry(crate::entities::traits::entity_type_name(entity).to_string())
                    .or_default() += 1;
            }
        }
        let mut by_type: Vec<(String, usize)> = by_type.into_iter().collect();
        by_type.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if let Some(state) = self.qselect.as_mut() {
            state.live_count = matches.len();
            state.live_by_type = by_type;
        }
    }

    /// The values the chosen property takes in the scope, for the value list
    /// and its shortcuts. Typing a value doesn't change them.
    pub(in crate::app) fn qselect_refresh_values(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.qselect.as_ref() else {
            return;
        };
        let values = match &state.property {
            Some(property) => self.tabs[i].scene.qselect_value_counts(
                state.scope,
                state.type_filter.as_deref(),
                &property.field,
                QSELECT_VALUE_LIMIT,
            ),
            None => Vec::new(),
        };
        if let Some(state) = self.qselect.as_mut() {
            state.value_counts = values;
        }
    }

    /// The dialog goes away: its highlight goes with it.
    pub(in crate::app) fn qselect_clear_preview(&mut self) {
        let i = self.active_tab;
        self.tabs[i].scene.set_filter_preview_highlights(&[]);
    }
}

/// Properties of a picked object ticked by default: what it is and how it
/// looks, not its geometry.
use crate::app::SAMPLE_TICKED_FIELDS;

/// How many distinct values the value list offers.
const QSELECT_VALUE_LIMIT: usize = 50;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{QSelectMode, QSelectOp, QSelectScope};

    fn state() -> QSelectState {
        QSelectState {
            conditions: Vec::new(),
            editing_condition: None,
            scope: QSelectScope::CurrentSelection,
            available_types: Vec::new(),
            available_properties: Vec::new(),
            candidate_count: 2,
            type_filter: Some("Line".into()),
            property: None,
            operator: QSelectOp::Eq,
            value: "unfinished draft".into(),
            mode: QSelectMode::Exclude,
            result: crate::app::QSelectResult::New,
            error: None,
            live_count: 0,
            live_by_type: Vec::new(),
            show: false,
            value_counts: Vec::new(),
            sample: None,
        }
    }

    #[test]
    fn escape_restores_draft_and_scope_without_a_sample() {
        let mut command = QSelectSampleCommand {
            state: Some(Box::new(state())),
            tab_id: 17,
        };
        let CmdResult::QSelectSample {
            state,
            tab_id,
            handle,
        } = command.on_escape()
        else {
            panic!("Escape must restore the filter");
        };
        assert_eq!(tab_id, 17);
        assert!(handle.is_none());
        assert_eq!(state.value, "unfinished draft");
        assert!(matches!(state.scope, QSelectScope::CurrentSelection));
        assert!(matches!(state.mode, QSelectMode::Exclude));
    }

    #[test]
    fn empty_click_keeps_picker_active_and_enter_restores_dialog() {
        let mut command = QSelectSampleCommand {
            state: Some(Box::new(state())),
            tab_id: 17,
        };
        assert!(matches!(
            command.on_entity_pick(Handle::NULL, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_enter(),
            CmdResult::QSelectSample { handle: None, .. }
        ));
    }

    #[test]
    fn repeated_sampling_preserves_existing_conditions_without_duplicates() {
        let condition = state().draft_condition();
        let mut conditions = vec![condition.clone()];
        append_unique_conditions(&mut conditions, vec![condition.clone(), condition]);
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].value, "unfinished draft");
    }

    #[test]
    fn sample_round_trip_keeps_selection_and_dialog_geometry() {
        use document::entities::Line;
        use document::types::Vector3;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        let sample = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(20.0, 0.0, 0.0),
            )));
        app.tabs[i].scene.select_entity(first, false);
        let selection = app.tabs[i].scene.selected.clone();
        let dirty = app.tabs[i].dirty;
        app.qselect = Some(state());
        app.modal_offset = iced::Vector::new(10.0, 20.0);
        app.modal_resize = iced::Vector::new(30.0, 40.0);

        let _ = app.on_qselect_pick_object();
        assert!(app.qselect.is_none());
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(sample, DVec3::ZERO);
        let _ = app.apply_cmd_result(result);
        let restored = app.qselect.as_ref().unwrap();
        // The picked object's properties wait to be ticked; only the type,
        // layer, colour and linetype rows start ticked.
        let sample = restored
            .sample
            .as_ref()
            .expect("the pick offers its properties");
        assert_eq!(sample.type_name, "Line");
        assert!(sample.rows.iter().all(|(condition, ticked)| {
            *ticked
                == condition
                    .property
                    .as_ref()
                    .is_none_or(|p| super::SAMPLE_TICKED_FIELDS.contains(&p.field.as_str()))
        }));
        assert!(
            restored.conditions.is_empty(),
            "nothing is added before the ticks"
        );
        assert_eq!(restored.value, "unfinished draft");
        assert!(matches!(restored.scope, QSelectScope::CurrentSelection));
        assert_eq!(app.tabs[i].scene.selected, selection);
        assert_eq!(app.tabs[i].dirty, dirty);
        assert_eq!(app.modal_offset, iced::Vector::new(10.0, 20.0));
        assert_eq!(app.modal_resize, iced::Vector::new(30.0, 40.0));
        assert!(app.tabs[i].active_cmd.is_none());

        app.qselect.as_mut().unwrap().mode = QSelectMode::Include;
        app.on_qselect_sample_add();
        let restored = app.qselect.as_ref().unwrap();
        assert_eq!(
            restored.conditions.len(),
            1 + super::SAMPLE_TICKED_FIELDS.len(),
            "the type and the ticked rows"
        );
        // Geometry isn't ticked, so the selected line (the scope here) matches
        // the sample although it is shorter.
        assert_eq!(restored.live_count, 1);
        let count = restored.conditions.len();
        let _ = app.on_qselect_pick_object();
        let result = app.tabs[i].active_cmd.as_mut().unwrap().on_escape();
        let _ = app.apply_cmd_result(result);
        assert_eq!(app.qselect.as_ref().unwrap().conditions.len(), count);
        assert_eq!(app.tabs[i].scene.selected, selection);
    }

    /// #2611: the dialog counts what it finds as it is edited, highlights it
    /// on request without touching the selection, and Select adds or removes.
    #[test]
    fn live_count_show_and_result_modes() {
        use crate::app::{Message, QSelectResult};
        use document::entities::{Circle, Line};
        use document::types::Vector3;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = |app: &mut OpenCADStudio| {
            app.tabs[i]
                .scene
                .add_entity(document::EntityType::Line(Line::from_points(
                    Vector3::ZERO,
                    Vector3::new(10.0, 0.0, 0.0),
                )))
        };
        let first = line(&mut app);
        let second = line(&mut app);
        let circle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Circle(Circle::new()));
        app.tabs[i].scene.select_entity(circle, false);

        let _ = app.update(Message::QSelectOpen);
        let _ = app.update(Message::QSelectSetScope(QSelectScope::CurrentSpace));
        let _ = app.update(Message::QSelectSetType(Some("Line".into())));
        let state = app.qselect.as_ref().unwrap();
        assert_eq!(
            state.live_count, 2,
            "the two lines, before anything is applied"
        );
        assert!(state.available_types.contains(&("Line".to_string(), 2)));
        assert!(!state.show, "highlighting starts off");
        assert!(app.tabs[i].scene.hover_highlight_handles().is_empty());

        let _ = app.update(Message::QSelectSetShow(true));
        let shown = app.tabs[i].scene.hover_highlight_handles();
        assert!(shown.contains(&first) && shown.contains(&second) && !shown.contains(&circle));
        assert_eq!(
            app.tabs[i].scene.selected.len(),
            1,
            "showing doesn't select"
        );

        let _ = app.update(Message::QSelectSetResult(QSelectResult::Add));
        let _ = app.update(Message::QSelectApply);
        assert!(app.qselect.is_none());
        assert_eq!(
            app.tabs[i].scene.selected.len(),
            3,
            "the lines join the circle"
        );
        assert!(
            app.tabs[i].scene.hover_highlight_handles().is_empty(),
            "the highlight goes with the dialog"
        );

        let _ = app.update(Message::QSelectOpen);
        let state = app.qselect.as_ref().unwrap();
        assert!(state.show, "Show is remembered");
        assert_eq!(state.result, QSelectResult::Add, "so is the result");
        let _ = app.update(Message::QSelectSetResult(QSelectResult::Remove));
        let _ = app.update(Message::QSelectApply);
        assert_eq!(app.tabs[i].scene.selected.len(), 1);
        assert!(app.tabs[i].scene.selected.contains(&circle));

        let _ = app.update(Message::QSelectOpen);
        let _ = app.update(Message::QSelectClose);
        assert!(app.tabs[i].scene.hover_highlight_handles().is_empty());
    }
}
