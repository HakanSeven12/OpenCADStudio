//! Transport-neutral descriptors and generated schemas for curated MCP intent tools.

use crate::mcp_ops::{self, OpDef};
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Read,
    Capture,
    Execute,
}

pub struct Alias {
    pub public: &'static str,
    pub backing: &'static str,
}

pub struct IntentDescriptor {
    pub name: &'static str,
    pub title: &'static str,
    pub kind: IntentKind,
    pub op: Option<&'static OpDef>,
    pub aliases: &'static [Alias],
    pub destructive: bool,
}

const RUN_ALIASES: &[Alias] = &[Alias {
    public: "command",
    backing: "cmd",
}];

const fn read(name: &'static str, title: &'static str, op: &'static OpDef) -> IntentDescriptor {
    IntentDescriptor {
        name,
        title,
        kind: IntentKind::Read,
        op: Some(op),
        aliases: &[],
        destructive: false,
    }
}

const fn execute(
    name: &'static str,
    title: &'static str,
    op: &'static OpDef,
    aliases: &'static [Alias],
    destructive: bool,
) -> IntentDescriptor {
    IntentDescriptor {
        name,
        title,
        kind: IntentKind::Execute,
        op: Some(op),
        aliases,
        destructive,
    }
}

pub static DESCRIPTORS: &[IntentDescriptor] = &[
    read("ocs_inspect", "Inspect OCS state", &mcp_ops::STATE),
    read("ocs_query", "Query OCS entities", &mcp_ops::QUERY),
    read("ocs_find_text", "Find OCS text", &mcp_ops::TEXT_SEARCH),
    read("ocs_audit", "Audit OCS document", &mcp_ops::AUDIT),
    read("ocs_commands", "List OCS commands", &mcp_ops::COMMANDS),
    read("ocs_measure", "Measure OCS entities", &mcp_ops::MEASURE),
    IntentDescriptor {
        name: "ocs_render",
        title: "Render OCS viewport",
        kind: IntentKind::Capture,
        op: None,
        aliases: &[],
        destructive: false,
    },
    execute(
        "ocs_run",
        "Run OCS command",
        &mcp_ops::RUN,
        RUN_ALIASES,
        true,
    ),
    execute("ocs_new", "Create OCS drawing", &mcp_ops::NEW, &[], false),
    execute("ocs_open", "Open OCS drawing", &mcp_ops::OPEN, &[], false),
    execute("ocs_save", "Save OCS drawing", &mcp_ops::SAVE, &[], true),
    execute(
        "ocs_save_verified",
        "Save and verify OCS drawing",
        &mcp_ops::SAVE_VERIFIED,
        &[],
        true,
    ),
    execute(
        "ocs_select",
        "Select OCS entities",
        &mcp_ops::SELECT,
        &[],
        false,
    ),
    execute(
        "ocs_cancel",
        "Cancel OCS command",
        &mcp_ops::CANCEL,
        &[],
        true,
    ),
    execute(
        "ocs_undo",
        "Undo OCS modification",
        &mcp_ops::UNDO,
        &[],
        true,
    ),
    execute(
        "ocs_redo",
        "Redo OCS modification",
        &mcp_ops::REDO,
        &[],
        true,
    ),
];

pub fn find_descriptor(name: &str) -> Option<&'static IntentDescriptor> {
    DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.name == name)
}

/// Translate one curated public call into the existing facade's arguments.
/// The caller supplies a generated id only for execute calls that omitted one.
pub(crate) fn facade_arguments(
    descriptor: &IntentDescriptor,
    arguments: &Value,
    session_id: &str,
    generated_request_id: Option<&str>,
) -> (&'static str, Value) {
    let mut public = arguments.as_object().cloned().unwrap_or_default();
    public.remove("ocs_session_id");
    match descriptor.kind {
        IntentKind::Read => {
            let op = descriptor.op.expect("read intent operation");
            (
                "ocs_read",
                json!({
                    "ocs_session_id": session_id,
                    "op": op.name,
                    "parameters": Value::Object(public),
                }),
            )
        }
        IntentKind::Capture => {
            public
                .entry("view".to_string())
                .or_insert_with(|| json!("extents"));
            public.insert("ocs_session_id".to_string(), json!(session_id));
            ("ocs_capture", Value::Object(public))
        }
        IntentKind::Execute => {
            let op = descriptor.op.expect("execute intent operation");
            let request_id = public
                .remove("request_id")
                .or_else(|| generated_request_id.map(|id| json!(id)));
            let wait_seconds = public.remove("wait_seconds");
            let response_detail = public.remove("response_detail");
            for alias in descriptor.aliases {
                if let Some(value) = public.remove(alias.public) {
                    public.insert(alias.backing.to_string(), value);
                }
            }
            public.insert("op".to_string(), json!(op.name));
            if let Some(request_id) = request_id {
                public.insert("request_id".to_string(), request_id);
            }
            let mut facade = json!({"ocs_session_id":session_id,"request":Value::Object(public)});
            if let Some(wait_seconds) = wait_seconds {
                facade["wait_seconds"] = wait_seconds;
            }
            if let Some(response_detail) = response_detail {
                facade["response_detail"] = response_detail;
            }
            ("ocs_execute", facade)
        }
    }
}

fn session_property() -> Value {
    json!({"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."})
}

fn public_example(descriptor: &IntentDescriptor) -> Value {
    let op = descriptor.op.expect("operation descriptor");
    let mut example: Value = serde_json::from_str(op.example).expect("valid OpDef example");
    let object = example.as_object_mut().expect("object OpDef example");
    object.remove("op");
    for alias in descriptor.aliases {
        if let Some(value) = object.remove(alias.backing) {
            object.insert(alias.public.to_string(), value);
        }
    }
    example
}

fn operation_schema(descriptor: &IntentDescriptor) -> Value {
    let op = descriptor.op.expect("operation descriptor");
    let mut schema = mcp_ops::op_schema(op);
    let root = schema.as_object_mut().expect("object operation schema");
    let properties = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("operation properties");
    for key in mcp_ops::ENVELOPE_KEYS {
        properties.remove(*key);
    }
    for alias in descriptor.aliases {
        if let Some(mut property) = properties.remove(alias.backing) {
            if let Some(description) = property.get("description").and_then(Value::as_str) {
                property["description"] = json!(description.replace(alias.backing, alias.public));
            }
            properties.insert(alias.public.to_string(), property);
        }
    }
    properties.insert("ocs_session_id".to_string(), session_property());
    if descriptor.kind == IntentKind::Execute {
        properties.insert(
            "request_id".to_string(),
            json!({"type":"string","minLength":1,"maxLength":128,"description":"Optional caller-generated request id; the bridge assigns one when omitted."}),
        );
    }
    let mut required: Vec<Value> = root
        .remove("required")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name != "op")
        .map(|name| {
            let name = name.as_str().expect("string required parameter");
            json!(descriptor
                .aliases
                .iter()
                .find(|alias| alias.backing == name)
                .map_or(name, |alias| alias.public))
        })
        .collect();
    required.dedup();
    if !required.is_empty() {
        root.insert("required".to_string(), Value::Array(required));
    }
    root.insert("$defs".to_string(), mcp_ops::primitive_defs());
    root.insert("additionalProperties".to_string(), json!(false));
    schema
}

fn render_schema() -> Value {
    let mut schema = crate::mcp::capture_input_schema();
    let root = schema.as_object_mut().expect("capture schema object");
    root.remove("required");
    root.insert("additionalProperties".to_string(), json!(false));
    schema["properties"]
        .as_object_mut()
        .expect("capture properties")
        .remove("request_id");
    schema["properties"]["view"]["default"] = json!("extents");
    schema
}

fn definition(descriptor: &IntentDescriptor) -> Value {
    let (description, input_schema) = if descriptor.kind == IntentKind::Capture {
        (
            "Render the current drawing as a PNG, framed to extents by default, with optional highlighting and annotations.".to_string(),
            render_schema(),
        )
    } else {
        let op = descriptor.op.expect("operation descriptor");
        (
            format!("{} Example: {}", op.doc, public_example(descriptor)),
            operation_schema(descriptor),
        )
    };
    let (read_only, idempotent) = match descriptor.kind {
        IntentKind::Read => (true, true),
        IntentKind::Capture | IntentKind::Execute => (false, false),
    };
    let mut tool = json!({
        "name": descriptor.name,
        "title": descriptor.title,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": descriptor.destructive,
            "idempotentHint": idempotent,
            "openWorldHint": false,
        }
    });
    match descriptor.kind {
        IntentKind::Read => tool["outputSchema"] = crate::mcp::read_output_schema(),
        IntentKind::Execute => tool["outputSchema"] = crate::mcp::execute_output_schema(),
        IntentKind::Capture => {}
    }
    tool
}

pub fn tool_definitions() -> Vec<Value> {
    DESCRIPTORS.iter().map(definition).collect()
}

#[cfg(test)]
mod tests {
    use super::{facade_arguments, find_descriptor, tool_definitions, IntentKind, DESCRIPTORS};
    use serde_json::{json, Value};
    use std::collections::HashSet;

    const NAMES: &[&str] = &[
        "ocs_inspect",
        "ocs_query",
        "ocs_find_text",
        "ocs_audit",
        "ocs_commands",
        "ocs_measure",
        "ocs_render",
        "ocs_run",
        "ocs_new",
        "ocs_open",
        "ocs_save",
        "ocs_save_verified",
        "ocs_select",
        "ocs_cancel",
        "ocs_undo",
        "ocs_redo",
    ];

    fn generated(name: &str) -> Value {
        tool_definitions()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("generated definition for {name}"))
    }

    #[test]
    fn read_adapter_builds_the_legacy_read_facade_request() {
        let descriptor = find_descriptor("ocs_query").unwrap();
        let (facade, arguments) = facade_arguments(
            descriptor,
            &json!({"ocs_session_id":"ignored","layer":"Walls","limit":20}),
            "session-1",
            None,
        );
        assert_eq!(facade, "ocs_read");
        assert_eq!(
            arguments,
            json!({
                "ocs_session_id":"session-1",
                "op":"query",
                "parameters":{"layer":"Walls","limit":20}
            })
        );
    }

    #[test]
    fn execute_adapter_maps_aliases_and_uses_the_generated_request_id() {
        let descriptor = find_descriptor("ocs_run").unwrap();
        let (facade, arguments) = facade_arguments(
            descriptor,
            &json!({"ocs_session_id":"ignored","command":"LINE"}),
            "session-1",
            Some("generated-id"),
        );
        assert_eq!(facade, "ocs_execute");
        assert_eq!(
            arguments,
            json!({
                "ocs_session_id":"session-1",
                "request":{"op":"run","cmd":"LINE","request_id":"generated-id"}
            })
        );
    }

    #[test]
    fn execute_adapter_preserves_a_caller_request_id() {
        let descriptor = find_descriptor("ocs_run").unwrap();
        let (_, arguments) = facade_arguments(
            descriptor,
            &json!({"command":"LINE","request_id":"caller-id"}),
            "session-1",
            Some("generated-id"),
        );
        assert_eq!(arguments["request"]["request_id"], "caller-id");
        assert_eq!(arguments["request"]["cmd"], "LINE");
        assert!(arguments["request"].get("command").is_none());
    }

    #[test]
    fn execute_adapter_moves_facade_controls_out_of_the_gui_request() {
        let descriptor = find_descriptor("ocs_run").unwrap();
        let (_, arguments) = facade_arguments(
            descriptor,
            &json!({
                "command":"LINE",
                "request_id":"caller-id",
                "wait_seconds":0,
                "response_detail":"full"
            }),
            "session-1",
            None,
        );
        assert_eq!(
            arguments,
            json!({
                "ocs_session_id":"session-1",
                "wait_seconds":0,
                "response_detail":"full",
                "request":{"op":"run","cmd":"LINE","request_id":"caller-id"}
            })
        );
    }

    #[test]
    fn render_adapter_defaults_extents_and_targets_the_capture_facade() {
        let descriptor = find_descriptor("ocs_render").unwrap();
        let (facade, arguments) = facade_arguments(
            descriptor,
            &json!({"ocs_session_id":"ignored","annotate":true}),
            "session-1",
            None,
        );
        assert_eq!(facade, "ocs_capture");
        assert_eq!(
            arguments,
            json!({"ocs_session_id":"session-1","annotate":true,"view":"extents"})
        );
        assert!(arguments.get("request_id").is_none());

        let (_, explicit_view) =
            facade_arguments(descriptor, &json!({"view":"current"}), "session-1", None);
        assert_eq!(explicit_view["view"], "current");
    }

    #[test]
    fn catalogue_has_exactly_the_approved_unique_descriptors() {
        let names: Vec<_> = DESCRIPTORS
            .iter()
            .map(|descriptor| descriptor.name)
            .collect();
        assert_eq!(names, NAMES);
        assert_eq!(names.iter().copied().collect::<HashSet<_>>().len(), 16);
        for name in NAMES {
            let descriptor = find_descriptor(name).expect("descriptor lookup succeeds");
            assert_eq!(descriptor.name, *name);
            assert!(!descriptor.title.is_empty());
        }
        assert!(find_descriptor("ocs_run_script").is_none());
        assert!(find_descriptor("ocs_read").is_none());
    }

    #[test]
    fn descriptors_map_to_the_approved_backing_operations() {
        let expected = [
            ("ocs_inspect", "state"),
            ("ocs_query", "query"),
            ("ocs_find_text", "text_search"),
            ("ocs_audit", "audit"),
            ("ocs_commands", "commands"),
            ("ocs_measure", "measure"),
            ("ocs_run", "run"),
            ("ocs_new", "new"),
            ("ocs_open", "open"),
            ("ocs_save", "save"),
            ("ocs_save_verified", "save_verified"),
            ("ocs_select", "select"),
            ("ocs_cancel", "cancel"),
            ("ocs_undo", "undo"),
            ("ocs_redo", "redo"),
        ];
        for (name, op) in expected {
            assert_eq!(
                find_descriptor(name)
                    .and_then(|descriptor| descriptor.op)
                    .map(|op| op.name),
                Some(op),
                "{name}"
            );
        }
        assert!(find_descriptor("ocs_render").unwrap().op.is_none());
    }

    #[test]
    fn generated_schemas_expose_public_arguments_without_required_envelopes() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 16);
        for tool in &tools {
            let schema = &tool["inputSchema"];
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            let properties = schema["properties"].as_object().unwrap();
            for envelope in [
                "op",
                "document_id",
                "revision",
                "geometry_revision",
                "camera_revision",
                "selection",
                "client_id",
                "steps",
                "request",
            ] {
                assert!(
                    !properties.contains_key(envelope),
                    "{} exposes {envelope}",
                    tool["name"]
                );
            }
            assert_eq!(properties["ocs_session_id"]["type"], "string");
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            for forbidden in ["op", "ocs_session_id", "request_id", "revision"] {
                assert!(
                    !required.contains(&json!(forbidden)),
                    "{} requires {forbidden}",
                    tool["name"]
                );
            }
        }
        assert_eq!(
            generated("ocs_run")["inputSchema"]["required"],
            json!(["command"])
        );
        let run_properties = generated("ocs_run")["inputSchema"]["properties"].clone();
        assert!(run_properties.get("command").is_some());
        assert!(run_properties.get("cmd").is_none());
        assert_eq!(
            generated("ocs_open")["inputSchema"]["required"],
            json!(["path"])
        );
        assert_eq!(
            generated("ocs_save_verified")["inputSchema"]["required"],
            json!(["path"])
        );
        assert_eq!(
            generated("ocs_find_text")["inputSchema"]["required"],
            json!(["find"])
        );
    }

    #[test]
    fn execute_request_ids_are_optional_and_read_tools_do_not_advertise_them() {
        for descriptor in DESCRIPTORS {
            let schema = generated(descriptor.name)["inputSchema"].clone();
            match descriptor.kind {
                IntentKind::Execute => {
                    assert_eq!(schema["properties"]["request_id"]["type"], "string");
                    assert!(schema["required"]
                        .as_array()
                        .is_none_or(|required| { !required.contains(&json!("request_id")) }));
                }
                IntentKind::Read | IntentKind::Capture => {
                    assert!(schema["properties"].get("request_id").is_none());
                    assert!(crate::schema_validation::validate_shape(
                        &schema,
                        &json!({"request_id":"not-an-execute-intent"}),
                        ""
                    )
                    .is_err());
                }
            }
        }
    }

    #[test]
    fn descriptions_use_public_examples_that_map_to_valid_backing_requests() {
        let read_schema = crate::mcp::tool_definitions()[1]["inputSchema"].clone();
        for descriptor in DESCRIPTORS
            .iter()
            .filter(|descriptor| descriptor.op.is_some())
        {
            let op = descriptor.op.unwrap();
            let backing: Value = serde_json::from_str(op.example).unwrap();
            let mut public = backing.as_object().unwrap().clone();
            public.remove("op");
            for alias in descriptor.aliases {
                if let Some(value) = public.remove(alias.backing) {
                    public.insert(alias.public.to_string(), value);
                }
            }
            let public = Value::Object(public);
            let tool = generated(descriptor.name);
            crate::schema_validation::validate_shape(&tool["inputSchema"], &public, "")
                .unwrap_or_else(|error| panic!("{} public example: {error}", descriptor.name));
            let description = tool["description"].as_str().unwrap().to_string();
            assert!(
                description.contains(&public.to_string()),
                "{} lacks public example {public}",
                descriptor.name
            );
            match descriptor.kind {
                IntentKind::Execute => {
                    let mut request = public;
                    for alias in descriptor.aliases {
                        if let Some(value) = request.as_object_mut().unwrap().remove(alias.public) {
                            request[alias.backing] = value;
                        }
                    }
                    request["op"] = json!(op.name);
                    request["request_id"] = json!("intent-example");
                    crate::mcp_ops::validate_execute_request(&request, op.name)
                        .unwrap_or_else(|error| panic!("{} example: {error}", descriptor.name));
                }
                IntentKind::Read => {
                    let request = json!({"ocs_session_id":"s","op":op.name,"parameters":public});
                    crate::schema_validation::validate_shape(&read_schema, &request, "")
                        .unwrap_or_else(|error| panic!("{} example: {error}", descriptor.name));
                }
                IntentKind::Capture => unreachable!(),
            }
        }
    }

    fn comparable_schema(value: &Value, defs: &Value) -> Value {
        match value {
            Value::Object(object) => {
                if let Some(name) = object
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|reference| reference.strip_prefix("#/$defs/"))
                {
                    return comparable_schema(&defs[name], defs);
                }
                Value::Object(
                    object
                        .iter()
                        .filter(|(key, _)| key.as_str() != "description")
                        .map(|(key, value)| (key.clone(), comparable_schema(value, defs)))
                        .collect(),
                )
            }
            Value::Array(array) => Value::Array(
                array
                    .iter()
                    .map(|value| comparable_schema(value, defs))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }

    #[test]
    fn curated_read_schemas_match_the_legacy_read_parameter_constraints() {
        let curated: [(&str, &crate::mcp_ops::OpDef, &[&str]); 6] = [
            ("ocs_inspect", &crate::mcp_ops::STATE, &[]),
            (
                "ocs_query",
                &crate::mcp_ops::QUERY,
                &[
                    "handles",
                    "type",
                    "layer",
                    "detail",
                    "fields",
                    "where",
                    "near",
                    "contains_point",
                    "bounds",
                    "intersections",
                    "offset",
                    "limit",
                ],
            ),
            (
                "ocs_find_text",
                &crate::mcp_ops::TEXT_SEARCH,
                &[
                    "find",
                    "match_case",
                    "whole_word",
                    "ignore_accents",
                    "scope",
                ],
            ),
            (
                "ocs_audit",
                &crate::mcp_ops::AUDIT,
                &["path", "target_format", "target_version"],
            ),
            ("ocs_commands", &crate::mcp_ops::COMMANDS, &["name"]),
            ("ocs_measure", &crate::mcp_ops::MEASURE, &["handles"]),
        ];
        let tools = crate::mcp::tool_definitions();
        let legacy = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "ocs_read")
            .unwrap()["inputSchema"]["properties"]["parameters"]["properties"]
            .clone();
        let legacy = legacy.as_object().unwrap();

        for (name, op, expected_properties) in curated {
            let schema = generated(name)["inputSchema"].clone();
            let properties = schema["properties"].as_object().unwrap();
            let mut actual: Vec<_> = properties
                .keys()
                .filter(|key| key.as_str() != "ocs_session_id")
                .map(String::as_str)
                .collect();
            actual.sort_unstable();
            let mut expected = expected_properties.to_vec();
            expected.sort_unstable();
            assert_eq!(actual, expected, "{name} public properties for {}", op.name);

            for property in expected_properties {
                let public = comparable_schema(&properties[*property], &schema["$defs"]);
                let legacy = comparable_schema(&legacy[*property], &Value::Null);
                assert_eq!(public, legacy, "{name} parameter {property}");
            }
        }
    }

    #[test]
    fn commands_uses_the_command_manifest_name_filter() {
        let tool = generated("ocs_commands");
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        assert!(properties.contains_key("name"));
        assert!(!properties.contains_key("search"));
        assert!(tool["description"]
            .as_str()
            .unwrap()
            .contains(r#"{"name":"LINE"}"#));
    }

    #[test]
    fn render_reuses_capture_arguments_with_extents_default() {
        let facade = crate::mcp::tool_definitions()
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "ocs_capture")
            .unwrap()["inputSchema"]
            .clone();
        let render = generated("ocs_render");
        let schema = &render["inputSchema"];
        let facade_properties = facade["properties"].as_object().unwrap();
        let render_properties = schema["properties"].as_object().unwrap();
        assert_eq!(render_properties.len(), facade_properties.len() - 1);
        assert!(!render_properties.contains_key("request_id"));
        for name in facade_properties
            .keys()
            .filter(|name| *name != "request_id")
        {
            assert!(render_properties.contains_key(name), "render lacks {name}");
        }
        for (name, default) in [
            ("scope", json!("viewport")),
            ("max_dimension", json!(1600)),
            ("delivery", json!("inline")),
            ("annotate", json!(false)),
            ("pyramid_manifest", json!(false)),
        ] {
            assert_eq!(schema["properties"][name]["default"], default, "{name}");
            assert_eq!(
                schema["properties"][name], facade["properties"][name],
                "{name}"
            );
        }
        assert_eq!(facade["properties"]["view"]["default"], "current");
        assert_eq!(schema["properties"]["view"]["default"], "extents");
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["required"]
            .as_array()
            .is_none_or(|required| required.is_empty()));
        assert_eq!(render["annotations"]["readOnlyHint"], false);
        assert_eq!(render["annotations"]["destructiveHint"], false);
    }

    #[test]
    fn annotations_distinguish_reads_and_conservative_executes() {
        for descriptor in DESCRIPTORS {
            let tool = generated(descriptor.name);
            let annotations = &tool["annotations"];
            assert_eq!(annotations["destructiveHint"], descriptor.destructive);
            assert_eq!(annotations["openWorldHint"], false);
            match descriptor.kind {
                IntentKind::Read => {
                    assert_eq!(annotations["readOnlyHint"], true);
                    assert_eq!(annotations["idempotentHint"], true);
                    assert_eq!(annotations["destructiveHint"], false);
                }
                IntentKind::Capture => {
                    assert_eq!(annotations["readOnlyHint"], false);
                    assert_eq!(annotations["idempotentHint"], false);
                    assert_eq!(annotations["destructiveHint"], false);
                }
                IntentKind::Execute => {
                    assert_eq!(annotations["readOnlyHint"], false);
                    assert_eq!(annotations["idempotentHint"], false);
                }
            }
        }
        for name in [
            "ocs_run",
            "ocs_save",
            "ocs_save_verified",
            "ocs_cancel",
            "ocs_undo",
            "ocs_redo",
        ] {
            assert_eq!(
                generated(name)["annotations"]["destructiveHint"],
                true,
                "{name}"
            );
        }
    }

    #[test]
    fn read_and_execute_intents_advertise_facade_output_shapes() {
        for descriptor in DESCRIPTORS {
            let tool = generated(descriptor.name);
            match descriptor.kind {
                IntentKind::Read | IntentKind::Execute => {
                    assert_eq!(
                        tool["outputSchema"]["type"], "object",
                        "{} lacks its facade output schema",
                        descriptor.name
                    );
                    assert_eq!(tool["outputSchema"]["required"], json!(["ok"]));
                }
                IntentKind::Capture => assert!(
                    tool.get("outputSchema").is_none(),
                    "{} capture results carry image content instead",
                    descriptor.name
                ),
            }
        }
    }

    #[test]
    fn all_tools_keep_facades_first_and_append_sixteen_intents() {
        let tools = crate::mcp::tool_definitions();
        let tools = tools.as_array().unwrap();
        assert_eq!(tools.len(), 20);
        let names: Vec<_> = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            &names[..4],
            ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"]
        );
        assert_eq!(&names[4..], NAMES);
    }
}
