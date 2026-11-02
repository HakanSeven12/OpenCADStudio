//! Native descriptions share the existing value-tip path and hover dwell.
use super::*;
use crate::scene::block_authoring::{edit, metadata};
use crate::scene::block_grips::runtime;
use codec::{
    objects::{DynamicBlockData, ObjectType},
    Document,
};

fn description(
    doc: &Document,
    definition: Handle,
    parameter: Handle,
    component: Option<usize>,
) -> Option<String> {
    use metadata::Field::*;
    let metadata = metadata::read(doc, definition, parameter).ok()?;
    let mut descriptions = Vec::new();
    for (index, name, description) in [
        (0, Name, Description),
        (1, SecondaryName, SecondaryDescription),
    ] {
        if component.is_some_and(|component| component != index) {
            continue;
        }
        let Some(value) = metadata
            .text
            .iter()
            .find(|text| text.field == description)
            .map(|text| text.value)
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let name = metadata
            .text
            .iter()
            .find(|text| text.field == name)
            .map_or("", |text| text.value);
        if !descriptions.iter().any(|(_, previous)| *previous == value) {
            descriptions.push((name, value));
        }
    }
    if descriptions.is_empty() {
        return None;
    }
    Some(if descriptions.len() == 1 {
        descriptions[0].1.to_owned()
    } else {
        descriptions
            .into_iter()
            .map(|(name, text)| {
                if name.is_empty() {
                    text.to_owned()
                } else {
                    format!("{name}: {text}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

impl OpenCADStudio {
    pub(in crate::app) fn dynamic_grip_tip(&self, handle: Handle, id: usize) -> Option<String> {
        if !self.block_editor_preferences.grip_tips {
            return None;
        }
        let tab = &self.tabs[self.active_tab];
        if let Some(index) = super::super::block_authoring_edit::editor_grip_index(id) {
            let definition = tab.active_block_edit_session()?.br_handle;
            if tab.selected_authoring != Some(handle) {
                return None;
            }
            let item = edit::items(&tab.scene.document, definition)
                .into_iter()
                .find(|item| item.handle == handle)?;
            let (part, _) = item.grips.get(index)?;
            let component = match part {
                edit::Part::Label(index) => Some(*index),
                _ => None,
            };
            let help = description(&tab.scene.document, definition, handle, component)?;
            return Some(format!("{}\n{help}", item.name));
        }
        let (source, definition) =
            if let Some(test) = tab.block_test.as_ref().filter(|test| test.insert == handle) {
                (test.baseline.as_ref(), test.definition)
            } else {
                (
                    &tab.scene.document,
                    tab.scene.document.dynamic_definition_for_insert(handle)?,
                )
            };
        let mut component = None;
        let parameter = if id == super::super::visibility::VIS_GRIP_ID {
            tab.visibility_grip
                .as_ref()
                .filter(|grip| grip.insert_handle == handle)?;
            let parameter = crate::scene::block_visibility::parameter(source, definition)?;
            if parameter.property_info_count <= 0
                && crate::scene::block_grips::config::count(source, definition, parameter.handle)
                    == Some(0)
            {
                return None;
            }
            Some(parameter.handle)
        } else if id == super::super::visibility::TABLE_GRIP_ID {
            tab.table_grip
                .as_ref()
                .filter(|grip| grip.insert_handle == handle)?;
            crate::scene::block_properties_table::table(source, definition).ok()??;
            None // No authored description field exists on the current table model.
        } else if let Some(alignment) = tab
            .scene
            .alignment_grips(handle)
            .into_iter()
            .find(|grip| grip.id == id)
        {
            Some(alignment.parameter)
        } else {
            let (node, role) = runtime::decode(id)?;
            let p = actions::all_parameters(source, definition)
                .into_iter()
                .find(|p| p.id == node)?;
            if matches!(source.objects.get(&p.handle), Some(ObjectType::DynamicBlock(object)) if matches!(&object.data, DynamicBlockData::XYParameter(_)))
            {
                component = match role {
                    2 => Some(0),
                    3 => Some(1),
                    _ => None,
                };
            }
            Some(p.handle)
        };
        let summary = self.dynamic_grip_summary(handle, id)?;
        let help =
            parameter.and_then(|parameter| description(source, definition, parameter, component));
        Some(help.map_or_else(|| summary.clone(), |help| format!("{summary}\n{help}")))
    }

    pub(in crate::app) fn hovered_dynamic_grip_tip(&self) -> Option<(Point, String)> {
        let tab = &self.tabs[self.active_tab];
        let hover = self.grip_hover.as_ref()?;
        if hover.document != tab.id
            || hover.started.elapsed().as_millis() < 500
            || tab.active_cmd.is_some()
            || tab.active_grip.is_some()
            || self.grip_popup.is_some()
            || self.visibility_popup.is_some()
            || self.flip_popup.is_some()
            || !tab
                .selected_grips
                .iter()
                .zip(&tab.selected_grip_handles)
                .any(|(grip, handle)| *handle == hover.handle && grip.id == hover.grip_id)
        {
            return None;
        }
        Some((
            hover.screen,
            self.dynamic_grip_tip(hover.handle, hover.grip_id)?,
        ))
    }
}
