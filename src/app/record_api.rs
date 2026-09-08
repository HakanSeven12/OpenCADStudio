use acadrust::tables::{Table, TableEntry};
use iced::Task;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::sync::OnceLock;

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

fn collection_root_type(collection: &str) -> Option<&'static str> {
    Some(match collection {
        "entities" => "EntityType",
        "objects" => "ObjectType",
        "layers" => "Layer",
        "line_types" => "LineType",
        "text_styles" => "TextStyle",
        "block_records" => "BlockRecord",
        "dim_styles" => "DimStyle",
        "app_ids" => "AppId",
        "views" => "View",
        "vports" => "VPort",
        "ucss" => "Ucs",
        "vx_table" => "VxTableRecord",
        "header" => "HeaderVariables",
        "summary_info" => "SummaryInfo",
        "classes" => "DxfClass",
        "block_visibility" => "BlockVisibilityParameter",
        "context_scales"
        | "block_representations"
        | "vx_control_entries"
        | "section_view_representations"
        | "view_rep_references" => "Handle",
        "fields" => "FieldDef",
        "dgn_line_style_definitions" => "DgnLsDefinition",
        "dgn_line_style_components" => "DgnLsComponent",
        "notifications" => "NotificationCollection",
        "preview" => "Preview",
        "section_view_style" => "EntitySectionViewStyle",
        // This aggregate is assembled by OCS and has a live inferred schema.
        "document" => "Value",
        _ => return None,
    })
}

fn collection_is_mutable(collection: &str) -> bool {
    COLLECTIONS
        .iter()
        .find(|(name, _)| *name == collection)
        .is_some_and(|(_, mutable)| *mutable)
}

fn collection_record_name(collection: &str) -> Option<&'static str> {
    Some(match collection {
        "classes" => "Class",
        "fields" => "Field",
        "dgn_line_style_definitions" => "LineStyleDefinition",
        "dgn_line_style_components" => "LineStyleComponent",
        "section_view_style" => "SectionViewStyle",
        "view_rep_references" => "HandleReferences",
        "notifications" => "Notifications",
        "document" => "Document",
        _ => collection_root_type(collection)?,
    })
}

fn identity_paths(collection: &str) -> &'static [&'static str] {
    match collection {
        "entities" => &["/common/handle", "/common/owner_handle"],
        "objects" => &[
            "/handle",
            "/owner",
            "/common/handle",
            "/common/owner_handle",
        ],
        "header" => &["/handle_seed"],
        "summary_info" => &[],
        "layers" | "line_types" | "text_styles" | "block_records" | "dim_styles" | "app_ids"
        | "views" | "vports" | "ucss" | "vx_table" => &["/handle", "/name"],
        _ => &[],
    }
}

fn path_changes_identity(collection: &str, path: &str) -> bool {
    path_is_identity_field(collection, path)
        || identity_paths(collection).iter().any(|identity| {
            path == *identity
                || identity
                    .strip_prefix(path)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}

fn path_is_identity_field(collection: &str, path: &str) -> bool {
    match collection {
        "entities" => path.ends_with("/common/handle") || path.ends_with("/common/owner_handle"),
        "objects" => {
            matches!(path, "/handle" | "/owner")
                || path.ends_with("/common/handle")
                || path.ends_with("/common/owner_handle")
        }
        _ => identity_paths(collection).contains(&path),
    }
}

fn collect_identity_values(value: &Value, collection: &str, path: &str, output: &mut Vec<Value>) {
    if path_is_identity_field(collection, path) {
        output.push(json!({"path":path,"value":value}));
    }
    match value {
        Value::Object(object) => {
            for (name, value) in object {
                collect_identity_values(
                    value,
                    collection,
                    &format!("{path}/{}", json_pointer_name(name)),
                    output,
                );
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_identity_values(value, collection, &format!("{path}/{index}"), output);
            }
        }
        _ => {}
    }
}

fn registry_types() -> &'static Map<String, Value> {
    static REGISTRY: OnceLock<Value> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str(ocs_plugin_api::get_embedded_type_registry_json())
            .expect("embedded type registry")
    })["types"]
        .as_object()
        .expect("type registry object")
}

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

fn resolved_type_name(requested: &str) -> Option<String> {
    let types = registry_types();
    types
        .get_key_value(requested)
        .or_else(|| {
            types
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(requested))
        })
        .map(|(name, _)| name.clone())
}

fn collect_type_references(value: &Value, pending: &mut VecDeque<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "type_id" {
                    if let Some(type_id) = value.as_str() {
                        for token in type_id.split(|character: char| {
                            !character.is_ascii_alphanumeric() && character != '_'
                        }) {
                            if registry_types().contains_key(token) {
                                pending.push_back(token.to_string());
                            }
                        }
                    }
                }
                collect_type_references(value, pending);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_type_references(value, pending);
            }
        }
        _ => {}
    }
}

fn referenced_definitions(root: &str) -> Map<String, Value> {
    let mut definitions = Map::new();
    let mut pending = VecDeque::from([root.to_string()]);
    while let Some(name) = pending.pop_front() {
        if definitions.contains_key(&name) {
            continue;
        }
        let Some(info) = registry_types().get(&name) else {
            continue;
        };
        collect_type_references(info, &mut pending);
        definitions.insert(name, info.clone());
    }
    definitions
}

fn json_pointer_name(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

fn field_unit(name: &str, type_id: &str) -> Option<&'static str> {
    let numeric = matches!(
        type_id,
        "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "Vector2"
            | "Vector3"
    );
    if !numeric {
        return None;
    }
    let name = name.to_ascii_lowercase();
    if name.contains("angle") || name == "rotation" || name.contains("twist") {
        return Some("radians");
    }
    if name.contains("scale") || name.contains("ratio") || name.contains("factor") {
        return Some("unitless");
    }
    if [
        "point",
        "position",
        "origin",
        "center",
        "location",
        "elevation",
        "height",
        "width",
        "length",
        "radius",
        "diameter",
        "offset",
        "thickness",
        "distance",
        "size",
        "spacing",
    ]
    .iter()
    .any(|part| name.contains(part))
    {
        return Some("drawing_units");
    }
    None
}

fn type_constraints(type_id: &str) -> Option<Value> {
    let integer_bounds = match type_id {
        "i8" => Some(json!({"minimum":i8::MIN,"maximum":i8::MAX})),
        "i16" => Some(json!({"minimum":i16::MIN,"maximum":i16::MAX})),
        "i32" => Some(json!({"minimum":i32::MIN,"maximum":i32::MAX})),
        "i64" => Some(json!({"minimum":i64::MIN,"maximum":i64::MAX})),
        "u8" => Some(json!({"minimum":0,"maximum":u8::MAX})),
        "u16" => Some(json!({"minimum":0,"maximum":u16::MAX})),
        "u32" => Some(json!({"minimum":0,"maximum":u32::MAX})),
        "u64" | "Handle" => Some(json!({"minimum":0,"maximum":u64::MAX})),
        _ => None,
    };
    if integer_bounds.is_some() {
        return integer_bounds;
    }
    let info = registry_types().get(type_id)?;
    if info["kind"] != "Enum" {
        return None;
    }
    Some(json!({
        "allowed_variants":info["variants"].as_array().into_iter().flatten()
            .filter_map(|variant|variant["name"].as_str()).collect::<Vec<_>>()
    }))
}

fn append_schema_fields(
    owner_type: &str,
    type_fields: &[Value],
    prefix: &str,
    collection: Option<&str>,
    depth: usize,
    stack: &mut Vec<String>,
    fields: &mut Vec<Value>,
) {
    for field in type_fields {
        let Some(name) = field["name"].as_str() else {
            continue;
        };
        let type_id = field["type_id"].as_str().unwrap_or("Value");
        let path = format!("{prefix}/{}", json_pointer_name(name));
        let sequence = field["is_sequence"].as_bool().unwrap_or(false);
        let writable = collection.is_some_and(collection_is_mutable)
            && collection.is_none_or(|collection| !path_changes_identity(collection, &path));
        let mut description = json!({
            "path":path,
            "name":name,
            "description":format!("{} property on {owner_type}", name.replace('_', " ")),
            "type":type_id,
            "optional":field["optional"].as_bool().unwrap_or(false),
            "sequence":sequence,
            "writable":writable,
        });
        let object = description.as_object_mut().expect("field description");
        if sequence {
            object.insert(
                "item_path_template".into(),
                json!(format!("{path}/{{index}}")),
            );
        }
        if let Some(unit) = field_unit(name, type_id) {
            object.insert("unit".into(), json!(unit));
        }
        if let Some(constraints) = type_constraints(type_id) {
            object.insert("constraints".into(), constraints);
        }
        fields.push(description);

        let nested_prefix = if sequence {
            format!("{path}/{{index}}")
        } else {
            path
        };
        if registry_types().contains_key(type_id) {
            append_field_descriptions(
                type_id,
                &nested_prefix,
                collection,
                depth + 1,
                stack,
                fields,
            );
        }
    }
}

fn append_field_descriptions(
    type_name: &str,
    prefix: &str,
    collection: Option<&str>,
    depth: usize,
    stack: &mut Vec<String>,
    fields: &mut Vec<Value>,
) {
    if depth > 12 || stack.iter().any(|name| name == type_name) {
        return;
    }
    let Some(info) = registry_types().get(type_name) else {
        return;
    };
    stack.push(type_name.to_string());
    match info["kind"].as_str() {
        Some("Struct") => append_schema_fields(
            type_name,
            info["fields"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            prefix,
            collection,
            depth,
            stack,
            fields,
        ),
        Some("Enum") => append_enum_descriptions(
            type_name,
            info["variants"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            prefix,
            collection,
            depth,
            stack,
            fields,
        ),
        _ => {}
    }
    stack.pop();
}

fn append_enum_descriptions(
    enum_type: &str,
    variants: &[Value],
    prefix: &str,
    collection: Option<&str>,
    depth: usize,
    stack: &mut Vec<String>,
    fields: &mut Vec<Value>,
) {
    for variant in variants {
        let Some(variant_name) = variant["name"].as_str() else {
            continue;
        };
        let Some(variant_fields) = variant["fields"]
            .as_array()
            .filter(|fields| !fields.is_empty())
        else {
            continue;
        };
        let path = format!("{prefix}/{}", json_pointer_name(variant_name));
        let newtype = (variant_fields.len() == 1 && variant_fields[0]["name"] == "0")
            .then(|| &variant_fields[0]);
        let type_id = newtype
            .and_then(|field| field["type_id"].as_str())
            .unwrap_or("struct_variant");
        let sequence = newtype
            .and_then(|field| field["is_sequence"].as_bool())
            .unwrap_or(false);
        let writable = collection.is_some_and(collection_is_mutable)
            && collection.is_none_or(|collection| !path_changes_identity(collection, &path));
        let mut description = json!({
            "path":path,
            "name":variant_name,
            "description":format!("{variant_name} variant on {enum_type}"),
            "type":type_id,
            "enum":enum_type,
            "variant":variant_name,
            "optional":false,
            "sequence":sequence,
            "writable":writable,
        });
        if sequence {
            description
                .as_object_mut()
                .expect("variant description")
                .insert(
                    "item_path_template".into(),
                    json!(format!("{path}/{{index}}")),
                );
        }
        if let Some(unit) = field_unit(variant_name, type_id) {
            description
                .as_object_mut()
                .expect("variant description")
                .insert("unit".into(), json!(unit));
        }
        if let Some(constraints) = type_constraints(type_id) {
            description
                .as_object_mut()
                .expect("variant description")
                .insert("constraints".into(), constraints);
        }
        fields.push(description);

        if newtype.is_some() {
            if registry_types().contains_key(type_id) {
                let nested_prefix = if sequence {
                    format!("{path}/{{index}}")
                } else {
                    path
                };
                append_field_descriptions(
                    type_id,
                    &nested_prefix,
                    collection,
                    depth + 1,
                    stack,
                    fields,
                );
            }
        } else {
            append_schema_fields(
                variant_name,
                variant_fields,
                &path,
                collection,
                depth + 1,
                stack,
                fields,
            );
        }
    }
}

fn append_variant_field_descriptions(
    enum_type: &str,
    variant_name: &str,
    collection: Option<&str>,
    fields: &mut Vec<Value>,
) {
    let Some(variant_fields) = registry_types()
        .get(enum_type)
        .and_then(|info| info["variants"].as_array())
        .and_then(|variants| {
            variants.iter().find(|variant| {
                variant["name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(variant_name))
            })
        })
        .and_then(|variant| variant["fields"].as_array())
    else {
        return;
    };
    let mut stack = vec![enum_type.to_string()];
    append_schema_fields(
        variant_name,
        variant_fields,
        "",
        collection,
        0,
        &mut stack,
        fields,
    );
}

fn runtime_value_schema(value: &Value, collection: &str, path: &str) -> Value {
    let mut schema = match value {
        Value::Null => json!({"type":"null"}),
        Value::Bool(value) => json!({"type":"boolean","example":value}),
        Value::Number(value) if value.is_i64() || value.is_u64() => {
            json!({"type":"integer","example":value})
        }
        Value::Number(value) => json!({"type":"number","example":value}),
        Value::String(value) => {
            let example: String = value.chars().take(160).collect();
            json!({"type":"string","example":example})
        }
        Value::Array(values) => {
            let item = values
                .first()
                .map(|value| runtime_value_schema(value, collection, &format!("{path}/0")))
                .unwrap_or_else(|| json!({}));
            json!({"type":"array","items":item})
        }
        Value::Object(values) => {
            let properties = values
                .iter()
                .map(|(name, value)| {
                    let child_path = format!("{path}/{}", json_pointer_name(name));
                    (
                        name.clone(),
                        runtime_value_schema(value, collection, &child_path),
                    )
                })
                .collect::<Map<_, _>>();
            json!({
                "type":"object",
                "properties":properties,
                "required":values.keys().collect::<Vec<_>>(),
                "additionalProperties":false,
            })
        }
    };
    if !path.is_empty() {
        let object = schema.as_object_mut().expect("runtime schema");
        object.insert(
            "readOnly".into(),
            json!(!collection_is_mutable(collection) || path_changes_identity(collection, path)),
        );
        let name = path.rsplit('/').next().unwrap_or("");
        let scalar_type = if value.is_number() { "f64" } else { "Value" };
        if let Some(unit) = field_unit(name, scalar_type) {
            object.insert("x-unit".into(), json!(unit));
        }
    }
    schema
}

fn editing_description(collection: Option<&str>) -> Value {
    let mutable = collection.is_some_and(collection_is_mutable);
    json!({
        "read_operation":"records",
        "write_operation":mutable.then_some("set_properties"),
        "selector":match collection {
            Some("entities" | "objects") => "handle",
            Some("header" | "summary_info") => "singleton",
            Some(_) if mutable => "handle or name",
            Some(_) => "read-only derived record",
            None => "collection plus handle or name",
        },
        "path":"RFC 6901 JSON Pointer relative to record.properties",
        "sequence_path":"Replace {index} in item_path_template with a current zero-based array index",
        "identity_paths":collection.map(identity_paths).unwrap_or(&[]),
        "atomic":true,
        "compare_and_set":"Add expected to an update to reject stale field values",
        "validation":["document revision","request id","JSON type","enum variant","integer bounds","record identity","layer lock"],
    })
}

fn collection_record_types(collection: &str) -> Vec<Value> {
    let root = collection_root_type(collection).unwrap_or("Value");
    if matches!(collection, "entities" | "objects") {
        return registry_types()[root]["variants"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|variant| {
                let fields = variant["fields"].as_array();
                let schema_type = fields
                    .filter(|fields| fields.len() == 1 && fields[0]["name"] == "0")
                    .and_then(|fields| fields[0]["type_id"].as_str())
                    .unwrap_or(root);
                json!({"type":variant["name"],"schema_type":schema_type})
            })
            .collect();
    }
    vec![json!({
        "type":collection_record_name(collection).unwrap_or(root),
        "schema_type":root,
    })]
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
    let mut original_identity = Vec::new();
    collect_identity_values(properties, collection, "", &mut original_identity);
    let mut paths = Vec::with_capacity(updates.len());
    for update in updates {
        let path = update["path"]
            .as_str()
            .ok_or_else(|| "each update requires a JSON Pointer path".to_string())?;
        if path.is_empty() || !path.starts_with('/') {
            return Err("updates require a non-empty JSON Pointer path".into());
        }
        if path_changes_identity(collection, path) {
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
    let mut edited_identity = Vec::new();
    collect_identity_values(properties, collection, "", &mut edited_identity);
    if edited_identity != original_identity {
        return Err("record identity is read-only".into());
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
                "describe":"record_schema",
                "path":"RFC 6901 JSON Pointer relative to properties",
                "filters":["eq","ne","lt","lte","gt","gte","contains","starts_with","ends_with","in","exists","not_exists"],
                "collections":COLLECTIONS.iter().map(|(name, mutable)|json!({
                    "name":name,
                    "mutable":mutable,
                    "root_type":collection_root_type(name),
                    "count":collection_count(document, name),
                })).collect::<Vec<_>>()
            },
            "editor":{"selection":true,"properties":true,"commands":true,"files":true,"capture":true,"events":true}
        })
    }

    pub(super) fn record_schema(&self, request: &Value) -> Value {
        let collection = request["collection"].as_str();
        if collection.is_some_and(|name| collection_root_type(name).is_none()) {
            return failure(
                "unknown_collection",
                format!("unknown record collection: {}", collection.unwrap()),
            );
        }
        let collections = COLLECTIONS
            .iter()
            .map(|(name, mutable)| {
                json!({
                    "name":name,
                    "mutable":mutable,
                    "root_type":collection_root_type(name),
                    "identity_paths":identity_paths(name),
                })
            })
            .collect::<Vec<_>>();

        let Some(requested_type) = request["type"].as_str() else {
            let mut record_types = if let Some(collection) = collection {
                collection_record_types(collection)
            } else {
                registry_types()
                    .iter()
                    .map(|(name, info)| json!({"type":name,"schema_type":name,"kind":info["kind"]}))
                    .collect()
            };
            if let Some(search) = request["search"].as_str().filter(|value| !value.is_empty()) {
                let search = search.to_ascii_lowercase();
                record_types.retain(|entry| {
                    entry["type"]
                        .as_str()
                        .is_some_and(|name| name.to_ascii_lowercase().contains(&search))
                });
            }
            let count = record_types.len();
            let offset = request["offset"].as_u64().unwrap_or(0) as usize;
            let limit = request["limit"].as_u64().unwrap_or(1000).min(10_000) as usize;
            let record_types = record_types
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            return json!({
                "ok":true,
                "schema_version":1,
                "collection":collection,
                "root_type":collection.and_then(collection_root_type),
                "collections":collections,
                "count":count,
                "returned":record_types.len(),
                "next_offset":(offset + record_types.len() < count).then_some(offset + record_types.len()),
                "record_types":record_types,
                "editing":editing_description(collection),
                "next":"Call record_schema again with type set to a record type or schema_type returned here."
            });
        };

        let root_type = collection
            .and_then(|collection| {
                collection_record_types(collection)
                    .into_iter()
                    .find(|entry| {
                        ["type", "schema_type"].iter().any(|key| {
                            entry[*key]
                                .as_str()
                                .is_some_and(|name| name.eq_ignore_ascii_case(requested_type))
                        })
                    })
                    .and_then(|entry| entry["schema_type"].as_str().map(str::to_string))
            })
            .or_else(|| resolved_type_name(requested_type))
            .or_else(|| {
                (requested_type == "Value"
                    || collection.and_then(collection_root_type) == Some("Value"))
                .then(|| "Value".to_string())
            });
        let Some(root_type) = root_type else {
            return failure(
                "unknown_record_type",
                format!("unknown record type: {requested_type}"),
            );
        };
        let mut fields = Vec::new();
        let direct_collection_variant =
            collection
                .and_then(collection_root_type)
                .is_some_and(|collection_root| {
                    collection_root == root_type && !requested_type.eq_ignore_ascii_case(&root_type)
                })
                && registry_types()
                    .get(&root_type)
                    .is_some_and(|info| info["kind"] == "Enum");
        if direct_collection_variant {
            append_variant_field_descriptions(&root_type, requested_type, collection, &mut fields);
        } else {
            append_field_descriptions(&root_type, "", collection, 0, &mut Vec::new(), &mut fields);
        }
        let definitions = referenced_definitions(&root_type);
        let runtime_schema = collection.and_then(|collection| {
            collection_records(&self.tabs[self.active_tab].scene.document, collection)
                .ok()?
                .into_iter()
                .find(|record| {
                    let type_matches = record["type"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(requested_type))
                        || collection_root_type(collection)
                            .is_some_and(|name| name.eq_ignore_ascii_case(requested_type));
                    type_matches
                        && request["handle"].as_str().is_none_or(|handle| {
                            record["handle"].as_str().is_some_and(|actual| {
                                actual.eq_ignore_ascii_case(without_hex_prefix(handle))
                            })
                        })
                        && request["name"].as_str().is_none_or(|name| {
                            record["name"]
                                .as_str()
                                .is_some_and(|actual| actual.eq_ignore_ascii_case(name))
                        })
                })
                .map(|record| runtime_value_schema(&record["properties"], collection, ""))
        });
        json!({
            "ok":true,
            "schema_version":1,
            "collection":collection,
            "record_type":requested_type,
            "schema_type":root_type,
            "schema":registry_types().get(&root_type),
            "definitions":definitions,
            "fields":fields,
            "runtime_schema":runtime_schema,
            "editing":editing_description(collection),
            "notes":[
                "Enum constraints list every accepted serialized variant.",
                "Integer constraints reflect the complete serialized range.",
                "Unit annotations are supplied only where the serialized field name and type are unambiguous.",
                "runtime_schema describes an existing matching record when one is available."
            ]
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
    fn record_schema_describes_absent_types_fields_enums_units_and_write_rules() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        let catalog = app.record_schema(&json!({"collection":"entities"}));
        assert_eq!(catalog["ok"], true, "{catalog}");
        assert_eq!(catalog["root_type"], "EntityType");
        assert_eq!(catalog["count"], 48);
        assert!(catalog["record_types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["type"] == "Insert" && record["schema_type"] == "Insert"));

        let schema = app.record_schema(&json!({
            "collection":"entities",
            "type":"Insert"
        }));
        assert_eq!(schema["ok"], true, "{schema}");
        assert!(schema["definitions"].get("Insert").is_some());
        assert!(schema["definitions"].get("AttributeEntity").is_some());
        assert!(schema["runtime_schema"].is_null());
        let fields = schema["fields"].as_array().unwrap();
        let field = |path: &str| {
            fields
                .iter()
                .find(|field| field["path"] == path)
                .unwrap_or_else(|| panic!("missing schema field {path}"))
        };
        assert_eq!(field("/common/handle")["writable"], false);
        assert_eq!(field("/insert_point")["unit"], "drawing_units");
        assert_eq!(field("/rotation")["unit"], "radians");
        assert_eq!(
            field("/attributes")["item_path_template"],
            "/attributes/{index}"
        );
        assert_eq!(field("/attributes/{index}/value")["writable"], true);

        let text = app.record_schema(&json!({"collection":"entities","type":"Text"}));
        let alignment = text["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["path"] == "/horizontal_alignment")
            .expect("horizontal alignment schema");
        assert!(alignment["constraints"]["allowed_variants"]
            .as_array()
            .is_some_and(|variants| variants.len() >= 3));

        let unknown = app.record_schema(&json!({"collection":"objects","type":"Unknown"}));
        assert_eq!(unknown["schema_type"], "ObjectType");
        let fields = unknown["fields"].as_array().unwrap();
        let field = |path: &str| {
            fields
                .iter()
                .find(|field| field["path"] == path)
                .unwrap_or_else(|| panic!("missing enum variant field {path}"))
        };
        assert_eq!(field("/type_name")["writable"], true);
        assert_eq!(field("/handle")["writable"], false);
        assert_eq!(field("/owner")["writable"], false);
        assert_eq!(field("/raw_dwg_handle_bits")["writable"], true);

        let dimension = app.record_schema(&json!({"collection":"entities","type":"Dimension"}));
        let fields = dimension["fields"].as_array().unwrap();
        assert!(fields.iter().any(|field| field["path"] == "/Linear"));
        assert!(fields.iter().any(
            |field| field["path"] == "/Linear/base/common/handle" && field["writable"] == false
        ));
        assert!(fields.iter().any(|field| field["path"] == "/Radius"));

        let section_style = app
            .record_schema(&json!({"collection":"section_view_style","type":"SectionViewStyle"}));
        assert_eq!(section_style["schema_type"], "EntitySectionViewStyle");
        assert!(section_style["fields"]
            .as_array()
            .is_some_and(|fields| fields
                .iter()
                .any(|field| field["path"] == "/arrow_size" && field["unit"] == "drawing_units")));

        let class = app.record_schema(&json!({"collection":"classes","type":"Class"}));
        assert_eq!(class["schema_type"], "DxfClass");
        assert!(class["fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().any(|field| field["path"] == "/dxf_name")));

        for (collection, root) in [("entities", "EntityType"), ("objects", "ObjectType")] {
            let complete = app.record_schema(&json!({"collection":collection,"type":root}));
            assert_eq!(complete["ok"], true, "{root}: {complete}");
            assert!(complete["fields"]
                .as_array()
                .is_some_and(|fields| !fields.is_empty()));
            let bytes = serde_json::to_vec(&complete).unwrap().len();
            assert!(bytes < 8 * 1024 * 1024, "{root} schema is {bytes} bytes");
        }
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

        let identity_parent = execute(
            &mut app,
            json!({"op":"set_properties","collection":"entities","handle":handle,
                "updates":[{"path":"/common","value":{}}]}),
            "identity-parent",
        );
        assert_eq!(
            identity_parent["code"], "invalid_update",
            "{identity_parent}"
        );

        let compare = execute(
            &mut app,
            json!({"op":"set_properties","collection":"entities","handle":handle,
                "updates":[{"path":"/rotation","expected":1.0,"value":2.0}]}),
            "compare",
        );
        assert_eq!(compare["code"], "invalid_update", "{compare}");
    }
}
