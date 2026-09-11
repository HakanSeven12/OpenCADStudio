//! Preserve embedded loft inputs when the target DWG version stores only
//! database references in its native surface record.

use super::dwg::dwg_version::DwgVersion;
use super::dwg::embedded_entity::{decode_embedded_entity, encode_embedded_entity};
use super::loft_parameters::{has_record, remove_record, writable_dictionary};
use crate::entities::{EmbeddedEntity, SurfaceData};
use crate::objects::{ObjectType, XRecord, XRecordEntry, XRecordValue};
use crate::types::DxfVersion;
use crate::{CadDocument, EntityType};

const KEY: &str = "CADCODEC_LOFT_CURVES_V1";

pub(crate) fn has_inputs(document: &CadDocument) -> bool {
    document.entities().any(|entity| matches!(entity, EntityType::Surface(surface)
        if matches!(&surface.surface_data, SurfaceData::Lofted {
            cross_section_entities, guide_entities, path_entity, ..
        } if !cross_section_entities.is_empty() || !guide_entities.is_empty() || path_entity.is_some()
            || has_record(document, surface.common.xdictionary_handle, KEY))))
}

pub(crate) fn store(document: &mut CadDocument) {
    let inputs = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Surface(surface) = entity else {
                return None;
            };
            let SurfaceData::Lofted {
                cross_section_entities,
                guide_entities,
                path_entity,
                ..
            } = &surface.surface_data
            else {
                return None;
            };
            if cross_section_entities.is_empty()
                && guide_entities.is_empty()
                && path_entity.is_none()
                && !has_record(document, surface.common.xdictionary_handle, KEY)
            {
                return None;
            }
            Some((
                surface.common.handle,
                surface.common.xdictionary_handle,
                cross_section_entities.clone(),
                guide_entities.clone(),
                path_entity.clone(),
            ))
        })
        .collect::<Vec<_>>();
    for (handle, previous, sections, guides, path) in inputs {
        let mut dictionary = writable_dictionary(document, handle, previous);
        if let Some(entity) = document.get_entity_mut(handle) {
            entity.common_mut().xdictionary_handle = Some(dictionary.handle);
        }
        document.xdic_by_handle.insert(handle, dictionary.handle);
        if sections.is_empty() && guides.is_empty() && path.is_none() {
            remove_record(document, &mut dictionary, KEY);
            document
                .objects
                .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
            continue;
        }
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
        for (role, entity) in sections
            .iter()
            .map(|entity| (0, entity))
            .chain(guides.iter().map(|entity| (1, entity)))
            .chain(path.iter().map(|entity| (2, entity)))
        {
            let version =
                DwgVersion::from_dxf_version(DxfVersion::AC1032).expect("supported version");
            let body = encode_embedded_entity(entity, version, DxfVersion::AC1032);
            record.add_int16(70, role);
            record.add_int32(91, body.type_code);
            record.add_int32(92, body.bit_length as i32);
            for bytes in body.bytes.chunks(127) {
                record.add_entry(XRecordEntry::new(310, XRecordValue::Chunk(bytes.to_vec())));
            }
        }
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(KEY));
        dictionary.add_entry(KEY, record_handle);
        dictionary.set_entry_hard_owner(KEY, true);
        document
            .objects
            .insert(record_handle, ObjectType::XRecord(record));
        document
            .objects
            .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
    }
}

pub(crate) fn restore(document: &mut CadDocument) {
    let inputs = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Surface(surface) = entity else {
                return None;
            };
            let SurfaceData::Lofted {
                cross_section_entities,
                guide_entities,
                path_entity,
                ..
            } = &surface.surface_data
            else {
                return None;
            };
            // The native input group is authoritative when present. Mixing a stale
            // fallback with a modified native group can resurrect removed guides.
            if !cross_section_entities.is_empty()
                || !guide_entities.is_empty()
                || path_entity.is_some()
            {
                return None;
            }
            let ObjectType::Dictionary(dictionary) =
                document.objects.get(&surface.common.xdictionary_handle?)?
            else {
                return None;
            };
            let handle = dictionary.get(KEY)?;
            let ObjectType::XRecord(record) = document.objects.get(&handle)? else {
                return None;
            };
            Some((surface.common.handle, decode(record)?))
        })
        .collect::<Vec<_>>();
    for (handle, (sections, guides, path)) in inputs {
        if let Some(EntityType::Surface(surface)) = document.get_entity_mut(handle) {
            if let SurfaceData::Lofted {
                cross_section_entities,
                guide_entities,
                path_entity,
                ..
            } = &mut surface.surface_data
            {
                *cross_section_entities = sections;
                *guide_entities = guides;
                *path_entity = path;
            }
        }
    }
}

type Inputs = (
    Vec<EmbeddedEntity>,
    Vec<EmbeddedEntity>,
    Option<EmbeddedEntity>,
);
fn decode(record: &XRecord) -> Option<Inputs> {
    let entries = &record.entries;
    if entries.first()?.code != 90 || entries[0].value.as_i32()? != 1 {
        return None;
    }
    let mut result = (Vec::new(), Vec::new(), None);
    let mut index = 1;
    while index < entries.len() {
        if entries.get(index)?.code != 70
            || entries.get(index + 1)?.code != 91
            || entries.get(index + 2)?.code != 92
        {
            return None;
        }
        let role = entries[index].value.as_i32()?;
        let entity_type = entries[index + 1].value.as_i32()?;
        let bits = usize::try_from(entries[index + 2].value.as_i32()?).ok()?;
        if bits == 0 || bits > 64 * 1024 * 1024 {
            return None;
        }
        index += 3;
        let mut bytes = Vec::new();
        while entries.get(index).is_some_and(|entry| entry.code == 310) {
            let XRecordValue::Chunk(chunk) = &entries[index].value else {
                return None;
            };
            if bytes.len() + chunk.len() > bits.div_ceil(8) {
                return None;
            }
            bytes.extend_from_slice(chunk);
            index += 1;
        }
        if bytes.len() != bits.div_ceil(8) {
            return None;
        }
        let version = DwgVersion::from_dxf_version(DxfVersion::AC1032).ok()?;
        let entity = decode_embedded_entity(entity_type, bits, bytes, version, DxfVersion::AC1032)?;
        match role {
            0 => result.0.push(entity),
            1 => result.1.push(entity),
            2 if result.2.is_none() => result.2 = Some(entity),
            _ => return None,
        }
    }
    Some(result)
}
