use super::{Message, OpenCADStudio};
use crate::scene::model::object::{PropSection, PropValue, Property};
use codec::{EntityType, Handle};
use iced::Task;

impl OpenCADStudio {
    fn block_property_target(&self, i: usize) -> Option<(String, Handle, Handle)> {
        let tab = &self.tabs[i];
        if !tab.scene.selected_entities().is_empty() {
            return None;
        }
        let session = tab.active_block_edit_session()?;
        let record = tab.scene.document.block_records.get(&session.block_name)?;
        (record.handle == session.br_handle).then(|| {
            (
                record.name.clone(),
                record.handle,
                record.block_entity_handle,
            )
        })
    }

    pub(super) fn open_block_property_hyperlink(&mut self, i: usize) -> bool {
        let Some((name, definition, _)) = self.block_property_target(i) else {
            return false;
        };
        let (url, description) =
            crate::scene::block_metadata::hyperlink(&self.tabs[i].scene.document, &name);
        self.hyperlink_editor_block = Some((self.tabs[i].id, definition));
        self.hyperlink_editor_handles.clear();
        self.hyperlink_editor_url = url;
        self.hyperlink_editor_description = description;
        self.hyperlink_editor_mixed = false;
        self.hyperlink_editor_dirty = false;
        self.hyperlink_options = crate::ui::window::hyperlink_dialog::HyperlinkOptions {
            edit: !self.hyperlink_editor_url.is_empty(),
            dwf: true,
            address_only: true,
            ..Default::default()
        };
        self.active_modal = Some(super::ModalKind::Hyperlink);
        true
    }

    pub(super) fn apply_block_property_hyperlink(
        &mut self,
        url: &str,
        description: &str,
    ) -> Task<Message> {
        let i = self.active_tab;
        let target = self.block_property_target(i).filter(|(_, definition, _)| {
            self.hyperlink_editor_block == Some((self.tabs[i].id, *definition))
        });
        if let Some((name, definition, _)) = target {
            let current =
                crate::scene::block_metadata::hyperlink(&self.tabs[i].scene.document, &name);
            let next = (
                url.trim().to_string(),
                if url.trim().is_empty() {
                    String::new()
                } else {
                    description.trim().to_string()
                },
            );
            if current != next {
                self.push_undo_snapshot(i, super::document::HistoryLabel::Message("Block hyperlink"));
                crate::scene::block_metadata::set_hyperlink(
                    &mut self.tabs[i].scene.document,
                    definition,
                    &next.0,
                    &next.1,
                );
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.finish_pending_history(i);
                self.refresh_properties();
            }
        } else {
            self.command_line
                .push_error(crate::t!("The block being edited has changed.").as_ref());
        }
        self.close_active_modal();
        Task::none()
    }

    pub(super) fn block_property_section(&self, i: usize) -> Option<PropSection> {
        let (name, _, _) = self.block_property_target(i)?;
        let doc = &self.tabs[i].scene.document;
        let record = doc.block_records.get(&name)?;
        let (annotative, orientation) = crate::scene::block_metadata::annotation_flags(doc, &name);
        let yes_no = |value: bool| if value { "Yes" } else { "No" }.to_string();
        let choice = |label: &str, field, value: bool| Property {
            label: crate::t!(label).into_owned(),
            field,
            value: PropValue::Choice {
                selected: yes_no(value),
                options: vec!["Yes".into(), "No".into()],
            },
        };
        let mut orientation_row = choice(
            "Match orientation to layout",
            "bedit_match_orientation",
            orientation,
        );
        if !annotative {
            orientation_row.value = PropValue::ReadOnlyWithTooltip {
                value: crate::t!("No").into_owned(),
                tooltip: crate::t!("Enable Annotative to match orientation to layout.")
                    .into_owned(),
            };
        }
        let (hyperlink, _) = crate::scene::block_metadata::hyperlink(doc, &name);
        let units = super::properties::insunits_name(record.units).to_string();
        let mut unit_options: Vec<_> = (0..=24)
            .map(|code| super::properties::insunits_name(code).to_string())
            .collect();
        if !unit_options.contains(&units) {
            unit_options.push(units.clone());
        }
        Some(PropSection {
            title: crate::t!("Block").into_owned(),
            props: vec![
                Property {
                    label: crate::t!("Block name").into_owned(),
                    field: "bedit_block_name",
                    value: PropValue::ReadOnly(name),
                },
                choice("Annotative", "bedit_annotative", annotative),
                orientation_row,
                choice(
                    "Scale uniformly",
                    "bedit_scale_uniformly",
                    record.scale_uniformly,
                ),
                choice(
                    "Allow exploding",
                    "bedit_allow_exploding",
                    record.explodable,
                ),
                Property {
                    label: crate::t!("Units").into_owned(),
                    field: "bedit_units",
                    value: PropValue::Choice {
                        selected: units,
                        options: unit_options,
                    },
                },
                Property {
                    label: crate::t!("Description").into_owned(),
                    field: "bedit_description",
                    value: PropValue::PlainText(record.description.clone()),
                },
                Property {
                    label: crate::t!("Hyperlink").into_owned(),
                    field: "bedit_hyperlink",
                    value: PropValue::Hyperlink(hyperlink),
                },
            ],
        })
    }

    pub(super) fn set_block_property(&mut self, field: &'static str, value: &str) -> Task<Message> {
        let i = self.active_tab;
        let Some((name, definition, marker)) = self.block_property_target(i) else {
            self.command_line.push_error(
                crate::t!("Block properties require an open Block Editor with no selection.")
                    .as_ref(),
            );
            return Task::none();
        };
        let section = self.block_property_section(i).unwrap();
        let Some(row) = section.props.iter().find(|row| row.field == field) else {
            return Task::none();
        };
        let current = match &row.value {
            PropValue::Choice { selected, options } => {
                if !options.iter().any(|option| option == value) {
                    self.command_line
                        .push_error(crate::t!("Value is not an available option.").as_ref());
                    self.refresh_properties();
                    return Task::none();
                }
                selected.as_str()
            }
            PropValue::PlainText(text) | PropValue::Hyperlink(text) => text.as_str(),
            _ => {
                self.command_line
                    .push_error(crate::t!("Property cannot be edited.").as_ref());
                return Task::none();
            }
        };
        if current == value {
            return Task::none();
        }
        let fresh = !self.block_palette_stale();
        self.push_undo_snapshot(i, super::document::HistoryLabel::Message("Block properties"));
        let doc = &mut self.tabs[i].scene.document;
        match field {
            "bedit_annotative" | "bedit_match_orientation" => {
                let (mut annotation, mut orientation) =
                    crate::scene::block_metadata::annotation_flags(doc, &name);
                if field == "bedit_annotative" {
                    annotation = value == "Yes";
                } else {
                    orientation = value == "Yes";
                }
                crate::scene::block_metadata::set_annotation_flags(
                    doc,
                    definition,
                    annotation,
                    orientation,
                );
            }
            "bedit_scale_uniformly" => {
                doc.block_records.get_mut(&name).unwrap().scale_uniformly = value == "Yes"
            }
            "bedit_allow_exploding" => {
                doc.block_records.get_mut(&name).unwrap().explodable = value == "Yes"
            }
            "bedit_units" => {
                doc.block_records.get_mut(&name).unwrap().units = (0..=24)
                    .find(|code| super::properties::insunits_name(*code) == value)
                    .unwrap()
            }
            "bedit_description" => {
                doc.block_records.get_mut(&name).unwrap().description = value.into();
                if let Some(EntityType::Block(block)) = doc.get_entity_mut(marker) {
                    block.description = value.into();
                }
            }
            "bedit_hyperlink" => {
                let (_, description) = crate::scene::block_metadata::hyperlink(doc, &name);
                crate::scene::block_metadata::set_hyperlink(doc, definition, value, &description);
            }
            _ => return Task::none(),
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.bump_geometry();
        self.finish_pending_history(i);
        self.refresh_properties();
        // Only this block's definition changed.
        self.note_block_changes(fresh, [&name]);
        self.refresh_block_palette();
        Task::none()
    }

    pub(super) fn commit_block_property(&mut self, field: &'static str) -> Task<Message> {
        let panel = &mut self.tabs[self.active_tab].properties;
        panel.active_field = None;
        let Some(value) = panel
            .edit_buf
            .remove(&crate::ui::properties::FieldKey::Geom(field))
        else {
            return Task::none();
        };
        self.set_block_property(field, &value)
    }
}
