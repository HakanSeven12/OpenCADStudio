use super::{document::DocumentTab, Message, OpenCADStudio};
use codec::entities::{AttributeEntity, Entity, Insert};
use codec::EntityType;
use iced::Task;

pub(super) struct BlockTestSession {
    pub source_id: u64,
}

impl OpenCADStudio {
    pub(super) fn refuse_block_test_save(&mut self, i: usize) -> bool {
        if self.tabs[i].block_test.is_none() {
            return false;
        }
        self.command_line.push_error(
            crate::t!("Test Block is temporary. Close it and save from the Block Editor.").as_ref(),
        );
        true
    }

    pub(super) fn open_block_test(&mut self, i: usize) -> Task<Message> {
        let source_id = self.tabs[i]
            .block_test
            .as_ref()
            .map_or(self.tabs[i].id, |test| test.source_id);
        let Some(i) = self.tabs.iter().position(|tab| tab.id == source_id) else {
            return Task::none();
        };
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("No block editor is open.").as_ref());
            return Task::none();
        };
        let name = session.block_name.clone();
        let definition = session.br_handle;
        self.finish_pending_history(i);
        let mut document = self.tabs[i].scene.document.clone();
        // Keep definitions and their native relationships in the clone. Only
        // drawing-space contents are replaced by the temporary reference.
        let spaces: Vec<_> = document
            .block_records
            .iter()
            .filter(|record| record.is_model_space() || record.is_paper_space())
            .map(|record| record.handle)
            .collect();
        let removed: Vec<_> = document
            .entities()
            .filter(|entity| {
                spaces.contains(&entity.common().owner_handle)
                    && !matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_))
            })
            .map(|entity| entity.common().handle)
            .collect();
        for handle in removed {
            document.remove_entity(handle);
        }
        for record in document
            .block_records
            .iter_mut()
            .filter(|record| spaces.contains(&record.handle))
        {
            record.entity_handles.retain(|handle| {
                *handle == record.block_entity_handle || *handle == record.block_end_handle
            });
            record.insert_handles.clear();
            record.insert_count_bytes.clear();
        }
        document.header.show_model_space = true;
        crate::scene::block_visibility::finish_editing(&mut document, definition);
        let record = document.block_records.get(&name).unwrap();
        let base = record.base_point;
        let mut insert = Insert::new(&name, codec::types::Vector3::ZERO);
        insert.common.layer = self.tabs[i].active_layer.clone();
        insert.common.handle = document.allocate_handle();
        insert.common.owner_handle = document.header.model_space_block_handle;
        let transform = crate::scene::render_graph::insert_transform_with_base(&insert, base);
        for entity in document.entities_in_block(&name) {
            if let EntityType::AttributeDefinition(definition) = entity {
                if !definition.flags.constant {
                    let mut attribute = AttributeEntity::from_definition(definition, None);
                    attribute.attdef_handle = definition.common.handle;
                    attribute.apply_transform(&transform);
                    insert.attributes.push(attribute);
                }
            }
        }
        for attribute in &mut insert.attributes {
            attribute.common.handle = document.allocate_handle();
            attribute.common.owner_handle = insert.common.handle;
            if let Some(text) = &mut attribute.embedded_mtext {
                text.common.handle = document.allocate_handle();
                text.common.owner_handle = attribute.common.handle;
                text.common.raw_record = None;
                text.common.xdictionary_handle = None;
                text.common.reactors.clear();
            }
        }
        if !insert.attributes.is_empty() {
            insert.seqend_handle = Some(document.allocate_handle());
        }
        if let Err(error) = document.add_entity(EntityType::Insert(insert)) {
            self.command_line.push_error(&error.to_string());
            return Task::none();
        }
        let mut tab = DocumentTab::new_drawing(0);
        tab.tab_title = crate::tf!("Test Block Window - {}", name).into_owned();
        tab.block_test = Some(BlockTestSession { source_id });
        tab.active_layer = self.tabs[i].active_layer.clone();
        tab.scene.document = document;
        tab.scene.named_parameters = self.tabs[i].scene.named_parameters.clone();
        tab.scene.parametric_constraints = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .filter(|set| {
                matches!(
                    set.scope,
                    crate::scene::parametric_constraints::ParametricScope::Block(_)
                )
            })
            .cloned()
            .collect();
        tab.scene.material_base_dir = self.tabs[i].scene.material_base_dir.clone();
        tab.scene.sync_annotation_scale_from_header();
        tab.scene.rebuild_derived_caches();
        tab.scene.set_current_layout("Model".into());
        self.discard_block_tests(source_id);
        self.visibility_popup = None;
        self.grip_popup = None;
        self.grip_hover = None;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.sync_ribbon_layers();
        self.sync_ribbon_styles();
        self.sync_ribbon_from_selection();
        self.refresh_block_palette();
        self.refresh_properties();
        self.ribbon.set_block_editing(false);
        self.ribbon.set_block_testing(true);
        self.dispatch_command("ZOOM EXTENTS")
    }

    pub(super) fn close_block_test(&mut self, i: usize) -> Task<Message> {
        if self.tabs[i].block_test.is_none() {
            self.command_line
                .push_error(crate::t!("No Test Block is open.").as_ref());
            return Task::none();
        }
        self.on_tab_close(i)
    }

    pub(super) fn discard_block_tests(&mut self, source_id: u64) {
        for index in (0..self.tabs.len()).rev() {
            if self.tabs[index]
                .block_test
                .as_ref()
                .is_some_and(|test| test.source_id == source_id)
            {
                let _ = self.on_tab_close(index);
            }
        }
    }
}
