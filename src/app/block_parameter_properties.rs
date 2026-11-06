use super::{Message, OpenCADStudio};
use crate::scene::block_parameter_properties::{self as native, Input};
use crate::scene::model::object::{BlockParameterComponent, BlockParameterKey, PropSection, PropValue};
use crate::ui::properties::FieldKey;
use document::Handle;
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
    pub(super) fn nested_block_property_session(&self, i: usize, insert: Handle) -> Option<u64> {
        let tab = &self.tabs[i];
        let session = tab.active_block_edit_session()?;
        let entity = tab.scene.document.get_entity(insert)?;
        let document::EntityType::Insert(reference) = entity else { return None };
        let definition = tab.scene.document.dynamic_definition_for_insert(insert)?;
        if reference.common.owner_handle != session.br_handle || definition == session.br_handle {
            return None;
        }
        Some(tab.block_edit_history.as_ref()?.id)
    }

    fn batch_block_scope(&self, i: usize, handles: &[Handle]) -> Option<(Handle, Option<u64>)> {
        let tab = &self.tabs[i];
        if handles.len() < 2 || tab.block_test.is_some() {
            return None;
        }
        let editor_session = if tab.active_block_edit_session().is_some() {
            Some(self.nested_block_property_session(i, handles[0])?)
        } else {
            None
        };
        let definition =
            |handle| crate::scene::block_actions::instance::definition(&tab.scene.document, handle);
        let first = definition(handles[0])?;
        handles
            .iter()
            .all(|&handle| {
                !tab.scene.is_layer_locked(handle)
                    && definition(handle) == Some(first)
                    && (editor_session.is_none()
                        || self.nested_block_property_session(i, handle) == editor_session)
            })
            .then_some((first, editor_session))
    }

    pub(super) fn batch_block_property_section(
        &self,
        i: usize,
        handles: &[Handle],
        generation: u64,
    ) -> Option<PropSection> {
        let (_, editor_session) = self.batch_block_scope(i, handles)?;
        let tab = &self.tabs[i];
        let rows = |insert| {
            let doc = &tab.scene.document;
            let mut rows = native::properties(doc, None, tab.id, insert);
            rows.extend(Self::indexed_visibility_property(doc, tab.id, insert));
            rows
        };
        let key = |value: &PropValue| match value {
            PropValue::BlockNumber { key, .. } | PropValue::BlockChoice { key, .. } => Some(*key),
            _ => None,
        };
        let identity = |key: BlockParameterKey| (key.parameter, key.component);
        let mut props = rows(handles[0]);
        for &handle in &handles[1..] {
            let other: std::collections::HashMap<_, _> = rows(handle)
                .into_iter()
                .filter_map(|row| Some((identity(key(&row.value)?), row.value)))
                .collect();
            props.retain_mut(|row| {
                let Some(other) = key(&row.value).and_then(|key| other.get(&identity(key))) else {
                    return false;
                };
                match (&mut row.value, other) {
                    (
                        PropValue::BlockNumber { value, .. },
                        PropValue::BlockNumber { value: next, .. },
                    ) => {
                        if value != next {
                            *value = crate::tr!("common", "varies");
                        }
                        true
                    }
                    (
                        PropValue::BlockChoice {
                            selected, options, ..
                        },
                        PropValue::BlockChoice {
                            selected: next,
                            options: next_options,
                            ..
                        },
                    ) if options == next_options => {
                        if selected != next {
                            *selected = crate::ui::properties::MIXED_BLOCK_CHOICE;
                        }
                        true
                    }
                    _ => false,
                }
            });
            if props.is_empty() {
                return None;
            }
        }
        for row in &mut props {
            match &mut row.value {
                PropValue::BlockNumber { key, .. } | PropValue::BlockChoice { key, .. } => {
                    key.insert = Handle::NULL;
                    key.batch_generation = Some(generation);
                    key.editor_session = editor_session;
                }
                _ => unreachable!(),
            }
        }
        Some(PropSection {
            title: crate::t!("Custom").into_owned(),
            props,
        })
    }

    pub(super) fn block_property_target_is_current(&self, key: BlockParameterKey) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.id != key.document || tab.active_cmd.is_some() || tab.active_grip.is_some() {
            return false;
        }
        if let Some(generation) = key.batch_generation {
            let handles = &tab.properties.source_handles;
            return self.property_target_override.is_none()
                && tab.properties.block_batch_generation == generation
                && tab.scene.selected_entities().iter().map(|(handle, _)| handle).eq(handles)
                && self.batch_block_scope(i, handles) == Some((key.definition, key.editor_session))
                && tab.properties.sections.iter().flat_map(|section| &section.props).any(|row| {
                    matches!(&row.value,
                        PropValue::BlockNumber { key: shown, .. } | PropValue::BlockChoice { key: shown, .. }
                        if *shown == key)
                });
        }
        if tab.active_block_edit_session().is_some() {
            if key.editor_session.is_none()
                || key.editor_session != self.nested_block_property_session(i, key.insert)
                || self.property_target_override.is_some()
                || tab.scene.selected_entities().iter().map(|(handle, _)| *handle).ne([key.insert])
            {
                return false;
            }
        } else if key.editor_session.is_some() {
            return false;
        }
        let definition = if matches!(key.component, BlockParameterComponent::Visibility(_)) {
            tab.scene.document.dynamic_visibility_for_insert(key.insert).map(|(definition, _)| definition)
        } else {
            crate::scene::block_actions::instance::definition(&tab.scene.document, key.insert)
        };
        self.property_target_handles(i).as_slice() == [key.insert]
            && definition == Some(key.definition)
    }
    pub(super) fn commit_block_parameter_property(
        &mut self,
        key: BlockParameterKey,
        choice: Option<usize>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if !self.block_property_target_is_current(key) {
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
        let result = if key.batch_generation.is_some() {
            let mut staged = None;
            tab.properties
                .source_handles
                .iter()
                .try_for_each(|&insert| {
                    let source = staged.as_ref().unwrap_or(&tab.scene.document);
                    let target = BlockParameterKey {
                        insert,
                        batch_generation: None,
                        ..key
                    };
                    let next = match (target.component, input) {
                        (BlockParameterComponent::Visibility(_), Input::Choice(state)) => {
                            Self::stage_visibility_property(source, target, state)
                        }
                        _ => native::evaluate(source, None, target, input),
                    }?;
                    if let Some(next) = next {
                        staged = Some(next);
                    }
                    Ok(())
                })
                .map(|()| staged)
        } else {
            match (key.component, input) {
                (BlockParameterComponent::Visibility(_), Input::Choice(state)) => {
                    Self::stage_visibility_property(&tab.scene.document, key, state)
                }
                _ => native::evaluate(&tab.scene.document, baseline, key, input),
            }
        };
        match result {
            Ok(staged) => {
                if let Some(staged) = staged {
                    self.push_undo_snapshot(
                        i,
                        super::tab_document::HistoryLabel::Message("Block reference parameter"),
                    );
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
