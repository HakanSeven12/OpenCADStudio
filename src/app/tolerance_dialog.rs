use acadrust::{EntityType, Handle};

impl super::OpenCADStudio {
    pub(super) fn open_tolerance_dialog(&mut self, editing: Option<Handle>) {
        let i = self.active_tab;
        if editing.is_some_and(|handle| self.tabs[i].scene.is_layer_locked(handle)) {
            return;
        }
        let text = editing
            .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
            .and_then(|entity| match entity {
                EntityType::Tolerance(tolerance) => Some(tolerance.text.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        self.geometric_tolerance = Some(
            crate::ui::window::geometric_tolerance::State::from_text(editing, text),
        );
        self.active_modal = Some(super::ModalKind::GeometricTolerance);
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        self.modal_content_size = None;
        self.modal_drag_last = None;
        self.modal_dragging = false;
        self.modal_resizing = false;
    }

    pub(super) fn apply_tolerance_dialog_edit(&mut self) -> bool {
        let Some(state) = self.geometric_tolerance.as_ref() else {
            return false;
        };
        let Some(handle) = state.editing else {
            return false;
        };
        if !state.is_valid() || self.tabs[self.active_tab].scene.is_layer_locked(handle) {
            return false;
        }
        let text = state.to_text();
        let i = self.active_tab;
        let unchanged = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .is_some_and(|entity| {
                matches!(entity, EntityType::Tolerance(tolerance) if tolerance.text == text)
            });
        if unchanged {
            return true;
        }
        self.push_undo_snapshot(i, "TOLERANCE");
        let Some(EntityType::Tolerance(tolerance)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        else {
            return false;
        };
        tolerance.text = text;
        self.tabs[i]
            .scene
            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    pub(super) fn begin_tolerance_placement(&mut self) -> bool {
        let Some(state) = self.geometric_tolerance.take() else {
            return false;
        };
        if !state.is_valid() || state.editing.is_some() {
            self.geometric_tolerance = Some(state);
            return false;
        }
        let i = self.active_tab;
        let mut command =
            crate::modules::annotate::tolerance_cmd::ToleranceCommand::with_text(state.to_text());
        use crate::command::CadCommand;
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        command.set_working_plane(plane);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.active_modal = None;
        true
    }
}
