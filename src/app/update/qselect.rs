//! Read-only object sampling for the selection-filter dialog.

use crate::app::{OpenCADStudio, QSelectCondition, QSelectState};
use crate::command::{CadCommand, CmdResult};
use codec::Handle;
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
            let conditions = self.tabs[i].scene.qselect_conditions_from_entity(handle);
            append_unique_conditions(&mut state.conditions, conditions);
            state.error = None;
        }
        self.qselect = Some(state);
    }
}

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
            append: false,
            error: None,
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
        use codec::entities::Line;
        use codec::types::Vector3;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        let sample = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
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
        assert!(!restored.conditions.is_empty());
        assert_eq!(restored.value, "unfinished draft");
        assert!(matches!(restored.scope, QSelectScope::CurrentSelection));
        assert_eq!(app.tabs[i].scene.selected, selection);
        assert_eq!(app.tabs[i].dirty, dirty);
        assert_eq!(app.modal_offset, iced::Vector::new(10.0, 20.0));
        assert_eq!(app.modal_resize, iced::Vector::new(30.0, 40.0));
        assert!(app.tabs[i].active_cmd.is_none());

        let count = restored.conditions.len();
        let _ = app.on_qselect_pick_object();
        let result = app.tabs[i].active_cmd.as_mut().unwrap().on_escape();
        let _ = app.apply_cmd_result(result);
        assert_eq!(app.qselect.as_ref().unwrap().conditions.len(), count);
        assert_eq!(app.tabs[i].scene.selected, selection);
    }
}
