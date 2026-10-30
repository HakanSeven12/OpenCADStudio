//! Reference-only designation (API v9).
//!
//! A reference-only entity is plugin-owned display geometry: it renders and
//! snaps normally, but the built-in edit paths treat it like an entity on a
//! locked layer — no grips, no click picking (a click passes through to the
//! editable entity underneath), and user-driven writes refuse it. The owning
//! plugin keeps full write access through the plugin host API, so it can
//! refresh derived geometry in place.
//!
//! Storage is a scene-scoped in-memory set of entity handles — the host
//! writes nothing of its own into the saved DWG/DXF: no APPID registration,
//! no XDATA marker on the entity, no host-owned state a saved file depends
//! on. The set lives and dies with the scene (per drawing tab): closing a
//! tab forgets its designations, and a freshly opened drawing starts
//! undesignated. The owning plugin re-marks its entities when the host
//! dispatches the document-opened notification (API v9's
//! `BuiltinPlugin::on_notification` surface), which makes the in-memory
//! designation fully equivalent to a persisted one for its use case.
//!
//! Undo keeps the entity-write contract: a designation change performed
//! inside a plugin-opened `push_undo` scope joins that scope, a scope-less
//! change is one undo step of its own, and undo/redo restores the
//! designation with the entities it belongs to. Because the set is not part
//! of the document, set mutations are recorded as explicit first-touch
//! before-images on the history delta (see
//! `DeltaSnapshot::reference_designations`), the same channel pattern
//! `parametric_constraints` uses.

use codec::types::Handle;
use rustc_hash::FxHashSet;

/// The in-memory designation set owned by a [`Scene`](crate::Scene).
///
/// Deliberately tiny: a plain set of handles plus the operations the scene,
/// the plugin host and the history channels use. Not `Serialize` — the set
/// is never written anywhere and never leaves the process.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReferenceOnlyDesignation {
    handles: FxHashSet<Handle>,
}

impl ReferenceOnlyDesignation {
    /// Raw membership (by handle) — the caller guarantees the entity exists,
    /// or accepts a stale entry; [`Scene::is_reference_only`] does the
    /// existential check.
    pub(crate) fn is_marked(&self, handle: Handle) -> bool {
        self.handles.contains(&handle)
    }

    /// Insert or remove `handle`, returning whether the membership changed.
    /// Idempotent: re-marking a designated handle reports `false` and touches
    /// nothing, which is what terminates the re-mark loop driven by the
    /// document-changed notification dispatch.
    pub(crate) fn set(&mut self, handle: Handle, on: bool) -> bool {
        if on {
            self.handles.insert(handle)
        } else {
            self.handles.remove(&handle)
        }
    }

    /// Forget `handle` — called when its entity is erased so the set never
    /// leaks an entry for a removed entity. The undo channels restore the
    /// entry together with the entity on undo.
    pub(crate) fn remove(&mut self, handle: Handle) {
        self.handles.remove(&handle);
    }

    /// The marked handles, for the tessellation pipelines (wire stamping)
    /// and the history channels.
    pub(crate) fn handles(&self) -> &FxHashSet<Handle> {
        &self.handles
    }

    /// True when nothing is designated.
    pub(crate) fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// Drop entries whose entities no longer exist. The undo/redo channels
    /// model the exact designation state, so this is only a defensive sweep
    /// after history replay (an entity the chosen delta side removed must
    /// never stay designated through a stale entry).
    pub(crate) fn retain_present(&mut self, document: &codec::Document) {
        self.handles
            .retain(|handle| document.get_entity(*handle).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{EntityType, Line};

    fn handle(value: u64) -> Handle {
        Handle::new(value)
    }

    #[test]
    fn marking_and_unmarking_report_and_track_membership() {
        let mut designation = ReferenceOnlyDesignation::default();
        assert!(!designation.is_marked(handle(7)));
        assert!(designation.set(handle(7), true));
        assert!(designation.is_marked(handle(7)));
        // Idempotent: a second mark changes nothing and reports it — the
        // property that terminates re-mark loops.
        assert!(!designation.set(handle(7), true));
        assert!(designation.set(handle(7), false));
        assert!(!designation.is_marked(handle(7)));
        assert!(!designation.set(handle(7), false));
        assert!(designation.handles().is_empty());
    }

    #[test]
    fn removing_a_handle_forgets_it() {
        let mut designation = ReferenceOnlyDesignation::default();
        designation.set(handle(7), true);
        designation.remove(handle(7));
        assert!(!designation.is_marked(handle(7)));
        // Removing an unmarked handle is a no-op.
        designation.remove(handle(8));
        assert!(designation.is_empty());
    }

    #[test]
    fn retain_present_drops_entries_of_absent_entities() {
        let mut document = codec::Document::new();
        let live = document.add_entity(EntityType::Line(Line::new())).unwrap();
        let mut designation = ReferenceOnlyDesignation::default();
        designation.set(live, true);
        designation.set(handle(999_999), true);
        designation.retain_present(&document);
        assert!(designation.is_marked(live));
        assert!(!designation.is_marked(handle(999_999)));
    }

    #[test]
    fn scene_membership_is_guarded_by_entity_presence() {
        let mut scene = crate::scene::Scene::new();
        let handle = scene
            .document
            .add_entity(EntityType::Line(Line::new()))
            .unwrap();
        assert!(!scene.is_reference_only(handle));
        assert!(scene.set_reference_only_membership(handle, true));
        assert!(scene.is_reference_only(handle));
        // A stale set entry alone never designates: absent entities are not
        // reference-only even if the set still lists them.
        scene.set_reference_only_membership(Handle::new(999_999), true);
        assert!(!scene.is_reference_only(Handle::new(999_999)));
    }

    #[test]
    fn tessellated_wires_stamp_from_the_designation_set() {
        let document = codec::Document::new();
        let line = EntityType::Line(Line::new());
        let handle = line.common().handle;
        let selected: rustc_hash::FxHashSet<Handle> = Default::default();
        let none: rustc_hash::FxHashSet<Handle> = Default::default();
        let mut designated: rustc_hash::FxHashSet<Handle> = Default::default();
        designated.insert(handle);
        let context = crate::scene::cache::block_cache::BlockCache::default();
        for designated in [&none, &designated] {
            let wires = crate::scene::tessellate_entity(
                &document,
                &selected,
                None,
                [0.0, 0.0, 0.0, 1.0],
                1.0,
                None,
                &line,
                Some(&context),
                None,
                None,
                false,
                designated,
            );
            assert!(!wires.is_empty());
            assert!(
                wires
                    .iter()
                    .all(|wire| wire.reference_only == designated.contains(&handle)),
                "every wire carries the set's stamp for its entity"
            );
        }
    }
}
