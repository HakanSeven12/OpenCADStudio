use super::document::{
    DeltaSnapshot, HistorySnapshot, HistoryState, ParametricConstraintsEntryDelta,
    StructureSnapshot,
};
use super::{Message, ModalKind, OpenCADStudio};
use codec::{Document, EntityType};
use iced::Task;
use rustc_hash::{FxHashMap, FxHashSet};

/// A single save checkpoint. Entity payloads remain shared through the
/// document's Arc storage; only modified entities detach during editing.
pub(super) struct BlockEditCheckpoint {
    document: Document,
    structure: Document,
    constraints: Vec<crate::scene::parametric_constraints::ParametricConstraintSet>,
    parameters: crate::scene::named_parameters::ParameterTable,
    dirty: bool,
    active_layer: String,
    hatches: FxHashMap<codec::Handle, crate::scene::HatchModel>,
    images: FxHashMap<codec::Handle, crate::scene::ImageModel>,
    meshes: FxHashMap<codec::Handle, crate::scene::MeshLodSet>,
    block_meshes: FxHashMap<codec::Handle, crate::scene::MeshLodSet>,
    solid_models: FxHashMap<codec::Handle, kernel::brep::Body>,
}

pub(super) struct BlockEditHistory {
    pub main: HistoryState,
    checkpoint: BlockEditCheckpoint,
    return_isolation: crate::scene::ObjectIsolationState,
}

#[derive(Clone, Debug)]
pub(super) enum BlockEditContinuation {
    Close,
    Switch(String),
    Create(String),
    Layout(String),
    Tab(u64),
    Quit,
}

fn same_constraints(
    before: &crate::scene::parametric_constraints::ParametricConstraintSet,
    after: &crate::scene::parametric_constraints::ParametricConstraintSet,
) -> bool {
    before.constraints == after.constraints
        && before.local_parameters == after.local_parameters
        && before.retained_standard_groups == after.retained_standard_groups
        && before.dimensions == after.dimensions
}

fn normalize_allocation(document: &mut Document, next: u64) {
    if document.next_handle() < next {
        document.header.handle_seed = next - 1;
        document.allocate_handle();
    }
    document.header.handle_seed = next;
}

impl OpenCADStudio {
    pub(super) fn reconcile_block_edit_membership(&mut self, i: usize) {
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            return;
        };
        let name = session.block_name.clone();
        let document = &mut self.tabs[i].scene.document;
        let Some(record) = document.block_records.get(&name) else {
            return;
        };
        let members: Vec<_> = record
            .entity_handles
            .iter()
            .copied()
            .filter(|handle| document.get_entity(*handle).is_some())
            .collect();
        if members != record.entity_handles {
            document
                .block_records
                .get_mut(&name)
                .unwrap()
                .entity_handles = members;
        }
    }

    fn block_edit_checkpoint(&mut self, i: usize) -> BlockEditCheckpoint {
        let tab = &mut self.tabs[i];
        let structure = tab.scene.document.snapshot_structure();
        BlockEditCheckpoint {
            document: tab.scene.document.clone(),
            structure,
            constraints: tab.scene.parametric_constraints.clone(),
            parameters: tab.scene.named_parameters.clone(),
            dirty: tab.dirty,
            active_layer: tab.active_layer.clone(),
            hatches: tab.scene.hatches.clone(),
            images: tab.scene.images.clone(),
            meshes: tab.scene.meshes.clone(),
            block_meshes: tab.scene.block_meshes.clone(),
            solid_models: tab.scene.solid_models.clone(),
        }
    }

    pub(super) fn begin_block_edit_history(&mut self, i: usize) {
        self.finish_pending_history(i);
        let checkpoint = self.block_edit_checkpoint(i);
        let main = std::mem::take(&mut self.tabs[i].history);
        let return_isolation = self.tabs[i].scene.object_isolation.clone();
        self.tabs[i].block_edit_history = Some(BlockEditHistory {
            main,
            checkpoint,
            return_isolation,
        });
    }

    fn block_edit_delta(&mut self, i: usize, for_save: bool) -> Option<DeltaSnapshot> {
        self.reconcile_block_edit_membership(i);
        self.finish_pending_history(i);
        let current_structure = self.tabs[i].scene.document.snapshot_structure();
        let tab = &self.tabs[i];
        let history = tab.block_edit_history.as_ref()?;
        let checkpoint = &history.checkpoint;
        let session = tab.active_block_edit_session()?;
        let (before_doc, before_structure_base) = if for_save {
            if let Some(ref pre_doc) = session.unpersisted_pre_document {
                let mut pre_clone = pre_doc.clone();
                let pre_struct = pre_clone.snapshot_structure();
                (pre_doc, pre_struct)
            } else {
                (&checkpoint.document, checkpoint.structure.clone())
            }
        } else {
            (&checkpoint.document, checkpoint.structure.clone())
        };
        let mut candidate_handles: FxHashSet<codec::Handle> = FxHashSet::default();
        if let Some(record) = before_doc.block_records.get(&session.block_name) {
            candidate_handles.extend(record.entity_handles.iter().copied());
            if !record.block_entity_handle.is_null() {
                candidate_handles.insert(record.block_entity_handle);
            }
            if !record.block_end_handle.is_null() {
                candidate_handles.insert(record.block_end_handle);
            }
        }
        if let Some(record) = tab.scene.document.block_records.get(&session.block_name) {
            candidate_handles.extend(record.entity_handles.iter().copied());
            if !record.block_entity_handle.is_null() {
                candidate_handles.insert(record.block_entity_handle);
            }
            if !record.block_end_handle.is_null() {
                candidate_handles.insert(record.block_end_handle);
            }
        }
        // Action-set edits can rebuild anonymous references outside the editor
        // definition. Include their entities in the single drawing save delta.
        for document in [before_doc, &tab.scene.document] {
            for entity in document.entities() {
                if let EntityType::Insert(insert) = entity {
                    if document.dynamic_definition_for_insert(insert.common.handle)
                        == Some(session.br_handle)
                    {
                        candidate_handles.insert(insert.common.handle);
                        if let Some(record) = document.block_records.get(&insert.block_name) {
                            candidate_handles.extend(record.entity_handles.iter().copied());
                            candidate_handles.insert(record.block_entity_handle);
                            candidate_handles.insert(record.block_end_handle);
                        }
                    }
                }
            }
        }
        let mut sub_handles = Vec::new();
        for &h in &candidate_handles {
            for document in [before_doc, &tab.scene.document] {
                if let Some(EntityType::Insert(insert)) = document.get_entity(h) {
                    sub_handles.extend(insert.attributes.iter().map(|a| a.common.handle));
                    sub_handles.extend(insert.seqend_handle);
                }
            }
        }
        candidate_handles.extend(sub_handles);

        let mut entities: Vec<_> = candidate_handles
            .into_iter()
            .filter_map(|h| {
                let a = before_doc.get_entity_arc(h);
                let b = tab.scene.document.get_entity_arc(h);
                match (&a, &b) {
                    (Some(a_arc), Some(b_arc)) if std::sync::Arc::ptr_eq(a_arc, b_arc) => None,
                    (Some(a_arc), Some(b_arc)) if a_arc == b_arc => None,
                    (None, None) => None,
                    _ => Some((h, a, b)),
                }
            })
            .collect();
        entities.sort_by_key(|(handle, _, _)| handle.value());
        let mut before_structure = before_structure_base;
        let mut after_structure = current_structure;
        // The editor temporarily uses Model rendering. Drawing history must
        // retain the layout to which the editor returns.
        let model = session.return_layout == "Model";
        before_structure.header.show_model_space = model;
        after_structure.header.show_model_space = model;
        if !model {
            crate::io::set_saved_active_layout(&mut before_structure, &session.return_layout);
            crate::io::set_saved_active_layout(&mut after_structure, &session.return_layout);
        }
        let next = before_structure
            .next_handle()
            .max(after_structure.next_handle())
            .max(before_structure.header.handle_seed)
            .max(after_structure.header.handle_seed);
        // Reserving handles is not a content edit. Undoing every edit should
        // return a session to clean without reusing allocated handles.
        normalize_allocation(&mut before_structure, next);
        normalize_allocation(&mut after_structure, next);
        let structure = (before_structure != after_structure)
            .then_some(StructureSnapshot::Full(before_structure));
        let mut scopes: Vec<_> = checkpoint.constraints.iter().map(|c| c.scope).collect();
        for set in &tab.scene.parametric_constraints {
            if !scopes.contains(&set.scope) {
                scopes.push(set.scope);
            }
        }
        let parametric_constraints: Vec<_> = scopes
            .into_iter()
            .filter_map(|scope| {
                let empty =
                    || crate::scene::parametric_constraints::ParametricConstraintSet::new(scope);
                let a = checkpoint
                    .constraints
                    .iter()
                    .find(|c| c.scope == scope)
                    .cloned()
                    .unwrap_or_else(empty);
                let b = tab
                    .scene
                    .parametric_constraints
                    .iter()
                    .find(|c| c.scope == scope)
                    .cloned()
                    .unwrap_or_else(empty);
                (!same_constraints(&a, &b)).then_some(ParametricConstraintsEntryDelta {
                    scope,
                    before: a,
                    after: b,
                })
            })
            .collect();
        let named_parameters = (checkpoint.parameters != tab.scene.named_parameters).then(|| {
            (
                checkpoint.parameters.clone(),
                tab.scene.named_parameters.clone(),
            )
        });
        let active_layer = (checkpoint.active_layer != tab.active_layer)
            .then(|| (checkpoint.active_layer.clone(), tab.active_layer.clone()));
        if entities.is_empty()
            && structure.is_none()
            && parametric_constraints.is_empty()
            && named_parameters.is_none()
            && active_layer.is_none()
        {
            return None;
        }
        Some(DeltaSnapshot {
            visibility_states: None,
            entities,
            current_layout_before: session.return_layout.clone(),
            current_layout_after: session.return_layout.clone(),
            selected_before: Vec::new(),
            selected_after: Vec::new(),
            dirty_before: checkpoint.dirty,
            dirty_after: true,
            active_layer,
            layiso_state: None,
            structure,
            parametric_constraints,
            reference_designations: Vec::new(),
            named_parameters,
            label: format!("Edit block {}", session.block_name),
        })
    }

    pub(super) fn block_edit_dirty(&mut self, i: usize) -> bool {
        let tab = &self.tabs[i];
        let Some(history) = tab.block_edit_history.as_ref() else {
            return false;
        };
        let Some(session) = tab.active_block_edit_session() else {
            return false;
        };
        if tab.history.undo_stack.is_empty()
            && tab.history.pending.is_none()
            && session.unpersisted_pre_document.is_none()
            && history.checkpoint.active_layer == tab.active_layer
            && history.checkpoint.dirty == tab.dirty
            && history.checkpoint.parameters == tab.scene.named_parameters
        {
            return false;
        }
        self.block_edit_delta(i, false).is_some()
    }

    pub(super) fn validate_block_save_as_name(
        &self,
        i: usize,
        raw: &str,
    ) -> Result<String, String> {
        let name =
            crate::scene::block_service::validate_block_name(raw).map_err(|e| e.to_string())?;
        if self.tabs[i]
            .scene
            .document
            .block_records
            .get(&name)
            .is_some()
        {
            return Err(
                crate::scene::block_service::BlockServiceError::NameExists(name).to_string(),
            );
        }
        Ok(name)
    }

    pub(super) fn open_bedit_dialog(&mut self, i: usize) {
        if self.block_editor_preferences.locked {
            self.command_line.push_error(
                crate::t!("The Block Editor is locked by BLOCKEDITLOCK.").as_ref(),
            );
            return;
        }
        if self.tabs[i].refedit_session.is_some() {
            self.command_line.push_error(
                crate::t!("BEDIT: finish the active REFEDIT (REFCLOSE) first.").as_ref(),
            );
            return;
        }

        let existing_names =
            crate::scene::block_service::bedit_candidate_block_names(&self.tabs[i].scene.document);

        let initial_name = if let Some(session) = self.tabs[i].active_block_edit_session() {
            session.block_name.clone()
        } else if let Some(first) = existing_names.first() {
            first.clone()
        } else {
            String::new()
        };

        let preview = if !initial_name.is_empty() {
            self.tabs[i].scene.block_preview_wires(&initial_name)
        } else {
            Vec::new()
        };

        let description = if !initial_name.is_empty() {
            self.tabs[i]
                .scene
                .document
                .block_records
                .get(&initial_name)
                .map(|b| b.description.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let error = if initial_name.is_empty() {
            None
        } else {
            crate::scene::block_service::validate_block_name(&initial_name)
                .err()
                .map(|e| e.to_string())
        };

        self.block_edit_picker = Some(crate::ui::window::block_edit_picker::State {
            document_id: self.tabs[i].id,
            name: initial_name,
            existing_names,
            preview,
            description,
            error,
        });
        self.active_modal = Some(ModalKind::BlockEditPicker);
        self.reset_modal_geometry();
    }

    pub(super) fn apply_bedit_dialog(&mut self) -> Task<Message> {
        let Some(state) = self.block_edit_picker.as_ref() else {
            return Task::none();
        };
        let id = state.document_id;
        let name = state.name.trim().to_string();
        if name.is_empty() || state.error.is_some() {
            return Task::none();
        }
        let Some(i) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Task::none();
        };
        self.active_tab = i;
        self.block_edit_picker = None;
        self.active_modal = None;
        self.reset_modal_geometry();

        if let Some(record) = self.tabs[i].scene.document.block_records.get(&name) {
            let real_name = record.name.clone();
            self.dispatch_command(&format!("BEDIT_OPEN:{real_name}"))
        } else {
            self.dispatch_command(&format!("BEDIT_NEW:{name}"))
        }
    }

    pub(super) fn open_block_save_as_dialog(&mut self, i: usize) {
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("No block editor is open.").as_ref());
            return;
        };
        let source = session.br_handle;
        let name = session.block_name.clone();
        let mut existing_names = self.tabs[i].scene.custom_block_names();
        existing_names.sort_by_key(|name| name.to_ascii_lowercase());
        self.block_save_as = Some(crate::ui::window::block_save_as::State {
            document_id: self.tabs[i].id,
            source,
            name: String::new(),
            existing_names,
            preview: self.tabs[i].scene.block_preview_wires(&name),
            error: self.validate_block_save_as_name(i, "").err(),
        });
        self.active_modal = Some(ModalKind::BlockSaveAs);
        self.reset_modal_geometry();
    }

    pub(super) fn apply_block_save_as_dialog(&mut self) -> Task<Message> {
        let Some(state) = self.block_save_as.as_ref() else {
            return Task::none();
        };
        let id = state.document_id;
        let source = state.source;
        let name = state.name.clone();
        let Some(i) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Task::none();
        };
        if self.tabs[i]
            .active_block_edit_session()
            .is_none_or(|session| session.br_handle != source)
        {
            if let Some(state) = self.block_save_as.as_mut() {
                state.error = Some(
                    crate::t!("The block editor has changed; reopen Save Block As.").into_owned(),
                );
            }
            return Task::none();
        }
        self.active_tab = i;
        if self.save_block_edit_as(i, &name) {
            self.block_save_as = None;
            self.active_modal = None;
            self.reset_modal_geometry();
        } else if let Some(state) = self.block_save_as.as_mut() {
            state.error = self.command_line.last_error.clone();
        }
        Task::none()
    }

    pub(super) fn save_block_edit_as(&mut self, i: usize, raw: &str) -> bool {
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("No block editor is open.").as_ref());
            return false;
        };
        let source_name = session.block_name.clone();
        let source_handle = session.br_handle;
        let fresh = !self.block_palette_stale();
        let name = match self.validate_block_save_as_name(i, raw) {
            Ok(name) => name,
            Err(error) => {
                self.command_line.push_error(&error);
                return false;
            }
        };
        self.finish_pending_history(i);
        self.reconcile_block_edit_membership(i);
        let Some(history) = self.tabs[i].block_edit_history.as_ref() else {
            return false;
        };
        let copied = match super::block_save_as::copy_definition(
            &self.tabs[i].scene.document,
            history.checkpoint.document.clone(),
            &source_name,
            &name,
        ) {
            Ok(copied) => copied,
            Err(error) => {
                self.command_line.push_error(&error);
                return false;
            }
        };
        let mut constraints = history.checkpoint.constraints.clone();
        for mut set in self.tabs[i].scene.parametric_constraints.iter().filter(|set| {
            matches!(set.scope, crate::scene::parametric_constraints::ParametricScope::Block(handle) if copied.handles.contains_key(&handle))
        }).cloned() {
            for group in &mut set.retained_standard_groups {
                let Some(new) = copied.handles.get(group) else {
                    self.command_line.push_error(crate::t!("A retained block constraint group is missing.").as_ref());
                    return false;
                };
                *group = *new;
            }
            let crate::scene::parametric_constraints::ParametricScope::Block(old) = set.scope else { unreachable!() };
            set.scope = crate::scene::parametric_constraints::ParametricScope::Block(copied.handles[&old]);
            for constraint in &mut set.constraints {
                if let Some(origin) = &mut constraint.native_origin {
                    let Some(new) = copied.handles.get(&origin.group) else {
                        self.command_line.push_error(crate::t!("A retained block constraint group is missing.").as_ref());
                        return false;
                    };
                    origin.group = *new;
                }
                for reference in constraint.refs.iter_mut().chain(constraint.rigid_points.iter_mut().map(|(r, _)| r)) {
                    let Some(new) = copied.handles.get(&reference.entity) else {
                        self.command_line.push_error(crate::t!("A block constraint refers to geometry outside the definition.").as_ref());
                        return false;
                    };
                    reference.entity = *new;
                }
            }
            for handle in set.dimensions.values_mut() {
                let Some(new) = copied.handles.get(handle) else {
                    self.command_line.push_error(crate::t!("A block constraint dimension is missing.").as_ref());
                    return false;
                };
                *handle = *new;
            }
            set.dof = None;
            set.conflicts.clear();
            constraints.push(set);
        }
        for entity in copied.document.entities_in_block(&name) {
            if let EntityType::Insert(insert) = entity {
                if crate::scene::block_service::would_create_cycle(
                    &copied.document,
                    &name,
                    &insert.block_name,
                ) {
                    self.command_line.push_error(
                        &crate::scene::block_service::circular_dependency_error(
                            &name,
                            &insert.block_name,
                        ),
                    );
                    return false;
                }
            }
        }
        let parameters = self.tabs[i].scene.named_parameters.clone();
        let active_layer = self.tabs[i].active_layer.clone();
        let before = std::mem::replace(&mut self.tabs[i].scene.document, copied.document);
        let touched = crate::app::update::blocks_palette::changed_block_names(
            &before,
            &self.tabs[i].scene.document,
        );
        drop(before);
        self.tabs[i].scene.parametric_constraints = constraints;
        self.tabs[i].scene.named_parameters = parameters;
        self.tabs[i].active_layer = active_layer;
        let mut delta = self
            .block_edit_delta(i, true)
            .expect("a new definition changes the document");
        delta.label = format!("Save block as {name}");
        let mut history = self.tabs[i].block_edit_history.take().unwrap();
        std::mem::swap(&mut self.tabs[i].history, &mut history.main);
        self.push_undo_entry(i, HistorySnapshot::Delta(Box::new(delta)));
        std::mem::swap(&mut self.tabs[i].history, &mut history.main);
        self.tabs[i].history = HistoryState::default();
        self.tabs[i].dirty = true;
        let session = self.tabs[i].active_block_edit_session_mut().unwrap();
        debug_assert_eq!(session.br_handle, source_handle);
        session.br_handle = copied.record;
        session.block_name = name.clone();
        session.unpersisted_pre_document = None;
        self.tabs[i].scene.block_edit_block = Some(copied.record);
        self.reset_block_constraint_display(i);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;
        self.table_popup = None;
        self.flip_popup = None;
        history.checkpoint = self.block_edit_checkpoint(i);
        self.tabs[i].block_edit_history = Some(history);
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].refresh_active_ucs();
        self.note_block_changes(fresh, &touched);
        self.refresh_block_palette();
        self.refresh_properties();
        self.sync_ribbon_layers();
        self.command_line
            .push_output(crate::tf!("Block {} saved.", name).as_ref());
        true
    }

    /// Save-time representation edits belong to the preceding editor command,
    /// so local Undo cannot leave untracked members or reuse their handles.
    fn coalesce_saved_representations(&mut self, i: usize, before: &Document) {
        let tab = &mut self.tabs[i];
        let doc = &tab.scene.document;
        if before == doc {
            return;
        }
        let Some(HistorySnapshot::Delta(delta)) = tab.history.undo_stack.last_mut() else {
            return;
        };
        let handles = before
            .entities()
            .chain(doc.entities())
            .map(|e| e.common().handle)
            .collect::<FxHashSet<_>>();
        for handle in handles {
            let a = before.get_entity_arc(handle);
            let b = doc.get_entity_arc(handle);
            if a == b {
                continue;
            }
            if let Some((_, _, after)) = delta.entities.iter_mut().find(|(h, _, _)| *h == handle) {
                *after = b;
            } else {
                delta.entities.push((handle, a, b));
            }
        }
        if !delta
            .structure
            .as_ref()
            .is_some_and(StructureSnapshot::is_full)
        {
            let mut previous = before.clone();
            let broad = StructureSnapshot::Full(previous.snapshot_structure());
            delta.structure = Some(match delta.structure.take() {
                Some(specialized) => StructureSnapshot::Combined(vec![broad, specialized]),
                None => broad,
            });
        }
        delta.dirty_after = true;
    }

    pub(super) fn save_block_edit(&mut self, i: usize) -> bool {
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("No block editor is open.").as_ref());
            return false;
        };
        let name = session.block_name.clone();
        // The palette keeps the other blocks' previews when it was up to
        // date before the save.
        let fresh = !self.block_palette_stale();
        let mut touched = vec![name.clone()];
        if let Err(error) = crate::scene::block_service::validate_block_name(&name) {
            // Existing anonymous definitions are editable; name validation
            // applies to named definitions rather than generated variants.
            if !name.starts_with('*') {
                self.command_line.push_error(&error.to_string());
                return false;
            }
        }
        let nested: Vec<_> = self.tabs[i]
            .scene
            .document
            .entities_in_block(&name)
            .filter_map(|entity| match entity {
                EntityType::Insert(ins) => Some(ins.block_name.clone()),
                _ => None,
            })
            .collect();
        for child in nested {
            if crate::scene::block_service::would_create_cycle(
                &self.tabs[i].scene.document,
                &name,
                &child,
            ) {
                self.command_line.push_error(
                    &crate::scene::block_service::circular_dependency_error(&name, &child),
                );
                return false;
            }
        }
        // A dynamic block leaves the editor showing its first visibility
        // state, with erased objects dropped from its states.
        let definition = session.br_handle;
        if self.block_edit_delta(i, true).is_some() {
            self.reconcile_block_edit_membership(i);
            self.finish_pending_history(i);
            let before_sync = self.tabs[i].scene.document.clone();
            let mut staged = before_sync.clone();
            crate::scene::block_visibility::finish_editing(&mut staged, definition);
            crate::scene::block_actions::prune_erased_targets(&mut staged, definition);
            let before = &self.tabs[i]
                .block_edit_history
                .as_ref()
                .unwrap()
                .checkpoint
                .document;
            match crate::scene::block_actions::instance::refresh_after_edit(
                &staged, before, definition,
            ) {
                Ok(document) => {
                    self.apply_action_document(i, document);
                    self.coalesce_saved_representations(i, &before_sync);
                    // A dynamic block's references are drawn again too.
                    touched.extend(crate::app::update::blocks_palette::changed_block_names(
                        &before_sync,
                        &self.tabs[i].scene.document,
                    ));
                }
                Err(error) => {
                    self.command_line.push_error(crate::t!(error).as_ref());
                    return false;
                }
            }
            self.tabs[i].scene.visibility_states.remove(&definition);
        }
        if let Some(delta) = self.block_edit_delta(i, true) {
            let mut history = self.tabs[i].block_edit_history.take().unwrap();
            std::mem::swap(&mut self.tabs[i].history, &mut history.main);
            self.push_undo_entry(i, HistorySnapshot::Delta(Box::new(delta)));
            std::mem::swap(&mut self.tabs[i].history, &mut history.main);
            self.tabs[i].dirty = true;
            history.checkpoint = self.block_edit_checkpoint(i);
            self.tabs[i].block_edit_history = Some(history);
            self.tabs[i].scene.bump_geometry();
            self.note_block_changes(fresh, &touched);
        }
        if let Some(session) = self.tabs[i].active_block_edit_session_mut() {
            session.unpersisted_pre_document = None;
        }
        self.command_line
            .push_output(crate::tf!("Block {} saved.", name).as_ref());
        true
    }

    pub(super) fn close_block_edit(&mut self, i: usize, discard: bool) {
        self.finish_pending_history(i);
        let Some(session) = self.tabs[i].block_edits.pop() else {
            return;
        };
        if let Some(history) = self.tabs[i].block_edit_history.take() {
            if discard {
                if let Some(pre_doc) = session.unpersisted_pre_document {
                    self.tabs[i].scene.document = pre_doc;
                } else {
                    self.tabs[i].scene.document = history.checkpoint.document;
                }
                self.tabs[i].scene.parametric_constraints = history.checkpoint.constraints;
                self.tabs[i].scene.named_parameters = history.checkpoint.parameters;
                self.tabs[i].active_layer = history.checkpoint.active_layer;
                self.tabs[i].dirty = history.checkpoint.dirty;
                self.tabs[i].scene.hatches = history.checkpoint.hatches;
                self.tabs[i].scene.images = history.checkpoint.images;
                self.tabs[i].scene.meshes = history.checkpoint.meshes;
                self.tabs[i].scene.block_meshes = history.checkpoint.block_meshes;
                self.tabs[i].scene.solid_models = history.checkpoint.solid_models;
            }
            self.tabs[i].history = history.main;
            self.tabs[i].scene.object_isolation = history.return_isolation;
        }
        self.tabs[i].active_block_edit = None;
        self.restore_after_block_edit_close(
            i,
            None,
            &session.return_layout,
            &session.return_camera,
            !discard,
        );
        self.cancel_attr_editor();
        self.discard_block_tests(self.tabs[i].id);
    }

    pub(super) fn request_block_edit_close(
        &mut self,
        i: usize,
        continuation: BlockEditContinuation,
    ) -> Task<Message> {
        let id = self.tabs[i].id;
        if self.tabs[i].active_block_edit_session().is_some() {
            if self.block_edit_dirty(i) {
                self.active_tab = i;
                self.pending_block_close = Some((self.tabs[i].id, continuation));
                self.active_modal = Some(ModalKind::BlockEditChanges);
                return Task::none();
            }
            self.close_block_edit(i, true);
        }
        let i = self.tabs.iter().position(|tab| tab.id == id).unwrap();
        self.continue_after_block_edit(continuation, i)
    }

    fn continue_after_block_edit(
        &mut self,
        continuation: BlockEditContinuation,
        i: usize,
    ) -> Task<Message> {
        match continuation {
            BlockEditContinuation::Close => Task::none(),
            BlockEditContinuation::Switch(name) => {
                self.active_tab = i;
                self.dispatch_command(&format!("BEDIT_OPEN:{name}"))
            }
            BlockEditContinuation::Create(name) => {
                self.active_tab = i;
                self.dispatch_command(&format!("BEDIT_NEW:{name}"))
            }
            BlockEditContinuation::Layout(name) => {
                self.active_tab = i;
                self.on_layout_switch(name)
            }
            BlockEditContinuation::Tab(id) => self
                .tabs
                .iter()
                .position(|tab| tab.id == id)
                .map(|index| {
                    let close = self.on_tab_close(index);
                    if self.pending_close.is_none() && self.pending_block_close.is_none() {
                        Task::batch([close, self.continue_tab_close_queue()])
                    } else {
                        close
                    }
                })
                .unwrap_or_else(Task::none),
            BlockEditContinuation::Quit => self.request_quit_with_block_edits(),
        }
    }

    pub(super) fn resolve_block_edit_close(&mut self, save: bool) -> Task<Message> {
        let Some((id, continuation)) = self.pending_block_close.clone() else {
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == id) else {
            self.cancel_block_edit_close();
            return Task::none();
        };
        if save && !self.save_block_edit(i) {
            return Task::none();
        }
        self.pending_block_close = None;
        self.active_modal = None;
        self.close_block_edit(i, !save);
        let i = self.tabs.iter().position(|tab| tab.id == id).unwrap();
        self.continue_after_block_edit(continuation, i)
    }

    pub(super) fn cancel_block_edit_close(&mut self) {
        self.pending_block_close = None;
        self.pending_tab_closes.clear();
        if self.active_modal == Some(ModalKind::BlockEditChanges) {
            self.active_modal = None;
        }
    }

    pub(super) fn request_quit_with_block_edits(&mut self) -> Task<Message> {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|tab| tab.active_block_edit_session().is_some())
        {
            return self.request_block_edit_close(i, BlockEditContinuation::Quit);
        }
        if self
            .tabs
            .iter()
            .any(|tab| tab.dirty && tab.block_test.is_none())
        {
            self.pending_close = Some(super::PendingClose::Quit);
            self.open_unsaved_dialog_window()
        } else {
            self.exit_app()
        }
    }
}
