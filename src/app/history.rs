use super::{
    document::{
        DeltaSnapshot, HistorySnapshot, ObjectEntryDelta, ObjectVisibilitySnapshot,
        PendingHistorySnapshot, StructureSnapshot, TableEntryDelta,
    },
    OpenCADStudio,
};
use crate::scene::ObjectIsolationState;
use acadrust::{EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet as HashSet};
use std::sync::Arc;

const DEFAULT_HISTORY_MAX_ENTRIES: usize = 256;
const DEFAULT_HISTORY_MAX_BYTES: usize = 512 * 1024 * 1024;

fn history_limits() -> (usize, usize) {
    use std::sync::OnceLock;
    static LIMITS: OnceLock<(usize, usize)> = OnceLock::new();
    *LIMITS.get_or_init(|| {
        let entries = std::env::var("OCS_HISTORY_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_HISTORY_MAX_ENTRIES);
        let mib = std::env::var("OCS_HISTORY_MAX_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_HISTORY_MAX_BYTES / (1024 * 1024));
        (
            if entries == 0 { usize::MAX } else { entries },
            if mib == 0 {
                usize::MAX
            } else {
                mib.saturating_mul(1024 * 1024)
            },
        )
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn defer_history_drop(entries: Vec<HistorySnapshot>) {
    if entries.is_empty() {
        return;
    }
    use std::sync::{mpsc, OnceLock};
    static TX: OnceLock<mpsc::Sender<Vec<HistorySnapshot>>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Vec<HistorySnapshot>>();
        let _ = std::thread::Builder::new()
            .name("ocs-history-drop".to_string())
            .spawn(move || {
                while let Ok(entries) = rx.recv() {
                    drop(entries);
                }
            });
        tx
    });
    if let Err(err) = tx.send(entries) {
        drop(err.0);
    }
}

#[cfg(target_arch = "wasm32")]
fn defer_history_drop(entries: Vec<HistorySnapshot>) {
    drop(entries);
}

/// Pre-command state captured by [`OpenCADStudio::begin_undo`] and handed back to
/// [`OpenCADStudio::commit_undo_delta`] to close a delta entry. Lives on the
/// stack across the (synchronous) command body — no per-tab field needed.
pub(super) struct PendingDelta {
    label: String,
    current_layout: String,
    selected_before: Vec<Handle>,
    dirty_before: bool,
    structure_before: Option<acadrust::CadDocument>,
}

pub(super) struct PendingLayerDelta {
    label: String,
    current_layout: String,
    selected_before: Vec<Handle>,
    dirty_before: bool,
    before: Vec<(String, Option<acadrust::tables::Layer>)>,
}

pub(super) struct PendingTextStyleDelta {
    label: String,
    current_layout: String,
    selected_before: Vec<Handle>,
    dirty_before: bool,
    before: Vec<(String, Option<acadrust::tables::TextStyle>)>,
}

pub(super) struct PendingDimStyleDelta {
    label: String,
    current_layout: String,
    selected_before: Vec<Handle>,
    dirty_before: bool,
    before: Vec<(String, Option<acadrust::tables::DimStyle>)>,
}

pub(super) struct PendingObjectDelta {
    label: String,
    current_layout: String,
    selected_before: Vec<Handle>,
    dirty_before: bool,
    before: FxHashMap<Handle, acadrust::objects::ObjectType>,
}

impl OpenCADStudio {
    pub(super) fn history_label_from_active_cmd(&self, i: usize, fallback: &'static str) -> String {
        self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|cmd| cmd.name().to_string())
            .unwrap_or_else(|| fallback.to_string())
    }

    fn clear_redo_history(&mut self, i: usize) {
        let discarded = std::mem::take(&mut self.tabs[i].history.redo_stack);
        defer_history_drop(discarded);
    }

    fn trim_history(&mut self, i: usize) {
        let (max_entries, max_bytes) = history_limits();
        let history = &mut self.tabs[i].history;
        let mut entries = history
            .undo_stack
            .len()
            .saturating_add(history.redo_stack.len());
        let mut bytes = history
            .undo_stack
            .iter()
            .chain(history.redo_stack.iter())
            .fold(0usize, |sum, item| {
                sum.saturating_add(item.estimated_bytes())
            });
        let mut discarded = Vec::new();
        while entries > max_entries || bytes > max_bytes {
            let item = if !history.undo_stack.is_empty() {
                history.undo_stack.remove(0)
            } else if !history.redo_stack.is_empty() {
                history.redo_stack.remove(0)
            } else {
                break;
            };
            entries -= 1;
            bytes = bytes.saturating_sub(item.estimated_bytes());
            discarded.push(item);
        }
        defer_history_drop(discarded);
    }

    fn push_undo_entry(&mut self, i: usize, snapshot: HistorySnapshot) {
        self.tabs[i].history.undo_stack.push(snapshot);
        self.tabs[i].edit_revision = self.tabs[i].edit_revision.wrapping_add(1);
        self.clear_redo_history(i);
        self.trim_history(i);
    }

    pub(super) fn push_object_visibility_history(
        &mut self,
        i: usize,
        label: impl Into<String>,
        before: ObjectIsolationState,
        selected_before: Vec<Handle>,
    ) {
        self.finish_pending_history(i);
        let after = self.tabs[i].scene.object_isolation.clone();
        let selected_after: Vec<Handle> =
            self.tabs[i].scene.selected.iter().copied().collect();
        if before == after && selected_before == selected_after {
            return;
        }
        self.push_undo_entry(
            i,
            HistorySnapshot::ObjectVisibility(ObjectVisibilitySnapshot {
                before,
                after,
                selected_before,
                selected_after,
                label: label.into(),
            }),
        );
    }

    pub(super) fn push_entity_group_history(
        &mut self,
        i: usize,
        label: impl Into<String>,
        before: Vec<(Handle, Arc<EntityType>)>,
        object_before: Vec<(Handle, acadrust::objects::ObjectType)>,
        dirty_before: bool,
    ) {
        self.finish_pending_history(i);
        let entities: Vec<_> = before
            .into_iter()
            .filter_map(|(handle, original)| {
                let after = self.tabs[i].scene.document.get_entity_arc(handle)?;
                Some((handle, Some(original), Some(after)))
            })
            .collect();
        let objects: Vec<_> = object_before
            .into_iter()
            .filter_map(|(handle, before)| {
                let after = self.tabs[i].scene.document.objects.get(&handle).cloned();
                (Some(before.clone()) != after).then_some(ObjectEntryDelta {
                    handle,
                    before: Some(before),
                    after,
                })
            })
            .collect();
        if entities.is_empty() && objects.is_empty() {
            return;
        }
        let selected: Vec<Handle> = self.tabs[i].scene.selected.iter().copied().collect();
        let delta = DeltaSnapshot {
            entities,
            current_layout_before: self.tabs[i].scene.current_layout.clone(),
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: selected.clone(),
            selected_after: selected,
            dirty_before,
            dirty_after: true,
            active_layer: None,
            structure: (!objects.is_empty()).then_some(StructureSnapshot::Objects(objects)),
            label: label.into(),
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    pub(super) fn defer_live_entity_history_after(&mut self, i: usize, handle: Handle) {
        let Some(HistorySnapshot::Delta(delta)) =
            self.tabs[i].history.undo_stack.last_mut()
        else {
            return;
        };
        if let Some((_, _, after)) = delta
            .entities
            .iter_mut()
            .find(|(entry_handle, _, _)| *entry_handle == handle)
        {
            *after = None;
        }
    }

    pub(super) fn finish_live_entity_history(&mut self, i: usize, handle: Handle) {
        let Some(after) = self.tabs[i].scene.document.get_entity_arc(handle) else {
            return;
        };
        let selected_after = self.tabs[i].scene.selected.iter().copied().collect();
        let current_layout_after = self.tabs[i].scene.current_layout.clone();
        let dirty_after = self.tabs[i].dirty;
        let Some(HistorySnapshot::Delta(delta)) =
            self.tabs[i].history.undo_stack.last_mut()
        else {
            return;
        };
        let Some((_, _, entry_after)) = delta
            .entities
            .iter_mut()
            .find(|(entry_handle, _, _)| *entry_handle == handle)
        else {
            return;
        };
        *entry_after = Some(after);
        delta.selected_after = selected_after;
        delta.current_layout_after = current_layout_after;
        delta.dirty_after = dirty_after;
    }

    pub(super) fn discard_last_undo_entry(&mut self, i: usize) {
        if self.tabs[i].history.pending.take().is_some() {
            self.tabs[i].scene.document.end_entity_change_recording();
            return;
        }
        if let Some(discarded) = self.tabs[i].history.undo_stack.pop() {
            defer_history_drop(vec![discarded]);
        }
    }

    pub(super) fn finish_pending_history(&mut self, i: usize) {
        let Some(mut pending) = self.tabs[i].history.pending.take() else {
            return;
        };
        self.tabs[i].scene.document.end_entity_change_recording();
        let structure_after = self.tabs[i].scene.document.snapshot_structure();
        let before_images = pending.recorder.take_before_images();
        let added_handles: Vec<Handle> = before_images
            .iter()
            .filter_map(|(handle, before)| {
                (before.is_none() && self.tabs[i].scene.document.get_entity(*handle).is_some())
                    .then_some(*handle)
            })
            .collect();
        acadrust::CadDocument::align_added_entity_structure(
            &mut pending.structure_before,
            &structure_after,
            &added_handles,
        );
        let structure_changed = pending.structure_before != structure_after;
        let entities: Vec<_> = before_images
            .into_iter()
            .map(|(handle, before)| {
                let after = self.tabs[i].scene.document.get_entity_arc(handle);
                (handle, before, after)
            })
            .filter(|(_, before, after)| match (before, after) {
                (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
                (None, None) => false,
                _ => true,
            })
            .collect();
        let selected_after: Vec<Handle> = self.tabs[i].scene.selected.iter().copied().collect();
        let dirty_after = self.tabs[i].dirty;
        let active_layer_after = self.tabs[i].active_layer.clone();
        if entities.is_empty()
            && !structure_changed
            && pending.selected_before == selected_after
            && pending.dirty_before == dirty_after
            && pending.active_layer == active_layer_after
        {
            return;
        }
        let delta = DeltaSnapshot {
            entities,
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after,
            dirty_before: pending.dirty_before,
            dirty_after,
            active_layer: (pending.active_layer != active_layer_after)
                .then_some((pending.active_layer, active_layer_after)),
            structure: structure_changed.then_some(StructureSnapshot::Full(pending.structure_before)),
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    pub(super) fn finish_all_pending_history(&mut self) {
        for i in 0..self.tabs.len() {
            self.finish_pending_history(i);
        }
    }

    pub(super) fn push_undo_snapshot(&mut self, i: usize, label: impl Into<String>) {
        self.finish_pending_history(i);
        let label = label.into();
        let current_layout = self.tabs[i].scene.current_layout.clone();
        let selected_before = self.tabs[i].scene.selected.iter().copied().collect();
        let dirty_before = self.tabs[i].dirty;
        let structure_before = self.tabs[i].scene.document.snapshot_structure();
        let recorder = self.tabs[i].scene.document.begin_entity_change_recording();
        self.tabs[i].history.pending = Some(PendingHistorySnapshot {
            label,
            current_layout,
            active_layer: self.tabs[i].active_layer.clone(),
            selected_before,
            dirty_before,
            structure_before,
            recorder,
        });
    }

    /// Begin undo capture for an entity edit that will touch `touched` entities.
    /// Starts a cheap Scene delta recording and returns the pre-command state to
    /// pass to [`OpenCADStudio::commit_undo_delta`] after the mutation. A command
    /// that may touch layers/objects/block records also retains a structure-only
    /// document image; the O(N) entity store is excluded.
    pub(super) fn begin_undo(
        &mut self,
        i: usize,
        label: impl Into<String>,
        _touched: usize,
        delta_safe: bool,
    ) -> Option<PendingDelta> {
        self.finish_pending_history(i);
        let label = label.into();
        let structure_before =
            (!delta_safe).then(|| self.tabs[i].scene.document.snapshot_structure());
        self.tabs[i].scene.begin_undo_recording();
        Some(PendingDelta {
            label,
            current_layout: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: self.tabs[i].dirty,
            structure_before,
        })
    }

    pub(super) fn begin_layer_undo(
        &mut self,
        i: usize,
        label: impl Into<String>,
        names: &[String],
    ) -> PendingLayerDelta {
        self.finish_pending_history(i);
        PendingLayerDelta {
            label: label.into(),
            current_layout: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: self.tabs[i].dirty,
            before: names
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        self.tabs[i].scene.document.layers.get(name).cloned(),
                    )
                })
                .collect(),
        }
    }

    pub(super) fn commit_layer_undo(&mut self, i: usize, pending: PendingLayerDelta) {
        let entries: Vec<_> = pending
            .before
            .into_iter()
            .filter_map(|(name, before)| {
                let after = self.tabs[i].scene.document.layers.get(&name).cloned();
                (before != after).then_some(TableEntryDelta {
                    name,
                    before,
                    after,
                })
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        let selected_after = self.tabs[i].scene.selected.iter().copied().collect();
        let delta = DeltaSnapshot {
            entities: Vec::new(),
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after,
            dirty_before: pending.dirty_before,
            dirty_after: self.tabs[i].dirty,
            active_layer: None,
            structure: Some(StructureSnapshot::Layers(entries)),
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    pub(super) fn begin_text_style_undo(
        &mut self,
        i: usize,
        label: impl Into<String>,
        names: &[String],
    ) -> PendingTextStyleDelta {
        self.finish_pending_history(i);
        PendingTextStyleDelta {
            label: label.into(),
            current_layout: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: self.tabs[i].dirty,
            before: names
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        self.tabs[i].scene.document.text_styles.get(name).cloned(),
                    )
                })
                .collect(),
        }
    }

    pub(super) fn commit_text_style_undo(&mut self, i: usize, pending: PendingTextStyleDelta) {
        let entries: Vec<_> = pending
            .before
            .into_iter()
            .filter_map(|(name, before)| {
                let after = self.tabs[i].scene.document.text_styles.get(&name).cloned();
                (before != after).then_some(TableEntryDelta {
                    name,
                    before,
                    after,
                })
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        let delta = DeltaSnapshot {
            entities: Vec::new(),
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: pending.dirty_before,
            dirty_after: self.tabs[i].dirty,
            active_layer: None,
            structure: Some(StructureSnapshot::TextStyles(entries)),
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    pub(super) fn begin_dim_style_undo(
        &mut self,
        i: usize,
        label: impl Into<String>,
        names: &[String],
    ) -> PendingDimStyleDelta {
        self.finish_pending_history(i);
        PendingDimStyleDelta {
            label: label.into(),
            current_layout: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: self.tabs[i].dirty,
            before: names
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        self.tabs[i].scene.document.dim_styles.get(name).cloned(),
                    )
                })
                .collect(),
        }
    }

    pub(super) fn commit_dim_style_undo(&mut self, i: usize, pending: PendingDimStyleDelta) {
        let entries: Vec<_> = pending
            .before
            .into_iter()
            .filter_map(|(name, before)| {
                let after = self.tabs[i].scene.document.dim_styles.get(&name).cloned();
                (before != after).then_some(TableEntryDelta {
                    name,
                    before,
                    after,
                })
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        let delta = DeltaSnapshot {
            entities: Vec::new(),
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: pending.dirty_before,
            dirty_after: self.tabs[i].dirty,
            active_layer: None,
            structure: Some(StructureSnapshot::DimStyles(entries)),
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    fn group_object_state(&self, i: usize) -> FxHashMap<Handle, acadrust::objects::ObjectType> {
        use acadrust::objects::ObjectType;
        let document = &self.tabs[i].scene.document;
        let dictionary = document.header.acad_group_dict_handle;
        document
            .objects
            .iter()
            .filter(|(handle, object)| {
                **handle == dictionary || matches!(object, ObjectType::Group(_))
            })
            .map(|(handle, object)| (*handle, object.clone()))
            .collect()
    }

    pub(super) fn begin_group_undo(
        &mut self,
        i: usize,
        label: impl Into<String>,
    ) -> PendingObjectDelta {
        self.finish_pending_history(i);
        PendingObjectDelta {
            label: label.into(),
            current_layout: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: self.tabs[i].dirty,
            before: self.group_object_state(i),
        }
    }

    pub(super) fn commit_group_undo(&mut self, i: usize, pending: PendingObjectDelta) {
        let after = self.group_object_state(i);
        let mut handles: HashSet<Handle> = pending.before.keys().copied().collect();
        handles.extend(after.keys().copied());
        let entries: Vec<_> = handles
            .into_iter()
            .filter_map(|handle| {
                let before = pending.before.get(&handle).cloned();
                let after = after.get(&handle).cloned();
                (before != after).then_some(ObjectEntryDelta {
                    handle,
                    before,
                    after,
                })
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        let delta = DeltaSnapshot {
            entities: Vec::new(),
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before: pending.dirty_before,
            dirty_after: self.tabs[i].dirty,
            active_layer: None,
            structure: Some(StructureSnapshot::Objects(entries)),
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    pub(super) fn commit_style_undo(
        &mut self,
        i: usize,
        before: super::style_ops::StyleStateSnapshot,
        after: super::style_ops::StyleStateSnapshot,
        dirty_before: bool,
    ) {
        if before == after {
            return;
        }
        self.finish_pending_history(i);
        let (text_names, dim_names, object_handles) = after.changed_keys(&before);
        let delta = DeltaSnapshot {
            entities: Vec::new(),
            current_layout_before: self.tabs[i].scene.current_layout.clone(),
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: self.tabs[i].scene.selected.iter().copied().collect(),
            selected_after: self.tabs[i].scene.selected.iter().copied().collect(),
            dirty_before,
            dirty_after: true,
            active_layer: None,
            structure: Some(StructureSnapshot::Styles {
                before,
                after,
                text_names,
                dim_names,
                object_handles,
            }),
            label: "STYLE".to_string(),
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    /// Dimension copies still need a full snapshot because they clone a fresh
    /// anonymous `*D` block record. Complete group copies record their exact
    /// Group/dictionary object entries and remain delta-safe.
    pub(super) fn delta_copy_safe(&self, i: usize, handles: &[Handle]) -> bool {
        let doc = &self.tabs[i].scene.document;
        !handles
            .iter()
            .any(|h| matches!(doc.get_entity(*h), Some(EntityType::Dimension(_))))
    }

    /// Erase records any affected Group/object dictionary entries alongside the
    /// entity before-images, so it never needs a whole-structure snapshot.
    pub(super) fn delta_erase_safe(&self, _i: usize, _handles: &[Handle]) -> bool {
        true
    }

    /// Add is delta-safe when its layer and XData application IDs already exist
    /// and it creates no block records.
    pub(super) fn delta_add_safe(&self, i: usize, entity: &EntityType) -> bool {
        if matches!(
            entity,
            EntityType::Block(_)
                | EntityType::BlockEnd(_)
        ) {
            return false;
        }
        let layer = entity.common().layer.clone();
        let doc = &self.tabs[i].scene.document;
        let layer_exists = layer.trim().is_empty() || doc.layers.contains(&layer);
        let app_ids_exist = entity
            .common()
            .extended_data
            .records()
            .iter()
            .all(|record| {
                record.application_name.trim().is_empty()
                    || doc.app_ids.contains(&record.application_name)
            });
        layer_exists && app_ids_exist
    }

    /// Close the delta transaction opened by [`OpenCADStudio::begin_undo`]:
    /// harvests the recorded before-images, pairs each with the entity's current
    /// (after) state, and pushes a symmetric [`DeltaSnapshot`] onto the undo
    /// stack. Called after the command's mutations (and after `dirty`/selection
    /// reach their final values).
    pub(super) fn commit_undo_delta(&mut self, i: usize, pending: PendingDelta) {
        let Some(rec) = self.tabs[i].scene.take_undo_recording() else {
            return;
        };
        // The predicate should have kept the command entity-only; if a primitive
        // still poisoned the recording we've already mutated and the pre-command
        // full state is gone, so the entity delta is the best we can do (its
        // entity part stays correct; a leaked layer/group/block just won't
        // revert). Loud in debug, a warning in release — never a silent wrong.
        debug_assert!(
            !rec.is_poisoned() || pending.structure_before.is_some(),
            "delta command '{}' mutated unrecorded non-entity state",
            pending.label
        );
        if rec.is_empty() && pending.structure_before.is_none() {
            // Nothing actually changed (e.g. every target was on a locked
            // layer). Leave the undo/redo stacks untouched.
            return;
        }
        if rec.is_poisoned() && pending.structure_before.is_none() {
            eprintln!(
                "[undo] delta '{}' touched non-entity state; undo may be incomplete",
                pending.label
            );
        }
        let (entity_before, object_before) = rec.into_recorded_images();
        let entities: Vec<(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)> = entity_before
            .into_iter()
            .map(|(h, before)| {
                let after = self.tabs[i].scene.document.get_entity_arc(h);
                (h, before, after)
            })
            .collect();
        let objects: Vec<ObjectEntryDelta> = object_before
            .into_iter()
            .filter_map(|(handle, before)| {
                let after = self.tabs[i].scene.document.objects.get(&handle).cloned();
                (before != after).then_some(ObjectEntryDelta {
                    handle,
                    before,
                    after,
                })
            })
            .collect();
        let selected_after = self.tabs[i].scene.selected.iter().copied().collect();
        let dirty_after = self.tabs[i].dirty;
        let mut structure = pending.structure_before.map(StructureSnapshot::Full);
        if let Some(before_structure) = structure.as_mut() {
            if let StructureSnapshot::Full(before_structure) = before_structure {
                let after_structure = self.tabs[i].scene.document.snapshot_structure();
                let added_handles: Vec<Handle> = entities
                    .iter()
                    .filter_map(|(handle, before, after)| {
                        (before.is_none() && after.is_some()).then_some(*handle)
                    })
                    .collect();
                acadrust::CadDocument::align_added_entity_structure(
                    before_structure,
                    &after_structure,
                    &added_handles,
                );
                if *before_structure == after_structure {
                    structure = None;
                }
            }
        }
        if structure.is_none() && !objects.is_empty() {
            structure = Some(StructureSnapshot::Objects(objects));
        }
        let delta = DeltaSnapshot {
            entities,
            current_layout_before: pending.current_layout,
            current_layout_after: self.tabs[i].scene.current_layout.clone(),
            selected_before: pending.selected_before,
            selected_after,
            dirty_before: pending.dirty_before,
            dirty_after,
            active_layer: None,
            structure,
            label: pending.label,
        };
        self.push_undo_entry(i, HistorySnapshot::Delta(delta));
    }

    /// Apply one side of a delta entry in place: `undo` restores each entity's
    /// before-image, `!undo` (redo) its after-image. `None` on the chosen side
    /// means the entity is absent there (erase it). Reports the exact touched
    /// handles through `bump_entities` so the incremental caches patch per-handle
    /// — no full re-tessellation, no `populate_*` document walk.
    fn apply_delta_state(
        &mut self,
        i: usize,
        d: &mut DeltaSnapshot,
        undo: bool,
    ) -> Vec<(Handle, crate::scene::ChangeKind)> {
        // Install the chosen side of every entity image. Derived-cache, geometry
        // and UI invalidation are deferred until every requested undo/redo step
        // has been applied.
        if let Some(structure) = d.structure.as_mut() {
            match structure {
                StructureSnapshot::Full(stored) => {
                    let inverse = self.tabs[i].scene.document.swap_structure(
                        std::mem::replace(stored, acadrust::CadDocument::new()),
                    );
                    *stored = inverse;
                    self.tabs[i].scene.invalidate_dependency_index();
                }
                StructureSnapshot::Layers(entries) => {
                    let names: Vec<String> =
                        entries.iter().map(|entry| entry.name.clone()).collect();
                    for entry in entries {
                        let value = if undo {
                            entry.before.clone()
                        } else {
                            entry.after.clone()
                        };
                        if let Some(layer) = value {
                            self.tabs[i].scene.document.layers.add_or_replace(layer);
                        } else {
                            self.tabs[i].scene.document.layers.remove(&entry.name);
                        }
                    }
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                }
                StructureSnapshot::TextStyles(entries) => {
                    let names: Vec<String> =
                        entries.iter().map(|entry| entry.name.clone()).collect();
                    for entry in entries {
                        let value = if undo {
                            entry.before.clone()
                        } else {
                            entry.after.clone()
                        };
                        if let Some(style) = value {
                            self.tabs[i]
                                .scene
                                .document
                                .text_styles
                                .add_or_replace(style);
                        } else {
                            self.tabs[i].scene.document.text_styles.remove(&entry.name);
                        }
                    }
                    for name in names {
                        self.tabs[i].scene.invalidate_text_style_dependencies(&name);
                    }
                }
                StructureSnapshot::DimStyles(entries) => {
                    let names: Vec<String> =
                        entries.iter().map(|entry| entry.name.clone()).collect();
                    for entry in entries {
                        let value = if undo {
                            entry.before.clone()
                        } else {
                            entry.after.clone()
                        };
                        if let Some(style) = value {
                            self.tabs[i].scene.document.dim_styles.add_or_replace(style);
                        } else {
                            self.tabs[i].scene.document.dim_styles.remove(&entry.name);
                        }
                    }
                    for name in names {
                        self.tabs[i].scene.invalidate_dim_style_dependencies(&name);
                    }
                }
                StructureSnapshot::Objects(entries) => {
                    for entry in entries {
                        let value = if undo {
                            entry.before.clone()
                        } else {
                            entry.after.clone()
                        };
                        if let Some(object) = value {
                            self.tabs[i]
                                .scene
                                .document
                                .objects
                                .insert(entry.handle, object);
                        } else {
                            self.tabs[i].scene.document.objects.remove(&entry.handle);
                        }
                    }
                }
                StructureSnapshot::Styles {
                    before,
                    after,
                    text_names,
                    dim_names,
                    object_handles,
                } => {
                    let snapshot = if undo { before } else { after };
                    self.restore_style_state(i, snapshot);
                    self.tabs[i]
                        .scene
                        .invalidate_text_style_dependencies_many(text_names);
                    self.tabs[i]
                        .scene
                        .invalidate_dim_style_dependencies_many(dim_names);
                    self.tabs[i]
                        .scene
                        .invalidate_object_style_dependencies(object_handles);
                }
            }
        }
        let changes = self.tabs[i].scene.apply_entity_delta(&d.entities, undo);
        let scene = &mut self.tabs[i].scene;
        let (sel, dirty) = if undo {
            (&d.selected_before, d.dirty_before)
        } else {
            (&d.selected_after, d.dirty_after)
        };
        let restored: HashSet<Handle> = sel
            .iter()
            .copied()
            .filter(|h| scene.document.get_entity(*h).is_some())
            .collect();
        scene.replace_selection(restored);
        if scene
            .hover_highlight
            .is_some_and(|handle| scene.document.get_entity(handle).is_none())
        {
            scene.set_hover_highlight(None);
        }
        // Entity deltas currently preserve the layout; direct assignment avoids
        // clearing layout render caches between steps and keeps future widened
        // deltas batchable.
        scene.current_layout = if undo {
            d.current_layout_before.clone()
        } else {
            d.current_layout_after.clone()
        };
        if let Some((before, after)) = &d.active_layer {
            self.tabs[i].active_layer = if undo { before } else { after }.clone();
        }
        self.tabs[i].dirty = dirty;
        changes
    }

    fn apply_object_visibility_state(
        &mut self,
        i: usize,
        snapshot: &ObjectVisibilitySnapshot,
        undo: bool,
    ) {
        let (state, selected) = if undo {
            (&snapshot.before, &snapshot.selected_before)
        } else {
            (&snapshot.after, &snapshot.selected_after)
        };
        let scene = &mut self.tabs[i].scene;
        let mut affected = scene.object_isolation.hidden.clone();
        affected.extend(state.hidden.iter().copied());
        scene.object_isolation = state.clone();
        scene.replace_selection(
            selected
                .iter()
                .copied()
                .filter(|handle| scene.document.get_entity(*handle).is_some())
                .collect(),
        );
        let changes: Vec<_> = affected
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            scene.bump_entities(&changes);
        }
    }

    /// Perform the one expensive cache/UI synchronization required after a
    /// batch of history steps. Intermediate states are never rendered, so
    /// rebuilding them only multiplies latency.
    fn finish_history_apply(
        &mut self,
        i: usize,
        had_full: bool,
        layer_panel_changed: bool,
        changes: &[(Handle, crate::scene::ChangeKind)],
    ) {
        self.tabs[i].edit_revision = self.tabs[i].edit_revision.wrapping_add(1);
        {
            let scene = &mut self.tabs[i].scene;
            if had_full {
                // One document walk per derived category and one geometry bump
                // for the final state. Unlike the old path, decoded images stay
                // cached instead of being immediately cleared.
                scene.rebuild_derived_caches();
            } else if !changes.is_empty() {
                use crate::scene::ChangeKind::{Added, Modified, Removed};
                let mut net: FxHashMap<Handle, crate::scene::ChangeKind> = FxHashMap::default();
                for &(handle, kind) in changes {
                    match (net.get(&handle).copied(), kind) {
                        (None, kind) => {
                            net.insert(handle, kind);
                        }
                        (Some(Added), Removed) => {
                            net.remove(&handle);
                        }
                        (Some(Added), _) => {}
                        (Some(Removed), Added) => {
                            net.insert(handle, Modified);
                        }
                        (Some(Modified), Removed) => {
                            net.insert(handle, Removed);
                        }
                        (Some(Removed), _) | (Some(Modified), _) => {}
                    }
                }
                let final_changes: Vec<_> = net.into_iter().collect();
                for &(handle, _) in &final_changes {
                    scene.reseed_derived_caches(handle);
                }
                if !final_changes.is_empty() {
                    if scene.changes_touch_block_definition(&final_changes) {
                        // A block-local delta changes the assembled definition
                        // consumed by every INSERT (including nested blocks).
                        // Replaying only the child handle leaves those cached
                        // references visually stale after BEDIT closes.
                        scene.bump_geometry();
                    } else {
                        scene.bump_entities(&final_changes);
                    }
                }
            }
            scene.clear_preview_wire();
        }
        // UCS state is cached separately from its persisted header/viewport
        // fields. History may just have restored those fields, so adopt them
        // before drawing the icon/grid or accepting the next coordinate.
        self.tabs[i].refresh_active_ucs();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].active_grip = None;
        if layer_panel_changed {
            let doc_layers = self.tabs[i].scene.document.layers.clone();
            let vp_info = self.tabs[i].scene.viewport_list();
            self.tabs[i]
                .layers
                .sync_with_viewports(&doc_layers, vp_info);
            self.sync_ribbon_layers();
        }
        self.refresh_properties();
    }

    pub(super) fn undo_active_tab(&mut self) {
        self.undo_steps(1);
    }

    pub(super) fn redo_active_tab(&mut self) {
        self.redo_steps(1);
    }

    pub(super) fn undo_steps(&mut self, steps: usize) {
        let i = self.active_tab;
        self.finish_pending_history(i);
        let available = self.tabs[i].history.undo_stack.len();
        let steps = steps.min(available);
        if steps == 0 {
            self.command_line.push_info(crate::t!("Nothing to undo.").as_ref());
            return;
        }

        let mut last_label = String::new();
        let mut had_full = false;
        let mut layer_panel_changed = false;
        let mut changes = Vec::new();
        for _ in 0..steps {
            let Some(snapshot) = self.tabs[i].history.undo_stack.pop() else {
                break;
            };
            last_label = snapshot.label().to_string();
            match snapshot {
                HistorySnapshot::Delta(mut d) => {
                    // Symmetric: undo applies the before side, then the same
                    // delta rides to the redo stack (it still holds the after
                    // side) — no current-state capture needed.
                    layer_panel_changed |= d.structure.as_ref().is_some_and(|structure| {
                        matches!(
                            structure,
                            StructureSnapshot::Full(_) | StructureSnapshot::Layers(_)
                        )
                    }) || d.active_layer.is_some() || d.entities.iter().any(|(_, before, after)| {
                        before
                            .as_deref()
                            .is_some_and(|entity| matches!(entity, EntityType::Viewport(_)))
                            || after
                                .as_deref()
                                .is_some_and(|entity| matches!(entity, EntityType::Viewport(_)))
                    });
                    had_full |= d.structure.as_ref().is_some_and(StructureSnapshot::is_full);
                    changes.extend(self.apply_delta_state(i, &mut d, true));
                    self.tabs[i]
                        .history
                        .redo_stack
                        .push(HistorySnapshot::Delta(d));
                }
                HistorySnapshot::ObjectVisibility(v) => {
                    self.apply_object_visibility_state(i, &v, true);
                    self.tabs[i]
                        .history
                        .redo_stack
                        .push(HistorySnapshot::ObjectVisibility(v));
                }
            }
        }
        self.finish_history_apply(i, had_full, layer_panel_changed, &changes);
        self.command_line
            .push_output(crate::tf!("Undo: {last_label}").as_ref());
    }

    pub(super) fn redo_steps(&mut self, steps: usize) {
        let i = self.active_tab;
        self.finish_pending_history(i);
        let available = self.tabs[i].history.redo_stack.len();
        let steps = steps.min(available);
        if steps == 0 {
            self.command_line.push_info(crate::t!("Nothing to redo.").as_ref());
            return;
        }

        let mut last_label = String::new();
        let mut had_full = false;
        let mut layer_panel_changed = false;
        let mut changes = Vec::new();
        for _ in 0..steps {
            let Some(snapshot) = self.tabs[i].history.redo_stack.pop() else {
                break;
            };
            last_label = snapshot.label().to_string();
            match snapshot {
                HistorySnapshot::Delta(mut d) => {
                    layer_panel_changed |= d.structure.as_ref().is_some_and(|structure| {
                        matches!(
                            structure,
                            StructureSnapshot::Full(_) | StructureSnapshot::Layers(_)
                        )
                    }) || d.active_layer.is_some() || d.entities.iter().any(|(_, before, after)| {
                        before
                            .as_deref()
                            .is_some_and(|entity| matches!(entity, EntityType::Viewport(_)))
                            || after
                                .as_deref()
                                .is_some_and(|entity| matches!(entity, EntityType::Viewport(_)))
                    });
                    had_full |= d.structure.as_ref().is_some_and(StructureSnapshot::is_full);
                    changes.extend(self.apply_delta_state(i, &mut d, false));
                    self.tabs[i]
                        .history
                        .undo_stack
                        .push(HistorySnapshot::Delta(d));
                }
                HistorySnapshot::ObjectVisibility(v) => {
                    self.apply_object_visibility_state(i, &v, false);
                    self.tabs[i]
                        .history
                        .undo_stack
                        .push(HistorySnapshot::ObjectVisibility(v));
                }
            }
        }
        self.finish_history_apply(i, had_full, layer_panel_changed, &changes);
        self.command_line
            .push_output(crate::tf!("Redo: {last_label}").as_ref());
    }
}

pub(super) fn history_dropdown_labels(stack: &[HistorySnapshot]) -> Vec<String> {
    stack
        .iter()
        .rev()
        .map(|snapshot| snapshot.label().to_string())
        .collect()
}
