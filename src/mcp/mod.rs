//! Client-neutral MCP adapter for the live desktop editor.
//!
//! `OpenCADStudio --mcp` speaks MCP over stdio. All drawing work is forwarded
//! to the authenticated GUI control bridge; this module contains no geometry.

mod cache;
mod resources;
mod session;
mod tools;
mod transport;
#[cfg(test)]
mod tests;

pub(crate) use tools::{
    capture_input_schema, execute_output_schema, read_output_schema, tool_definitions,
};

use cache::*;
use resources::*;
use session::*;
use tools::*;
use transport::*;

use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Write},
    thread,
};

const PROTOCOL_VERSION: &str = "2025-11-25";
const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";

pub(crate) const INSTRUCTIONS: &str = "Prefer intent tools: ocs_inspect, ocs_query, ocs_run, ocs_new/open/save, ocs_render. Inspect and drive plugin panel surfaces with ocs_panels and ocs_panel_action. Use ocs_read/ocs_execute for advanced work and explicit envelopes. ocs_session_id is optional with one session. Intent calls need no request_id, but reuse one to retry a mutation; after an uncertain outcome query the operation, and never replay a mutation with a new id. Read capabilities and record_schema before unfamiliar ops; use entity_schema for creation, validate for preflight. Preserve document_id and revision from state. running/waiting_input are not completion: follow state.command.accepts, options, input_example. Let the geometry kernel do the math. Before delivery audit the target format/version, then save_verified or ocs_save_verified to an absolute path. Announce the bridge build on connection and change. For batches use response_detail: summary; bindings reuse earlier results. Read guides via cad://guides/{name} or ocs_read op: guide, parameters.name: workflow, architecture, capture, plugin-panels, batch-bindings. Read architecture before floorplan vectorization; capture before visual verification.";

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

/// Spec-pure implementation block: exactly name/title/version, so strict
/// clients (deny_unknown_fields, strict Zod schemas) never reject the
/// handshake. Build extras live canonically in [`bridge_identity`],
/// surfaced via `capabilities.experimental` and the in-band `bridge` stamp.
fn server_info() -> Value {
    json!({
        "name":"OpenCADStudio",
        "title":"Open CAD Studio",
        "version":env!("OCS_APP_VERSION"),
    })
}

/// Build identity agents CAN see. MCP `serverInfo` never reaches tool
/// callers, so the same fields ride on handshake payloads (`ocs_sessions`
/// states, `hello`/`capabilities` responses) under the `bridge` key.
fn bridge_identity() -> Value {
    json!({
        "name":"OpenCADStudio",
        "version":env!("OCS_APP_VERSION"),
        "build_rev":env!("OCS_GIT_REV"),
        "build_profile":env!("OCS_BUILD_PROFILE"),
        "tool_schema":tool_schema_digest(),
    })
}

/// Stamp a `bridge` identity onto a handshake payload. Non-objects pass
/// through untouched; an existing `bridge` key (GUI-provided) is kept.
fn with_bridge_identity(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.entry("bridge").or_insert_with(bridge_identity);
    }
    value
}

fn modern_request(params: &Value) -> bool {
    params["_meta"]["io.modelcontextprotocol/protocolVersion"].as_str()
        == Some(MODERN_PROTOCOL_VERSION)
}

fn supports_tasks(params: &Value) -> bool {
    params["_meta"]["io.modelcontextprotocol/clientCapabilities"]["extensions"]
        ["io.modelcontextprotocol/tasks"]
        .is_object()
}

fn task_arguments(name: &str, arguments: &Value, response: &Value) -> Value {
    let mut retained = arguments.clone();
    if crate::mcp_intent::find_descriptor(name)
        .is_some_and(|descriptor| descriptor.kind == crate::mcp_intent::IntentKind::Execute)
        && arguments.get("request_id").is_none()
    {
        if let (Some(object), Some(request_id)) =
            (retained.as_object_mut(), response["request_id"].as_str())
        {
            object.insert("request_id".to_string(), json!(request_id));
        }
    }
    retained
}

fn task_value(task: &McpTask, status: &str) -> Value {
    let mut value = json!({
        "resultType":"complete",
        "taskId":task.id,
        "status":status,
        "createdAt":task.created_at,
        "lastUpdatedAt":task.last_updated_at,
        "ttlMs":TASK_TTL_MS,
        "pollIntervalMs":250
    });
    if let Some(result) = &task.result {
        value["result"] = result.clone();
    }
    if let Some(error) = &task.error {
        value["error"] = error.clone();
    }
    value
}

fn poll_task(
    task: &mut McpTask,
    clients: &mut HashMap<String, GuiClient>,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
) -> Value {
    if task.cancelled {
        // Cancelled carries neither result nor error, even if a late
        // completion landed after the cancel was acknowledged.
        let mut value = task_value(task, "cancelled");
        if let Some(object) = value.as_object_mut() {
            object.remove("result");
            object.remove("error");
        }
        return value;
    }
    if task.result.is_some() {
        return task_value(task, "completed");
    }
    if task.error.is_some() {
        return task_value(task, "failed");
    }
    task.last_updated_at = iso8601_now();
    let mut arguments = task.arguments.clone();
    arguments["wait_seconds"] = Value::from(0);
    // Task re-issues are server-driven with no client wait: no MCP id to
    // match cancels against, and a cancel here only means "stop polling".
    match call_tool(&task.name, &arguments, clients, resources, pump, None) {
        Ok(value) if matches!(value["status"].as_str(), Some("accepted" | "running")) => {
            task_value(task, "working")
        }
        Ok(value) => {
            task.result = Some(tool_result(value));
            task_value(task, "completed")
        }
        // Don't cache a cancel as failure: the client moved on, and the
        // next poll simply re-issues.
        Err(CallError::Cancelled) => task_value(task, "working"),
        Err(error) => {
            // Public code: the legacy -32000 range is grandfathered, and
            // new emissions stay out of it.
            task.error = Some(json!({"code":-32603,"message":error.message()}));
            task_value(task, "failed")
        }
    }
}

fn protocol_result(mut result: Value, modern: bool, ttl_ms: Option<u64>) -> Value {
    // SEP-2549 freshness hints are version-independent: legacy clients
    // ignore unknown fields, so list results always carry them. The modern
    // envelope (resultType/_meta) stays negotiated.
    if let Some(ttl_ms) = ttl_ms {
        if let Some(object) = result.as_object_mut() {
            object.insert("ttlMs".into(), Value::from(ttl_ms));
            object.insert("cacheScope".into(), Value::String("public".into()));
        }
    }
    if modern {
        let object = result
            .as_object_mut()
            .expect("MCP results are JSON objects");
        object
            .entry("resultType")
            .or_insert_with(|| Value::String("complete".into()));
        object.insert(
            "_meta".into(),
            json!({"io.modelcontextprotocol/serverInfo":server_info()}),
        );
    }
    result
}

fn rpc_error(id: Value, code: i64, message: impl ToString) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.to_string()}})
}

fn unsupported_protocol(id: Value, requested: &str) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "error":{
            "code":-32022,
            "message":format!("Unsupported protocol version: {requested}"),
            "data":{"requested":requested,"supported":[MODERN_PROTOCOL_VERSION,PROTOCOL_VERSION]}
        }
    })
}

fn handle_message(
    message: Value,
    clients: &mut HashMap<String, GuiClient>,
    tasks: &mut TaskStore,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
) -> Option<Value> {
    // Non-object input (batches included: the spec defines no batch
    // semantics for us) is Invalid Request, never silence: a sender must
    // not hang waiting for a response that will never come. Id-less
    // *objects* stay silent (notifications).
    if !message.is_object() {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "Invalid request: expected a JSON-RPC object",
        ));
    }
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str)?;
    id.as_ref()?;
    let id = id.unwrap();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    if let Some(requested) = params["_meta"]["io.modelcontextprotocol/protocolVersion"].as_str() {
        if requested != MODERN_PROTOCOL_VERSION {
            return Some(unsupported_protocol(id, requested));
        }
    }
    let modern = modern_request(&params);
    Some(match method {
        "initialize" if modern => rpc_error(id, -32601, "Method not found: initialize"),
        "initialize" => {
            let requested = params["protocolVersion"]
                .as_str()
                .unwrap_or(PROTOCOL_VERSION);
            let protocol =
                if ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"].contains(&requested) {
                    requested
                } else {
                    PROTOCOL_VERSION
                };
            response(
                id,
                json!({
                    "protocolVersion":protocol,
                    "capabilities":{
                        "tools":{"listChanged":false},
                        "resources":{"subscribe":false,"listChanged":false},
                        "experimental":{"opencadstudio.build":bridge_identity()}
                    },
                    "serverInfo":server_info(),
                    "instructions":INSTRUCTIONS
                }),
            )
        }
        "server/discover" => response(
            id,
            protocol_result(
                json!({
                    "supportedVersions":[MODERN_PROTOCOL_VERSION,PROTOCOL_VERSION],
                    "capabilities":{
                        "tools":{},
                        "resources":{},
                        "extensions":{"io.modelcontextprotocol/tasks":{}}
                    },
                    "instructions":INSTRUCTIONS
                }),
                true,
                Some(CACHE_TTL_MS),
            ),
        ),
        "ping" => response(id, protocol_result(json!({}), modern, None)),
        "resources/list" => {
            let mut session_ids = Vec::new();
            if let Ok(available) = descriptors() {
                for (desc, _) in available {
                    session_ids.push(desc.session_id);
                }
            }
            for k in clients.keys() {
                if !session_ids.contains(k) {
                    session_ids.push(k.clone());
                }
            }
            for k in resources.latest.keys() {
                if !session_ids.contains(k) {
                    session_ids.push(k.clone());
                }
            }
            for k in resources.pyramid_manifests.keys() {
                if !session_ids.contains(k) {
                    session_ids.push(k.clone());
                }
            }
            for (k, _) in resources.layout_manifests.keys() {
                if !session_ids.contains(k) {
                    session_ids.push(k.clone());
                }
            }
            // Deterministic order: HashMap iteration is random, and stable
            // ordering keeps client caches and prompt caches hitting.
            session_ids.sort();
            let mut list = guide_resources();
            list.extend(resources.list_resources(&session_ids));
            response(
                id,
                protocol_result(json!({ "resources": list }), modern, Some(RESOURCE_TTL_MS)),
            )
        }
        "resources/read" => {
            let Some(uri) = params["uri"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing resource uri"));
            };
            match read_resource(uri, clients, resources, pump, Some(&id)) {
                Ok(contents) => {
                    response(id, protocol_result(contents, modern, Some(RESOURCE_TTL_MS)))
                }
                // -32002 was THE not-found code in 2025-11-25 and earlier;
                // 2026-07-28 says MUST NOT emit it, so modern gets -32602.
                Err(err) => rpc_error(id, if modern { -32602 } else { -32002 }, err),
            }
        }
        "resources/templates/list" => response(
            id,
            protocol_result(
                json!({"resourceTemplates": [
                    {
                        "uriTemplate": "cad://session/{session_id}/tile/{level}/{x}/{y}.png",
                        "name": "Pyramid tile",
                        "description": "DeepZoom viewport tile at zoom level, column x, row y.",
                        "mimeType": "image/png"
                    },
                    {
                        "uriTemplate": "cad://session/{session_id}/layout/{layout}/tile/{level}/{x}/{y}.png",
                        "name": "Layout pyramid tile",
                        "description": "DeepZoom off-screen tile for a specific layout at zoom level, column x, row y.",
                        "mimeType": "image/png"
                    },
                    {
                        "uriTemplate": "cad://session/{session_id}/snapshot/{hash}.png",
                        "name": "Viewport snapshot",
                        "description": "Captured viewport frame addressed by content hash.",
                        "mimeType": "image/png"
                    }
                ]}),
                modern,
                Some(RESOURCE_TTL_MS),
            ),
        ),
        "tools/list" => response(
            id,
            protocol_result(
                json!({"tools":tool_definitions()}),
                modern,
                Some(CACHE_TTL_MS),
            ),
        ),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing tool name"));
            };
            // Unknown tools are Protocol Errors (-32602), never isError
            // results; the valid names ride along so the model can recover.
            let available = available_tool_names();
            if !available.contains(&name) {
                return Some(rpc_error(
                    id,
                    -32602,
                    format!(
                        "Unknown tool: {name}. Available tools: {}",
                        available.join(", ")
                    ),
                ));
            }
            let mut arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let called = match normalize_intent_arguments(name, &arguments, clients) {
                Ok(normalized) => {
                    arguments = normalized;
                    call_tool(name, &arguments, clients, resources, pump, Some(&id))
                }
                Err(error) => Err(error),
            };
            if modern && supports_tasks(&params) {
                if let Ok(value) = &called {
                    if matches!(value["status"].as_str(), Some("accepted" | "running")) {
                        let task_id =
                            random_id().unwrap_or_else(|_| format!("task-{}", iso8601_now()));
                        let now = iso8601_now();
                        tasks.insert(McpTask {
                            id: task_id.clone(),
                            name: name.to_owned(),
                            arguments: task_arguments(name, &arguments, value),
                            created_at: now.clone(),
                            last_updated_at: now,
                            result: None,
                            error: None,
                            cancelled: false,
                        });
                        return Some(response(
                            id,
                            protocol_result(
                                json!({
                                    "resultType":"task",
                                    "taskId":task_id,
                                    "status":"working",
                                    "statusMessage":"OCS operation is running.",
                                    "createdAt":tasks.tasks.back().unwrap().task.created_at,
                                    "lastUpdatedAt":tasks.tasks.back().unwrap().task.last_updated_at,
                                    "ttlMs":TASK_TTL_MS,
                                    "pollIntervalMs":250
                                }),
                                true,
                                None,
                            ),
                        ));
                    }
                }
            }
            // Tool-domain failures stay isError results (model-actionable);
            // infrastructure failures become -32603 (nothing to fix in-band);
            // a cancelled request gets no response at all (spec SHOULD).
            let result = match called {
                Ok(value) => tool_result(value),
                Err(CallError::Tool(message)) => error_result(message),
                Err(CallError::Infra(message)) => return Some(rpc_error(id, -32603, message)),
                Err(CallError::Cancelled) => return None,
            };
            response(id, protocol_result(result, modern, None))
        }
        "tasks/get" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            let Some(task) = tasks.get_mut(task_id) else {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            };
            response(
                id,
                protocol_result(poll_task(task, clients, resources, pump), true, None),
            )
        }
        "tasks/update" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            if tasks.get_mut(task_id).is_none() {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            }
            response(id, protocol_result(json!({}), true, None))
        }
        "tasks/cancel" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            let Some(task) = tasks.get_mut(task_id) else {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            };
            // Cooperative cancel, honored: the op is dropped (nothing
            // re-issues once cancelled) and polls report the terminal
            // `cancelled` state with neither result nor error.
            task.cancelled = true;
            task.result = None;
            task.error = None;
            task.last_updated_at = iso8601_now();
            response(id, protocol_result(json!({}), true, None))
        }
        _ => rpc_error(id, -32601, format!("Method not found: {method}")),
    })
}

/// Synchronize agent tool schemas and instructions to ~/.gemini/antigravity/mcp/opencadstudio/.
/// Returns true if the target directory was found or created and schemas were written.
pub fn sync_agent_tool_schemas() -> bool {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    if home.is_empty() {
        return false;
    }
    let mcp_root = std::path::PathBuf::from(home)
        .join(".gemini")
        .join("antigravity")
        .join("mcp");
    if !mcp_root.exists() {
        return false;
    }
    let base_dir = mcp_root.join("opencadstudio");
    if std::fs::create_dir_all(&base_dir).is_err() {
        return false;
    }

    let tools = tool_definitions();
    let Some(tool_arr) = tools.as_array() else {
        return false;
    };
    for tool in tool_arr {
        let Some(name) = tool["name"].as_str() else {
            continue;
        };
        let schema = json!({
            "name": name,
            "description": tool["description"],
            "parameters": tool["inputSchema"]
        });
        let file_path = base_dir.join(format!("{name}.json"));
        if let Ok(pretty) = serde_json::to_string_pretty(&schema) {
            let _ = std::fs::write(&file_path, pretty);
        }
    }
    let instructions_path = base_dir.join("instructions.md");
    let _ = std::fs::write(instructions_path, INSTRUCTIONS);
    true
}

/// A cancel arriving while nothing waits targets an op that is pending
/// server-side between polls: find its GUI request via the inflight map and
/// dismiss it now, so the next poll answers `cancelled`. Unknown ids are
/// ignored (spec: fire-and-forget, races expected).
fn idle_cancel(clients: &mut HashMap<String, GuiClient>, params: &Value) {
    let Some(key) = params.get("requestId").map(|id| id.to_string()) else {
        return;
    };
    for gui in clients.values_mut() {
        if let Some(gui_id) = gui.inflight.remove(&key) {
            gui.dismiss(Some(&gui_id));
        }
    }
}

/// Agent schema sync is a local convenience, not protocol: operators
/// disable it with `OCS_SKIP_SCHEMA_SYNC` set to any value.
fn schema_sync_enabled() -> bool {
    std::env::var_os("OCS_SKIP_SCHEMA_SYNC").is_none()
}

/// Run the MCP stdio loop until the client closes stdin.
///
/// A reader thread feeds lines through a channel so wait loops can observe
/// `notifications/cancelled` mid-wait (see [`CancelPump`]). The reader never
/// writes: this loop is the only stdout writer.
pub fn run() {
    if schema_sync_enabled() && sync_agent_tool_schemas() {
        eprintln!("MCP agent schemas synchronized");
    }
    let exe_stamp = exe_fingerprint();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut clients = HashMap::new();
    let mut tasks = TaskStore::default();
    let mut resources = ResourceStore::default();
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let stdin = io::stdin();
    thread::spawn(move || {
        for line in
            crate::io::line_read::lines_capped(stdin.lock(), crate::io::line_read::MAX_LINE_BYTES)
        {
            if tx.send(line.map_err(|error| error.to_string())).is_err() {
                break;
            }
        }
    });
    let mut pump = CancelPump {
        rx,
        backlog: VecDeque::new(),
    };
    loop {
        let Some(line) = pump.next() else {
            break;
        };
        // Serve the in-flight request with the old schema first (it was made
        // against it), then exit so the client respawns a fresh bridge. This
        // ordering means a rebuild never fails a call that is already running.
        let superseded = exe_superseded(&exe_stamp);
        let response = if line.trim().is_empty() {
            None
        } else {
            match serde_json::from_str::<Value>(&line) {
                Ok(message) => {
                    if message.get("id").is_none()
                        && message.get("method").and_then(Value::as_str)
                            == Some("notifications/cancelled")
                    {
                        idle_cancel(&mut clients, &message["params"]);
                        None
                    } else {
                        handle_message(message, &mut clients, &mut tasks, &mut resources, &mut pump)
                    }
                }
                Err(error) => Some(rpc_error(Value::Null, -32700, error)),
            }
        };
        if let Some(response) = response {
            // Serialize once: write_fmt on a Value Display streams every
            // JSON token through the (line-buffered) stdout lock as its own
            // write, so big replies pay hundreds of pipe writes. One String
            // + one write_all produces the identical bytes.
            let mut response_line = response.to_string();
            response_line.push('\n');
            if output
                .write_all(response_line.as_bytes())
                .and_then(|_| output.flush())
                .is_err()
            {
                break;
            }
        }
        if superseded {
            eprintln!("MCP bridge superseded by rebuild; exiting for respawn");
            break;
        }
    }
}
