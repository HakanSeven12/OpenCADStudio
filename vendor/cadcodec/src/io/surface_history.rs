//! Preserve the public Surface history link without adding a 3DSOLID-only
//! history slot to native surface records. The root keeps its entity owner.

use super::loft_parameters::{has_record, remove_record, writable_dictionary};
use crate::entities::EntityType;
use crate::objects::{DynamicBlockData, ObjectType, XRecord, XRecordValue};
use crate::types::Handle;
use crate::CadDocument;

const KEY: &str = "CADCODEC_SURFACE_HISTORY_V1";

fn owned_history(document: &CadDocument, surface: Handle, history: Handle) -> Option<Handle> {
    if history.is_null() {
        return None;
    }
    match document.objects.get(&history) {
        Some(ObjectType::DynamicBlock(root))
            if root.owner == surface && matches!(root.data, DynamicBlockData::SolidHistory(_)) =>
        {
            Some(history)
        }
        _ => None,
    }
}

pub(crate) fn has_references(document: &CadDocument) -> bool {
    document.entities().any(|entity| {
        let EntityType::Surface(surface) = entity else {
            return false;
        };
        surface
            .history_handle
            .and_then(|history| owned_history(document, surface.common.handle, history))
            .is_some()
            || has_record(
                document,
                document.extension_dictionary_handle(surface.common.handle),
                KEY,
            )
    })
}

pub(crate) fn store(document: &mut CadDocument) {
    let references = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Surface(surface) = entity else {
                return None;
            };
            let owner = surface.common.handle;
            let previous = document.extension_dictionary_handle(owner);
            let history = surface
                .history_handle
                .and_then(|history| owned_history(document, owner, history));
            (history.is_some() || has_record(document, previous, KEY))
                .then_some((owner, previous, history))
        })
        .collect::<Vec<_>>();
    for (owner, previous, history) in references {
        let mut dictionary = writable_dictionary(document, owner, previous);
        if let Some(entity) = document.get_entity_mut(owner) {
            entity.common_mut().xdictionary_handle = Some(dictionary.handle);
        }
        document.xdic_by_handle.insert(owner, dictionary.handle);
        if let Some(history) = history {
            let record_handle = dictionary
                .get(KEY)
                .filter(|handle| {
                    matches!(document.objects.get(handle), Some(ObjectType::XRecord(record))
                    if record.owner == dictionary.handle)
                })
                .unwrap_or_else(|| document.allocate_handle());
            let mut record = XRecord::named(KEY);
            record.handle = record_handle;
            record.owner = dictionary.handle;
            record.add_int32(90, 1);
            // A real soft pointer participates in document handle remapping;
            // the history root is owned by the surface, not by this XRecord.
            record.add_handle(330, history);
            dictionary
                .entries
                .retain(|(key, _)| !key.eq_ignore_ascii_case(KEY));
            dictionary.add_entry(KEY, record_handle);
            dictionary.set_entry_hard_owner(KEY, true);
            document
                .objects
                .insert(record_handle, ObjectType::XRecord(record));
        } else {
            remove_record(document, &mut dictionary, KEY);
        }
        document
            .objects
            .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
    }
}

pub(crate) fn restore(document: &mut CadDocument) {
    let references = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Surface(surface) = entity else {
                return None;
            };
            // A native link supplied by a reader always has priority over fallback
            // metadata, including a link whose target could not be recovered.
            if surface
                .history_handle
                .is_some_and(|history| !history.is_null())
            {
                return None;
            }
            let owner = surface.common.handle;
            let record = document.xrecord(owner, KEY)?;
            if record.get_first_by_code(90)?.value.as_i32()? != 1 {
                return None;
            }
            let XRecordValue::Handle(history) = &record.get_first_by_code(330)?.value else {
                return None;
            };
            Some((owner, owned_history(document, owner, *history)?))
        })
        .collect::<Vec<_>>();
    for (owner, history) in references {
        if let Some(EntityType::Surface(surface)) = document.get_entity_mut(owner) {
            surface.history_handle = Some(history);
        }
    }
}
