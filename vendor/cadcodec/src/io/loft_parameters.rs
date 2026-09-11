//! Versioned, public XRECORD extension for settings absent from the native
//! solid-history loft stream. Native section and guide records stay intact.

use super::dwg::dwg_version::DwgVersion;
use super::dwg::embedded_entity::{decode_embedded_entity, encode_embedded_entity};
use crate::objects::{
    Dictionary, DynamicBlockData, ObjectType, SolidHistoryLoftParameters, SolidHistoryOperation,
    XRecord, XRecordEntry, XRecordValue,
};
use crate::types::{DxfVersion, Handle};
use crate::CadDocument;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

const KEY: &str = "CADCODEC_LOFT_OPTIONS_V1";

pub(super) fn has_record(document: &CadDocument, dictionary: Option<Handle>, key: &str) -> bool {
    matches!(dictionary.and_then(|handle| document.objects.get(&handle)),
        Some(ObjectType::Dictionary(dictionary)) if dictionary.get(key).is_some())
}

/// Detach owned, understood metadata when copied entities share a dictionary.
/// Foreign records remain reachable as non-owning references; their private
/// contents must not be rewritten or assigned a second owner.
pub(super) fn writable_dictionary(
    document: &mut CadDocument,
    owner: Handle,
    previous: Option<Handle>,
) -> Dictionary {
    let existing = previous.and_then(|handle| match document.objects.get(&handle) {
        Some(ObjectType::Dictionary(dictionary)) => Some(dictionary.clone()),
        _ => None,
    });
    let Some(mut dictionary) = existing else {
        let mut dictionary = Dictionary::new();
        dictionary.handle = document.allocate_handle();
        dictionary.owner = owner;
        return dictionary;
    };
    let shared = document.entities().any(|entity| entity.common().handle != owner
        && entity.common().xdictionary_handle == previous)
        || document.objects.iter().any(|(handle, object)| *handle != owner
            && (matches!(object, ObjectType::DynamicBlock(object) if object.xdictionary_handle == previous)
                || document.extension_dictionary_handle(*handle) == previous))
        || document.xdic_by_handle.iter().any(|(handle, value)| *handle != owner && Some(*value) == previous);
    if dictionary.owner == owner && !dictionary.handle.is_null() && !shared {
        return dictionary;
    }
    let source = dictionary.handle;
    dictionary.xdictionary_handle = document.extension_dictionary_handle(source);
    let target = document.allocate_handle();
    let mut remapped = HashMap::from([(source, target)]);
    dictionary.handle = target;
    dictionary.owner = owner;
    clone_dictionary_children(document, &mut dictionary, source, &mut remapped, 0);
    // References between siblings can only be remapped after every child has
    // been allocated. Source metadata remains untouched in the output copy.
    for handle in remapped
        .values()
        .copied()
        .filter(|handle| *handle != target)
    {
        let Some(object) = document.objects.get_mut(&handle) else {
            continue;
        };
        let remap = |handle: &mut Handle| {
            if let Some(new) = remapped.get(handle) {
                *handle = *new;
            }
        };
        match object {
            ObjectType::Dictionary(value) => {
                for (_, handle) in &mut value.entries {
                    remap(handle);
                }
                for handle in &mut value.reactors {
                    remap(handle);
                }
                if let Some(handle) = value.xdictionary_handle.as_mut() {
                    remap(handle);
                }
            }
            ObjectType::XRecord(value) => {
                for entry in &mut value.entries {
                    if let XRecordValue::Handle(handle) = &mut entry.value {
                        remap(handle);
                    }
                }
                for reference in &mut value.object_references {
                    remap(&mut reference.handle);
                }
                for handle in &mut value.reactors {
                    remap(handle);
                }
                if let Some(handle) = value.xdictionary_handle.as_mut() {
                    remap(handle);
                }
                value.raw_dwg_data = None;
                value.raw_dwg_version = None;
            }
            ObjectType::Field(value) => value.visit_handles_mut(&mut |handle| remap(handle)),
            ObjectType::FieldList(value) => {
                for handle in &mut value.fields {
                    remap(handle);
                }
            }
            _ => {}
        }
    }
    for (source, target) in &remapped {
        if let Some(extension) = document.extension_dictionary_handle(*source) {
            if let Some(extension) = remapped.get(&extension) {
                document.xdic_by_handle.insert(*target, *extension);
            }
        }
    }
    for (_, handle) in &mut dictionary.entries {
        if let Some(new) = remapped.get(handle) {
            *handle = *new;
        }
    }
    for handle in &mut dictionary.reactors {
        if let Some(new) = remapped.get(handle) {
            *handle = *new;
        }
    }
    dictionary
}

fn clone_dictionary_children(
    document: &mut CadDocument,
    dictionary: &mut Dictionary,
    source: Handle,
    remapped: &mut HashMap<Handle, Handle>,
    depth: usize,
) {
    let originally_hard = dictionary.hard_owner;
    dictionary.hard_owner = false;
    for (key, handle) in dictionary.entries.clone() {
        let owned = originally_hard
            || dictionary.is_entry_hard_owner(&key)
            || document.object_owner(handle) == Some(source);
        let cloned = owned
            .then(|| clone_metadata(document, handle, dictionary.handle, remapped, depth + 1))
            .flatten();
        if let Some(cloned) = cloned {
            for (name, value) in &mut dictionary.entries {
                if name == &key {
                    *value = cloned;
                }
            }
        }
        dictionary.set_entry_hard_owner(&key, cloned.is_some());
    }
    if let Some(handle) = dictionary.xdictionary_handle {
        dictionary.xdictionary_handle =
            clone_metadata(document, handle, dictionary.handle, remapped, depth + 1)
                .or(Some(handle));
    }
}

fn clone_metadata(
    document: &mut CadDocument,
    source: Handle,
    owner: Handle,
    remapped: &mut HashMap<Handle, Handle>,
    depth: usize,
) -> Option<Handle> {
    if let Some(handle) = remapped.get(&source) {
        return Some(*handle);
    }
    if !cloneable_metadata(document, source, depth) {
        return None;
    }
    let mut object = document.objects.get(&source)?.clone();
    let extension = document.extension_dictionary_handle(source);
    if !matches!(
        &object,
        ObjectType::Dictionary(_)
            | ObjectType::Field(_)
            | ObjectType::FieldList(_)
            | ObjectType::DictionaryVariable(_)
            | ObjectType::XRecord(XRecord {
                entries_complete: true,
                ..
            })
    ) {
        return None;
    }
    let handle = document.allocate_handle();
    remapped.insert(source, handle);
    object.set_handle(handle);
    match &mut object {
        ObjectType::Dictionary(value) => {
            value.owner = owner;
            value.xdictionary_handle = extension;
            clone_dictionary_children(document, value, source, remapped, depth);
        }
        ObjectType::XRecord(value) => {
            value.owner = owner;
            value.xdictionary_handle = extension;
            for child in value.get_references() {
                if document.object_owner(child) == Some(source) {
                    clone_metadata(document, child, handle, remapped, depth + 1);
                }
            }
            if let Some(child) = value.xdictionary_handle {
                clone_metadata(document, child, handle, remapped, depth + 1);
            }
        }
        ObjectType::Field(value) => {
            value.owner = owner;
            for child in &value.child_fields {
                clone_metadata(document, *child, handle, remapped, depth + 1);
            }
        }
        ObjectType::FieldList(value) => {
            value.owner = owner;
            for child in &value.fields {
                if document.object_owner(*child) == Some(source) {
                    clone_metadata(document, *child, handle, remapped, depth + 1);
                }
            }
        }
        ObjectType::DictionaryVariable(value) => value.owner_handle = owner,
        _ => unreachable!(),
    }
    if let Some(child) = extension {
        clone_metadata(document, child, handle, remapped, depth + 1);
    }
    document.objects.insert(handle, object);
    Some(handle)
}

// A dictionary can retain foreign entries as soft references. Other metadata
// cannot demote an owned child without changing its meaning, so keep that
// entire record as a foreign reference when its owned graph is not understood.
fn cloneable_metadata(document: &CadDocument, source: Handle, depth: usize) -> bool {
    let mut pending = vec![(source, depth)];
    let mut seen = HashSet::new();
    while let Some((handle, depth)) = pending.pop() {
        if !seen.insert(handle) {
            continue;
        }
        if depth > 128 {
            return false;
        }
        if let Some(child) = document.extension_dictionary_handle(handle) {
            pending.push((child, depth + 1));
        }
        match document.objects.get(&handle) {
            Some(ObjectType::Dictionary(value)) => {
                if let Some(child) = value.xdictionary_handle {
                    pending.push((child, depth + 1));
                }
            }
            Some(ObjectType::XRecord(value)) if value.entries_complete => {
                for child in value.get_references() {
                    if document.object_owner(child) == Some(handle) {
                        pending.push((child, depth + 1));
                    }
                }
                if let Some(child) = value.xdictionary_handle {
                    pending.push((child, depth + 1));
                }
            }
            Some(ObjectType::Field(value)) => {
                pending.extend(value.child_fields.iter().map(|child| (*child, depth + 1)));
            }
            Some(ObjectType::FieldList(value)) => {
                pending.extend(
                    value
                        .fields
                        .iter()
                        .filter(|child| document.object_owner(**child) == Some(handle))
                        .map(|child| (*child, depth + 1)),
                );
            }
            Some(ObjectType::DictionaryVariable(_)) => {}
            _ => return false,
        }
    }
    true
}

pub(super) fn remove_record(document: &mut CadDocument, dictionary: &mut Dictionary, key: &str) {
    let old = dictionary.get(key);
    dictionary
        .entries
        .retain(|(name, _)| !name.eq_ignore_ascii_case(key));
    dictionary.set_entry_hard_owner(key, false);
    if let Some(handle) = old {
        if !dictionary.entries.iter().any(|(_, value)| *value == handle)
            && matches!(document.objects.get(&handle), Some(ObjectType::XRecord(record)) if record.owner == dictionary.handle)
            && !document.objects.iter().any(|(handle, object)| *handle != dictionary.handle
                && matches!(object, ObjectType::Dictionary(other) if other.entries.iter().any(|(_, value)| Some(*value) == old))) {
            document.objects.remove(&handle);
        }
    }
}

/// Work only on an output copy. Saving must not add history/undo edits to the
/// live document, and copied history nodes must not share mutable settings.
pub(crate) fn prepared(document: &CadDocument) -> Cow<'_, CadDocument> {
    let settings = document
        .objects
        .iter()
        .filter_map(|(handle, object)| {
            let ObjectType::DynamicBlock(object) = object else {
                return None;
            };
            let DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Loft(loft)) =
                &object.data
            else {
                return None;
            };
            (loft.parameters.is_some() || has_record(document, object.xdictionary_handle, KEY))
                .then(|| (*handle, loft.parameters.clone()))
        })
        .collect::<Vec<_>>();
    if settings.is_empty()
        && !super::loft_surface_curves::has_inputs(document)
        && !super::surface_history::has_references(document)
    {
        return Cow::Borrowed(document);
    }
    let mut output = document.clone();
    for (node, settings) in settings {
        let previous = match output.objects.get(&node) {
            Some(ObjectType::DynamicBlock(object)) => object.xdictionary_handle,
            _ => None,
        };
        let mut dictionary = writable_dictionary(&mut output, node, previous);
        if let Some(ObjectType::DynamicBlock(object)) = output.objects.get_mut(&node) {
            object.xdictionary_handle = Some(dictionary.handle);
        }
        output.xdic_by_handle.insert(node, dictionary.handle);
        let Some(settings) = settings else {
            remove_record(&mut output, &mut dictionary, KEY);
            output
                .objects
                .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
            continue;
        };
        let old_record = dictionary.get(KEY);
        let record_handle = old_record
            .filter(|handle| {
                matches!(output.objects.get(handle),
            Some(ObjectType::XRecord(record)) if record.owner == dictionary.handle)
            })
            .unwrap_or_else(|| output.allocate_handle());
        let mut record = encode(&settings);
        record.handle = record_handle;
        record.owner = dictionary.handle;
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(KEY));
        dictionary.add_entry(KEY, record_handle);
        dictionary.set_entry_hard_owner(KEY, true);
        if let Some(ObjectType::DynamicBlock(object)) = output.objects.get_mut(&node) {
            object.xdictionary_handle = Some(dictionary.handle);
        }
        output
            .objects
            .insert(record_handle, ObjectType::XRecord(record));
        output
            .objects
            .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
    }
    super::loft_surface_curves::store(&mut output);
    super::surface_history::store(&mut output);
    Cow::Owned(output)
}

pub(crate) fn restore(document: &mut CadDocument) {
    super::loft_surface_curves::restore(document);
    super::surface_history::restore(document);
    let restored = document
        .objects
        .iter()
        .filter_map(|(handle, object)| {
            let ObjectType::DynamicBlock(object) = object else {
                return None;
            };
            if !matches!(
                object.data,
                DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Loft(_))
            ) {
                return None;
            }
            let ObjectType::Dictionary(dictionary) =
                document.objects.get(&object.xdictionary_handle?)?
            else {
                return None;
            };
            let record_handle = dictionary.get(KEY)?;
            let ObjectType::XRecord(record) = document.objects.get(&record_handle)? else {
                return None;
            };
            Some((*handle, decode(record)?))
        })
        .collect::<Vec<_>>();
    for (handle, settings) in restored {
        if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) {
            if let DynamicBlockData::SolidHistoryNode(SolidHistoryOperation::Loft(loft)) =
                &mut object.data
            {
                loft.parameters = Some(settings);
            }
        }
    }
}

fn encode(value: &SolidHistoryLoftParameters) -> XRecord {
    let mut record = XRecord::named(KEY);
    record.add_int32(90, 1);
    record.add_int16(70, value.normals as i16);
    record.add_double(40, value.start_draft_angle);
    record.add_double(41, value.end_draft_angle);
    record.add_double(42, value.start_magnitude);
    record.add_double(43, value.end_magnitude);
    record.add_int16(71, value.start_continuity as i16);
    record.add_int16(72, value.end_continuity as i16);
    record.add_double(44, value.start_bulge);
    record.add_double(45, value.end_bulge);
    record.add_bool(290, value.closed);
    record.add_bool(291, value.surface);
    record.add_bool(292, value.align_direction);
    record.add_bool(293, value.periodic);
    for count in &value.section_counts {
        record.add_int32(91, *count as i32);
    }
    if let Some(entity) = &value.path_entity {
        let version = DwgVersion::from_dxf_version(DxfVersion::AC1032).expect("supported version");
        let encoded = encode_embedded_entity(entity, version, DxfVersion::AC1032);
        record.add_int32(92, encoded.type_code);
        record.add_int32(93, encoded.bit_length as i32);
        for bytes in encoded.bytes.chunks(127) {
            record.add_entry(XRecordEntry::new(310, XRecordValue::Chunk(bytes.to_vec())));
        }
    }
    record
}

fn decode(record: &XRecord) -> Option<SolidHistoryLoftParameters> {
    let entry = |code| {
        record
            .entries
            .iter()
            .find(|entry| entry.code == code)
            .map(|entry| &entry.value)
    };
    if entry(90)?.as_i32()? != 1 {
        return None;
    }
    let mut value = SolidHistoryLoftParameters {
        normals: entry(70)?.as_i32()?,
        start_draft_angle: entry(40)?.as_double()?,
        end_draft_angle: entry(41)?.as_double()?,
        start_magnitude: entry(42)?.as_double()?,
        end_magnitude: entry(43)?.as_double()?,
        start_continuity: entry(71).map(XRecordValue::as_i32).unwrap_or(Some(1))?,
        end_continuity: entry(72).map(XRecordValue::as_i32).unwrap_or(Some(1))?,
        start_bulge: entry(44)
            .map(XRecordValue::as_double)
            .unwrap_or(Some(0.5))?,
        end_bulge: entry(45)
            .map(XRecordValue::as_double)
            .unwrap_or(Some(0.5))?,
        closed: boolean(entry(290)?)?,
        surface: boolean(entry(291)?)?,
        align_direction: boolean(entry(292)?)?,
        periodic: entry(293).map(boolean).unwrap_or(Some(true))?,
        ..SolidHistoryLoftParameters::default()
    };
    if !(0..=6).contains(&value.normals)
        || !(0..=1).contains(&value.start_continuity)
        || !(0..=1).contains(&value.end_continuity)
        || [
            value.start_draft_angle,
            value.end_draft_angle,
            value.start_magnitude,
            value.end_magnitude,
            value.start_bulge,
            value.end_bulge,
        ]
        .iter()
        .any(|number| !number.is_finite())
        || value.start_magnitude < 0.0
        || value.end_magnitude < 0.0
        || value.start_bulge < 0.0
        || value.end_bulge < 0.0
    {
        return None;
    }
    for item in record.entries.iter().filter(|item| item.code == 91) {
        let count = usize::try_from(item.value.as_i32()?).ok()?;
        if count == 0 || count > 100_000 || value.section_counts.len() >= 100_000 {
            return None;
        }
        value.section_counts.push(count);
    }
    if let Some(entity_type) = entry(92) {
        let bit_count = usize::try_from(entry(93)?.as_i32()?).ok()?;
        if bit_count == 0 || bit_count > 64 * 1024 * 1024 {
            return None;
        }
        let mut bytes = Vec::new();
        for item in record.entries.iter().filter(|item| item.code == 310) {
            let XRecordValue::Chunk(chunk) = &item.value else {
                return None;
            };
            if bytes.len() + chunk.len() > bit_count.div_ceil(8) {
                return None;
            }
            bytes.extend_from_slice(chunk);
        }
        if bytes.len() != bit_count.div_ceil(8) {
            return None;
        }
        let version = DwgVersion::from_dxf_version(DxfVersion::AC1032).ok()?;
        value.path_entity = Some(decode_embedded_entity(
            entity_type.as_i32()?,
            bit_count,
            bytes,
            version,
            DxfVersion::AC1032,
        )?);
    }
    Some(value)
}

fn boolean(value: &XRecordValue) -> Option<bool> {
    match value {
        XRecordValue::Bool(value) => Some(*value),
        XRecordValue::Byte(value) if *value <= 1 => Some(*value == 1),
        _ => value
            .as_i32()
            .filter(|value| (0..=1).contains(value))
            .map(|value| value == 1),
    }
}
