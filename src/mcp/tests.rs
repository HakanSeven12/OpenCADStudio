use super::*;
use super::cache::*;
use super::resources::*;
use super::session::*;
use super::tools::*;
use super::transport::*;
use crate::mcp_ops::{MAX_BATCH_STEPS, batch_bindings, validate_execute_request};
use serde_json::{Value, json};
use serial_test::serial;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// A pump with no reader: waits behave exactly as before (quiet
/// immediately), so tests that never cancel need no other changes.
fn detached_pump() -> CancelPump {
    let (_tx, rx) = std::sync::mpsc::channel();
    CancelPump {
        rx,
        backlog: VecDeque::new(),
    }
}

#[test]
fn advertises_the_shared_tools() {
    let tools = tool_definitions();
    let names: Vec<_> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        &names[..4],
        ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"]
    );
    assert_eq!(names.len(), 22);
    // The build announcement must live in the entry-point tool
    // description: global instructions don't reliably reach agents.
    assert!(
        tools[0]["description"]
            .as_str()
            .unwrap()
            .contains("`bridge`"),
        "ocs_sessions description must point at the bridge build identity"
    );
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);
    for tool in [&tools[1], &tools[2], &tools[3]] {
        assert!(
            tool["inputSchema"]["properties"]
                .get("session_id")
                .is_none()
        );
    }
    for tool in [&tools[2], &tools[3]] {
        assert!(
            tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("ocs_session_id"))
        );
    }
    let read_schema = &tools[1]["inputSchema"];
    for op in ["guide", "entity_schema", "validate"] {
        crate::schema_validation::validate_shape(read_schema, &json!({"op":op}), "").unwrap();
    }
    assert!(
        crate::schema_validation::validate_shape(read_schema, &json!({"op":"state"}), "")
            .is_err()
    );
    assert_eq!(
        tools[2]["inputSchema"]["properties"]["request"]["required"],
        json!(["op", "request_id"])
    );
    let request_variants = tools[2]["inputSchema"]["properties"]["request"]["anyOf"]
        .as_array()
        .unwrap();
    assert_eq!(request_variants.len(), EXECUTE_OPS.len());
    assert_eq!(
        tools[2]["inputSchema"]["properties"]["request"]["properties"]["steps"]["maxItems"],
        MAX_BATCH_STEPS
    );
    assert_eq!(
        tools[2]["inputSchema"]["properties"]["response_detail"]["default"],
        "compact"
    );
    assert_eq!(
        tools[3]["inputSchema"]["properties"]["scope"]["default"],
        "viewport"
    );
    assert_eq!(
        tools[3]["inputSchema"]["properties"]["delivery"]["default"],
        "inline"
    );
    let find_variant = |op_name: &str| {
        request_variants
            .iter()
            .map(|v| {
                tools[2]["inputSchema"]
                    .pointer(v["$ref"].as_str().unwrap().strip_prefix('#').unwrap())
                    .expect("operation reference resolves")
            })
            .find(|v| v["properties"]["op"]["enum"][0] == op_name)
            .cloned()
            .unwrap_or_else(|| panic!("variant for {op_name} exists"))
    };
    let run_var = find_variant("run");
    assert_eq!(run_var["properties"]["cmd"]["type"], "string");
    assert!(READ_OPS.contains(&"capabilities"));
    assert!(READ_OPS.contains(&"blocks"));
    assert!(READ_OPS.contains(&"records"));
    assert!(READ_OPS.contains(&"record_schema"));
    assert!(READ_OPS.contains(&"audit"));
    assert!(READ_OPS.contains(&"text_search"));
    assert!(READ_OPS.contains(&"text_audit"));
    assert!(EXECUTE_OPS.contains(&"set_properties"));
    assert!(EXECUTE_OPS.contains(&"save_verified"));
    assert!(EXECUTE_OPS.contains(&"text_replace"));
    assert!(crate::mcp_ops::find_op("text_replace").unwrap().batchable);
    let save_var = find_variant("save_verified");
    assert_eq!(save_var["properties"]["target_version"]["enum"][0], "R14");
    assert_eq!(
        tools[1]["inputSchema"]["properties"]["parameters"]["properties"]["where"]["items"]["properties"]
            ["op"]["enum"],
        json!([
            "eq",
            "ne",
            "lt",
            "lte",
            "gt",
            "gte",
            "contains",
            "starts_with",
            "ends_with",
            "in",
            "exists",
            "not_exists"
        ])
    );
    let set_props_var = find_variant("set_properties");
    assert_eq!(
        set_props_var["properties"]["updates"]["items"]["required"],
        json!(["path", "value"])
    );
    assert!(tools[0].get("outputSchema").is_some());
    assert!(tools[1].get("outputSchema").is_some());
    assert!(tools[2].get("outputSchema").is_some());
}

#[test]
fn negotiates_and_lists_tools() {
    let mut clients = HashMap::new();
    let initialized = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        initialized["result"]["capabilities"]["resources"]["subscribe"],
        false
    );
    assert!(
        initialized["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("geometry kernel")
    );

    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 22);
    // SEP-2549: freshness hints ship on every protocol version; only
    // the modern envelope stays negotiated (see
    // legacy_list_results_carry_sep2549_ttl).
    assert_eq!(listed["result"]["ttlMs"], CACHE_TTL_MS);
    assert_eq!(listed["result"]["cacheScope"], "public");
    assert!(listed["result"].get("resultType").is_none());
}

#[test]
fn supports_modern_stateless_discovery() {
    let mut clients = HashMap::new();
    let discovered = handle_message(
        json!({"jsonrpc":"2.0","id":"discover","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(discovered["result"]["resultType"], "complete");
    assert_eq!(discovered["result"]["ttlMs"], CACHE_TTL_MS);
    assert_eq!(discovered["result"]["cacheScope"], "public");
    assert_eq!(discovered["result"]["supportedVersions"][0], "2026-07-28");
    assert!(discovered["result"]["capabilities"]["resources"].is_object());
    assert!(
        discovered["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/tasks"]
            .is_object()
    );
    assert_eq!(
        discovered["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "OpenCADStudio"
    );

    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":"tools","method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(listed["result"]["resultType"], "complete");
    assert_eq!(listed["result"]["ttlMs"], CACHE_TTL_MS);
    assert_eq!(listed["result"]["cacheScope"], "public");
}

#[test]
fn resource_lists_use_the_short_dynamic_ttl() {
    // resources/* vary as sessions open and snapshots land: 60 s.
    // Static tools/discover keep the hour.
    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":"res","method":"resources/list"}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(listed["result"]["ttlMs"], RESOURCE_TTL_MS);
    assert_eq!(listed["result"]["ttlMs"], 60_000);
}

#[test]
fn legacy_list_results_carry_sep2549_ttl() {
    // SEP-2549 makes ttlMs mandatory on list results; legacy clients
    // ignore unknown fields, so the stamp is version-independent while
    // the modern envelope (resultType/_meta) stays negotiated.
    for (method, ttl) in [
        ("tools/list", CACHE_TTL_MS),
        ("resources/list", RESOURCE_TTL_MS),
    ] {
        let listed = handle_message(
            json!({"jsonrpc":"2.0","id":method,"method":method}),
            &mut HashMap::new(),
            &mut TaskStore::default(),
            &mut ResourceStore::default(),
            &mut detached_pump(),
        )
        .unwrap();
        assert_eq!(listed["result"]["ttlMs"], ttl, "{method}");
        assert_eq!(listed["result"]["cacheScope"], "public", "{method}");
        assert!(listed["result"].get("resultType").is_none(), "{method}");
        assert!(listed["result"].get("_meta").is_none(), "{method}");
    }
}

#[test]
fn initialize_is_spec_pure_with_experimental_build() {
    // serverInfo carries only the spec'd name/title/version; build
    // extras live canonically in capabilities.experimental (single
    // source: bridge_identity), never copied.
    let initialized = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    let info = &initialized["result"]["serverInfo"];
    assert_eq!(
        info.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["name", "title", "version"]
    );
    assert_eq!(info["name"], "OpenCADStudio");
    let build = &initialized["result"]["capabilities"]["experimental"]["opencadstudio.build"];
    assert_eq!(build["tool_schema"], Value::String(tool_schema_digest()));
    assert!(build["build_rev"].as_str().is_some());
    assert!(build["build_profile"].as_str().is_some());
    // Unknown versions fall back to our newest instead of erroring.
    let fallback = handle_message(
        json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(fallback["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(fallback.get("error").is_none());
}

#[test]
fn resource_not_found_code_follows_era() {
    // Modern path MUST NOT emit -32002 (2026-07-28); legacy path keeps
    // it (it was the code in 2025-11-25 and earlier).
    let modern = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"cad://session/nope/snapshot/nope.png","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(modern["error"]["code"], -32602);
    let legacy = handle_message(
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"cad://session/nope/snapshot/nope.png"}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(legacy["error"]["code"], -32002);
}

#[test]
fn non_object_input_is_invalid_request() {
    // Batches and other non-objects get -32600 (so senders never hang)
    // while id-less objects stay silent (notifications).
    for bad in [json!([1, 2]), json!("tools/list"), json!(42), json!(null)] {
        let rejected = handle_message(
            bad,
            &mut HashMap::new(),
            &mut TaskStore::default(),
            &mut ResourceStore::default(),
            &mut detached_pump(),
        );
        assert_eq!(rejected.unwrap()["error"]["code"], -32600);
    }
    let silent = handle_message(
        json!({"jsonrpc":"2.0","method":"ping"}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    );
    assert!(silent.is_none());
}

#[test]
fn unknown_tool_is_a_protocol_error_with_names() {
    // Spec lists unknown tools under Protocol Errors (-32602). The
    // message carries the valid names so the model can still recover.
    let unknown = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ocs_frobnicate","arguments":{}}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(unknown["error"]["code"], -32602);
    let message = unknown["error"]["message"].as_str().unwrap();
    for name in ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"]
        .into_iter()
        .chain(crate::mcp_intent::DESCRIPTORS.iter().map(|d| d.name))
    {
        assert!(message.contains(name), "{message}");
    }
}

#[test]
fn every_advertised_intent_passes_the_unknown_tool_gate() {
    for (index, descriptor) in crate::mcp_intent::DESCRIPTORS.iter().enumerate() {
        let called = handle_message(
            json!({"jsonrpc":"2.0","id":index,"method":"tools/call","params":{"name":descriptor.name,"arguments":{"ocs_session_id":"offline"}}}),
            &mut HashMap::new(),
            &mut TaskStore::default(),
            &mut ResourceStore::default(),
            &mut detached_pump(),
        )
        .unwrap();
        assert_ne!(
            called["error"]["code"], -32602,
            "{} was rejected as unknown: {called}",
            descriptor.name
        );
    }
}

#[test]
fn every_intent_dispatches_through_its_existing_facade() {
    for descriptor in crate::mcp_intent::DESCRIPTORS {
        let mut arguments = descriptor
            .op
            .map(|op| {
                let example: Value = serde_json::from_str(op.example).unwrap();
                let mut example = example.as_object().unwrap().clone();
                example.remove("op");
                for alias in descriptor.aliases {
                    if let Some(value) = example.remove(alias.backing) {
                        example.insert(alias.public.to_string(), value);
                    }
                }
                Value::Object(example)
            })
            .unwrap_or_else(|| json!({}));
        arguments["ocs_session_id"] = json!("offline");
        if descriptor.kind == crate::mcp_intent::IntentKind::Execute {
            arguments["request_id"] = json!("caller-id");
        }
        let error = call_tool(
            descriptor.name,
            &arguments,
            &mut HashMap::new(),
            &mut ResourceStore::default(),
            &mut detached_pump(),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(error, CallError::Infra(_)),
            "{} did not reach its facade: {error:?}",
            descriptor.name
        );
    }
}

#[test]
fn task_arguments_retain_a_generated_intent_request_id() {
    let arguments = task_arguments(
        "ocs_run",
        &json!({"command":"LINE","ocs_session_id":"session-1"}),
        &json!({"status":"running","request_id":"generated-id"}),
    );
    assert_eq!(
        arguments,
        json!({
            "command":"LINE",
            "ocs_session_id":"session-1",
            "request_id":"generated-id"
        })
    );

    let caller = task_arguments(
        "ocs_run",
        &json!({"command":"LINE","request_id":"caller-id"}),
        &json!({"status":"running","request_id":"generated-id"}),
    );
    assert_eq!(caller["request_id"], "caller-id");
    assert_eq!(
        task_arguments(
            "ocs_inspect",
            &json!({}),
            &json!({"status":"running","request_id":"generated-id"}),
        ),
        json!({})
    );
}

#[test]
fn intent_task_redispatch_reuses_the_request_and_implicit_session() {
    let client = |session_id: &str| GuiClient {
        descriptor: Descriptor {
            session_id: session_id.into(),
            port: 0,
            token: String::new(),
            pid: None,
        },
        state: json!({}),
        client_id: "test".into(),
        batches: VecDeque::new(),
        inflight: HashMap::new(),
        dismissed: VecDeque::new(),
    };
    let mut clients = HashMap::new();
    clients.insert("session-1".into(), client("session-1"));

    let normalized =
        normalize_intent_arguments("ocs_run", &json!({"command":"LINE"}), &clients).unwrap();
    assert_eq!(normalized["ocs_session_id"], "session-1");
    let request_id = normalized["request_id"].as_str().unwrap();
    let descriptor = crate::mcp_intent::find_descriptor("ocs_run").unwrap();
    let (facade, initial) =
        crate::mcp_intent::facade_arguments(descriptor, &normalized, "session-1", None);
    assert_eq!(facade, "ocs_execute");
    assert_eq!(
        initial["request"],
        json!({"op":"run","cmd":"LINE","request_id":request_id})
    );

    let retained = task_arguments(
        "ocs_run",
        &normalized,
        &json!({"status":"running","request_id":request_id}),
    );
    clients.insert("session-2".into(), client("session-2"));
    let mut poll = normalize_intent_arguments("ocs_run", &retained, &clients).unwrap();
    poll["wait_seconds"] = json!(0);
    let (_, polled) = crate::mcp_intent::facade_arguments(
        descriptor,
        &poll,
        poll["ocs_session_id"].as_str().unwrap(),
        None,
    );

    let mut expected = initial;
    expected["wait_seconds"] = json!(0);
    assert_eq!(polled, expected);
    assert_eq!(polled["ocs_session_id"], "session-1");
    assert_eq!(polled["request"]["request_id"], request_id);
    assert!(polled["request"].get("wait_seconds").is_none());
}

#[test]
fn intent_execute_keeps_facade_request_id_validation() {
    let error = call_tool(
        "ocs_run",
        &json!({"ocs_session_id":"offline","command":"LINE","request_id":""}),
        &mut HashMap::new(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
        None,
    )
    .unwrap_err();
    assert!(matches!(error, CallError::Tool(message) if message == "Missing request_id"));
}

#[test]
fn intent_session_selection_prefers_an_explicit_non_empty_id() {
    let selected = select_session_candidate(
        Some("explicit"),
        &["connected".to_string()],
        &["discovered".to_string()],
    )
    .unwrap();
    assert_eq!(selected.as_deref(), Some("explicit"));

    let selected = select_session_candidate(
        Some(""),
        &["connected".to_string()],
        &["discovered".to_string()],
    )
    .unwrap();
    assert_eq!(selected.as_deref(), Some("connected"));
}

#[test]
fn intent_session_selection_uses_the_sole_connected_or_discovered_session() {
    let connected = select_session_candidate(
        None,
        &["connected".to_string()],
        &["discovered-1".to_string(), "discovered-2".to_string()],
    )
    .unwrap();
    assert_eq!(connected.as_deref(), Some("connected"));

    let discovered = select_session_candidate(None, &[], &["discovered".to_string()]).unwrap();
    assert_eq!(discovered.as_deref(), Some("discovered"));
    assert_eq!(select_session_candidate(None, &[], &[]).unwrap(), None);
}

#[test]
fn intent_session_selection_rejects_multiple_candidates() {
    for (connected, discovered) in [
        (vec!["one".to_string(), "two".to_string()], vec![]),
        (vec![], vec!["one".to_string(), "two".to_string()]),
    ] {
        let error = select_session_candidate(None, &connected, &discovered).unwrap_err();
        assert!(error.contains("Multiple"), "{error}");
        assert!(error.contains("ocs_session_id"), "{error}");
    }
}

#[test]
fn infrastructure_failures_are_marked_infra() {
    // A session id matching no live GUI is bridge/GUI infrastructure,
    // not a tool-domain error: must route to -32603, not isError text.
    let mut clients = HashMap::new();
    let mut resources = ResourceStore::default();
    let err = call_tool(
        "ocs_read",
        &json!({"ocs_session_id":"00000000","op":"state"}),
        &mut clients,
        &mut resources,
        &mut detached_pump(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, CallError::Infra(_)), "got {err:?}");
}

#[test]
fn cancelled_notifications_are_accepted_silently() {
    // No response to notifications, even for cancel (nothing in flight
    // in a unit test; live waits honor it — see wait_deadline below).
    let silent = handle_message(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    );
    assert!(silent.is_none());
}

#[test]
fn cancel_matching_and_wait_deadlines() {
    // Only a cancel naming our in-flight MCP id counts; anything else
    // (other ids, pings, results) is ignored by the wait, not eaten.
    assert!(is_cancel_for(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#,
        "7"
    ));
    assert!(!is_cancel_for(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":8}}"#,
        "7"
    ));
    assert!(!is_cancel_for(
        r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
        "7"
    ));
    assert!(!is_cancel_for("not json at all", "7"));
    // String ids compare as strings: 7 != "7".
    assert!(!is_cancel_for(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#,
        "\"7\""
    ));
    // Interactive picks may legitimately take minutes; everything else
    // keeps the 60 s clamp.
    assert_eq!(
        wait_deadline("user_select", 3600.0),
        Duration::from_secs(600)
    );
    assert_eq!(wait_deadline("getpoint", 5.0), Duration::from_secs(5));
    assert_eq!(wait_deadline("run", 3600.0), Duration::from_secs(60));
    assert_eq!(wait_deadline("run", 5.0), Duration::from_secs(5));
    // The ceiling dismisses; anything below returns running for re-poll.
    assert!(!interactive_timeout(5.0));
    assert!(!interactive_timeout(599.0));
    assert!(interactive_timeout(600.0));
    assert!(interactive_timeout(3600.0));
}

#[test]
fn templates_list_serves_tile_and_snapshot_templates() {
    for params in [
        json!({}),
        json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}),
    ] {
        let listed = handle_message(
            json!({"jsonrpc":"2.0","id":"tpl","method":"resources/templates/list","params":params}),
            &mut HashMap::new(),
            &mut TaskStore::default(),
            &mut ResourceStore::default(),
            &mut detached_pump(),
        )
        .unwrap();
        let templates = listed["result"]["resourceTemplates"].as_array().unwrap();
        assert!(
            templates
                .iter()
                .any(|t| t["uriTemplate"].as_str().unwrap().contains("{level}"))
        );
        assert!(
            templates
                .iter()
                .any(|t| t["uriTemplate"].as_str().unwrap().contains("{hash}"))
        );
        assert_eq!(listed["result"]["ttlMs"], RESOURCE_TTL_MS);
        assert_eq!(listed["result"]["cacheScope"], "public");
    }
}

#[test]
fn tool_titles_are_top_level_and_annotations_allowlisted() {
    // Title lives top-level (2026-07-28 Tool shape), not nested.
    let tools = tool_definitions();
    for tool in tools.as_array().unwrap() {
        assert!(tool["title"].as_str().is_some());
        assert!(tool["annotations"].get("title").is_none());
        let annotations = tool["annotations"].as_object().unwrap();
        for key in annotations.keys() {
            assert!(
                [
                    "readOnlyHint",
                    "destructiveHint",
                    "idempotentHint",
                    "openWorldHint"
                ]
                .contains(&key.as_str()),
                "unexpected annotation key: {key}"
            );
        }
    }
}

#[test]
fn structured_results_conform_to_their_output_schemas() {
    // Spot-check the shape every schema'd result must have: an object
    // carrying ok, reachable through tool_result unchanged.
    let sample = json!({
        "ok": true,
        "status": "completed",
        "document_id": 1,
        "revision": 2,
        "geometry_revision": 3,
        "camera_revision": 4
    });
    let result = tool_result(sample);
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["ok"], true);
    assert_eq!(result["structuredContent"]["revision"], 2);
}

fn modern_params() -> Value {
    json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}}}}})
}

fn stub_task(id: &str) -> McpTask {
    McpTask {
        id: id.into(),
        name: "ocs_read".into(),
        arguments: json!({}),
        created_at: iso8601_now(),
        last_updated_at: iso8601_now(),
        result: None,
        error: None,
        cancelled: false,
    }
}

#[test]
fn tasks_cancel_marks_cancelled_without_result() {
    let mut tasks = TaskStore::default();
    tasks.insert(stub_task("t-cancel"));
    let cancelled = handle_message(
        json!({"jsonrpc":"2.0","id":"c","method":"tasks/cancel","params":{"taskId":"t-cancel","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}}}}}}),
        &mut HashMap::new(),
        &mut tasks,
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert!(cancelled.get("error").is_none());
    let mut params = modern_params();
    params["taskId"] = Value::String("t-cancel".into());
    let status = handle_message(
        json!({"jsonrpc":"2.0","id":"g","method":"tasks/get","params":params}),
        &mut HashMap::new(),
        &mut tasks,
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(status["result"]["status"], "cancelled");
    assert!(status["result"].get("result").is_none());
    assert!(status["result"].get("error").is_none());
    // A late completion landing after the cancel is discarded.
    tasks.get_mut("t-cancel").unwrap().result = Some(json!({"ok": true}));
    let mut params = modern_params();
    params["taskId"] = Value::String("t-cancel".into());
    let again = handle_message(
        json!({"jsonrpc":"2.0","id":"g2","method":"tasks/get","params":params}),
        &mut HashMap::new(),
        &mut tasks,
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(again["result"]["status"], "cancelled");
    assert!(again["result"].get("result").is_none());
}

#[test]
fn task_envelope_errors_use_public_codes() {
    // The legacy -32000 range is grandfathered, not for new use.
    let mut task = stub_task("t-fail");
    task.arguments = json!({"ocs_session_id":"00000000","op":"state","wait_seconds":0});
    let failed = poll_task(
        &mut task,
        &mut HashMap::new(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    );
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], -32603);
}

#[test]
#[serial]
fn schema_sync_respects_opt_out() {
    std::env::set_var("OCS_SKIP_SCHEMA_SYNC", "1");
    assert!(!schema_sync_enabled());
    std::env::remove_var("OCS_SKIP_SCHEMA_SYNC");
    assert!(schema_sync_enabled());
}

#[test]
fn stale_snapshots_never_delete_fresh_descriptors() {
    // Regression: a cached PID snapshot predates a GUI spawn, so the
    // newborn pid is "missing" from it. Deletion must still wait until
    // the file is older than the grace period (slow starters write
    // late); unknown snapshot state never deletes either.
    assert!(!stale_snapshot_may_delete(true, Duration::from_secs(0)));
    assert!(!stale_snapshot_may_delete(true, Duration::from_secs(59)));
    assert!(stale_snapshot_may_delete(true, Duration::from_secs(120)));
    assert!(stale_snapshot_may_delete(true, Duration::from_secs(3600)));
    assert!(!stale_snapshot_may_delete(false, Duration::from_secs(3600)));
}

#[test]
fn capture_accepts_optional_request_id() {
    // Strict clients insist on sending request_id even though capture
    // is not idempotency-keyed: declare it optional so they can.
    let tools = tool_definitions();
    let capture = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ocs_capture")
        .unwrap();
    let prop = &capture["inputSchema"]["properties"]["request_id"];
    assert_eq!(prop["type"], "string");
    assert!(
        !capture["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("request_id"))
    );
}

#[test]
fn read_op_capture_points_at_the_capture_tool() {
    // ocs_read advertises "capture" but the GUI op needs a file path;
    // agents hitting it bare get guidance, not a cryptic failure.
    let guided = call_tool(
        "ocs_read",
        &json!({"ocs_session_id":"s","op":"capture"}),
        &mut HashMap::new(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
        None,
    )
    .unwrap();
    assert_eq!(guided["ok"], false);
    assert!(guided["error"].as_str().unwrap().contains("ocs_capture"));
}

#[test]
fn malformed_requests_get_jsonrpc_errors() {
    let unknown = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"frobnicate"}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
    let nameless = handle_message(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(nameless["error"]["code"], -32602);
    let uriless = handle_message(
        json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{}}),
        &mut HashMap::new(),
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(uriless["error"]["code"], -32602);
}

#[test]
fn read_tool_rejects_mutations_before_connecting() {
    let mut clients = HashMap::new();
    let called = handle_message(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ocs_read","arguments":{"ocs_session_id":"missing","op":"save"}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(called["result"]["isError"], true);
}

#[test]
fn rejects_unknown_modern_protocol_versions() {
    let mut clients = HashMap::new();
    let rejected = handle_message(
        json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2099-01-01"}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(rejected["error"]["code"], -32022);
    assert_eq!(rejected["error"]["data"]["requested"], "2099-01-01");
    assert_eq!(
        rejected["error"]["data"]["supported"][0],
        MODERN_PROTOCOL_VERSION
    );
}

#[test]
fn execute_requires_a_visible_request_id() {
    let mut clients = HashMap::new();
    let called = handle_message(
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"ocs_execute","arguments":{"ocs_session_id":"missing","request":{"op":"undo"}}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(called["result"]["isError"], true);
    assert!(called["result"].get("structuredContent").is_none());
    assert!(
        called["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("request_id")
    );
}

#[test]
fn execute_errors_explain_missing_operation_fields() {
    let mut clients = HashMap::new();
    let called = handle_message(
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"ocs_execute","arguments":{"ocs_session_id":"missing","request":{"op":"run","request_id":"run-1"}}}}),
        &mut clients,
        &mut TaskStore::default(),
        &mut ResourceStore::default(),
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(called["result"]["isError"], true);
    assert!(called["result"].get("structuredContent").is_none());
    let error = called["result"]["content"][0]["text"].as_str().unwrap();
    assert!(error.contains("Missing cmd"), "{error}");
    assert!(error.contains("LINE 0,0 10,10"), "{error}");
}

#[test]
fn resources_list_and_read_snapshots() {
    let mut clients = HashMap::new();
    let mut tasks = TaskStore::default();
    let mut resources = ResourceStore::default();

    let dummy_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let dummy_hash = "abcdef0123456789";
    let uri = resources.insert(
        "sess1",
        dummy_hash,
        dummy_data.to_string(),
        68,
        json!({"width": 100, "height": 100, "annotations": []}),
    );
    assert_eq!(uri, "cad://session/sess1/snapshot/abcdef0123456789.png");

    // Test resources/list
    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":"r-list","method":"resources/list"}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let res_arr = listed["result"]["resources"].as_array().unwrap();
    assert!(res_arr.iter().any(|r| r["uri"] == uri));
    assert!(
        res_arr
            .iter()
            .any(|r| r["uri"] == "cad://session/sess1/snapshot/latest.png")
    );

    // Test resources/read with png
    let read_png = handle_message(
        json!({"jsonrpc":"2.0","id":"r-read","method":"resources/read","params":{"uri":uri}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let contents = read_png["result"]["contents"].as_array().unwrap();
    assert_eq!(contents[0]["mimeType"], "image/png");
    assert_eq!(contents[0]["blob"], dummy_data);

    // Test resources/read with latest
    let read_latest = handle_message(
        json!({"jsonrpc":"2.0","id":"r-read-lat","method":"resources/read","params":{"uri":"cad://session/sess1/snapshot/latest.png"}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(read_latest["result"]["contents"][0]["blob"], dummy_data);

    // Test resources/read with json metadata
    let read_meta = handle_message(
        json!({"jsonrpc":"2.0","id":"r-read-meta","method":"resources/read","params":{"uri":"cad://session/sess1/snapshot/abcdef0123456789.json"}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let text = read_meta["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"width\": 100"));

    // Legacy resources/read on non-existent resource keeps -32002
    // (modern gets -32602; see resource_not_found_code_follows_era)
    let read_err = handle_message(
        json!({"jsonrpc":"2.0","id":"r-err","method":"resources/read","params":{"uri":"cad://session/sess1/snapshot/nonexistent.png"}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(read_err["error"]["code"], -32002);
}

#[test]
fn tool_result_handles_resource_and_both_modes() {
    // Resource-only delivery mode
    let res_value = json!({
        "$resource": {
            "uri": "cad://session/s1/snapshot/hash123.png",
            "mimeType": "image/png",
            "hash": "hash123",
            "bytes": 1024
        },
        "metadata": {
            "width": 800,
            "height": 600,
            "_spatial": {
                "crs": "CAD_WCS",
                "unit": "Millimeters",
                "pixel_to_world_matrix": [0.05, 0.0, 0.0, -0.05, -20.0, 15.0],
                "world_to_pixel_matrix": [20.0, 0.0, 0.0, -20.0, 400.0, 300.0]
            }
        }
    });
    let res_res = tool_result(res_value);
    assert_eq!(res_res["isError"], false);
    assert_eq!(res_res["structuredContent"]["_spatial"]["crs"], "CAD_WCS");
    assert_eq!(
        res_res["structuredContent"]["_spatial"]["unit"],
        "Millimeters"
    );
    assert_eq!(
        res_res["structuredContent"]["_spatial"]["pixel_to_world_matrix"],
        json!([0.05, 0.0, 0.0, -0.05, -20.0, 15.0])
    );
    let content = res_res["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .contains("cad://session/s1/snapshot/hash123.png")
    );
    assert_eq!(content[1]["type"], "resource_link");
    assert_eq!(content[1]["name"], "Captured viewport");
    assert_eq!(content[1]["mimeType"], "image/png");
    assert!(content[1].get("resource").is_none());
    assert_eq!(content[1]["uri"], "cad://session/s1/snapshot/hash123.png");
    // Ensure no image block was sent
    assert!(!content.iter().any(|c| c["type"] == "image"));

    // Both mode
    let both_value = json!({
        "$image": "base64data",
        "$resource": {
            "uri": "cad://session/s1/snapshot/hash123.png",
            "mimeType": "image/png",
            "hash": "hash123",
            "bytes": 1024
        },
        "metadata": {"width": 800}
    });
    let both_res = tool_result(both_value);
    let both_content = both_res["content"].as_array().unwrap();
    assert!(both_content.iter().any(|c| c["type"] == "resource_link"));
    assert!(both_content.iter().any(|c| c["type"] == "image"));
}

#[test]
fn capture_does_not_advertise_a_visual_diff() {
    let tools = tool_definitions();
    let capture_tool = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ocs_capture")
        .expect("ocs_capture tool exists");
    let props = &capture_tool["inputSchema"]["properties"];
    for retired in ["diff", "diff_mode", "reset_diff_baseline"] {
        assert!(
            props.get(retired).is_none(),
            "ocs_capture still advertises '{retired}'"
        );
    }
    assert_eq!(props["delivery"]["type"], "string");

    for retired in ["diff", "diff_mode", "reset_diff_baseline"] {
        let mut arguments = json!({"ocs_session_id": "s1"});
        arguments[retired] = json!(true);
        let error = refuse_retired_capture_parameters(&arguments)
            .expect_err("the retired parameter must be refused, not ignored");
        assert!(
            error.contains(retired),
            "'{retired}' must be named in the refusal: {error}"
        );
    }
    assert!(refuse_retired_capture_parameters(&json!({"ocs_session_id": "s1"})).is_ok());
}

#[test]
fn capture_schema_exposes_layout_and_aspect_fit() {
    let schema = capture_input_schema();
    let props = schema["properties"].as_object().unwrap();
    assert_eq!(props["layout"]["type"], "string");
    assert_eq!(props["layout"]["minLength"], 1);
    assert_eq!(props["aspect_fit"]["type"], "boolean");
    assert_eq!(props["aspect_fit"]["default"], false);
    assert_eq!(props["renderer"]["type"], "string");
    assert_eq!(props["renderer"]["enum"], json!(["auto", "gpu", "cpu"]));
    assert_eq!(props["renderer"]["default"], "auto");
    assert_eq!(props["document_id"]["type"], "integer");
    assert_eq!(props["document_id"]["minimum"], 0);

    let required = schema["required"].as_array().unwrap();
    assert!(!required.contains(&json!("layout")));
    assert!(!required.contains(&json!("aspect_fit")));
    assert!(!required.contains(&json!("renderer")));
    assert!(!required.contains(&json!("document_id")));
}

#[test]
fn capture_layout_cache_isolates_bounds_across_layouts() {
    let mut resources = ResourceStore::default();
    let m_uri = resources.insert_layout_manifest(
        "s1",
        "Model",
        json!({"world_bounds":[0.0, 0.0, 1000.0, 500.0],"renderer":"gpu"}),
    );
    assert_eq!(m_uri, "cad://session/s1/layout/Model/pyramid/manifest.json");
    let s_uri = resources.insert_layout_manifest(
        "s1",
        "Sheet 1",
        json!({"world_bounds":[0.0, 0.0, 297.0, 210.0],"renderer":"cpu"}),
    );
    assert_eq!(
        s_uri,
        "cad://session/s1/layout/Sheet%201/pyramid/manifest.json"
    );

    assert_eq!(
        resolve_tile_base_bounds(&json!({}), &resources, "s1", Some("model")).unwrap(),
        [0.0, 0.0, 1000.0, 500.0]
    );
    assert_eq!(
        resolve_tile_base_bounds(&json!({}), &resources, "s1", Some("sheet 1")).unwrap(),
        [0.0, 0.0, 297.0, 210.0]
    );
    assert!(
        resolve_tile_base_bounds(&json!({}), &resources, "s1", Some("Uncaptured")).is_err()
    );
    assert_eq!(
        resolve_tile_base_bounds(
            &json!({"bounds": [0.0, 0.0, 420.0, 297.0]}),
            &resources,
            "s1",
            Some("Uncaptured"),
        )
        .unwrap(),
        [0.0, 0.0, 420.0, 297.0]
    );

    let t_model = resources.insert_tile_for_layout(
        "s1",
        Some("Model"),
        1,
        0,
        0,
        "AAAA".into(),
        3,
        json!({"layout":"Model","tag":"model-tile"}),
    );
    let t_sheet = resources.insert_tile_for_layout(
        "s1",
        Some("Sheet 1"),
        1,
        0,
        0,
        "BBBB".into(),
        3,
        json!({"layout":"Sheet 1","tag":"sheet-tile"}),
    );
    assert_eq!(t_model, "cad://session/s1/layout/Model/tile/1/0/0.png");
    assert_eq!(t_sheet, "cad://session/s1/layout/Sheet%201/tile/1/0/0.png");

    let mut clients = HashMap::new();
    let mut tasks = TaskStore::default();
    let read_s_man = handle_message(
        json!({"jsonrpc":"2.0","id":"r-sman","method":"resources/read","params":{"uri":s_uri}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let s_man_txt = read_s_man["result"]["contents"][0]["text"]
        .as_str()
        .unwrap();
    assert!(s_man_txt.contains("cad://session/s1/layout/Sheet%201/tile/{level}/{x}/{y}.png"));

    let read_m_tile = handle_message(
        json!({"jsonrpc":"2.0","id":"r-mtile","method":"resources/read","params":{"uri":t_model}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    assert_eq!(read_m_tile["result"]["contents"][0]["blob"], "AAAA");

    let read_s_tile_json = handle_message(
        json!({"jsonrpc":"2.0","id":"r-stile-json","method":"resources/read","params":{"uri":"cad://session/s1/layout/Sheet%201/tile/1/0/0.json"}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    assert!(
        read_s_tile_json["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("sheet-tile")
    );

    let untagged = json!({"world_bounds": [0.0, 0.0, 10.0, 10.0]});
    let model = json!({"layout": "Model", "world_bounds": [0.0, 0.0, 10.0, 10.0]});
    assert!(!manifest_layout_differs(Some(&untagged), None));
    assert!(manifest_layout_differs(Some(&untagged), Some("Sheet1")));
    assert!(!manifest_layout_differs(Some(&model), Some("model")));
    assert!(manifest_layout_differs(Some(&model), Some("Sheet1")));
    assert!(manifest_layout_differs(Some(&model), None));
}

#[test]
fn every_capture_delivers_its_image() {
    for metadata in [
        json!({"hash":"hash-1","uri":"cad://session/s/snapshot/hash-1.png"}),
        json!({"hash":"hash-2","uri":"cad://session/s/snapshot/hash-2.png","revision":12}),
    ] {
        let result = tool_result(json!({
            "$image":"capture-png",
            "metadata":metadata
        }));
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"], metadata);
        assert!(
            result["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["type"] == "image" && block["data"] == "capture-png")
        );
    }
}

#[test]
fn pyramid_tiling_protocol_and_resources() {
    let mut clients = HashMap::new();
    let mut tasks = TaskStore::default();
    let mut resources = ResourceStore::default();

    // 1. Verify ocs_capture schema properties
    let tools = tool_definitions();
    let capture_tool = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ocs_capture")
        .expect("ocs_capture tool exists");
    let props = &capture_tool["inputSchema"]["properties"];
    assert!(props.get("tile").is_some());
    assert_eq!(props["tile"]["type"], "object");
    assert!(props.get("pyramid_manifest").is_some());
    assert_eq!(props["pyramid_manifest"]["type"], "boolean");

    // 2. Insert pyramid manifest and tile
    let manifest = crate::app::control::vision::compute_pyramid_manifest(
        "sess1",
        "Millimeters",
        [0.0, 0.0, 500.0, 300.0],
        3,
        512,
    );
    let manifest_val = serde_json::to_value(&manifest).unwrap();
    let manifest_uri = resources.insert_pyramid_manifest("sess1", manifest_val);
    assert_eq!(manifest_uri, "cad://session/sess1/pyramid/manifest.json");

    let tile_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let tile_uri = resources.insert_tile(
        "sess1",
        1,
        0,
        1,
        tile_data.to_string(),
        68,
        json!({
            "width": 512,
            "height": 512,
            "pyramid": {
                "level": 1,
                "x": 0,
                "y": 1,
                "tile_world_bounds": [0.0, 0.0, 250.0, 150.0]
            }
        }),
    );
    assert_eq!(tile_uri, "cad://session/sess1/tile/1/0/1.png");

    // 3. Test resources/list includes manifest and tile
    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":"r-list-pyr","method":"resources/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let res_arr = listed["result"]["resources"].as_array().unwrap();
    assert!(res_arr.iter().any(|r| r["uri"] == manifest_uri));
    assert!(res_arr.iter().any(|r| r["uri"] == tile_uri));

    // 4. Test resources/read with manifest
    let read_man = handle_message(
        json!({"jsonrpc":"2.0","id":"r-read-man","method":"resources/read","params":{"uri":manifest_uri}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let man_contents = read_man["result"]["contents"].as_array().unwrap();
    assert_eq!(man_contents[0]["mimeType"], "application/json");
    assert!(
        man_contents[0]["text"]
            .as_str()
            .unwrap()
            .contains("cad://session/sess1/tile/{level}/{x}/{y}.png")
    );

    // 5. Test resources/read with tile png
    let read_tile = handle_message(
        json!({"jsonrpc":"2.0","id":"r-read-tile","method":"resources/read","params":{"uri":tile_uri}}),
        &mut clients,
        &mut tasks,
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let tile_contents = read_tile["result"]["contents"].as_array().unwrap();
    assert_eq!(tile_contents[0]["mimeType"], "image/png");
    assert_eq!(tile_contents[0]["blob"], tile_data);

    // 6. Test tool_result with tile payload
    let tile_tool_output = json!({
        "$resource": {
            "uri": tile_uri,
            "mimeType": "image/png",
            "hash": "tile-1-0-1",
            "bytes": 68
        },
        "metadata": {
            "width": 512,
            "height": 512,
            "pyramid": {
                "level": 1,
                "x": 0,
                "y": 1,
                "tile_world_bounds": [0.0, 0.0, 250.0, 150.0],
                "tile_uri": tile_uri,
                "manifest_uri": manifest_uri
            }
        }
    });
    let structured = tool_result(tile_tool_output);
    assert_eq!(structured["isError"], false);
    assert_eq!(structured["structuredContent"]["pyramid"]["level"], 1);
    assert_eq!(structured["structuredContent"]["pyramid"]["x"], 0);
    assert_eq!(structured["structuredContent"]["pyramid"]["y"], 1);
    assert_eq!(
        structured["structuredContent"]["pyramid"]["tile_uri"],
        tile_uri
    );
}

#[test]
fn validates_batch_steps_and_compacts_state() {
    let batch = json!({
        "op":"batch",
        "request_id":"draw",
        "steps":[{"op":"run","cmd":"LINE 0,0 10,0"},{"op":"run","cmd":"CIRCLE 5,5 2"}]
    });
    assert!(validate_execute_request(&batch, "batch").is_ok());
    let invalid = json!({
        "op":"batch",
        "request_id":"draw",
        "steps":[{"op":"run","request_id":"nested","cmd":"LINE 0,0 10,0"}]
    });
    assert!(
        validate_execute_request(&invalid, "batch")
            .unwrap_err()
            .contains("omit request_id")
    );

    let compact = compact_state(&json!({
        "session_id":"s","document_id":3,"revision":4,"geometry_revision":5,
        "camera_revision":6,"selection":[],"command":null,"documents":[1,2,3],
        "camera":{"distance":100.0},"event_cursor":7
    }));
    assert_eq!(compact["revision"], 4);
    assert!(compact.get("camera").is_none());
    assert!(compact.get("documents").is_none());
}

#[test]
fn task_metadata_uses_the_modern_shape() {
    let now = iso8601_now();
    assert_eq!(now.len(), 20);
    assert!(now.ends_with('Z'));
    let task = McpTask {
        id: "task".into(),
        name: "ocs_execute".into(),
        arguments: json!({}),
        created_at: now.clone(),
        last_updated_at: now,
        result: None,
        error: None,
        cancelled: false,
    };
    let value = task_value(&task, "working");
    assert_eq!(value["resultType"], "complete");
    assert_eq!(value["status"], "working");
    assert_eq!(value["pollIntervalMs"], 250);
}

#[test]
fn task_store_evicts_tasks_past_their_advertised_ttl() {
    let mk = |id: &str| McpTask {
        id: id.into(),
        name: "ocs_execute".into(),
        arguments: json!({}),
        created_at: iso8601_now(),
        last_updated_at: iso8601_now(),
        result: None,
        error: None,
        cancelled: false,
    };
    let t0 = Instant::now();
    let mut store = TaskStore::default();
    store.insert_at(mk("first"), t0);

    let within_ttl = t0 + Duration::from_millis(TASK_TTL_MS) - Duration::from_secs(1);
    assert!(store.get_mut_at("first", within_ttl).is_some());

    let past_ttl = t0 + Duration::from_millis(TASK_TTL_MS) + Duration::from_secs(1);
    assert!(store.get_mut_at("first", past_ttl).is_none());

    store.insert_at(mk("first"), t0);
    store.insert_at(mk("second"), past_ttl);
    assert_eq!(store.tasks.len(), 1);
    assert!(store.get_mut_at("second", past_ttl).is_some());
}

#[test]
fn only_a_first_step_stale_state_is_retryable() {
    let stale = json!({"ok":false,"code":"stale_state"});
    let batch = |next| BatchExecution {
        id: "b".into(),
        request: json!({}),
        steps: Vec::new(),
        next,
        active: None,
        pending: None,
        results: Vec::new(),
        changes: Vec::new(),
        state: None,
        terminal: None,
    };
    assert!(stale_before_first_step(&batch(1), &stale));
    assert!(!stale_before_first_step(&batch(2), &stale), "step 0 ran");
    assert!(!stale_before_first_step(
        &batch(1),
        &json!({"ok":false,"code":"command_busy"})
    ));
}

#[test]
fn a_refresh_never_retargets_another_document() {
    let current = json!({"document_id":2,"revision":1});
    assert!(adopt_refreshed_state(
        &current,
        &json!({"ok":true,"document_id":2,"revision":9})
    ));
    assert!(!adopt_refreshed_state(
        &current,
        &json!({"ok":true,"document_id":3,"revision":9})
    ));
    assert!(!adopt_refreshed_state(&current, &json!({"ok":false})));
}

#[test]
fn gui_failures_are_mcp_errors() {
    let result = tool_result(json!({
        "ok":false,
        "status":"failed",
        "code":"stale_state",
        "error":"Refresh state before editing"
    }));
    assert_eq!(result["isError"], true);
    assert!(result.get("structuredContent").is_none());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stale_state")
    );
}

#[test]
fn all_op_examples_pass_pre_dispatch_validation() {
    for op in crate::mcp_ops::OPS {
        let mut req: Value = serde_json::from_str(op.example)
            .unwrap_or_else(|e| panic!("invalid json example for {}: {e}", op.name));
        if let Some(obj) = req.as_object_mut() {
            obj.insert("request_id".into(), Value::String("example-id".into()));
        }
        let res = crate::mcp_ops::validate_request(&req, false);
        assert!(
            res.is_ok(),
            "example for op '{}' failed validation: {:?}",
            op.name,
            res.err()
        );

        if op.batchable {
            let mut batch_req: Value = serde_json::from_str(op.example).unwrap();
            if let Some(obj) = batch_req.as_object_mut() {
                obj.remove("request_id");
            }
            let b_res = crate::mcp_ops::validate_request(&batch_req, true);
            assert!(
                b_res.is_ok(),
                "batch example for op '{}' failed validation: {:?}",
                op.name,
                b_res.err()
            );
        }
    }
}

#[test]
fn pre_dispatch_validator_catches_unknown_parameters_as_warnings() {
    let req = json!({
        "op": "run",
        "request_id": "r1",
        "cmd": "LINE 0,0 10,0",
        "extra_param": 123
    });
    let res = crate::mcp_ops::validate_request(&req, false).expect("validation passes");
    assert_eq!(res.warnings.len(), 1);
    assert!(res.warnings[0].contains("unknown parameter 'extra_param' ignored for op 'run'"));

    // Batch step warnings propagation
    let batch = json!({
        "op": "batch",
        "request_id": "b1",
        "steps": [
            {"op": "run", "cmd": "LINE 0,0 10,0", "stray": true}
        ]
    });
    let batch_res = validate_execute_request(&batch, "batch").expect("batch passes");
    assert_eq!(batch_res.warnings.len(), 1);
    assert!(
        batch_res.warnings[0]
            .contains("batch step 0: unknown parameter 'stray' ignored for op 'run'")
    );
}

#[test]
fn entities_set_rejects_unknown_fields_without_changing_legacy_warnings() {
    let request = json!({
        "op":"entities_set",
        "handles":["2A"],
        "layer":"Walls",
        "colour":"ByLayer"
    });
    let error = crate::mcp_ops::validate_request(&request, false).unwrap_err();
    assert!(error.contains("unknown parameter 'colour'"), "{error}");

    let bound_batch = json!({
        "op":"batch",
        "request_id":"entities-set-bound",
        "steps":[
            {"op":"entities_create","entities":[{"type":"Point","location":[0,0]}]},
            {"op":"entities_set","color":"ByLayer","bindings":{
                "handles":{"step":0,"path":"/result/handles"}
            }}
        ]
    });
    validate_execute_request(&bound_batch, "batch").expect("bound entities_set batch");
}

#[test]
fn xref_operations_reject_unknown_fields_and_require_a_path_choice() {
    for op in [
        "xref_reload",
        "xref_unload",
        "xref_detach",
        "xref_bind",
        "xref_set_path",
        "xref_set_type",
    ] {
        let mut request = json!({"op":op,"id":"2A","stray":true});
        if op == "xref_set_path" {
            request["path"] = json!("site.dwg");
        } else if op == "xref_set_type" {
            request["reference_type"] = json!("overlay");
        }
        let error = crate::mcp_ops::validate_request(&request, false).unwrap_err();
        assert!(error.contains("unknown parameter 'stray'"), "{op}: {error}");
    }
    let error =
        crate::mcp_ops::validate_request(&json!({"op":"xref_set_path","id":"2A"}), false)
            .unwrap_err();
    assert!(
        error.contains("Supply exactly one of path, path_type for xref_set_path"),
        "{error}"
    );
    let error = crate::mcp_ops::validate_request(
        &json!({"op":"xref_set_path","id":"2A","path":"site.dwg","path_type":"full"}),
        false,
    )
    .unwrap_err();
    assert!(error.contains("Supply exactly one"), "{error}");
    for request in [
        json!({"op":"xref_set_path","id":"2A","path":null}),
        json!({"op":"xref_set_path","id":"2A","path":null,"path_type":"full"}),
        json!({"op":"xref_set_path","id":"2A","path":"site.dwg","path_type":null}),
    ] {
        let error = crate::mcp_ops::validate_request(&request, false).unwrap_err();
        assert!(error.contains("Supply exactly one"), "{request}: {error}");
    }
    let schema = crate::mcp_ops::op_schema(
        crate::mcp_ops::find_op("xref_set_path").expect("xref_set_path schema"),
    );
    assert_eq!(schema["oneOf"].as_array().map(Vec::len), Some(2));
}

#[test]
fn pre_dispatch_validator_enforces_required_fields_and_rules() {
    // Missing required field
    let missing_cmd = json!({"op": "run", "request_id": "r1"});
    let err = crate::mcp_ops::validate_request(&missing_cmd, false).unwrap_err();
    assert!(
        err.contains("Missing cmd for run. Example request:"),
        "{err}"
    );

    // Required-when rule (scale requires factor)
    let scale_missing_factor = json!({
        "op": "entities_transform",
        "request_id": "r2",
        "handles": ["2A"],
        "action": "scale"
    });
    let err2 = crate::mcp_ops::validate_request(&scale_missing_factor, false).unwrap_err();
    assert!(
        err2.contains("Missing factor for entities_transform. Example request:"),
        "{err2}"
    );

    // Hex handle enforcement
    let invalid_handle = json!({
        "op": "entities_delete",
        "request_id": "r3",
        "handles": ["NOT_A_HEX_HANDLE!"]
    });
    let err3 = crate::mcp_ops::validate_request(&invalid_handle, false).unwrap_err();
    assert!(err3.contains("hexadecimal handle strings"), "{err3}");

    // Non-batchable op rejected in batch
    let stop_in_batch = json!({"op": "stop"});
    let err4 = crate::mcp_ops::validate_request(&stop_in_batch, true).unwrap_err();
    assert!(
        err4.contains("operation 'stop' cannot be used in a batch step"),
        "{err4}"
    );
}

#[test]
fn execute_schema_shares_operations_and_resolves_local_references() {
    fn check_refs(value: &Value, root: &Value) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref") {
                    let pointer = reference.as_str().unwrap().strip_prefix('#').unwrap();
                    assert!(root.pointer(pointer).is_some(), "unresolved {reference}");
                }
                for child in object.values() {
                    check_refs(child, root);
                }
            }
            Value::Array(array) => {
                for child in array {
                    check_refs(child, root);
                }
            }
            _ => {}
        }
    }
    let schema = execute_input_schema();
    check_refs(&schema, &schema);
    let standalone = execute_request_schema();
    check_refs(&standalone, &standalone);
    assert!(schema["properties"]["request"].get("$defs").is_none());
    let variants = schema["properties"]["request"]["anyOf"].as_array().unwrap();
    let batch = schema["$defs"]["batch_step"]["anyOf"].as_array().unwrap();
    for (op, variant) in crate::mcp_ops::OPS.iter().zip(variants) {
        assert_eq!(variant, &json!({"$ref":format!("#/$defs/op_{}", op.name)}));
        assert_eq!(batch.contains(variant), op.batchable);
        assert_eq!(
            schema["$defs"][format!("op_{}", op.name)],
            crate::mcp_ops::op_schema(op)
        );
    }

    // Reconstruct the old duplicated layout using the same operation
    // definitions, so the size comparison stays meaningful as ops evolve.
    let mut old = standalone.clone();
    old["anyOf"] = json!(
        crate::mcp_ops::OPS
            .iter()
            .map(|op| crate::mcp_ops::op_schema(op))
            .collect::<Vec<_>>()
    );
    old["$defs"]["batch_step"]["anyOf"] = json!(
        crate::mcp_ops::OPS
            .iter()
            .filter(|op| op.batchable)
            .map(|op| crate::mcp_ops::op_schema(op))
            .collect::<Vec<_>>()
    );
    old["$defs"]
        .as_object_mut()
        .unwrap()
        .retain(|key, _| !key.starts_with("op_"));
    let before = old.to_string().len();
    let after = standalone.to_string().len();
    eprintln!("Execute request schema: {before} -> {after} bytes");
    assert!(after * 100 < before * 70, "expected at least 30% reduction");
}

#[test]
fn entity_schemas_and_preflight_are_local_and_never_dispatch() {
    let mut clients = HashMap::new();
    let mut resources = ResourceStore::default();
    let mut call = |op: &str, parameters: Value| {
        call_tool(
            "ocs_read",
            &json!({"op":op,"parameters":parameters}),
            &mut clients,
            &mut resources,
            &mut detached_pump(),
            None,
        )
        .unwrap()
    };
    let catalog = call("entity_schema", json!({}));
    assert_eq!(catalog["types"].as_array().unwrap().len(), 15);
    assert!(catalog.get("schema").is_none());
    let circle = call("entity_schema", json!({"type":"Circle"}));
    assert_eq!(
        circle["schema"]["required"],
        json!(["type", "center", "radius"])
    );
    let request = json!({"op":"entities_create","entities":[{"type":"Circle","center":[0,0],"radius":2}]});
    let validated = call("validate", json!({"request":request}));
    assert_eq!(validated["valid"], true);
    assert_eq!(validated["executed"], false);
    assert_eq!(validated["scope"], "static");
    crate::schema_validation::validate_shape(&read_output_schema(), &validated, "").unwrap();
    let invalid = call(
        "validate",
        json!({"request":{"op":"entities_create","entities":[{"type":"Circle","center":["bad",0],"radius":2}]}}),
    );
    assert_eq!(invalid["valid"], false);
    assert!(
        invalid["error"]
            .as_str()
            .unwrap()
            .contains("/entities/0/center/0")
    );
    assert!(clients.is_empty());
    assert!(resources.latest.is_empty());
    let rejected = call_tool(
        "ocs_execute",
        &json!({"ocs_session_id":"offline","request":{"op":"entities_create","request_id":"invalid","entities":[{"type":"Circle","center":[0,0],"radius":-1}]}}),
        &mut clients,
        &mut resources,
        &mut detached_pump(),
        None,
    );
    assert!(rejected.is_err());
    assert!(clients.is_empty());
}

#[test]
fn preflight_checks_nested_data_envelopes_and_deferred_bindings() {
    let malformed = json!({"op":"set_properties","collection":"entities","handle":"A","updates":[{"path":123,"value":"Walls"}]});
    let error = preflight_request(&malformed);
    assert_eq!(error["valid"], false);
    assert!(error["error"].as_str().unwrap().contains("/updates/0/path"));
    for request in [
        json!({"op":"run","cmd":"LINE","document_id":-1}),
        json!({"op":"run","cmd":"LINE","request_id":""}),
        json!({"op":"run","cmd":"LINE","selection":[42]}),
        json!({"op":"not_an_operation"}),
    ] {
        assert_eq!(preflight_request(&request)["valid"], false);
    }
    let batch = json!({"op":"batch","steps":[{"op":"run","cmd":"LINE"},{"op":"group_create","name":"Walls","bindings":{"handles":{"step":0,"path":"/result/handles"}}}]});
    let validated = preflight_request(&batch);
    assert_eq!(validated["valid"], true);
    assert_eq!(validated["deferred_bindings"][0]["step"], 1);
    assert!(!validated["not_checked"].as_array().unwrap().is_empty());
    let mut invalid = batch;
    invalid["steps"][1]["bindings"]["handles"]["step"] = json!(1);
    assert_eq!(preflight_request(&invalid)["valid"], false);
}

#[test]
fn guides_are_discoverable_without_a_gui_session() {
    assert!(INSTRUCTIONS.len() < 1200);
    assert!(INSTRUCTIONS.contains("state.command.accepts"));
    assert!(INSTRUCTIONS.contains("never replay a mutation"));
    let mut clients = HashMap::new();
    let mut resources = ResourceStore::default();
    let listed = handle_message(
        json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
        &mut clients,
        &mut TaskStore::default(),
        &mut resources,
        &mut detached_pump(),
    )
    .unwrap();
    let tool_list = call_tool(
        "ocs_read",
        &json!({"ocs_session_id":"offline","op":"guide"}),
        &mut clients,
        &mut resources,
        &mut detached_pump(),
        None,
    )
    .unwrap();
    assert_eq!(tool_list["guides"].as_array().unwrap().len(), GUIDES.len());
    for (name, _, text) in GUIDES {
        let uri = format!("cad://guides/{name}");
        assert!(
            listed["result"]["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["uri"] == uri)
        );
        let resource = read_resource(
            &uri,
            &mut clients,
            &mut resources,
            &mut detached_pump(),
            None,
        )
        .unwrap();
        assert_eq!(resource["contents"][0]["text"], *text);
        let tool = call_tool(
            "ocs_read",
            &json!({"ocs_session_id":"offline","op":"guide","parameters":{"name":name}}),
            &mut clients,
            &mut resources,
            &mut detached_pump(),
            None,
        )
        .unwrap();
        assert_eq!(tool["contents"], resource["contents"]);
        assert_eq!(tool["ok"], true);
        assert_eq!(tool_list["ok"], true);
    }
    assert!(
        read_resource(
            "cad://guides/missing",
            &mut clients,
            &mut resources,
            &mut detached_pump(),
            None
        )
        .is_err()
    );
    assert!(clients.is_empty());
}

#[test]
fn batch_bindings_validate_and_preserve_types() {
    let step = json!({"op":"group_create","name":"walls","bindings":{"handles":{"step":0,"path":"/result/a~1b~0c"}}});
    let request = json!({"op":"batch","steps":[{"op":"run","cmd":"LINE 0,0 1,1"},step]});
    assert!(
        validate_execute_request(&request, "batch")
            .unwrap()
            .warnings
            .is_empty()
    );
    let results = vec![json!({"ok":true,"status":"completed","result":{"a/b~c":["A","B"]}})];
    let resolved = resolve_batch_step(&step, 1, &results).unwrap();
    assert_eq!(resolved["handles"], json!(["A", "B"]));
    assert!(resolved.get("bindings").is_none());
    assert!(
        resolve_batch_step(
            &step,
            1,
            &[json!({"status":"completed","result":{"a/b~c":42}})]
        )
        .is_err()
    );
    assert!(resolve_batch_step(&step, 1, &[json!({"status":"running"})]).is_err());
    assert!(resolve_batch_step(&step, 1, &[json!({"status":"completed"})]).is_err());
    for binding in [
        json!({"step":1,"path":"/result"}),
        json!({"step":2,"path":"/result"}),
        json!({"step":0,"path":"/bad~2"}),
        json!({"step":0,"path":"result"}),
        json!({"step":0,"path":"","extra":true}),
    ] {
        let mut invalid = request.clone();
        invalid["steps"][1]["bindings"]["handles"] = binding;
        assert!(validate_execute_request(&invalid, "batch").is_err());
    }
    for key in ["op", "request_id", "revision", "unknown"] {
        let invalid = json!({"op":"group_create","name":"walls","bindings":{key:{"step":0,"path":"/result"}}});
        assert!(batch_bindings(&invalid, 1).is_err());
    }
    let mut conflict = step.clone();
    conflict["handles"] = json!(["A"]);
    assert!(batch_bindings(&conflict, 1).is_err());
    let mut missing = request.clone();
    missing["steps"][1].as_object_mut().unwrap().remove("name");
    assert!(validate_execute_request(&missing, "batch").is_err());
}

#[test]
fn batch_binding_failure_is_cached_without_dispatch() {
    let step = json!({"op":"group_create","name":"walls","bindings":{"handles":{"step":0,"path":"/missing"}}});
    let request = json!({"op":"batch","request_id":"binding-test","steps":[{"op":"run","cmd":"LINE"},step]});
    let mut client = GuiClient {
        descriptor: Descriptor {
            session_id: "test".into(),
            port: 0,
            token: String::new(),
            pid: None,
        },
        state: json!({}),
        client_id: "test".into(),
        inflight: HashMap::new(),
        dismissed: VecDeque::new(),
        batches: VecDeque::from([BatchExecution {
            id: "binding-test".into(),
            request: request.clone(),
            steps: request["steps"].as_array().unwrap().clone(),
            next: 1,
            active: None,
            pending: None,
            results: vec![json!({"ok":true,"status":"completed"})],
            changes: vec![],
            state: None,
            terminal: None,
        }]),
    };
    let failed = client
        .execute_batch(request.clone(), 0.0, &mut detached_pump(), None)
        .unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["results"][1]["code"], "invalid_batch_binding");
    assert_eq!(
        client
            .execute_batch(request, 0.0, &mut detached_pump(), None)
            .unwrap(),
        failed
    );
}

#[test]
fn batch_summary_is_small_and_keeps_actionable_information() {
    let full = json!({
        "ok":true,"status":"completed","request_id":"b1",
        "completed_steps":64,"total_steps":64,"next_step":null,
        "results":(0..64).map(|i| json!({
            "step":i,"op":"run","ok":true,"status":"completed",
            "result":{"payload":"x".repeat(1000)}
        })).collect::<Vec<_>>(),
        "changes":(0..64).map(|i| json!({"step":i,"handle":format!("{i:X}")})).collect::<Vec<_>>(),
        "warnings":["top-level warning"],
        "state":{"document_id":7,"revision":65,"selection":[],"command":null,"documents":[{"large":"payload"}]}
    });
    let summary = summarize_batch_response(full.clone(), "s1");
    assert!(summary.get("results").is_none());
    assert!(summary.get("changes").is_none());
    assert_eq!(summary["change_count"], 64);
    assert_eq!(summary["failed_steps"], 0);
    assert_eq!(summary["completed_steps"], 64);
    assert_eq!(summary["warnings"], full["warnings"]);
    assert_eq!(summary["state"]["revision"], 65);
    assert!(summary.to_string().len() * 10 < full.to_string().len());
    assert_eq!(
        summary["details"]["arguments"]["parameters"]["request_id"],
        "b1"
    );
    let mut resolved = full.clone();
    resolved["results"] = json!([
        {"step":0,"status":"waiting_input"},
        {"step":1,"status":"completed"}
    ]);
    assert_eq!(
        summarize_batch_response(resolved, "s1")["attention"],
        json!([])
    );
    let early = summarize_batch_response(
        json!({
            "ok":true,"status":"running","request_id":"early","state":null
        }),
        "s1",
    );
    assert!(tool_result(early)["structuredContent"]["state"].is_object());

    for status in ["failed", "cancelled", "waiting_input"] {
        let step = json!({"step":1,"op":"run","status":status,
            "ok":status == "waiting_input", "code":"needs_attention",
            "error":"reason", "result":{"input_example":"10,20"}});
        let state = json!({"revision":3,"command":{"accepts":["point"],"input_example":"10,20"},"modal":{"title":"Input"}});
        let mut detailed = full.clone();
        detailed["status"] = json!(status);
        detailed["ok"] = json!(status == "waiting_input");
        detailed["state"] = state.clone();
        detailed["results"] = json!([
            {"step":0,"op":"run","ok":true,"status":"completed","warnings":["approximation"],"result":{"big":"x".repeat(1000)}},
            step.clone()
        ]);
        let summary = summarize_batch_response(detailed.clone(), "s1");
        assert_eq!(summary["ok"], detailed["ok"]);
        assert_eq!(summary["status"], status);
        assert_eq!(
            summary["failed_steps"],
            if status == "waiting_input" { 0 } else { 1 }
        );
        assert_eq!(
            summary["attention"][0]["warnings"],
            json!(["approximation"])
        );
        assert!(summary["attention"][0].get("result").is_none());
        assert_eq!(summary["attention"][1], step);
        assert_eq!(summary["state"], state);
    }
}

#[test]
fn batch_details_are_read_only_session_scoped_and_expire() {
    let batch = BatchExecution {
        id: "b1".into(),
        request: json!({"op":"batch","request_id":"b1","steps":[{"op":"run","cmd":"LINE 0,0 1,1"}]}),
        steps: vec![json!({"op":"run","cmd":"LINE 0,0 1,1"})],
        next: 0,
        active: Some("step1".into()),
        pending: Some(
            json!({"step":0,"op":"run","status":"running","result":{"prompt":"Pick point"},"warnings":["notice"]}),
        ),
        results: vec![],
        changes: vec![],
        state: Some(json!({"revision":4,"command":{"accepts":["point"]}})),
        terminal: None,
    };
    let running = batch_result(&batch, "running", true);
    let summary = summarize_batch_response(running.clone(), "s1");
    assert_eq!(summary["active_step"], running["active_step"]);
    assert_eq!(summary["state"]["command"]["accepts"], json!(["point"]));
    let mut clients = HashMap::new();
    clients.insert(
        "s1".into(),
        GuiClient {
            descriptor: Descriptor {
                session_id: "s1".into(),
                port: 0,
                token: String::new(),
                pid: None,
            },
            state: json!({}),
            client_id: "test".into(),
            batches: VecDeque::new(),
            inflight: HashMap::new(),
            dismissed: VecDeque::new(),
        },
    );
    clients.get_mut("s1").unwrap().batches.push_back(batch);
    assert_eq!(cached_batch_result(&clients, "s1", "b1").unwrap(), running);
    assert_eq!(clients["s1"].batches[0].next, 0);
    let mut batch = clients.get_mut("s1").unwrap().batches.pop_front().unwrap();
    batch.next = 1;
    batch.active = None;
    batch.pending = None;
    batch.results = vec![
        json!({"step":0,"op":"run","ok":true,"status":"completed","result":{"handles":["A"]}}),
    ];
    let full = batch_result(&batch, "completed", true);
    for detail in ["compact", "full"] {
        let response = shape_execute_response(
            full.clone(),
            detail,
            clients.get_mut("s1").unwrap(),
            &mut detached_pump(),
        )
        .unwrap();
        assert_eq!(response["results"], full["results"]);
        assert_eq!(response["changes"], full["changes"]);
    }
    batch.terminal = Some(full.clone());
    clients.get_mut("s1").unwrap().batches.push_back(batch);
    let summary = summarize_batch_response(full.clone(), "s1");
    let read = |clients: &mut HashMap<String, GuiClient>, args: &Value| {
        call_tool(
            "ocs_read",
            args,
            clients,
            &mut ResourceStore::default(),
            &mut detached_pump(),
            None,
        )
    };
    assert_eq!(
        read(&mut clients, &summary["details"]["arguments"]).unwrap(),
        full
    );
    assert_eq!(clients["s1"].batches[0].terminal, Some(full.clone()));
    let mut wrong_session = summary["details"]["arguments"].clone();
    wrong_session["ocs_session_id"] = json!("s2");
    assert!(read(&mut clients, &wrong_session).is_err());
    assert_eq!(clients.len(), 1);
    // Idempotent replay still returns the complete cached result.
    let request = clients["s1"].batches[0].request.clone();
    assert_eq!(
        clients
            .get_mut("s1")
            .unwrap()
            .execute_batch(request, 0.0, &mut detached_pump(), None)
            .unwrap(),
        full
    );
    for i in 0..64 {
        clients
            .get_mut("s1")
            .unwrap()
            .batches
            .push_back(BatchExecution {
                id: format!("other-{i}"),
                request: json!({}),
                steps: vec![],
                next: 0,
                active: None,
                pending: None,
                results: vec![],
                changes: vec![],
                state: None,
                terminal: None,
            });
    }
    trim_batches(&mut clients.get_mut("s1").unwrap().batches);
    assert!(read(&mut clients, &summary["details"]["arguments"]).is_err());
}

#[test]
fn schema_size_and_defs_structure() {
    let schema = execute_request_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(schema["$defs"]["handle"].is_object());
    assert!(schema["$defs"]["point"].is_object());
    assert!(schema["$defs"]["window"].is_object());
    assert!(schema["$defs"]["batch_step"].is_object());
    let json_str = schema.to_string();
    // Generates cleanly without exponential blowup: compact ~15KB to 48KB
    // (the catalogue's 13th execute op, plugin_panel_action, sits inside
    // the deliberate 2026-11 budget revisit)
    assert!(
        json_str.len() < 48_000,
        "schema size is {} bytes",
        json_str.len()
    );
}

#[test]
fn export_agent_tool_schemas() {
    let synced = sync_agent_tool_schemas();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let base_dir = std::path::PathBuf::from(home)
        .join(".gemini")
        .join("antigravity")
        .join("mcp")
        .join("opencadstudio");
    if base_dir.exists() {
        assert!(
            synced,
            "sync_agent_tool_schemas must succeed when directory exists"
        );
        assert!(base_dir.join("ocs_capture.json").exists());
        assert!(base_dir.join("ocs_execute.json").exists());
        assert!(base_dir.join("ocs_read.json").exists());
        assert!(base_dir.join("ocs_sessions.json").exists());
        assert!(base_dir.join("instructions.md").exists());
    }
}

#[test]
fn read_op_tools_returns_current_schemas_and_instructions() {
    let mut clients = HashMap::new();
    let mut resources = ResourceStore::default();
    let req = json!({
        "name": "ocs_read",
        "arguments": {
            "ocs_session_id": "test_session",
            "op": "tools"
        }
    });
    let result = call_tool(
        "ocs_read",
        &req["arguments"],
        &mut clients,
        &mut resources,
        &mut detached_pump(),
        None,
    )
    .expect("ocs_read op: tools must succeed");
    assert_eq!(result["ok"], true);
    assert!(result["tools"].is_array());
    assert!(result["instructions"].is_string());
}

#[test]
fn tool_schema_digest_is_pinned() {
    // If this fails, the MCP tool surface changed: review the diff, then
    // update TOOL_SCHEMA_DIGEST deliberately (never blindly).
    assert_eq!(tool_schema_digest(), TOOL_SCHEMA_DIGEST);
}

#[test]
fn bridge_identity_is_exposed_on_handshake_payloads() {
    // Agents never see MCP serverInfo, so the build announcement must
    // ride on payloads they do see: session states and hello/capabilities.
    let identity = bridge_identity();
    assert_eq!(identity["name"], "OpenCADStudio");
    assert!(identity["version"].as_str().is_some());
    assert!(identity["build_rev"].as_str().is_some());
    assert!(identity["build_profile"].as_str().is_some());
    assert_eq!(identity["tool_schema"], Value::String(tool_schema_digest()));

    let stamped = with_bridge_identity(json!({"ok": true, "session_id": "s"}));
    assert_eq!(stamped["bridge"]["name"], "OpenCADStudio");
    assert_eq!(stamped["ok"], true);
    // Non-objects pass through untouched.
    assert_eq!(with_bridge_identity(json!([1, 2])), json!([1, 2]));
}

#[test]
fn startup_lock_serializes_concurrent_launches() {
    // A fresh lock held by a live pid means another GUI is already
    // starting: the second caller must wait, never spawn.
    let dir = std::env::temp_dir().join(format!(
        "ocs-startup-lock-test-{}-{}",
        std::process::id(),
        random_id().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // No lock yet: first caller claims it and may spawn.
    assert!(try_claim_startup_lock(&dir, std::process::id() as u64));
    // Second caller sees the fresh live claim and must wait.
    assert!(!try_claim_startup_lock(&dir, u64::MAX - 1));

    // A stale lock (previous boot left it behind) is reclaimable.
    let stale = now_unix_secs().saturating_sub(STARTUP_LOCK_TTL_SECS + 60);
    write_startup_lock_at(&dir, 12345, stale).unwrap();
    assert!(try_claim_startup_lock(&dir, std::process::id() as u64));

    // A fresh lock from a dead pid is reclaimable (crashed starter).
    let fresh = now_unix_secs();
    write_startup_lock_at(&dir, u64::MAX - 2, fresh).unwrap();
    assert!(try_claim_startup_lock(&dir, std::process::id() as u64));

    release_startup_lock(&dir);
    assert!(!startup_lock_path_for(&dir).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn private_capture_dir_and_input_schema_work_cleanly() {
    let dir = private_capture_dir().expect("private capture dir");
    assert!(dir.is_dir());
    let file = dir.join(format!("test-{}.png", random_id().unwrap()));
    std::fs::write(&file, b"png").unwrap();
    assert_eq!(read_private_capture(&file).unwrap(), b"png");
    let _ = std::fs::remove_file(&file);

    let schema = execute_input_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(schema["$defs"].is_object());
    assert!(schema["properties"]["request"].get("$defs").is_none());
}

#[cfg(unix)]
#[test]
fn private_descriptor_rejects_symlinks_and_group_readable_modes() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "ocs-priv-desc-{}-{}",
        std::process::id(),
        random_id().unwrap()
    ));
    let dir = root.join("0700");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let good = dir.join("good.json");
    std::fs::write(&good, b"{}").unwrap();
    std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(private_descriptor(&good));

    let leaky = dir.join("leaky.json");
    std::fs::write(&leaky, b"{}").unwrap();
    std::fs::set_permissions(&leaky, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(!private_descriptor(&leaky));

    let sym = dir.join("sym.json");
    std::os::unix::fs::symlink(&good, &sym).unwrap();
    assert!(!private_descriptor(&sym));

    let _ = std::fs::remove_dir_all(&root);
}
