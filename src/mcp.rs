//! Client-neutral MCP adapter for the live desktop editor.
//!
//! `OpenCADStudio --mcp` speaks MCP over stdio. All drawing work is forwarded
//! to the authenticated GUI control bridge; this module contains no geometry.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROTOCOL_VERSION: &str = "2025-11-25";
const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
const MAX_REQUEST: usize = 1_048_576;
const MAX_RESPONSE: u64 = 16 * 1024 * 1024;
const CACHE_TTL_MS: u64 = 3_600_000;
const INSTRUCTIONS: &str = "Use ocs_sessions first; it opens OpenCADStudio when needed. Read state and commands before editing. Preserve session_id, document_id, revision, selection and request_id. After a timeout, query the existing operation and never replay a mutation with a new request_id. waiting_input and running are not completion. Let OpenCADStudio and its geometry kernel calculate geometry. Verify important results with queries and ocs_capture, and save only to an explicit path.";
const READ_OPS: &[&str] = &[
    "state",
    "hello",
    "query",
    "entities",
    "layers",
    "header",
    "properties",
    "measure",
    "history",
    "commands",
    "events",
    "operation",
];
const EXECUTE_OPS: &[&str] = &[
    "new", "open", "activate", "run", "start", "input", "cancel", "undo", "redo", "select",
    "property", "action", "save", "stop",
];

#[derive(Clone, Deserialize)]
struct Descriptor {
    session_id: String,
    port: u16,
    token: String,
}

struct GuiClient {
    descriptor: Descriptor,
    state: Value,
    client_id: String,
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(unix)]
fn private_descriptor(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(file) = path.metadata() else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = parent.metadata() else {
        return false;
    };
    file.uid() == directory.uid() && file.mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn private_descriptor(_: &Path) -> bool {
    true
}

fn exchange(descriptor: &Descriptor, request: Value, timeout: Duration) -> Result<Value, String> {
    let mut object = request
        .as_object()
        .cloned()
        .ok_or_else(|| "GUI request must be an object".to_string())?;
    object.insert("token".into(), Value::String(descriptor.token.clone()));
    object.insert(
        "session_id".into(),
        Value::String(descriptor.session_id.clone()),
    );
    object.insert("protocol".into(), Value::from(1));
    let mut wire = serde_json::to_vec(&Value::Object(object)).map_err(|e| e.to_string())?;
    wire.push(b'\n');
    if wire.len() > MAX_REQUEST {
        return Err("Request exceeds 1 MiB".into());
    }

    let mut stream =
        TcpStream::connect(("127.0.0.1", descriptor.port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream.write_all(&wire).map_err(|error| error.to_string())?;

    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE + 1)
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    if response.is_empty() || response.len() as u64 > MAX_RESPONSE {
        return Err("No valid OCS response; query request_id before retrying a mutation".into());
    }
    serde_json::from_str(response.trim_end()).map_err(|error| error.to_string())
}

fn descriptors() -> Result<Vec<(Descriptor, Value)>, String> {
    let directory = crate::config::config_dir()
        .ok_or_else(|| "No user configuration directory".to_string())?
        .join("automation");
    let Ok(entries) = directory.read_dir() else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    paths.sort();

    let mut found = Vec::new();
    for path in paths {
        if !private_descriptor(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(descriptor) = serde_json::from_str::<Descriptor>(&text) else {
            continue;
        };
        let Ok(state) = exchange(&descriptor, json!({"op":"hello"}), Duration::from_secs(1)) else {
            continue;
        };
        if state["ok"].as_bool() == Some(true)
            && state["session_id"].as_str() == Some(descriptor.session_id.as_str())
        {
            found.push((descriptor, state));
        }
    }
    Ok(found)
}

fn log_file() -> Result<File, String> {
    let directory = crate::config::config_dir()
        .ok_or_else(|| "No user configuration directory".to_string())?
        .join("automation");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("gui.log"))
        .map_err(|error| error.to_string())
}

fn start_gui() -> Result<Child, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let log = log_file()?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg("--new-instance")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| error.to_string())
}

fn sessions(launch_if_none: bool) -> Result<Vec<Value>, String> {
    let mut available = descriptors()?;
    if available.is_empty() && launch_if_none {
        let mut child = start_gui()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                return Err(format!("OpenCADStudio exited while starting ({status})"));
            }
            thread::sleep(Duration::from_millis(200));
            available = descriptors()?;
            if !available.is_empty() {
                break;
            }
        }
        if available.is_empty() {
            return Err("OpenCADStudio is still starting; call ocs_sessions again".into());
        }
    }
    Ok(available.into_iter().map(|(_, state)| state).collect())
}

fn insert_default(object: &mut Map<String, Value>, key: &str, value: Value) {
    if !object.contains_key(key) {
        object.insert(key.into(), value);
    }
}

impl GuiClient {
    fn connect(session_id: &str) -> Result<Self, String> {
        let mut matching: Vec<_> = descriptors()?
            .into_iter()
            .filter(|(descriptor, _)| descriptor.session_id == session_id)
            .collect();
        if matching.len() != 1 {
            return Err(format!(
                "Choose session_id from ocs_sessions; found {} matching sessions",
                matching.len()
            ));
        }
        let (descriptor, state) = matching.remove(0);
        Ok(Self {
            descriptor,
            state,
            client_id: random_id()?,
        })
    }

    fn request(&mut self, request: Value, wait_seconds: f64) -> Result<Value, String> {
        let mut object = request
            .as_object()
            .cloned()
            .ok_or_else(|| "request must be an object".to_string())?;
        let op = object
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "request must contain op".to_string())?
            .to_string();
        if !READ_OPS.contains(&op.as_str()) {
            insert_default(&mut object, "request_id", Value::String(random_id()?));
            insert_default(
                &mut object,
                "client_id",
                Value::String(self.client_id.clone()),
            );
            insert_default(
                &mut object,
                "document_id",
                self.state["document_id"].clone(),
            );
            insert_default(&mut object, "revision", self.state["revision"].clone());
            if ["input", "property", "run", "action", "save", "undo", "redo"].contains(&op.as_str())
            {
                insert_default(&mut object, "selection", self.state["selection"].clone());
            }
        }

        let request_id = object.get("request_id").cloned();
        let mut response = exchange(
            &self.descriptor,
            Value::Object(object),
            Duration::from_secs(15),
        )?;
        let wait = wait_seconds.clamp(0.0, 60.0);
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        while matches!(response["status"].as_str(), Some("accepted" | "running"))
            && Instant::now() < deadline
        {
            let Some(request_id) = request_id.clone() else {
                break;
            };
            thread::sleep(Duration::from_millis(50));
            response = exchange(
                &self.descriptor,
                json!({"op":"operation","request_id":request_id}),
                Duration::from_secs(15),
            )?;
        }
        if response.get("state").is_some() {
            self.state = response["state"].clone();
        } else if matches!(op.as_str(), "hello" | "state") && response["ok"].as_bool() == Some(true)
        {
            self.state = response.clone();
        }
        Ok(response)
    }
}

fn client<'a>(
    clients: &'a mut HashMap<String, GuiClient>,
    session_id: &str,
) -> Result<&'a mut GuiClient, String> {
    if !clients.contains_key(session_id) {
        clients.insert(session_id.into(), GuiClient::connect(session_id)?);
    }
    Ok(clients.get_mut(session_id).expect("client inserted"))
}

fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}

fn call_tool(
    name: &str,
    arguments: &Value,
    clients: &mut HashMap<String, GuiClient>,
) -> Result<Value, String> {
    match name {
        "ocs_sessions" => {
            let launch = arguments["launch_if_none"].as_bool().unwrap_or(true);
            Ok(Value::Array(sessions(launch)?))
        }
        "ocs_read" => {
            let session_id = required_string(arguments, "session_id")?;
            let op = arguments["op"].as_str().unwrap_or("state");
            if !READ_OPS.contains(&op) {
                return Err("Use ocs_execute for mutations".into());
            }
            let mut request = arguments["parameters"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            request.insert("op".into(), Value::String(op.into()));
            client(clients, session_id)?.request(Value::Object(request), 30.0)
        }
        "ocs_execute" => {
            let session_id = required_string(arguments, "session_id")?;
            let request = arguments["request"]
                .as_object()
                .cloned()
                .map(Value::Object)
                .ok_or_else(|| "Missing request object".to_string())?;
            let op = required_string(&request, "op")?;
            if !EXECUTE_OPS.contains(&op) {
                return Err(format!("Unknown mutation operation: {op}"));
            }
            let request_id = required_string(&request, "request_id")?;
            if request_id.len() > 128 {
                return Err("request_id must not exceed 128 bytes".into());
            }
            let wait = arguments["wait_seconds"].as_f64().unwrap_or(30.0);
            client(clients, session_id)?.request(request, wait)
        }
        "ocs_capture" => {
            let session_id = required_string(arguments, "session_id")?;
            let path = std::env::temp_dir().join(format!("ocs-capture-{}.png", random_id()?));
            let result = client(clients, session_id)?
                .request(json!({"op":"capture","path":path.to_string_lossy()}), 30.0)?;
            if result["ok"].as_bool() != Some(true)
                || result["status"].as_str() != Some("completed")
            {
                return Err(result.to_string());
            }
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let _ = std::fs::remove_file(path);
            Ok(json!({"$image":BASE64.encode(bytes)}))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name":"ocs_sessions",
            "description":"List real OpenCADStudio GUI sessions and documents. Launch the installed editor if none is running.",
            "inputSchema":{"type":"object","properties":{"launch_if_none":{"type":"boolean","default":true,"description":"Launch OpenCADStudio when no live session exists."}},"additionalProperties":false},
            "annotations":{"title":"List OCS sessions","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_read",
            "description":"Read state, commands/actions, query, entities, layers, header, properties, measurements, history, events or operation status from a live OCS session.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."},"op":{"type":"string","enum":READ_OPS,"default":"state"},"parameters":{"type":"object","description":"Operation-specific filters such as document_id, handles, after, or request_id."}},"required":["session_id"],"additionalProperties":false},
            "annotations":{"title":"Read OCS state","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_execute",
            "description":"Execute one semantic OCS action. Use state document_id and revision. request_id is the idempotency key. Accepted, running and waiting_input are not completion.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."},"request":{"type":"object","properties":{"op":{"type":"string","enum":EXECUTE_OPS,"description":"Semantic editor operation."},"request_id":{"type":"string","minLength":1,"maxLength":128,"description":"Caller-generated idempotency key. Reuse it only when retrying the identical request."},"document_id":{"type":"integer","minimum":0,"description":"Target document from current state."},"revision":{"type":"integer","minimum":0,"description":"Expected edit revision from current state."},"geometry_revision":{"type":"integer","minimum":0,"description":"Expected geometry revision when geometry state matters."},"camera_revision":{"type":"integer","minimum":0,"description":"Expected camera revision when view state matters."},"selection":{"type":"array","items":{"type":"string"},"description":"Expected selected handles from current state."}},"required":["op","request_id"]},"wait_seconds":{"type":"number","minimum":0,"maximum":60,"default":30}},"required":["session_id","request"],"additionalProperties":false},
            "annotations":{"title":"Execute OCS action","readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_capture",
            "description":"Capture the actual current OpenCADStudio window as PNG for visual verification.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."}},"required":["session_id"],"additionalProperties":false},
            "annotations":{"title":"Capture OCS window","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }
    ])
}

fn tool_result(value: Value) -> Value {
    if let Some(image) = value.get("$image").and_then(Value::as_str) {
        return json!({"content":[{"type":"image","data":image,"mimeType":"image/png"}]});
    }
    let structured = if value.is_object() {
        value.clone()
    } else {
        json!({"result":value.clone()})
    };
    json!({
        "content":[{"type":"text","text":value.to_string()}],
        "structuredContent":structured,
        "isError":value["ok"].as_bool() == Some(false)
    })
}

fn error_result(message: impl ToString) -> Value {
    let message = message.to_string();
    json!({
        "content":[{"type":"text","text":message.clone()}],
        "structuredContent":{"ok":false,"error":message},
        "isError":true
    })
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn server_info() -> Value {
    json!({"name":"OpenCADStudio","title":"Open CAD Studio","version":env!("OCS_APP_VERSION")})
}

fn modern_request(params: &Value) -> bool {
    params["_meta"]["io.modelcontextprotocol/protocolVersion"].as_str()
        == Some(MODERN_PROTOCOL_VERSION)
}

fn protocol_result(mut result: Value, modern: bool, cacheable: bool) -> Value {
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
        if cacheable {
            object.insert("ttlMs".into(), Value::from(CACHE_TTL_MS));
            object.insert("cacheScope".into(), Value::String("public".into()));
        }
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

fn handle_message(message: Value, clients: &mut HashMap<String, GuiClient>) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str)?;
    if id.is_none() {
        return None;
    }
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
                    "capabilities":{"tools":{"listChanged":false}},
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
                    "capabilities":{"tools":{}},
                    "instructions":INSTRUCTIONS
                }),
                true,
                true,
            ),
        ),
        "ping" => response(id, protocol_result(json!({}), modern, false)),
        "tools/list" => response(
            id,
            protocol_result(json!({"tools":tool_definitions()}), modern, true),
        ),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing tool name"));
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = call_tool(name, &arguments, clients)
                .map(tool_result)
                .unwrap_or_else(error_result);
            response(id, protocol_result(result, modern, false))
        }
        _ => rpc_error(id, -32601, format!("Method not found: {method}")),
    })
}

/// Run the MCP stdio loop until the client closes stdin.
pub fn run() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut clients = HashMap::new();
    for line in stdin.lock().lines() {
        let response = match line {
            Ok(line) if !line.trim().is_empty() => match serde_json::from_str::<Value>(&line) {
                Ok(message) => handle_message(message, &mut clients),
                Err(error) => Some(rpc_error(Value::Null, -32700, error)),
            },
            Ok(_) => None,
            Err(error) => {
                eprintln!("MCP input error: {error}");
                break;
            }
        };
        if let Some(response) = response {
            if writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            names,
            ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"]
        );
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);
        assert_eq!(
            tools[2]["inputSchema"]["properties"]["request"]["required"],
            json!(["op", "request_id"])
        );
    }

    #[test]
    fn negotiates_and_lists_tools() {
        let mut clients = HashMap::new();
        let initialized = handle_message(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            &mut clients,
        )
        .unwrap();
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        assert!(initialized["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("geometry kernel"));

        let listed = handle_message(
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &mut clients,
        )
        .unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);
        assert!(listed["result"].get("ttlMs").is_none());
        assert!(listed["result"].get("resultType").is_none());
    }

    #[test]
    fn supports_modern_stateless_discovery() {
        let mut clients = HashMap::new();
        let discovered = handle_message(
            json!({"jsonrpc":"2.0","id":"discover","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
            &mut clients,
        )
        .unwrap();
        assert_eq!(discovered["result"]["resultType"], "complete");
        assert_eq!(discovered["result"]["ttlMs"], CACHE_TTL_MS);
        assert_eq!(discovered["result"]["cacheScope"], "public");
        assert_eq!(discovered["result"]["supportedVersions"][0], "2026-07-28");
        assert_eq!(
            discovered["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "OpenCADStudio"
        );

        let listed = handle_message(
            json!({"jsonrpc":"2.0","id":"tools","method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
            &mut clients,
        )
        .unwrap();
        assert_eq!(listed["result"]["resultType"], "complete");
        assert_eq!(listed["result"]["ttlMs"], CACHE_TTL_MS);
        assert_eq!(listed["result"]["cacheScope"], "public");
    }

    #[test]
    fn read_tool_rejects_mutations_before_connecting() {
        let mut clients = HashMap::new();
        let called = handle_message(
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ocs_read","arguments":{"session_id":"missing","op":"save"}}}),
            &mut clients,
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
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"ocs_execute","arguments":{"session_id":"missing","request":{"op":"undo"}}}}),
            &mut clients,
        )
        .unwrap();
        assert_eq!(called["result"]["isError"], true);
        assert!(called["result"]["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("request_id"));
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
        assert_eq!(result["structuredContent"]["code"], "stale_state");
    }
}
