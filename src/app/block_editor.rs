use super::document::{
    DeltaSnapshot, HistorySnapshot, HistoryState, ParametricConstraintsEntryDelta,
    StructureSnapshot,
};
use super::{Message, ModalKind, OpenCADStudio};
use codec::{CadDocument, EntityType};
use iced::Task;
use rustc_hash::{FxHashMap, FxHashSet};

/// A single save checkpoint. Entity payloads remain shared through the
/// document's Arc storage; only modified entities detach during editing.
pub(super) struct BlockEditCheckpoint {
    document: CadDocument,
    constraints: Vec<crate::scene::parametric_constraints::ParametricConstraintSet>,
    parameters: crate::scene::named_parameters::ParameterTable,
    dirty: bool,
    active_layer: String,
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

fn normalize_allocation(document: &mut CadDocument, next: u64) {
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

    fn block_edit_checkpoint(&self, i: usize) -> BlockEditCheckpoint {
        let tab = &self.tabs[i];
        BlockEditCheckpoint {
            document: tab.scene.document.clone(),
            constraints: tab.scene.parametric_constraints.clone(),
            parameters: tab.scene.named_parameters.clone(),
            dirty: tab.dirty,
            active_layer: tab.active_layer.clone(),
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

    fn block_edit_delta(&mut self, i: usize) -> Option<DeltaSnapshot> {
        self.reconcile_block_edit_membership(i);
        self.finish_pending_history(i);
        let current_structure = self.tabs[i].scene.document.snapshot_structure();
        let tab = &self.tabs[i];
        let history = tab.block_edit_history.as_ref()?;
        let checkpoint = &history.checkpoint;
        let session = tab.active_block_edit_session()?;
        let before: FxHashMap<_, _> = checkpoint
            .document
            .entities()
            .map(|e| {
                let h = e.common().handle;
                (h, checkpoint.document.get_entity_arc(h).unwrap())
            })
            .collect();
        let after: FxHashMap<_, _> = tab
            .scene
            .document
            .entities()
            .map(|e| {
                let h = e.common().handle;
                (h, tab.scene.document.get_entity_arc(h).unwrap())
            })
            .collect();
        let mut handles: FxHashSet<_> = before.keys().copied().collect();
        handles.extend(after.keys().copied());
        let mut entities: Vec<_> = handles
            .into_iter()
            .filter_map(|h| {
                let a = before.get(&h);
                let b = after.get(&h);
                (a != b).then(|| (h, a.cloned(), b.cloned()))
            })
            .collect();
        entities.sort_by_key(|(handle, _, _)| handle.value());
        let mut before_structure = checkpoint.document.clone();
        before_structure = before_structure.snapshot_structure();
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
            entities,
            current_layout_before: session.return_layout.clone(),
            current_layout_after: session.return_layout.clone(),
            selected_before: Vec::new(),
            selected_after: Vec::new(),
            dirty_before: checkpoint.dirty,
            dirty_after: true,
            active_layer,
            structure,
            parametric_constraints,
            named_parameters,
            label: format!("Edit block {}", session.block_name),
        })
    }

    pub(super) fn block_edit_dirty(&mut self, i: usize) -> bool {
        self.block_edit_delta(i).is_some()
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
            if !set.retained_standard_groups.is_empty() || set.constraints.iter().any(|c| c.native_origin.is_some()) {
                self.command_line.push_error(crate::t!("Save Block As cannot copy retained native constraints yet.").as_ref());
                return false;
            }
            let crate::scene::parametric_constraints::ParametricScope::Block(old) = set.scope else { unreachable!() };
            set.scope = crate::scene::parametric_constraints::ParametricScope::Block(copied.handles[&old]);
            for constraint in &mut set.constraints {
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
        self.tabs[i].scene.document = copied.document;
        self.tabs[i].scene.parametric_constraints = constraints;
        self.tabs[i].scene.named_parameters = parameters;
        self.tabs[i].active_layer = active_layer;
        let mut delta = self
            .block_edit_delta(i)
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
        self.tabs[i].scene.block_edit_block = Some(copied.record);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].active_grip = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.visibility_popup = None;
        history.checkpoint = self.block_edit_checkpoint(i);
        self.tabs[i].block_edit_history = Some(history);
        self.tabs[i].scene.rebuild_derived_caches();
        self.tabs[i].refresh_active_ucs();
        self.refresh_block_palette();
        self.refresh_properties();
        self.sync_ribbon_layers();
        self.command_line
            .push_output(crate::tf!("Block {} saved.", name).as_ref());
        true
    }

    pub(super) fn save_block_edit(&mut self, i: usize) -> bool {
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            self.command_line
                .push_error(crate::t!("No block editor is open.").as_ref());
            return false;
        };
        let name = session.block_name.clone();
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
        crate::scene::block_visibility::finish_editing(
            &mut self.tabs[i].scene.document,
            definition,
        );
        self.tabs[i].scene.visibility_states.remove(&definition);
        if let Some(delta) = self.block_edit_delta(i) {
            let mut history = self.tabs[i].block_edit_history.take().unwrap();
            std::mem::swap(&mut self.tabs[i].history, &mut history.main);
            self.push_undo_entry(i, HistorySnapshot::Delta(Box::new(delta)));
            std::mem::swap(&mut self.tabs[i].history, &mut history.main);
            self.tabs[i].dirty = true;
            history.checkpoint = self.block_edit_checkpoint(i);
            self.tabs[i].block_edit_history = Some(history);
            self.tabs[i].scene.bump_geometry();
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
                self.tabs[i].scene.document = history.checkpoint.document;
                self.tabs[i].scene.parametric_constraints = history.checkpoint.constraints;
                self.tabs[i].scene.named_parameters = history.checkpoint.parameters;
                self.tabs[i].active_layer = history.checkpoint.active_layer;
                self.tabs[i].dirty = history.checkpoint.dirty;
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
        );
        self.cancel_attr_editor();
    }

    pub(super) fn request_block_edit_close(
        &mut self,
        i: usize,
        continuation: BlockEditContinuation,
    ) -> Task<Message> {
        if self.tabs[i].active_block_edit_session().is_some() {
            if self.block_edit_dirty(i) {
                self.active_tab = i;
                self.pending_block_close = Some((self.tabs[i].id, continuation));
                self.active_modal = Some(ModalKind::BlockEditChanges);
                return Task::none();
            }
            self.close_block_edit(i, true);
        }
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
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.pending_close = Some(super::PendingClose::Quit);
            self.open_unsaved_dialog_window()
        } else {
            self.exit_app()
        }
    }
}
