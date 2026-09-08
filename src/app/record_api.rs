use acadrust::tables::{Table, TableEntry};
use iced::Task;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use super::{Message, OpenCADStudio};

const COLLECTIONS: &[(&str, bool)] = &[
    ("entities", true),
    ("objects", true),
    ("layers", true),
    ("line_types", true),
    ("text_styles", true),
    ("block_records", true),
    ("dim_styles", true),
    ("app_ids", true),
    ("views", true),
    ("vports", true),
    ("ucss", true),
    ("vx_table", true),
    ("header", true),
    ("summary_info", true),
    ("classes", false),
    ("block_visibility", false),
    ("context_scales", false),
    ("block_representations", false),
    ("fields", false),
    ("dgn_line_style_definitions", false),
    ("dgn_line_style_components", false),
    ("vx_control_entries", false),
    ("section_view_style", false),
    ("view_rep_references", false),
    ("section_view_representations", false),
    ("notifications", false),
    ("preview", false),
    ("document", false),
];

fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":message.into()})
}

fn json_value(value: &impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn enum_parts(value: &impl Serialize) -> Result<(String, Value), String> {
    let Value::Object(wrapper) = json_value(value)? else {
        return Err("record type is not represented by an object".into());
    };
    if wrapper.len() != 1 {
        return Err("record type has an invalid representation".into());
    }
    Ok(wrapper.into_iter().next().expect("one entry"))
}

fn decode_enum<T: DeserializeOwned>(kind: &str, properties: Value) -> Result<T, String> {
    let mut wrapper = Map::new();
    wrapper.insert(kind.to_string(), properties);
    serde_json::from_value(Value::Object(wrapper)).map_err(|error| error.to_string())
}

fn handle_text(handle: acadrust::Handle) -> String {
    format!("{:X}", handle.value())
}

fn without_hex_prefix(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

fn record(
    collection: &str,
    kind: &str,
    handle: Option<acadrust::Handle>,
    name: Option<&str>,
    mutable: bool,
    properties: Value,
) -> Value {
    let mut value = json!({
        "collection":collection,
        "type":kind,
        "mutable":mutable,
        "properties":properties,
    });
    let object = value.as_object_mut().expect("record object");
    if let Some(handle) = handle {
        object.insert("handle".into(), json!(handle_text(handle)));
    }
    if let Some(name) = name {
        object.insert("name".into(), json!(name));
    }
    value
}

fn entity_record(entity: &acadrust::EntityType) -> Result<Value, String> {
    let (kind, properties) = enum_parts(entity)?;
    let mut value = record(
        "entities",
        &kind,
        Some(entity.common().handle),
        None,
        true,
        properties,
    );
    let object = value.as_object_mut().expect("record object");
    object.insert(
        "display_type".into(),
        json!(crate::entities::names::ui_name(entity)),
    );
    object.insert(
        "record_type".into(),
        json!(crate::entities::names::dxf_name(entity)),
    );
    Ok(value)
}

fn object_record(
    handle: acadrust::Handle,
    object: &acadrust::objects::ObjectType,
) -> Result<Value, String> {
    let (kind, properties) = enum_parts(object)?;
    Ok(record(
        "objects",
        &kind,
        Some(handle),
        None,
        true,
        properties,
    ))
}

fn table_records<T: Serialize + TableEntry>(
    collection: &str,
    kind: &str,
    table: &Table<T>,
) -> Result<Vec<Value>, String> {
    table
        .iter()
        .map(|entry| {
            Ok(record(
                collection,
                kind,
                Some(entry.handle()),
                Some(entry.name()),
                true,
                json_value(entry)?,
            ))
        })
        .collect()
}

fn map_records<T: Serialize>(
    collection: &str,
    kind: &str,
    values: &std::collections::HashMap<acadrust::Handle, T>,
) -> Result<Vec<Value>, String> {
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_by_key(|(handle, _)| handle.value());
    entries
        .into_iter()
        .map(|(handle, value)| {
            Ok(record(
                collection,
                kind,
                Some(*handle),
                None,
                false,
                json_value(value)?,
            ))
        })
        .collect()
}

fn collection_records(
    document: &acadrust::CadDocument,
    collection: &str,
) -> Result<Vec<Value>, String> {
    match collection {
        "entities" => document.entities().map(entity_record).collect(),
        "objects" => {
            let mut objects: Vec<_> = document.objects.iter().collect();
            objects.sort_by_key(|(handle, _)| handle.value());
            objects
                .into_iter()
                .map(|(handle, object)| object_record(*handle, object))
                .collect()
        }
        "layers" => table_records("layers", "Layer", &document.layers),
        "line_types" => table_records("line_types", "LineType", &document.line_types),
        "text_styles" => table_records("text_styles", "TextStyle", &document.text_styles),
        "block_records" => table_records("block_records", "BlockRecord", &document.block_records),
        "dim_styles" => table_records("dim_styles", "DimStyle", &document.dim_styles),
        "app_ids" => table_records("app_ids", "AppId", &document.app_ids),
        "views" => table_records("views", "View", &document.views),
        "vports" => table_records("vports", "VPort", &document.vports),
        "ucss" => table_records("ucss", "Ucs", &document.ucss),
        "vx_table" => table_records("vx_table", "VxTableRecord", &document.vx_table),
        "header" => Ok(vec![record(
            "header",
            "HeaderVariables",
            None,
            Some("header"),
            true,
            json_value(&document.header)?,
        )]),
        "summary_info" => Ok(vec![record(
            "summary_info",
            "SummaryInfo",
            None,
            Some("summary_info"),
            true,
            json_value(&document.summary_info)?,
        )]),
        "classes" => document
            .classes
            .iter()
            .map(|value| {
                Ok(record(
                    "classes",
                    "Class",
                    None,
                    Some(&value.dxf_name),
                    false,
                    json_value(value)?,
                ))
            })
            .collect(),
        "block_visibility" => map_records(
            "block_visibility",
            "BlockVisibilityParameter",
            &document.block_visibility_params,
        ),
        "context_scales" => map_records("context_scales", "Handle", &document.context_scales),
        "block_representations" => map_records(
            "block_representations",
            "Handle",
            &document.block_representations,
        ),
        "fields" => map_records("fields", "Field", &document.fields),
        "dgn_line_style_definitions" => map_records(
            "dgn_line_style_definitions",
            "LineStyleDefinition",
            &document.dgn_ls_definitions,
        ),
        "dgn_line_style_components" => map_records(
            "dgn_line_style_components",
            "LineStyleComponent",
            &document.dgn_ls_components,
        ),
        "vx_control_entries" => Ok(document
            .vx_control_entries
            .iter()
            .map(|handle| {
                record(
                    "vx_control_entries",
                    "Handle",
                    Some(*handle),
                    None,
                    false,
                    json!({}),
                )
            })
            .collect()),
        "section_view_style" => Ok(document
            .section_view_style
            .as_ref()
            .map(|value| {
                record(
                    "section_view_style",
                    "SectionViewStyle",
                    None,
                    Some("section_view_style"),
                    false,
                    json_value(value).unwrap_or(Value::Null),
                )
            })
            .into_iter()
            .collect()),
        "view_rep_references" => map_records(
            "view_rep_references",
            "HandleReferences",
            &document.view_rep_refs,
        ),
        "section_view_representations" => Ok(document
            .section_view_reps
            .iter()
            .map(|handle| {
                record(
                    "section_view_representations",
                    "Handle",
                    Some(*handle),
                    None,
                    false,
                    json!({}),
                )
            })
            .collect()),
        "notifications" => Ok(vec![record(
            "notifications",
            "Notifications",
            None,
            Some("notifications"),
            false,
            json_value(&document.notifications)?,
        )]),
        "preview" => Ok(document
            .preview
            .as_ref()
            .map(|value| {
                record(
                    "preview",
                    "Preview",
                    None,
                    Some("preview"),
                    false,
                    json_value(value).unwrap_or(Value::Null),
                )
            })
            .into_iter()
            .collect()),
        "document" => Ok(vec![record(
            "document",
            "Document",
            None,
            Some("document"),
            false,
            json!({
                "version":document.version,
                "maintenance_version":document.maintenance_version,
                "source_path":document.source_path,
                "source_version":document.dwg_source_version,
            }),
        )]),
        _ => Err(format!("unknown record collection: {collection}")),
    }
}

fn collection_count(document: &acadrust::CadDocument, collection: &str) -> usize {
    match collection {
        "entities" => document.entities().count(),
        "objects" => document.objects.len(),
        "layers" => document.layers.len(),
        "line_types" => document.line_types.len(),
        "text_styles" => document.text_styles.len(),
        "block_records" => document.block_records.len(),
        "dim_styles" => document.dim_styles.len(),
        "app_ids" => document.app_ids.len(),
        "views" => document.views.len(),
        "vports" => document.vports.len(),
        "ucss" => document.ucss.len(),
        "vx_table" => document.vx_table.len(),
        "classes" => document.classes.len(),
        "block_visibility" => document.block_visibility_params.len(),
        "context_scales" => document.context_scales.len(),
        "block_representations" => document.block_representations.len(),
        "fields" => document.fields.len(),
        "dgn_line_style_definitions" => document.dgn_ls_definitions.len(),
        "dgn_line_style_components" => document.dgn_ls_components.len(),
        "vx_control_entries" => document.vx_control_entries.len(),
        "section_view_style" => usize::from(document.section_view_style.is_some()),
        "view_rep_references" => document.view_rep_refs.len(),
        "section_view_representations" => document.section_view_reps.len(),
        "preview" => usize::from(document.preview.is_some()),
        _ => 1,
    }
}

fn compare(
    actual: Option<&Value>,
    operator: &str,
    expected: Option<&Value>,
) -> Result<bool, String> {
    let exists = actual.is_some();
    if operator == "exists" {
        return Ok(exists);
    }
    if operator == "not_exists" {
        return Ok(!exists);
    }
    let Some(actual) = actual else {
        return Ok(false);
    };
    let expected = expected.ok_or_else(|| format!("filter operator {operator} requires value"))?;
    Ok(match operator {
        "eq" => actual == expected,
        "ne" => actual != expected,
        "lt" | "lte" | "gt" | "gte" => {
            let left = actual
                .as_f64()
                .ok_or_else(|| format!("filter operator {operator} requires numeric values"))?;
            let right = expected
                .as_f64()
                .ok_or_else(|| format!("filter operator {operator} requires numeric values"))?;
            match operator {
                "lt" => left < right,
                "lte" => left <= right,
                "gt" => left > right,
                _ => left >= right,
            }
        }
        "contains" => match (actual, expected) {
            (Value::String(left), Value::String(right)) => left.contains(right),
            (Value::Array(values), expected) => values.contains(expected),
            _ => return Err("contains requires a string/string or array/value pair".into()),
        },
        "starts_with" => actual
            .as_str()
            .zip(expected.as_str())
            .is_some_and(|(left, right)| left.starts_with(right)),
        "ends_with" => actual
            .as_str()
            .zip(expected.as_str())
            .is_some_and(|(left, right)| left.ends_with(right)),
        "in" => expected
            .as_array()
            .is_some_and(|values| values.contains(actual)),
        _ => return Err(format!("unknown filter operator: {operator}")),
    })
}

fn matches_filters(record: &Value, request: &Value) -> Result<bool, String> {
    if request["handle"].as_str().is_some_and(|requested| {
        !record["handle"]
            .as_str()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(without_hex_prefix(requested)))
    }) {
        return Ok(false);
    }
    if let Some(handles) = request["handles"].as_array() {
        let Some(actual) = record["handle"].as_str() else {
            return Ok(false);
        };
        if !handles
            .iter()
            .filter_map(Value::as_str)
            .any(|requested| actual.eq_ignore_ascii_case(without_hex_prefix(requested)))
        {
            return Ok(false);
        }
    }
    if request["type"].as_str().is_some_and(|requested| {
        !["type", "display_type", "record_type"].iter().any(|key| {
            record[*key]
                .as_str()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(requested))
        })
    }) {
        return Ok(false);
    }
    if request["name"].as_str().is_some_and(|requested| {
        !record["name"]
            .as_str()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(requested))
    }) {
        return Ok(false);
    }
    if let Some(filters) = request["where"].as_array() {
        for filter in filters {
            let path = filter["path"]
                .as_str()
                .ok_or_else(|| "each filter requires a JSON Pointer path".to_string())?;
            if !path.is_empty() && !path.starts_with('/') {
                return Err(format!("invalid JSON Pointer: {path}"));
            }
            let actual = if path.is_empty() {
                Some(&record["properties"])
            } else {
                record["properties"].pointer(path)
            };
            let operator = filter["op"].as_str().unwrap_or("eq");
            if !compare(actual, operator, filter.get("value"))? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn project_paths(mut record: Value, paths: Option<&Vec<Value>>) -> Result<Value, String> {
    let Some(paths) = paths else {
        return Ok(record);
    };
    let mut projected = Map::new();
    for path in paths {
        let path = path
            .as_str()
            .ok_or_else(|| "paths must contain JSON Pointer strings".to_string())?;
        if !path.is_empty() && !path.starts_with('/') {
            return Err(format!("invalid JSON Pointer: {path}"));
        }
        let value = if path.is_empty() {
            Some(&record["properties"])
        } else {
            record["properties"].pointer(path)
        };
        projected.insert(path.to_string(), value.cloned().unwrap_or(Value::Null));
    }
    let object = record.as_object_mut().expect("record object");
    object.remove("properties");
    object.insert("values".into(), Value::Object(projected));
    Ok(record)
}

fn requested_handle(request: &Value) -> Result<acadrust::Handle, String> {
    request["handle"]
        .as_str()
        .and_then(|value| u64::from_str_radix(without_hex_prefix(value), 16).ok())
        .map(acadrust::Handle::new)
        .ok_or_else(|| "set_properties requires a hexadecimal handle".to_string())
}

fn apply_updates(
    properties: &mut Value,
    request: &Value,
    collection: &str,
) -> Result<Vec<String>, String> {
    let updates = request["updates"]
        .as_array()
        .ok_or_else(|| "set_properties requires a non-empty updates array".to_string())?;
    if updates.is_empty() {
        return Err("set_properties requires a non-empty updates array".into());
    }
    let mut paths = Vec::with_capacity(updates.len());
    for update in updates {
        let path = update["path"]
            .as_str()
            .ok_or_else(|| "each update requires a JSON Pointer path".to_string())?;
        if path.is_empty() || !path.starts_with('/') {
            return Err("updates require a non-empty JSON Pointer path".into());
        }
        let identity = match collection {
            "entities" => matches!(path, "/common/handle" | "/common/owner_handle"),
            "objects" => matches!(path, "/handle" | "/common/handle"),
            "header" => path == "/handle_seed",
            "summary_info" => false,
            _ => matches!(path, "/handle" | "/name"),
        };
        if identity {
            return Err(format!("{path} is read-only identity data"));
        }
        let value = update
            .get("value")
            .ok_or_else(|| format!("update {path} requires value"))?
            .clone();
        let target = properties
            .pointer_mut(path)
            .ok_or_else(|| format!("property does not exist: {path}"))?;
        if let Some(expected) = update.get("expected") {
            if target != expected {
                return Err(format!("property changed before update: {path}"));
            }
        }
        *target = value;
        paths.push(path.to_string());
    }
    Ok(paths)
}

fn patched_table_entry<T>(
    table: &Table<T>,
    request: &Value,
    collection: &str,
) -> Result<(T, T, Vec<String>), String>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + TableEntry,
{
    let requested = request["handle"].as_str().and_then(|value| {
        u64::from_str_radix(without_hex_prefix(value), 16)
            .ok()
            .map(acadrust::Handle::new)
    });
    let name = request["name"].as_str();
    let source = table
        .iter()
        .find(|entry| {
            requested.is_some_and(|handle| entry.handle() == handle)
                || name.is_some_and(|name| entry.name().eq_ignore_ascii_case(name))
        })
        .cloned()
        .ok_or_else(|| format!("record does not exist in {collection}"))?;
    let mut properties = json_value(&source)?;
    let paths = apply_updates(&mut properties, request, collection)?;
    let edited: T = serde_json::from_value(properties).map_err(|error| error.to_string())?;
    if edited.handle() != source.handle() || edited.name() != source.name() {
        return Err("record identity is read-only".into());
    }
    Ok((source, edited, paths))
}

fn replace_table_entry<T: TableEntry>(table: &mut Table<T>, handle: acadrust::Handle, edited: T) {
    let target = table
        .iter_mut()
        .find(|entry| entry.handle() == handle)
        .expect("validated table entry");
    *target = edited;
}

impl OpenCADStudio {
    pub(super) fn record_capabilities(&self) -> Value {
        let document = &self.tabs[self.active_tab].scene.document;
        json!({
            "ok":true,
            "api":"ocs-cad-automation",
            "version":1,
            "concurrency":{"document_id":true,"revision":true,"request_id":true},
            "transactions":{"undo":true,"redo":true,"atomic_property_updates":true,"batch":true},
            "geometry":{"query":true,"kernel_measurements":true,"spatial_filters":true,"interactive_commands":true},
            "records":{
                "read":"records",
                "write":"set_properties",
                "path":"RFC 6901 JSON Pointer relative to properties",
                "filters":["eq","ne","lt","lte","gt","gte","contains","starts_with","ends_with","in","exists","not_exists"],
                "collections":COLLECTIONS.iter().map(|(name, mutable)|json!({
                    "name":name,
                    "mutable":mutable,
                    "count":collection_count(document, name),
                })).collect::<Vec<_>>()
            },
            "editor":{"selection":true,"properties":true,"commands":true,"files":true,"capture":true,"events":true}
        })
    }

    pub(super) fn record_query(&self, request: &Value) -> Value {
        let document = &self.tabs[self.active_tab].scene.document;
        let Some(collection) = request["collection"].as_str() else {
            return self.record_capabilities();
        };
        let mut records = Vec::new();
        if collection == "all" {
            for (name, _) in COLLECTIONS {
                match collection_records(document, name) {
                    Ok(mut values) => records.append(&mut values),
                    Err(error) => return failure("serialization_failed", error),
                }
            }
        } else {
            match collection_records(document, collection) {
                Ok(values) => records = values,
                Err(error) => return failure("unknown_collection", error),
            }
        }
        let mut matched = Vec::new();
        for record in records {
            match matches_filters(&record, request) {
                Ok(true) => matched.push(record),
                Ok(false) => {}
                Err(error) => return failure("invalid_filter", error),
            }
        }
        let count = matched.len();
        let offset = request["offset"].as_u64().unwrap_or(0) as usize;
        let limit = request["limit"].as_u64().unwrap_or(1000).min(10_000) as usize;
        let mut returned = Vec::new();
        for record in matched.into_iter().skip(offset).take(limit) {
            match project_paths(record, request["paths"].as_array()) {
                Ok(record) => returned.push(record),
                Err(error) => return failure("invalid_projection", error),
            }
        }
        json!({
            "ok":true,
            "document_id":self.tabs[self.active_tab].id,
            "revision":self.tabs[self.active_tab].edit_revision,
            "geometry_revision":self.tabs[self.active_tab].scene.geometry_epoch,
            "collection":collection,
            "count":count,
            "returned":returned.len(),
            "next_offset":(offset + returned.len() < count).then_some(offset + returned.len()),
            "records":returned,
        })
    }

    pub(super) fn control_set_record_properties(
        &mut self,
        request: &Value,
    ) -> Result<Task<Message>, Value> {
        let collection = request["collection"]
            .as_str()
            .ok_or_else(|| failure("collection_required", "set_properties requires collection"))?;
        if !COLLECTIONS
            .iter()
            .any(|(name, mutable)| *name == collection && *mutable)
        {
            return Err(failure(
                "read_only_collection",
                format!("collection is absent or read-only: {collection}"),
            ));
        }
        let i = self.active_tab;
        let changed;
        let mut result_handle = None;
        let mut result_name = None;
        let paths;
        match collection {
            "entities" => {
                let handle =
                    requested_handle(request).map_err(|error| failure("invalid_handle", error))?;
                if self.tabs[i].scene.is_layer_locked(handle) {
                    return Err(failure("layer_locked", "entity is on a locked layer"));
                }
                let source = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .cloned()
                    .ok_or_else(|| failure("record_absent", "entity does not exist"))?;
                let (kind, mut properties) =
                    enum_parts(&source).map_err(|error| failure("serialization_failed", error))?;
                paths = apply_updates(&mut properties, request, collection)
                    .map_err(|error| failure("invalid_update", error))?;
                let mut edited: acadrust::EntityType = decode_enum(&kind, properties)
                    .map_err(|error| failure("invalid_value", error))?;
                if edited.common().handle != handle {
                    return Err(failure("identity_changed", "entity identity is read-only"));
                }
                edited.preserve_storage_data_from(&source);
                changed = edited != source;
                if changed {
                    self.push_undo_snapshot(i, "MCP SET_PROPERTIES");
                    if !self.tabs[i].scene.update_entity(edited) {
                        self.discard_last_undo_entry(i);
                        return Err(failure("update_failed", "entity could not be updated"));
                    }
                    self.tabs[i].dirty = true;
                }
                result_handle = Some(handle_text(handle));
            }
            "objects" => {
                let handle =
                    requested_handle(request).map_err(|error| failure("invalid_handle", error))?;
                let source = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .get(&handle)
                    .cloned()
                    .ok_or_else(|| failure("record_absent", "object does not exist"))?;
                let (kind, mut properties) =
                    enum_parts(&source).map_err(|error| failure("serialization_failed", error))?;
                paths = apply_updates(&mut properties, request, collection)
                    .map_err(|error| failure("invalid_update", error))?;
                let mut edited: acadrust::objects::ObjectType = decode_enum(&kind, properties)
                    .map_err(|error| failure("invalid_value", error))?;
                edited.preserve_storage_data_from(&source);
                changed = edited != source;
                if changed {
                    self.push_undo_snapshot(i, "MCP SET_PROPERTIES");
                    self.tabs[i].scene.document.objects.insert(handle, edited);
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                }
                result_handle = Some(handle_text(handle));
            }
            "header" => {
                let source = self.tabs[i].scene.document.header.clone();
                let mut properties =
                    json_value(&source).map_err(|error| failure("serialization_failed", error))?;
                paths = apply_updates(&mut properties, request, collection)
                    .map_err(|error| failure("invalid_update", error))?;
                let edited =
                    serde_json::from_value(properties).map_err(|error: serde_json::Error| {
                        failure("invalid_value", error.to_string())
                    })?;
                changed = edited != source;
                if changed {
                    self.push_undo_snapshot(i, "MCP SET_PROPERTIES");
                    self.tabs[i].scene.document.header = edited;
                    self.tabs[i].adopt_active_ucs_from_header();
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                }
                result_name = Some("header".to_string());
            }
            "summary_info" => {
                let source = self.tabs[i].scene.document.summary_info.clone();
                let mut properties =
                    json_value(&source).map_err(|error| failure("serialization_failed", error))?;
                paths = apply_updates(&mut properties, request, collection)
                    .map_err(|error| failure("invalid_update", error))?;
                let edited =
                    serde_json::from_value(properties).map_err(|error: serde_json::Error| {
                        failure("invalid_value", error.to_string())
                    })?;
                changed = edited != source;
                if changed {
                    self.push_undo_snapshot(i, "MCP SET_PROPERTIES");
                    self.tabs[i].scene.document.summary_info = edited;
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                }
                result_name = Some("summary_info".to_string());
            }
            _ => {
                macro_rules! patch_table {
                    ($field:ident) => {{
                        let (source, edited, changed_paths) = patched_table_entry(
                            &self.tabs[i].scene.document.$field,
                            request,
                            collection,
                        )
                        .map_err(|error| failure("invalid_update", error))?;
                        paths = changed_paths;
                        changed = edited != source;
                        let handle = source.handle();
                        result_handle = Some(handle_text(handle));
                        result_name = Some(source.name().to_string());
                        if changed {
                            self.push_undo_snapshot(i, "MCP SET_PROPERTIES");
                            replace_table_entry(
                                &mut self.tabs[i].scene.document.$field,
                                handle,
                                edited,
                            );
                            self.tabs[i].scene.bump_geometry();
                            self.tabs[i].dirty = true;
                        }
                    }};
                }
                match collection {
                    "layers" => patch_table!(layers),
                    "line_types" => patch_table!(line_types),
                    "text_styles" => patch_table!(text_styles),
                    "block_records" => patch_table!(block_records),
                    "dim_styles" => patch_table!(dim_styles),
                    "app_ids" => patch_table!(app_ids),
                    "views" => patch_table!(views),
                    "vports" => patch_table!(vports),
                    "ucss" => patch_table!(ucss),
                    "vx_table" => patch_table!(vx_table),
                    _ => unreachable!(),
                }
            }
        }
        if changed {
            self.refresh_properties();
        }
        self.set_control_result(json!({
            "collection":collection,
            "handle":result_handle,
            "name":result_name,
            "paths":paths,
            "changed":changed,
        }));
        Ok(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use acadrust::entities::{AttributeEntity, EntityType, Insert};
    use acadrust::objects::{Dictionary, ObjectType};
    use acadrust::types::{Handle, Vector3};
    use serde_json::{Value, json};

    use super::OpenCADStudio;

    fn execute(app: &mut OpenCADStudio, mut request: Value, id: &str) -> Value {
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let object = request.as_object_mut().unwrap();
        object.insert("protocol".into(), json!(1));
        object.insert("request_id".into(), json!(id));
        object.insert("document_id".into(), state["document_id"].clone());
        object.insert("revision".into(), state["revision"].clone());
        app.automation_op(&request.to_string())
    }

    #[test]
    fn records_query_and_edit_every_mutable_record_family() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let mut insert = Insert::new("A_CPT", Vector3::new(12.0, 34.0, 5.0));
        insert
            .attributes
            .push(AttributeEntity::simple("COMPANY", "BPH"));
        let entity = app.tabs[i].scene.add_entity(EntityType::Insert(insert));

        let object_handle = Handle::new(0x500);
        let mut dictionary = Dictionary::new();
        dictionary.handle = object_handle;
        app.tabs[i]
            .scene
            .document
            .objects
            .insert(object_handle, ObjectType::Dictionary(dictionary));

        let records = app.record_query(&json!({
            "collection":"entities",
            "type":"Insert",
            "where":[{"path":"/attributes/0/tag","value":"COMPANY"}]
        }));
        assert_eq!(records["count"], 1);
        assert_eq!(records["records"][0]["properties"]["block_name"], "A_CPT");
        assert_eq!(
            records["records"][0]["properties"]["attributes"][0]["value"],
            "BPH"
        );

        let edited = execute(
            &mut app,
            json!({
                "op":"set_properties",
                "collection":"entities",
                "handle":format!("{:X}", entity.value()),
                "updates":[
                    {"path":"/attributes/0/value","expected":"BPH","value":"Edited"},
                    {"path":"/insert_point/x","value":20.0}
                ]
            }),
            "edit-entity",
        );
        assert_eq!(edited["status"], "completed", "{edited}");
        assert_eq!(edited["result"]["changed"], true);
        let EntityType::Insert(insert) = app.tabs[i].scene.document.get_entity(entity).unwrap()
        else {
            panic!("expected insert")
        };
        assert_eq!(insert.attributes[0].value, "Edited");
        assert_eq!(insert.insert_point.x, 20.0);

        let object_edit = execute(
            &mut app,
            json!({
                "op":"set_properties",
                "collection":"objects",
                "handle":"500",
                "updates":[{"path":"/hard_owner","value":true}]
            }),
            "edit-object",
        );
        assert_eq!(object_edit["result"]["changed"], true, "{object_edit}");
        let Some(ObjectType::Dictionary(dictionary)) =
            app.tabs[i].scene.document.objects.get(&object_handle)
        else {
            panic!("expected dictionary")
        };
        assert!(dictionary.hard_owner);

        let layer_edit = execute(
            &mut app,
            json!({
                "op":"set_properties",
                "collection":"layers",
                "name":"0",
                "updates":[{"path":"/is_plottable","value":false}]
            }),
            "edit-layer",
        );
        assert_eq!(layer_edit["result"]["changed"], true, "{layer_edit}");
        assert!(
            !app.tabs[i]
                .scene
                .document
                .layers
                .get("0")
                .unwrap()
                .is_plottable
        );

        let header_edit = execute(
            &mut app,
            json!({
                "op":"set_properties",
                "collection":"header",
                "updates":[{"path":"/point_display_size","value":7.5}]
            }),
            "edit-header",
        );
        assert_eq!(header_edit["result"]["changed"], true, "{header_edit}");
        assert_eq!(app.tabs[i].scene.document.header.point_display_size, 7.5);

        let summary_edit = execute(
            &mut app,
            json!({
                "op":"set_properties",
                "collection":"summary_info",
                "updates":[{"path":"/author","value":"MCP"}]
            }),
            "edit-summary",
        );
        assert_eq!(summary_edit["result"]["changed"], true, "{summary_edit}");
        assert_eq!(app.tabs[i].scene.document.summary_info.author, "MCP");

        let undo = execute(&mut app, json!({"op":"undo"}), "undo-summary");
        assert_eq!(undo["status"], "completed", "{undo}");
        assert!(app.tabs[i].scene.document.summary_info.author.is_empty());
    }

    #[test]
    fn record_updates_reject_type_identity_and_compare_failures() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let entity = app.tabs[i]
            .scene
            .add_entity(EntityType::Insert(Insert::new("A", Vector3::ZERO)));
        let handle = format!("{:X}", entity.value());

        let wrong_type = execute(
            &mut app,
            json!({"op":"set_properties","collection":"entities","handle":handle,
                "updates":[{"path":"/rotation","value":"invalid"}]}),
            "wrong-type",
        );
        assert_eq!(wrong_type["code"], "invalid_value", "{wrong_type}");

        let identity = execute(
            &mut app,
            json!({"op":"set_properties","collection":"entities","handle":handle,
                "updates":[{"path":"/common/handle","value":99}]}),
            "identity",
        );
        assert_eq!(identity["code"], "invalid_update", "{identity}");

        let compare = execute(
            &mut app,
            json!({"op":"set_properties","collection":"entities","handle":handle,
                "updates":[{"path":"/rotation","expected":1.0,"value":2.0}]}),
            "compare",
        );
        assert_eq!(compare["code"], "invalid_update", "{compare}");
    }
}
