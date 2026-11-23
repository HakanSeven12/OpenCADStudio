//! Keeping an out-of-process plugin's cached document snapshot current after
//! the plugin's own edits.
//!
//! The clients ([`client`](crate::ipc::client), [`v4`](crate::ipc::v4)) cache
//! one full document snapshot. Dropping it after every edit made the next read
//! re-transfer the whole drawing: on a drawing of tens of megabytes a command
//! that edits and re-reads entity by entity took minutes. Instead the clients
//! re-read only the entities they changed (`GetEntitiesBatch`) and patch the
//! cache with the host's post-edit copies. Whenever a patch cannot be exact
//! the cache is dropped and the next read fetches a fresh snapshot.

use document::xdata::ExtendedDataRecord;
use document::{Document, EntityType, Handle};

/// Whether an edit of `entity` can change other cached data than the entity
/// itself, so the snapshot must be refetched instead of patched. An attribute
/// is stored inside its block reference; the host replaces it there.
pub(crate) fn edit_needs_full_refresh(entity: &EntityType) -> bool {
    matches!(entity, EntityType::AttributeEntity(_))
}

/// Patch `doc` with the host's copies of the `refreshed` handles (`fresh`, in
/// the same order; `None` = the host no longer has the entity) and drop the
/// `removed` handles. Returns `false` when the patch cannot be exact: a length
/// mismatch, an entity that cannot be inserted, or an entity on a layer the
/// cached layer table does not know (the host created it). The caller then
/// drops the cache.
pub(crate) fn apply_refreshed(
    doc: &mut Document,
    refreshed: &[Handle],
    fresh: Vec<Option<EntityType>>,
    removed: &[Handle],
) -> bool {
    if fresh.len() != refreshed.len() {
        return false;
    }
    if fresh
        .iter()
        .flatten()
        .any(|entity| !doc.layers.contains(&entity.common().layer))
    {
        return false;
    }
    for handle in removed {
        doc.remove_entity(*handle);
    }
    for (handle, entity) in refreshed.iter().zip(fresh) {
        match entity {
            Some(entity) => {
                if let Some(slot) = doc.get_entity_mut(*handle) {
                    *slot = entity;
                } else if doc.add_entity(entity).is_err() {
                    return false;
                }
            }
            None => {
                doc.remove_entity(*handle);
            }
        }
    }
    true
}

/// Mirror a successful `WriteRecord` (`Some`) or `RemoveRecord` (`None`) in
/// the cached snapshot, the way the host applies it: the entity's record of
/// `app_name` is replaced or dropped, together with any verbatim DWG EED of
/// that application. Returns `false` when the write registered a new
/// application id on the host (the cached APPID table does not know it); the
/// caller then drops the cache.
pub(crate) fn mirror_record(
    doc: &mut Document,
    handle: Handle,
    app_name: &str,
    record: Option<ExtendedDataRecord>,
) -> bool {
    let app_handle = doc.app_ids.get(app_name).map(|app| app.handle.value());
    if record.is_some() && app_handle.is_none() {
        return false;
    }
    let Some(entity) = doc.get_entity_mut(handle) else {
        return true;
    };
    let xdata = &mut entity.common_mut().extended_data;
    let kept: Vec<ExtendedDataRecord> = xdata
        .records()
        .iter()
        .filter(|existing| existing.application_name != app_name)
        .cloned()
        .collect();
    xdata.clear();
    for existing in kept {
        xdata.add_record(existing);
    }
    if let Some(record) = record {
        xdata.add_record(record);
    }
    if let Some(app_handle) = app_handle {
        xdata.raw_dwg_eed.retain(|(app, _)| *app != app_handle);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::Point;

    fn doc_with_point() -> (Document, Handle) {
        let mut doc = Document::new();
        let handle = doc
            .add_entity(EntityType::Point(Point::from_coords(0.0, 0.0, 0.0)))
            .unwrap();
        (doc, handle)
    }

    #[test]
    fn refreshed_entities_replace_and_removed_ones_leave() {
        let (mut doc, handle) = doc_with_point();
        let mut moved = doc.get_entity(handle).unwrap().clone();
        if let EntityType::Point(point) = &mut moved {
            point.location.y = 4.0;
        }
        assert!(apply_refreshed(&mut doc, &[handle], vec![Some(moved)], &[]));
        let EntityType::Point(point) = doc.get_entity(handle).unwrap() else {
            panic!("point");
        };
        assert_eq!(point.location.y, 4.0);
        assert!(apply_refreshed(&mut doc, &[], Vec::new(), &[handle]));
        assert!(doc.get_entity(handle).is_none());
    }

    #[test]
    fn an_entity_on_a_layer_the_cache_does_not_know_is_not_patched() {
        let (mut doc, handle) = doc_with_point();
        let mut moved = doc.get_entity(handle).unwrap().clone();
        moved.common_mut().layer = "CREATED_BY_THE_HOST".to_string();
        assert!(!apply_refreshed(
            &mut doc,
            &[handle],
            vec![Some(moved)],
            &[]
        ));
    }

    #[test]
    fn a_short_batch_answer_is_not_patched() {
        let (mut doc, handle) = doc_with_point();
        assert!(!apply_refreshed(&mut doc, &[handle], Vec::new(), &[]));
    }

    #[test]
    fn only_attribute_edits_need_a_full_refresh() {
        assert!(!edit_needs_full_refresh(&EntityType::Point(Point::new())));
    }
}
