use super::{Message, OpenCADStudio};
use crate::scene::block_parameter_properties::{self as native, Input};
use crate::scene::model::object::BlockParameterKey;
use crate::ui::properties::FieldKey;
use iced::Task;

/// Read a value the palette printed, or an expression the user typed.
///
/// LUNITS can print a length as a fraction, as feet-and-inches or in
/// scientific notation, none of which reads as arithmetic, so the display
/// forms are tried after the expression grammar.
pub(crate) fn parse_value(raw: &str) -> Option<f64> {
    let text = raw.trim();
    super::expr_eval::eval_number(text)
        .or_else(|| text.parse::<f64>().ok())
        .or_else(|| crate::entities::common::parse_length(text))
        .filter(|value| value.is_finite())
}

impl OpenCADStudio {
    pub(super) fn block_property_target_is_current(&self, key: BlockParameterKey) -> bool {
        let tab = &self.tabs[self.active_tab];
        tab.id == key.document
            && tab.active_block_edit_session().is_none()
            && tab.active_cmd.is_none()
            && tab.active_grip.is_none()
            && self.property_target_handles(self.active_tab).as_slice() == [key.insert]
            && crate::scene::block_actions::instance::definition(&tab.scene.document, key.insert)
                == Some(key.definition)
    }
    pub(super) fn commit_block_parameter_property(
        &mut self,
        key: BlockParameterKey,
        choice: Option<usize>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if !self.block_property_target_is_current(key)
            || self.tabs[i].scene.is_layer_locked(key.insert)
        {
            return Task::none();
        }
        let field = FieldKey::BlockParameter(key);
        let input = if let Some(index) = choice {
            Input::Choice(index)
        } else {
            let Some(raw) = self.tabs[i].properties.edit_buf.get(&field) else {
                return Task::none();
            };
            let Some(value) = parse_value(raw) else {
                self.command_line
                    .push_error(crate::t!("Specify a finite numeric parameter value.").as_ref());
                return Task::none();
            };
            Input::Number(value)
        };
        let tab = &self.tabs[i];
        let baseline = tab
            .block_test
            .as_ref()
            .filter(|test| test.insert == key.insert && test.definition == key.definition)
            .map(|test| test.baseline.as_ref());
        if tab.block_test.is_some() && baseline.is_none() {
            return Task::none();
        }
        match native::evaluate(&tab.scene.document, baseline, key, input) {
            Ok(staged) => {
                if let Some(staged) = staged {
                    self.push_undo_snapshot(i, "Block reference parameter");
                    self.apply_action_document(i, staged);
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                }
                self.tabs[i].properties.edit_buf.remove(&field);
                self.tabs[i].properties.active_field = None;
                self.refresh_properties();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }
}
