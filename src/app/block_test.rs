use super::{document::DocumentTab, Message, OpenCADStudio};
use codec::entities::{AttributeEntity, Entity, Insert};
use codec::EntityType;
use iced::Task;

pub(super) struct BlockTestSession {
    pub source_id: u64,
    pub definition: codec::Handle,
    pub insert: codec::Handle,
    pub baseline: std::sync::Arc<codec::Document>,
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
        let base = crate::scene::render_graph::block_base_point(&document, &name);
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
        let insert_handle = insert.common.handle;
        if let Err(error) = document.add_entity(EntityType::Insert(insert)) {
            self.command_line.push_error(&error.to_string());
            return Task::none();
        }
        let mut tab = DocumentTab::new_drawing(0);
        tab.tab_title = crate::tf!("Test Block Window - {}", name).into_owned();
        tab.block_test = Some(BlockTestSession {
            source_id,
            definition,
            insert: insert_handle,
            baseline: std::sync::Arc::new(document.clone()),
        });
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
        let grip_points = {
            let actions = crate::scene::block_actions::actions(&tab.scene.document, definition);
            let transform = tab
                .scene
                .document
                .get_entity(insert_handle)
                .and_then(|entity| match entity {
                    EntityType::Insert(insert) => Some(
                        crate::scene::render_graph::insert_transform(&tab.scene.document, insert),
                    ),
                    _ => None,
                });
            match transform {
                Some(transform) => {
                    crate::scene::block_actions::all_parameters(&tab.scene.document, definition)
                        .into_iter()
                        .filter(|parameter| {
                            parameter.grip != codec::Handle::NULL && actions
                                .iter()
                                .any(|(_, _, _, parameter_id)| *parameter_id == parameter.id)
                        })
                        .filter_map(|parameter| {
                            let value =
                                crate::scene::block_actions::value(&tab.scene.document, &parameter)
                                    .ok()?;
                            Some(crate::scene::block_actions::v(transform.apply(
                                crate::scene::block_actions::native(
                                    crate::scene::block_actions::grip_position(&parameter, value),
                                ),
                            )))
                        })
                        .collect::<Vec<_>>()
                }
                None => Vec::new(),
            }
        };
        tab.scene.fit_all_with_extra_points(&grip_points);
        self.discard_block_tests(source_id);
        self.visibility_popup = None;
        self.table_popup = None;
        self.flip_popup = None;
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
        Task::none()
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
