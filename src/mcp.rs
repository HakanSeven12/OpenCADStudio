//! Client-neutral MCP adapter for the live desktop editor.
//!
//! `OpenCADStudio --mcp` speaks MCP over stdio. All drawing work is forwarded
//! to the authenticated GUI control bridge; this module contains no geometry.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, VecDeque},
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
const INSTRUCTIONS: &str = "Call ocs_sessions, then ocs_read state before editing. Preserve session_id, document_id, revision, selection and request_id. Use ocs_read commands with parameters.name for a command manifest. Use ocs_execute batch when several steps are known, and request changed_entities when the resulting geometry is needed. For interactive work, call start and follow state.command.accepts, options and input_example. A run.cmd contains the command name followed by prompt answers separated by spaces; points use x,y or x,y,z. After a timeout, query the existing operation and never replay a mutation with a new request_id. waiting_input and running are not completion. Let OCS and its geometry kernel calculate geometry; use query near, contains_point and intersections for exact relationships. Verify important results with queries and a viewport capture, and save only to an explicit path.";
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
    "property", "action", "save", "stop", "batch",
];
const BATCH_STEP_OPS: &[&str] = &[
    "new", "open", "activate", "run", "start", "input", "cancel", "undo", "redo", "select",
    "property", "action", "save", "stop",
];
const MAX_BATCH_STEPS: usize = 64;

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
    batches: VecDeque<BatchExecution>,
}

struct BatchExecution {
    id: String,
    request: Value,
    steps: Vec<Value>,
    next: usize,
    active: Option<String>,
    results: Vec<Value>,
    changes: Vec<Value>,
    state: Option<Value>,
    terminal: Option<Value>,
}

struct McpTask {
    id: String,
    name: String,
    arguments: Value,
    created_at: String,
    last_updated_at: String,
    result: Option<Value>,
    error: Option<Value>,
}

#[derive(Default)]
struct TaskStore {
    tasks: VecDeque<McpTask>,
}

impl TaskStore {
    fn insert(&mut self, task: McpTask) {
        self.tasks.push_back(task);
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut McpTask> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
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
            batches: VecDeque::new(),
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

    fn execute_batch(&mut self, request: Value, wait_seconds: f64) -> Result<Value, String> {
        let id = required_string(&request, "request_id")?.to_owned();
        let mut batch = if let Some(position) = self.batches.iter().position(|batch| batch.id == id)
        {
            let batch = self
                .batches
                .remove(position)
                .expect("batch position exists");
            if batch.request != request {
                self.batches.push_back(batch);
                return Err("request_id was already used for a different batch".into());
            }
            batch
        } else {
            let steps = request["steps"]
                .as_array()
                .cloned()
                .ok_or_else(|| "batch requires a steps array".to_string())?;
            BatchExecution {
                id,
                request,
                steps,
                next: 0,
                active: None,
                results: Vec::new(),
                changes: Vec::new(),
                state: None,
                terminal: None,
            }
        };

        if let Some(result) = batch.terminal.clone() {
            self.batches.push_back(batch);
            return Ok(result);
        }

        let deadline = Instant::now() + Duration::from_secs_f64(wait_seconds.clamp(0.0, 60.0));
        let mut attempted = false;
        loop {
            if batch.next == batch.steps.len() {
                let waiting = batch
                    .state
                    .as_ref()
                    .is_some_and(|state| !state["command"].is_null());
                let result = batch_result(
                    &batch,
                    if waiting {
                        "waiting_input"
                    } else {
                        "completed"
                    },
                    true,
                );
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            if attempted && Instant::now() >= deadline {
                let result = batch_result(&batch, "running", true);
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            attempted = true;

            let step_id = batch
                .active
                .clone()
                .unwrap_or_else(|| batch_step_id(&batch.id, batch.next));
            let response = if batch.active.is_some() {
                self.request(
                    json!({"op":"operation","request_id":step_id}),
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                )
            } else {
                let mut step = batch.steps[batch.next]
                    .as_object()
                    .cloned()
                    .ok_or_else(|| format!("batch step {} must be an object", batch.next))?;
                for key in [
                    "revision",
                    "geometry_revision",
                    "camera_revision",
                    "selection",
                    "client_id",
                ] {
                    step.remove(key);
                }
                step.insert("request_id".into(), Value::String(step_id.clone()));
                self.request(
                    Value::Object(step),
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                )
            };
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    batch.active = Some(step_id);
                    self.batches.push_back(batch);
                    trim_batches(&mut self.batches);
                    return Err(error);
                }
            };

            if matches!(response["status"].as_str(), Some("accepted" | "running")) {
                batch.active = Some(step_id);
                let result = batch_result(&batch, "running", true);
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }

            batch.active = None;
            if let Some(state) = response.get("state") {
                batch.state = Some(state.clone());
            }
            if let Some(changes) = response["changes"].as_array() {
                batch
                    .changes
                    .extend(changes.iter().cloned().map(|mut change| {
                        if let Some(object) = change.as_object_mut() {
                            object.insert("step".into(), Value::from(batch.next));
                        }
                        change
                    }));
            }
            let mut compact = response.clone();
            if let Some(object) = compact.as_object_mut() {
                object.remove("state");
                object.remove("changes");
                object.insert("step".into(), Value::from(batch.next));
                object.insert("op".into(), batch.steps[batch.next]["op"].clone());
            }
            batch.results.push(compact);
            batch.next += 1;

            if response["ok"].as_bool() == Some(false)
                || matches!(response["status"].as_str(), Some("failed" | "cancelled"))
            {
                let result = batch_result(
                    &batch,
                    response["status"].as_str().unwrap_or("failed"),
                    false,
                );
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            if response["status"] == "waiting_input"
                && batch
                    .steps
                    .get(batch.next)
                    .and_then(|step| step["op"].as_str())
                    .is_none_or(|op| !matches!(op, "input" | "cancel"))
            {
                let result = batch_result(&batch, "waiting_input", true);
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
        }
    }
}

fn batch_step_id(id: &str, step: usize) -> String {
    let hash = id
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("batch-{hash:016x}-{step}")
}

fn batch_result(batch: &BatchExecution, status: &str, ok: bool) -> Value {
    json!({
        "ok":ok,
        "status":status,
        "request_id":batch.id,
        "completed_steps":batch.next,
        "total_steps":batch.steps.len(),
        "next_step":(batch.next < batch.steps.len()).then_some(batch.next),
        "results":batch.results,
        "changes":batch.changes,
        "state":batch.state
    })
}

fn trim_batches(batches: &mut VecDeque<BatchExecution>) {
    while batches.len() > 64 {
        batches.pop_front();
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

fn validate_execute_request(request: &Value, op: &str) -> Result<(), String> {
    let missing = |field: &str, example: &str| {
        Err(format!(
            "Missing {field} for {op}. Example request: {example}"
        ))
    };
    match op {
        "batch" => {
            let steps = request["steps"].as_array().ok_or_else(|| {
                r#"Missing steps for batch. Example request: {"op":"batch","request_id":"draw-1","steps":[{"op":"run","cmd":"LINE 0,0 10,0"}]}"#.to_string()
            })?;
            if steps.is_empty() || steps.len() > MAX_BATCH_STEPS {
                return Err(format!(
                    "batch steps must contain 1 to {MAX_BATCH_STEPS} operations"
                ));
            }
            for (index, step) in steps.iter().enumerate() {
                let step = step
                    .as_object()
                    .ok_or_else(|| format!("batch step {index} must be an object"))?;
                if step.contains_key("request_id") {
                    return Err(format!(
                        "batch step {index} must omit request_id; the batch assigns idempotency keys"
                    ));
                }
                let step = Value::Object(step.clone());
                let step_op = step["op"]
                    .as_str()
                    .ok_or_else(|| format!("batch step {index} is missing op"))?;
                if !BATCH_STEP_OPS.contains(&step_op) {
                    return Err(format!("Unknown operation {step_op} in batch step {index}"));
                }
                validate_execute_request(&step, step_op)
                    .map_err(|error| format!("batch step {index}: {error}"))?;
            }
            Ok(())
        }
        "open" if request["path"].as_str().is_none_or(str::is_empty) => {
            missing("path", r#"{"op":"open","path":"/path/drawing.dxf"}"#)
        }
        "activate" if request["document_id"].as_u64().is_none() => {
            missing("document_id", r#"{"op":"activate","document_id":2}"#)
        }
        "run" | "start" if request["cmd"].as_str().is_none_or(str::is_empty) => {
            missing("cmd", r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#)
        }
        "input" => match request["kind"].as_str() {
            Some("text") => Ok(()),
            Some("token")
                if request["text"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()) =>
            {
                Ok(())
            }
            Some("token") => missing("text", r#"{"op":"input","kind":"token","text":"C"}"#),
            Some("point")
                if request["point"].as_array().is_some_and(|values| {
                    values.len() == 3 && values.iter().all(Value::is_number)
                }) =>
            {
                Ok(())
            }
            Some("point") => missing(
                "point",
                r#"{"op":"input","kind":"point","point":[0,0,0],"space":"wcs"}"#,
            ),
            Some("entity" | "structure")
                if request["handle"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
                    && request["point"].as_array().is_some_and(|values| {
                        values.len() == 3 && values.iter().all(Value::is_number)
                    }) =>
            {
                Ok(())
            }
            Some("entity" | "structure") => missing(
                "handle and point",
                r#"{"op":"input","kind":"entity","handle":"2A","point":[0,0,0]}"#,
            ),
            Some("selection" | "enter") => Ok(()),
            Some(kind) => Err(format!(
                "Unknown input kind {kind}. Use text, token, point, entity, structure, selection or enter"
            )),
            None => missing("kind", r#"{"op":"input","kind":"point","point":[0,0,0]}"#),
        },
        "property"
            if request["field"].as_str().is_none_or(str::is_empty)
                || request.get("value").is_none() =>
        {
            missing(
                "field or value",
                r#"{"op":"property","field":"color","value":1}"#,
            )
        }
        "action" => match request["name"].as_str() {
            Some(name) if crate::app::automation_action_names().contains(&name) => Ok(()),
            Some(name) => Err(format!(
                "Unknown action {name}. Call ocs_read commands to list actions"
            )),
            None => missing("name", r#"{"op":"action","name":"zoom_extents"}"#),
        },
        _ => Ok(()),
    }
}

fn compact_state(state: &Value) -> Value {
    let mut compact = Map::new();
    for key in [
        "session_id",
        "document_id",
        "revision",
        "geometry_revision",
        "camera_revision",
        "selection",
        "command",
        "modal",
        "event_cursor",
        "operation",
    ] {
        if let Some(value) = state.get(key) {
            compact.insert(key.into(), value.clone());
        }
    }
    Value::Object(compact)
}

fn response_handles(response: &Value) -> Vec<String> {
    let mut handles = Vec::new();
    if let Some(changes) = response["changes"].as_array() {
        for handle in changes
            .iter()
            .filter_map(|change| change["handle"].as_str())
        {
            if !handles.iter().any(|value| value == handle) {
                handles.push(handle.to_owned());
            }
        }
    }
    handles
}

fn shape_execute_response(
    mut response: Value,
    detail: &str,
    gui: &mut GuiClient,
) -> Result<Value, String> {
    if detail == "changed_entities" {
        let handles = response_handles(&response);
        if !handles.is_empty() {
            let entities = gui.request(
                json!({"op":"query","handles":handles,"detail":"geometry","limit":MAX_BATCH_STEPS * 100}),
                30.0,
            )?;
            if let Some(object) = response.as_object_mut() {
                object.insert("changed_entities".into(), entities["entities"].clone());
            }
        }
    }
    if detail != "full" {
        if let Some(state) = response.get("state").cloned() {
            if let Some(object) = response.as_object_mut() {
                object.insert("state".into(), compact_state(&state));
            }
        }
    }
    Ok(response)
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
            validate_execute_request(&request, op)?;
            let wait = arguments["wait_seconds"].as_f64().unwrap_or(30.0);
            let detail = arguments["response_detail"].as_str().unwrap_or("compact");
            let gui = client(clients, session_id)?;
            let response = if op == "batch" {
                gui.execute_batch(request, wait)?
            } else {
                gui.request(request, wait)?
            };
            shape_execute_response(response, detail, gui)
        }
        "ocs_capture" => {
            let session_id = required_string(arguments, "session_id")?;
            let path = std::env::temp_dir().join(format!("ocs-capture-{}.png", random_id()?));
            let scope = arguments["scope"].as_str().unwrap_or("viewport");
            let max_dimension = arguments["max_dimension"].as_u64().unwrap_or(1600);
            let result = client(clients, session_id)?
                .request(json!({"op":"capture","path":path.to_string_lossy(),"scope":scope,"max_dimension":max_dimension}), 30.0)?;
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

fn batch_step_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "op":{"type":"string","enum":BATCH_STEP_OPS},
            "document_id":{"type":"integer","minimum":0},
            "cmd":{"type":"string","minLength":1},
            "path":{"type":"string","minLength":1},
            "kind":{"type":"string","enum":["text","token","point","entity","structure","selection","enter"]},
            "text":{"type":"string"},
            "point":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},
            "space":{"type":"string","enum":["wcs","ucs","relative"]},
            "handle":{"type":"string"},
            "handles":{"type":"array","items":{"type":"string"}},
            "type":{"type":"string"},
            "layer":{"type":"string"},
            "clear":{"type":"boolean"},
            "field":{"type":"string"},
            "value":{"description":"New property value; its JSON type must match the property kind.","anyOf":[{"type":"string"},{"type":"number"},{"type":"boolean"},{"type":"object"},{"type":"array"},{"type":"null"}]},
            "name":{"type":"string","enum":crate::app::automation_action_names()}
        },
        "required":["op"],
        "additionalProperties":false
    })
}

fn execute_request_schema() -> Value {
    let handle = json!({
        "type":"string",
        "pattern":"^(0[xX])?[0-9A-Fa-f]+$",
        "description":"Entity handle returned by ocs_read entities or state selection."
    });
    let point = json!({
        "type":"array","items":{"type":"number"},"minItems":3,"maxItems":3,
        "description":"Finite [x,y,z] coordinates."
    });
    json!({
        "type":"object",
        "properties":{
            "op":{"type":"string","enum":EXECUTE_OPS,"description":"Semantic editor operation."},
            "request_id":{"type":"string","minLength":1,"maxLength":128,"description":"Caller-generated idempotency key. Reuse it only when retrying the identical request."},
            "document_id":{"type":"integer","minimum":0,"description":"Target document from current state."},
            "revision":{"type":"integer","minimum":0,"description":"Expected edit revision from current state."},
            "geometry_revision":{"type":"integer","minimum":0,"description":"Expected geometry revision when geometry state matters."},
            "camera_revision":{"type":"integer","minimum":0,"description":"Expected camera revision when view state matters."},
            "selection":{"type":"array","items":handle.clone(),"description":"Expected selected handles from current state."},
            "cmd":{"type":"string","minLength":1,"description":"Command name followed by its prompt answers separated by spaces. Points use x,y or x,y,z; option answers use their token. Read command details first when unsure.","examples":["LINE 0,0 10,10","CIRCLE 5,5 3","PLINE 0,0 10,0 10,10 C"]},
            "path":{"type":"string","minLength":1,"description":"Absolute drawing path for open or save."},
            "kind":{"type":"string","enum":["text","token","point","entity","structure","selection","enter"],"description":"Input kind listed in state.command.accepts."},
            "text":{"type":"string","description":"Free text or one option/value token."},
            "point":point,
            "space":{"type":"string","enum":["wcs","ucs","relative"],"default":"wcs","description":"Coordinate space for point input."},
            "handle":handle.clone(),
            "handles":{"type":"array","items":handle,"description":"Entity handles to select."},
            "type":{"type":"string","description":"Entity type filter for select."},
            "layer":{"type":"string","description":"Layer filter for select."},
            "clear":{"type":"boolean","description":"Clear the current selection before applying select filters."},
            "field":{"type":"string","minLength":1,"description":"Property id returned by ocs_read properties."},
            "value":{"description":"New property value; its JSON type must match the property kind.","anyOf":[{"type":"string"},{"type":"number"},{"type":"boolean"},{"type":"object"},{"type":"array"},{"type":"null"}]},
            "name":{"type":"string","enum":crate::app::automation_action_names(),"description":"UI action returned by ocs_read commands."},
            "steps":{"type":"array","minItems":1,"maxItems":MAX_BATCH_STEPS,"description":"Sequential editor operations executed with fresh state and idempotency keys. Execution stops at the first failure; completed_steps says what committed.","items":batch_step_schema()}
        },
        "required":["op","request_id"],
        "additionalProperties":false,
        "oneOf":[
            {"properties":{"op":{"const":"new"}}},
            {"properties":{"op":{"const":"open"}},"required":["path"]},
            {"properties":{"op":{"const":"activate"}},"required":["document_id"]},
            {"properties":{"op":{"const":"run"}},"required":["cmd"]},
            {"properties":{"op":{"const":"start"}},"required":["cmd"]},
            {"properties":{"op":{"const":"input"}},"required":["kind"],"oneOf":[
                {"properties":{"kind":{"const":"text"}}},
                {"properties":{"kind":{"const":"token"}},"required":["text"]},
                {"properties":{"kind":{"const":"point"}},"required":["point"]},
                {"properties":{"kind":{"const":"entity"}},"required":["handle","point"]},
                {"properties":{"kind":{"const":"structure"}},"required":["handle","point"]},
                {"properties":{"kind":{"const":"selection"}}},
                {"properties":{"kind":{"const":"enter"}}}
            ]},
            {"properties":{"op":{"const":"cancel"}}},
            {"properties":{"op":{"const":"undo"}}},
            {"properties":{"op":{"const":"redo"}}},
            {"properties":{"op":{"const":"select"}}},
            {"properties":{"op":{"const":"property"}},"required":["field","value"]},
            {"properties":{"op":{"const":"action"}},"required":["name"]},
            {"properties":{"op":{"const":"save"}}},
            {"properties":{"op":{"const":"stop"}}},
            {"properties":{"op":{"const":"batch"}},"required":["steps"]}
        ]
    })
}

fn read_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ok":{"type":"boolean"},"status":{"type":"string"},"code":{"type":"string"},
            "error":{"anyOf":[{"type":"string"},{"type":"null"}]},"document_id":{"type":"integer"},
            "revision":{"type":"integer"},"geometry_revision":{"type":"integer"},
            "camera_revision":{"type":"integer"}
        },
        "required":["ok"],"additionalProperties":true
    })
}

fn execute_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ok":{"type":"boolean"},
            "status":{"type":"string","enum":["accepted","running","waiting_input","completed","cancelled","failed"]},
            "request_id":{"type":"string"},"code":{"type":"string"},"error":{"anyOf":[{"type":"string"},{"type":"null"}]},
            "result":{"type":"object"},"changes":{"anyOf":[{"type":"array"},{"type":"null"}]},"state":{"type":"object"}
        },
        "required":["ok"],"additionalProperties":true
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name":"ocs_sessions",
            "description":"List real OpenCADStudio GUI sessions and documents. Launch the installed editor if none is running.",
            "inputSchema":{"type":"object","properties":{"launch_if_none":{"type":"boolean","default":true,"description":"Launch OpenCADStudio when no live session exists."}},"additionalProperties":false},
            "outputSchema":{"type":"object","properties":{"result":{"type":"array","items":{"type":"object","properties":{"ok":{"const":true},"session_id":{"type":"string"},"document_id":{"type":"integer"},"revision":{"type":"integer"},"selection":{"type":"array","items":{"type":"string"}},"documents":{"type":"array"}},"required":["ok","session_id","document_id","revision","selection","documents"],"additionalProperties":true}}},"required":["result"],"additionalProperties":false},
            "annotations":{"title":"List OCS sessions","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_read",
            "description":"Read state, command manifests, entities, layers, properties, kernel measurements and spatial relationships, history, events or operation status from a live OCS session.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."},"op":{"type":"string","enum":READ_OPS,"default":"state"},"parameters":{"type":"object","description":"Operation-specific filters.","properties":{"name":{"type":"string","description":"Command name for detailed commands help."},"search":{"type":"string","description":"Case-insensitive command-name search."},"document_id":{"type":"integer","minimum":0},"handle":{"type":"string"},"handles":{"type":"array","items":{"type":"string"},"description":"Exact entity handles for query or measure."},"type":{"type":"string","description":"Entity type filter for query."},"layer":{"type":"string","description":"Layer name filter for query."},"detail":{"type":"string","enum":["summary","geometry","full"],"default":"geometry","description":"Entity detail returned by query."},"fields":{"type":"array","items":{"type":"string"},"description":"Return only these entity fields plus handle."},"near":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Rank planar curves by exact kernel distance to this world XY point."},"contains_point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Return closed planar curves containing this world XY point."},"bounds":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"Filter entities whose world XY bounds overlap [min_x,min_y,max_x,max_y]."},"intersections":{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":2,"description":"Return exact kernel intersections between two planar curve handles."},"after":{"type":"integer","minimum":0,"description":"Event cursor."},"request_id":{"type":"string","description":"Operation id to query."},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":10000}},"additionalProperties":false}},"required":["session_id"],"additionalProperties":false},
            "outputSchema":read_output_schema(),
            "annotations":{"title":"Read OCS state","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_execute",
            "description":"Execute semantic OCS actions. Use current state fields and a unique request_id. Use run for one complete command, batch to remove round trips, or start plus input for guided steps. accepted, running and waiting_input are not completion.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."},"request":execute_request_schema(),"wait_seconds":{"type":"number","minimum":0,"maximum":60,"default":30,"description":"Total time to wait for completion before returning."},"response_detail":{"type":"string","enum":["compact","changed_entities","full"],"default":"compact","description":"compact returns only state needed for the next edit; changed_entities also returns current geometry for changed handles; full preserves the complete editor state."}},"required":["session_id","request"],"additionalProperties":false},
            "outputSchema":execute_output_schema(),
            "annotations":{"title":"Execute OCS action","readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_capture",
            "description":"Capture the actual current OCS drawing viewport or window as a bounded PNG for visual verification.",
            "inputSchema":{"type":"object","properties":{"session_id":{"type":"string","minLength":1,"description":"Session returned by ocs_sessions."},"scope":{"type":"string","enum":["viewport","window"],"default":"viewport","description":"Capture only the drawing viewport by default, or the complete application window."},"max_dimension":{"type":"integer","minimum":256,"maximum":4096,"default":1600,"description":"Resize the longest image edge to at most this many pixels."}},"required":["session_id"],"additionalProperties":false},
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
        "structuredContent":{"ok":false,"status":"failed","code":"invalid_arguments","error":message,"retryable":false},
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

fn supports_tasks(params: &Value) -> bool {
    params["_meta"]["io.modelcontextprotocol/clientCapabilities"]["extensions"]
        ["io.modelcontextprotocol/tasks"]
        .is_object()
}

fn task_value(task: &McpTask, status: &str) -> Value {
    let mut value = json!({
        "resultType":"complete",
        "taskId":task.id,
        "status":status,
        "createdAt":task.created_at,
        "lastUpdatedAt":task.last_updated_at,
        "ttlMs":3_600_000,
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

fn poll_task(task: &mut McpTask, clients: &mut HashMap<String, GuiClient>) -> Value {
    if task.result.is_some() {
        return task_value(task, "completed");
    }
    if task.error.is_some() {
        return task_value(task, "failed");
    }
    task.last_updated_at = iso8601_now();
    let mut arguments = task.arguments.clone();
    arguments["wait_seconds"] = Value::from(0);
    match call_tool(&task.name, &arguments, clients) {
        Ok(value) if matches!(value["status"].as_str(), Some("accepted" | "running")) => {
            task_value(task, "working")
        }
        Ok(value) => {
            task.result = Some(tool_result(value));
            task_value(task, "completed")
        }
        Err(error) => {
            task.error = Some(json!({"code":-32000,"message":error}));
            task_value(task, "failed")
        }
    }
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

fn handle_message(
    message: Value,
    clients: &mut HashMap<String, GuiClient>,
    tasks: &mut TaskStore,
) -> Option<Value> {
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
                    "capabilities":{"tools":{},"extensions":{"io.modelcontextprotocol/tasks":{}}},
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
            let called = call_tool(name, &arguments, clients);
            if modern && supports_tasks(&params) {
                if let Ok(value) = &called {
                    if matches!(value["status"].as_str(), Some("accepted" | "running")) {
                        let task_id =
                            random_id().unwrap_or_else(|_| format!("task-{}", iso8601_now()));
                        let now = iso8601_now();
                        tasks.insert(McpTask {
                            id: task_id.clone(),
                            name: name.to_owned(),
                            arguments,
                            created_at: now.clone(),
                            last_updated_at: now,
                            result: None,
                            error: None,
                        });
                        return Some(response(
                            id,
                            protocol_result(
                                json!({
                                    "resultType":"task",
                                    "taskId":task_id,
                                    "status":"working",
                                    "statusMessage":"OCS operation is running.",
                                    "createdAt":tasks.tasks.back().unwrap().created_at,
                                    "lastUpdatedAt":tasks.tasks.back().unwrap().last_updated_at,
                                    "ttlMs":3_600_000,
                                    "pollIntervalMs":250
                                }),
                                true,
                                false,
                            ),
                        ));
                    }
                }
            }
            let result = called.map(tool_result).unwrap_or_else(error_result);
            response(id, protocol_result(result, modern, false))
        }
        "tasks/get" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            let Some(task) = tasks.get_mut(task_id) else {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            };
            response(id, protocol_result(poll_task(task, clients), true, false))
        }
        "tasks/update" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            if tasks.get_mut(task_id).is_none() {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            }
            response(id, protocol_result(json!({}), true, false))
        }
        "tasks/cancel" if modern && supports_tasks(&params) => {
            let Some(task_id) = params["taskId"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing taskId"));
            };
            if tasks.get_mut(task_id).is_none() {
                return Some(rpc_error(id, -32602, "Unknown or expired taskId"));
            }
            response(id, protocol_result(json!({}), true, false))
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
    let mut tasks = TaskStore::default();
    for line in stdin.lock().lines() {
        let response = match line {
            Ok(line) if !line.trim().is_empty() => match serde_json::from_str::<Value>(&line) {
                Ok(message) => handle_message(message, &mut clients, &mut tasks),
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
        assert_eq!(
            tools[2]["inputSchema"]["properties"]["request"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            EXECUTE_OPS.len()
        );
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
            tools[2]["inputSchema"]["properties"]["request"]["properties"]["cmd"]["examples"][0],
            "LINE 0,0 10,10"
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
        )
        .unwrap();
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
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
            &mut TaskStore::default(),
        )
        .unwrap();
        assert_eq!(discovered["result"]["resultType"], "complete");
        assert_eq!(discovered["result"]["ttlMs"], CACHE_TTL_MS);
        assert_eq!(discovered["result"]["cacheScope"], "public");
        assert_eq!(discovered["result"]["supportedVersions"][0], "2026-07-28");
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
            &mut TaskStore::default(),
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
            &mut TaskStore::default(),
        )
        .unwrap();
        assert_eq!(called["result"]["isError"], true);
        assert!(
            called["result"]["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("request_id")
        );
    }

    #[test]
    fn execute_errors_explain_missing_operation_fields() {
        let mut clients = HashMap::new();
        let called = handle_message(
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"ocs_execute","arguments":{"session_id":"missing","request":{"op":"run","request_id":"run-1"}}}}),
            &mut clients,
            &mut TaskStore::default(),
        )
        .unwrap();
        assert_eq!(called["result"]["isError"], true);
        assert_eq!(
            called["result"]["structuredContent"]["code"],
            "invalid_arguments"
        );
        let error = called["result"]["structuredContent"]["error"]
            .as_str()
            .unwrap();
        assert!(error.contains("Missing cmd"), "{error}");
        assert!(error.contains("LINE 0,0 10,10"), "{error}");
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
        };
        let value = task_value(&task, "working");
        assert_eq!(value["resultType"], "complete");
        assert_eq!(value["status"], "working");
        assert_eq!(value["pollIntervalMs"], 250);
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
