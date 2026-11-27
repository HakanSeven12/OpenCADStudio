//! Keeping centre marks on the geometry they mark as it is edited.

use super::*;
use crate::entities::centermark::*;
use document::entities::{CenterMarkAssociation, CenterMarkSource, CenterMarkSourceKind};
use document::types::Vector3;
use glam::DVec3;
use ocs_geom::geom2d::{closest_point, Arc as KernelArc, BulgeArc, Curve};

impl Scene {
    pub(crate) fn reassociate_center_mark(
        &mut self,
        target: Handle,
        source_handle: Handle,
        pick: DVec3,
    ) -> bool {
        let Some(source_entity) = self.document.get_entity(source_handle) else {
            return false;
        };
        let Some((source, center, radius, x, y)) =
            picked_mark_source(source_entity, source_handle, pick)
        else {
            return false;
        };
        let Some(EntityType::Line(target_line)) = self.document.get_entity(target) else {
            return false;
        };
        let Some(mut association) = CenterMarkAssociation::read(&target_line.common.extended_data)
        else {
            return false;
        };
        association.source = source;
        apply_source_geometry(&mut association, center, radius, x, y);
        association.associated = true;
        if self.is_recording_undo() {
            let before = self.document.get_entity_arc(target);
            self.record_undo_before(target, before);
        }
        let Some(EntityType::Line(line)) = self.document.get_entity_mut(target) else {
            return false;
        };
        update_carrier(line, &association);
        // The mark now points at `source_handle`; drop the indexed edge.
        self.associative_centermark_source_cache.borrow_mut().take();
        self.bump_entities(&[(target, ChangeKind::Modified)]);
        true
    }

    /// Centre-mark carriers attached to the changed source handle, read from
    /// the reverse index (built on first use, dropped whenever an entity or an
    /// association may have changed). Candidates are re-validated against
    /// their current xdata, so a stale entry can only cost a lookup.
    fn associative_centermark_dependents(
        &self,
        changed: &rustc_hash::FxHashSet<Handle>,
    ) -> Vec<Handle> {
        if self.associative_centermark_source_cache.borrow().is_none() {
            let mut index: HashMap<Handle, Vec<Handle>> = HashMap::default();
            for entity in self.document.entities() {
                let EntityType::Line(line) = entity else {
                    continue;
                };
                let Some(association) = CenterMarkAssociation::read(&line.common.extended_data)
                else {
                    continue;
                };
                if !association.associated {
                    continue;
                }
                let carriers = index.entry(association.source.handle).or_default();
                if !carriers.contains(&line.common.handle) {
                    carriers.push(line.common.handle);
                }
            }
            *self.associative_centermark_source_cache.borrow_mut() = Some(index);
        }
        let cache = self.associative_centermark_source_cache.borrow();
        let index = cache.as_ref().expect("associative centre mark index");
        let mut handles = rustc_hash::FxHashSet::default();
        for source in changed {
            if let Some(carriers) = index.get(source) {
                handles.extend(carriers.iter().copied());
            }
        }
        handles.into_iter().collect()
    }

    pub(crate) fn refresh_associative_center_marks(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let candidates: Vec<_> = self
            .associative_centermark_dependents(&changed)
            .into_iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(handle)? else {
                    return None;
                };
                let association = CenterMarkAssociation::read(&line.common.extended_data)?;
                (association.associated && changed.contains(&association.source.handle))
                    .then_some((handle, association))
            })
            .collect();
        let mut result = Vec::new();
        for (handle, mut association) in candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(handle);
                self.record_undo_before(handle, before);
            }
            if let Some((center, radius, x, y)) =
                resolve_source(&self.document, &association.source)
            {
                apply_source_geometry(&mut association, center, radius, x, y);
            } else {
                association.associated = false;
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(handle) {
                update_carrier(line, &association);
                result.push((handle, ChangeKind::Modified));
            }
        }
        result
    }

    pub(crate) fn reset_center_marks(&mut self, handles: &[Handle]) -> usize {
        let settings = self.centerline_settings();
        let candidates: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(*handle)? else {
                    return None;
                };
                let mut association = CenterMarkAssociation::read(&line.common.extended_data)?;
                let diameter = association.radius * 2.0;
                association.cross_size =
                    resolve_center_measure(&settings.cross_size, diameter, 0.1);
                association.cross_gap =
                    resolve_center_measure(&settings.cross_gap, diameter, 0.05);
                association.cross_size_relative =
                    center_measure_is_relative(&settings.cross_size);
                association.cross_gap_relative =
                    center_measure_is_relative(&settings.cross_gap);
                association.extension_length = settings.extension;
                association.length_adjustments = [0.0; 4];
                association.overshoots = [0.0; 4];
                association.show_extensions = settings.mark_extensions;
                Some((*handle, association))
            })
            .collect();
        for (handle, association) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                update_carrier(line, association);
            }
        }
        if !candidates.is_empty() {
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }

    pub(crate) fn set_center_mark_association(
        &mut self,
        handles: &[Handle],
        associated: bool,
    ) -> usize {
        let candidates: Vec<_> = handles
            .iter()
            .filter_map(|handle| {
                let EntityType::Line(line) = self.document.get_entity(*handle)? else {
                    return None;
                };
                let mut association = CenterMarkAssociation::read(&line.common.extended_data)?;
                if associated {
                    let (center, radius, x, y) =
                        resolve_source(&self.document, &association.source)?;
                    apply_source_geometry(&mut association, center, radius, x, y);
                }
                association.associated = associated;
                Some((*handle, association))
            })
            .collect();
        for (handle, association) in &candidates {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(*handle);
                self.record_undo_before(*handle, before);
            }
            if let Some(EntityType::Line(line)) = self.document.get_entity_mut(*handle) {
                update_carrier(line, association);
            }
        }
        if !candidates.is_empty() {
            // The association flags just changed. Drop the index; the bump
            // below re-indexes from the new xdata (an all-disassociated bump
            // rebuilds an empty index and refreshes nothing).
            self.associative_centermark_source_cache.borrow_mut().take();
            let changes: Vec<_> = candidates
                .iter()
                .map(|(handle, _)| (*handle, ChangeKind::Modified))
                .collect();
            self.bump_entities(&changes);
        }
        candidates.len()
    }
}
