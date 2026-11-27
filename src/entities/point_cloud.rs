//! The point cloud definition an entity names, and the files it points at.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use document::Document;
use document::entities::PointCloudExData;
use document::objects::{ClassObjectData, ObjectType, PointCloudDefinition};
use rustc_hash::FxHashMap;

/// The point cloud definition a PointCloudEx names.
pub(crate) fn definition<'a>(
    document: &'a Document,
    data: &PointCloudExData,
) -> Option<&'a PointCloudDefinition> {
    match document.objects.get(&data.definition_handle)? {
        ObjectType::ClassObject(object) => match &object.data {
            ClassObjectData::PointCloudDefinitionEx(definition)
            | ClassObjectData::PointCloudDefinition(definition) => Some(definition),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) type Sources = Mutex<FxHashMap<String, PathBuf>>;

pub(crate) fn sources() -> &'static Sources {
    static SOURCES: OnceLock<Sources> = OnceLock::new();
    SOURCES.get_or_init(Default::default)
}

/// The scan file a PointCloudEx shows, when it exists: the stored path as
/// is, else where it was found next to the drawing, else relative to the
/// working folder.
pub(crate) fn resolve_source(document: &Document, data: &PointCloudExData) -> Option<PathBuf> {
    // An unloaded cloud draws as a missing one: its box and saved path.
    if definition(document, data).is_some_and(|def| !def.is_loaded) {
        return None;
    }
    let stored = definition(document, data)?.source_filename.trim();
    if stored.is_empty() {
        return None;
    }
    let path = PathBuf::from(stored.replace('\\', "/"));
    if path.is_absolute() && path.is_file() {
        return Some(path);
    }
    if let Some(found) = sources()
        .lock()
        .ok()
        .and_then(|map| map.get(stored).cloned())
    {
        if found.is_file() {
            return Some(found);
        }
    }
    path.is_file().then_some(path)
}
