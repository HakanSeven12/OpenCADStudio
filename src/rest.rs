//! REST transport — the automation API over plain localhost HTTP.
//!
//! `OpenCADStudio --http 8090` serves the same operation dispatcher as
//! `--serve` behind resource-oriented JSON endpoints that any HTTP client —
//! curl, Python `urllib`/`requests`, JS `fetch`, C# `HttpClient` — can call
//! with no SDK and no AI-specific machinery. The REST layer is a thin
//! adapter: every route builds the protocol-1 JSON request and feeds the
//! shared `automation_op`, so GUI, stdio, TCP, MCP, wasm and HTTP all run
//! the identical handlers with identical validation, undo and idempotency.
//!
//! Conventions: one request per connection (`Connection: close`), JSON
//! bodies, permissive CORS for local browser clients, loopback bind only.
//! Connections are served one thread each: a slow op (plot, open a large
//! drawing) or a client that connects and stalls must never keep other
//! callers — `state`, `get_selection`, `cancel`, `operation` — from being
//! answered. Mutation endpoints get a server-generated `request_id` and the
//! active `document_id`; a stale-state refusal refreshes state and retries
//! once.
//!
//! The route table is shared: `plan` resolves (method, path) to a `Plan`
//! that this headless server executes directly and the GUI-hosted bridge
//! executes over the live editor's envelope queue (`app::control::http_bridge`),
//! so both `--http` modes serve the identical surface.

use crate::app::OpenCADStudio;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_BODY: usize = 16 * 1024 * 1024;

/// Serial for request ids stamped onto `POST /api/v1/{op}` passthroughs that
/// arrive without one — the protocol-1 envelope requires a unique id, and
/// the caller's own id always wins when present.
static PASSTHROUGH_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Connections served at once. A client that connects and stalls pins one
/// thread until its idle timeout, not the whole listener.
const MAX_CONNECTIONS: usize = 8;

/// How long one connection may sit between bytes before the server gives up
/// on it (a client that opened a socket and never sent the request). Bounded
/// so a stuck client cannot pin a thread forever.
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);

/// Ops that go through the protocol-1 control envelope when posted via the
/// generic `POST /api/v1/{op}` passthrough. Everything else runs legacy.
const ENVELOPE_OPS: &[&str] = &[
    "entities_create",
    "entities_delete",
    "entities_transform",
    "block_define",
    "block_delete",
    "file_identity",
    "xdata_set",
    "view_focus",
    "wblock",
    "plot",
    "embed_image",
    "set_properties",
    "property",
    "activate",
    "get_selection",
    "getpoint",
];

/// Loopback REST channel hosted by the GUI process: `OpenCADStudio.exe
/// "<file>" --http <port>` boots the editor with a bridge that forwards
/// requests into the live app (see `app::control::http_bridge`), so a client
/// can read what the person at the screen selected. `--http` without files
/// keeps the headless server above.
pub fn set_gui_http_port(port: u16) {
    crate::app::control::http_bridge::set_gui_http_port(port);
}

/// Failure codes worth one state refresh + retry (the request never started).
pub(crate) const RETRYABLE: &[&str] = &[
    "document_required",
    "document_closed",
    "document_not_active",
    "stale_state",
    "session_changed",
];

pub fn serve(port: u16) {
    listen(OpenCADStudio::new(), port, Arc::new(AtomicU16::new(0)));
}

pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    query: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn param(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// One request handed from a connection thread to the dispatcher that owns
/// the private app. `OpenCADStudio` is not `Send` (it holds `Rc`/`RefCell`
/// scene state), so — exactly like the GUI-hosted bridge queues envelopes
/// into the editor — connections funnel JSON through this channel and the
/// dispatcher (the thread that called `listen`) answers on the per-request
/// reply channel.
struct Job {
    request: HttpRequest,
    reply: std::sync::mpsc::Sender<(u16, Value)>,
}

fn listen(mut app: OpenCADStudio, port: u16, bound_port: Arc<AtomicU16>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("--http: cannot bind 127.0.0.1:{port}: {error}");
            return;
        }
    };
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    bound_port.store(bound, Ordering::SeqCst);
    eprintln!("OpenCADStudio REST listening on http://127.0.0.1:{bound}/api/v1");
    let (jobs, incoming) = std::sync::mpsc::channel::<Job>();
    // Connections are served one thread each: a slow op (plot, open a large
    // drawing) or a client that connects and stalls must never keep other
    // callers — `state`, `get_selection`, `cancel`, `operation` — from being
    // accepted and answered.
    let clients = Arc::new(AtomicUsize::new(0));
    let acceptor = jobs.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if clients.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                clients.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let jobs = acceptor.clone();
            let clients = clients.clone();
            std::thread::spawn(move || {
                serve_connection(stream, jobs);
                clients.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    // The dispatcher keeps the private app on the thread that called
    // `listen` and answers one request at a time — the same one-session
    // pipeline every transport shares; concurrency lives in the connection
    // threads above, not in the app.
    let mut document_id: Option<u64> = None;
    let mut counter: u64 = 0;
    for job in incoming {
        let (status, body) = route(&mut app, &job.request, &mut document_id, &mut counter);
        let _ = job.reply.send((status, body));
    }
}

/// Read one request off a connection, hand it to the dispatcher, write the
/// answer. Everything here is per-connection I/O; the app is never touched.
fn serve_connection(mut stream: TcpStream, jobs: std::sync::mpsc::Sender<Job>) {
    let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IDLE_TIMEOUT));
    let Ok(Some(request)) = read_request(&mut stream) else {
        return;
    };
    if request.method == "OPTIONS" {
        let _ = write_response(&mut stream, 204, &Value::Null);
        return;
    }
    let (reply, answer) = std::sync::mpsc::channel();
    if jobs
        .send(Job {
            request,
            reply,
        })
        .is_err()
    {
        return;
    }
    if let Ok((status, mut body)) = answer.recv() {
        if let Some(object) = body.as_object_mut() {
            object.insert("http_status".into(), json!(status));
        }
        let _ = write_response(&mut stream, status, &body);
    }
}

/// Parse one HTTP/1.1 request. `Ok(None)` = the peer hung up cleanly.
pub(crate) fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), parse_query(query)),
        None => (target, Vec::new()),
    };

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header
            .strip_prefix("Content-Length:")
            .or_else(|| header.strip_prefix("content-length:"))
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    if content_length > MAX_BODY {
        return Ok(Some(HttpRequest {
            method,
            path,
            query,
            body: Vec::new(),
        }));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(Some(HttpRequest {
        method,
        path,
        query,
        body,
    }))
}

fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (percent_decode(key), percent_decode(value)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.replace('+', " ").into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn write_response(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let body = if body.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(body).unwrap_or_default()
    };
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(&body)?;
    }
    stream.flush()
}

/// One resolved REST call, shared by the two `--http` transports: this
/// headless server executes it against its private app, and the GUI-hosted
/// bridge (`app::control::http_bridge`) forwards the same request into the
/// live editor's automation queue. Building the request here keeps both
/// surfaces identical — same URLs, same ops, same document-id handling.
pub(crate) enum Plan {
    /// Answer without the automation pipeline (OpenAPI, unknown routes).
    Local(u16, Value),
    /// `GET /ready`: a state read rebuilt as the readiness snapshot.
    Ready,
    /// Run one prebuilt protocol-1 request as-is — reads, and callers that
    /// manage `document_id` themselves (the MCP-style clients).
    Run { request: Value, created: u16 },
    /// Run one op through the mutation wrapper: envelope with a request id,
    /// the cached `document_id` when the caller omitted it, one state-refresh
    /// retry after a stale refusal, and the bulky `state` snapshot stripped
    /// from the answer.
    Mutate { op: String, fields: Value, created: u16 },
}

impl Plan {
    /// The automation op this plan runs, for transports that vary connection
    /// behavior per op (the GUI bridge parks the interactive picks).
    pub(crate) fn op(&self) -> Option<&str> {
        match self {
            Plan::Local(..) | Plan::Ready => None,
            Plan::Run { request, .. } => request["op"].as_str(),
            Plan::Mutate { op, .. } => Some(op),
        }
    }
}

/// The whole REST surface: map (method, path) to the automation request it
/// runs. The caller splits `/api/v1` off already; the remaining segments
/// select the arm.
pub(crate) fn plan(method: &str, rest: &[&str], request: &HttpRequest) -> Plan {
    let raw = request.json();
    let body = if raw.is_object() { raw } else { json!({}) };
    match (method, rest) {
        ("GET", ["ready"]) | ("GET", []) | ("GET", [""]) => Plan::Ready,
        ("GET", ["capabilities"]) => Plan::Run {
            request: json!({"op":"capabilities"}),
            created: 200,
        },
        ("GET", ["state"]) | ("GET", ["documents"]) => Plan::Run {
            request: json!({"protocol":1,"op":"state"}),
            created: 200,
        },
        ("POST", ["documents"]) => {
            if let Some(template) = body["template"].as_str() {
                Plan::Mutate { op: "new".into(), fields: json!({"template": template}), created: 201 }
            } else if let Some(path) = body["path"].as_str() {
                Plan::Run { request: json!({"op":"open","path":path}), created: 201 }
            } else {
                // A fresh untitled document in
                // its own tab, so cross-document operations have a target.
                Plan::Mutate { op: "new".into(), fields: json!({}), created: 201 }
            }
        }
        ("DELETE", ["documents", id]) => {
            let mut fields = json!({});
            if let Some(id) = id.parse::<u64>().ok() {
                fields["document_id"] = json!(id);
            }
            if request.param("discard").is_some_and(|v| v == "true" || v == "1") {
                fields["discard"] = json!(true);
            }
            Plan::Mutate { op: "close".into(), fields, created: 200 }
        }
        ("GET", ["sysvars"]) => {
            let names = request
                .param("names")
                .map(|csv| json!(csv.split(',').collect::<Vec<_>>()))
                .unwrap_or_else(|| json!([]));
            Plan::Mutate { op: "sysvar".into(), fields: json!({"get": names}), created: 200 }
        }
        ("POST", ["sysvars"]) => Plan::Mutate { op: "sysvar".into(), fields: body, created: 200 },
        ("POST", ["layouts"]) => Plan::Mutate { op: "layout_create".into(), fields: body, created: 201 },
        ("PUT", ["layouts", layout, "page-setup"]) => {
            let mut fields = body;
            fields["layout"] = json!(layout);
            Plan::Mutate { op: "page_setup_set".into(), fields, created: 200 }
        }
        ("POST", ["entities", "copy-to"]) =>
            Plan::Mutate { op: "entities_copy_to".into(), fields: body, created: 201 },
        ("POST", ["groups"]) => Plan::Mutate { op: "group_create".into(), fields: body, created: 201 },
        ("POST", ["selection-sets"]) =>
            Plan::Mutate { op: "selection_set_save".into(), fields: body, created: 201 },
        ("GET", ["selection-sets", name]) => {
            let select = request.param("select").is_some_and(|v| v == "true" || v == "1");
            Plan::Mutate {
                op: "selection_set_load".into(),
                fields: json!({"name": name, "select": select}),
                created: 200,
            }
        }
        ("GET", ["entities"]) => {
            let mut request_json = json!({"op":"query"});
            if let Some(v) = request.param("type") { request_json["type"] = json!(v); }
            if let Some(v) = request.param("layer") { request_json["layer"] = json!(v); }
            if let Some(v) = request.param("detail") { request_json["detail"] = json!(v); }
            if let Some(v) = request.param("offset") { request_json["offset"] = json!(v); }
            if let Some(v) = request.param("limit") { request_json["limit"] = json!(v); }
            if let Some(v) = request.param("fields") {
                request_json["fields"] = json!(v.split(',').collect::<Vec<_>>());
            }
            if let Some(v) = request.param("handles") {
                request_json["handles"] = json!(v.split(',').collect::<Vec<_>>());
            }
            if let Some(v) = request.param("bounds") {
                let numbers: Vec<f64> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                if numbers.len() == 4 {
                    request_json["bounds"] = json!(numbers);
                }
            }
            if let Some(v) = request.param("near") {
                let numbers: Vec<f64> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                request_json["near"] = json!(numbers);
            }
            if let Some(v) = request.param("contains_point") {
                let numbers: Vec<f64> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                request_json["contains_point"] = json!(numbers);
            }
            if let Some(v) = request.param("intersections") {
                request_json["intersections"] = json!(v.split(',').collect::<Vec<_>>());
            }
            if let Some(v) = request.param("where") {
                match serde_json::from_str::<Value>(v) {
                    Ok(filters) => request_json["where"] = filters,
                    Err(error) => {
                        return Plan::Local(
                            400,
                            json!({"ok": false, "code": "invalid_where", "error": format!("where must be a JSON array: {error}")}),
                        )
                    }
                }
            }
            Plan::Run { request: request_json, created: 200 }
        }
        ("GET", ["entities", handle]) => Plan::Run {
            request: json!({"op":"query","handles":[handle],"detail":"full"}),
            created: 200,
        },
        ("POST", ["entities"]) =>
            Plan::Mutate { op: "entities_create".into(), fields: body, created: 201 },
        ("DELETE", ["entities"]) => {
            let handles = match request.param("handles") {
                Some(csv) => json!(csv.split(',').collect::<Vec<_>>()),
                None => body["handles"].clone(),
            };
            Plan::Mutate { op: "entities_delete".into(), fields: json!({"handles": handles}), created: 200 }
        }
        ("POST", ["entities", "transform"]) =>
            Plan::Mutate { op: "entities_transform".into(), fields: body, created: 200 },
        ("GET", ["entities", handle, "xdata"]) => {
            let mut request_json = json!({"protocol":1,"op":"xdata_get","handles":[handle]});
            if let Some(app_name) = request.param("app") {
                request_json["app"] = json!(app_name);
            }
            Plan::Run { request: request_json, created: 200 }
        }
        ("PUT", ["entities", handle, "xdata", app_name]) => {
            let data = request.json();
            let data = if data.is_null() { json!([]) } else { data };
            Plan::Mutate {
                op: "xdata_set".into(),
                fields: json!({"handles":[handle], "app": app_name, "data": data}),
                created: 200,
            }
        }
        ("DELETE", ["entities", handle, "xdata", app_name]) => Plan::Mutate {
            op: "xdata_set".into(),
            fields: json!({"handles":[handle], "app": app_name, "data": []}),
            created: 200,
        },
        ("POST", ["blocks"]) => Plan::Mutate { op: "block_define".into(), fields: body, created: 201 },
        ("DELETE", ["blocks", name]) =>
            Plan::Mutate { op: "block_delete".into(), fields: json!({"name": name}), created: 200 },
        ("POST", ["file-identity"]) =>
            Plan::Mutate { op: "file_identity".into(), fields: body, created: 200 },
        ("POST", ["plot"]) => Plan::Mutate { op: "plot".into(), fields: body, created: 200 },
        ("POST", ["wblock"]) => Plan::Mutate { op: "wblock".into(), fields: body, created: 200 },
        ("POST", ["images"]) => Plan::Mutate { op: "embed_image".into(), fields: body, created: 201 },
        ("POST", ["commands"]) => Plan::Run {
            request: json!({"op":"run","cmd":body["cmd"]}),
            created: 200,
        },
        ("POST", ["undo"]) | ("POST", ["redo"]) => {
            let op = if rest == ["undo"] { "undo" } else { "redo" };
            Plan::Run { request: json!({"op":op}), created: 200 }
        }
        ("POST", ["save"]) => {
            let mut request_json = json!({"op":"save"});
            if let Some(path) = body["path"].as_str() {
                request_json["path"] = json!(path);
            }
            Plan::Run { request: request_json, created: 200 }
        }
        ("GET", ["layers"]) => Plan::Run { request: json!({"op":"layers"}), created: 200 },
        ("GET", ["header"]) => Plan::Run { request: json!({"op":"header"}), created: 200 },
        ("GET", ["records"]) => {
            let mut request_json = json!({"op":"records"});
            if let Some(v) = request.param("collection") { request_json["collection"] = json!(v); }
            if let Some(v) = request.param("type") { request_json["type"] = json!(v); }
            if let Some(v) = request.param("offset") { request_json["offset"] = json!(v); }
            if let Some(v) = request.param("limit") { request_json["limit"] = json!(v); }
            Plan::Run { request: request_json, created: 200 }
        }
        ("GET", ["openapi"]) => Plan::Local(
            200,
            serde_json::from_str(include_str!("rest_openapi.json")).unwrap_or(json!({})),
        ),
        ("POST", [op]) if ENVELOPE_OPS.contains(op) =>
            Plan::Mutate { op: op.to_string(), fields: body, created: 200 },
        ("POST", [op]) => {
            let mut request_json = body;
            request_json["op"] = json!(op);
            // Stamp protocol and a request id the way the GUI bridge's
            // `prepare` does: the passthrough exists for ops only the
            // protocol-1 envelope knows (`user_select`, `operation`,
            // `cancel`), and unstamped they die as "unknown op" on the
            // legacy path instead of answering with their real contract.
            request_json["protocol"] = json!(1);
            if request_json["request_id"].as_str().is_none_or(str::is_empty) {
                let serial = PASSTHROUGH_SERIAL.fetch_add(1, Ordering::Relaxed);
                request_json["request_id"] = json!(format!("rest-{serial}"));
            }
            Plan::Run { request: request_json, created: 200 }
        }
        (method, _) if matches!(method, "GET" | "POST" | "PUT" | "DELETE") => {
            let path = request.path.trim_end_matches('/');
            let path = if path.is_empty() { "/" } else { path };
            Plan::Local(
                404,
                json!({"ok": false, "code": "unknown_route", "error": format!("No route {method} {path}")}),
            )
        }
        _ => Plan::Local(
            405,
            json!({"ok": false, "code": "method_not_allowed", "error": format!("{method} not supported here")}),
        ),
    }
}

/// The headless executor: run a resolved plan against this server's private
/// app. The GUI-hosted bridge runs the same plans over its envelope queue.
fn route(
    app: &mut OpenCADStudio,
    request: &HttpRequest,
    document_id: &mut Option<u64>,
    counter: &mut u64,
) -> (u16, Value) {
    let path = request.path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.first() != Some(&"api") || segments.get(1) != Some(&"v1") {
        return (404, json!({"ok": false, "code": "unknown_route", "error": "Use /api/v1/…"}));
    }
    match plan(&request.method, &segments[2..], request) {
        Plan::Local(status, body) => (status, body),
        Plan::Ready => {
            let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
            harvest(document_id, &state);
            (200, json!({
                "ok": true,
                "ready": true,
                "version": state["version"],
                "session_id": state["session_id"],
                "document_id": state["document_id"],
            }))
        }
        Plan::Run { request, created } => {
            let response = app.automation_op(&request.to_string());
            harvest(document_id, &response);
            (map_status(response.clone(), created == 201), response)
        }
        Plan::Mutate { op, fields, created } => {
            run_mutation(app, &op, fields, document_id, counter, created)
        }
    }
}

/// Track the active document from automation responses so later mutations
/// address it without the caller repeating `document_id`.
fn harvest(document_id: &mut Option<u64>, response: &Value) {
    let id = response
        .get("document_id")
        .and_then(Value::as_u64)
        .or_else(|| response["state"]["document_id"].as_u64());
    if let Some(id) = id {
        *document_id = Some(id);
    }
}

/// Wrap an operation in the protocol-1 envelope with a server-generated
/// request_id, address the active document, and — when the dispatcher
/// refuses because our cached state is behind — refresh once and retry.
fn run_mutation(
    app: &mut OpenCADStudio,
    op: &str,
    mut fields: Value,
    document_id: &mut Option<u64>,
    counter: &mut u64,
    created: u16,
) -> (u16, Value) {
    let mut attempt = |app: &mut OpenCADStudio, id: u64, document_id: &mut Option<u64>| -> Value {
        *counter += 1;
        let cached = *document_id;
        let mut envelope = json!({
            "protocol": 1,
            "op": op,
            "request_id": format!("http-{id}-{}", *counter),
        });
        if let Some(id) = cached {
            envelope["document_id"] = json!(id);
        }
        if let Some(object) = fields.as_object_mut() {
            for (key, value) in object.clone() {
                envelope[key] = value;
            }
        }
        let response = app.automation_op(&envelope.to_string());
        // Track the active document before the snapshot is stripped below.
        harvest(document_id, &response);
        compact(response)
    };

    let id = u64::from(std::process::id());
    let mut response = attempt(app, id, document_id);
    let code = response["code"].as_str().unwrap_or("");
    if response["ok"] == false && RETRYABLE.contains(&code) {
        if let Some(state) = fetch_state(app) {
            *document_id = Some(state);
        }
        response = attempt(app, id, document_id);
    }
    let status = map_status(response.clone(), created == 201);
    (if status == 200 { created } else { status }, response)
}

/// Drop the bulky `state` snapshot from settle responses; REST clients read
/// it explicitly from GET /state.
fn compact(response: Value) -> Value {
    if let Some(object) = response.as_object() {
        let trimmed: serde_json::Map<String, Value> = object
            .iter()
            .filter(|(key, _)| key.as_str() != "state")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        return Value::Object(trimmed);
    }
    response
}

fn fetch_state(app: &mut OpenCADStudio) -> Option<u64> {
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    state["document_id"].as_u64()
}

/// ok:true → 200 (the caller may upgrade to 201 for resource creation);
/// failures map to the closest HTTP status so ordinary clients can branch
/// on the status code, with the symbolic `code` preserved in the body.
pub(crate) fn map_status(response: Value, created: bool) -> u16 {
    if response["ok"] == true {
        return if created { 201 } else { 200 };
    }
    match response["code"].as_str().unwrap_or("") {
        "entity_absent" | "unknown_route" => 404,
        "stale_state" | "session_changed" | "document_not_active" | "gui_required" => 409,
        "busy" => 503,
        _ => 400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;

    fn request(method: &str, path: &str, body: &str) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            query: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn percent_decode_handles_escaped_and_plus_spaces() {
        assert_eq!(percent_decode("Walls%20and%2BFloors"), "Walls and+Floors");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("bad%2"), "bad%2");
    }

    /// One connection that never sends a request must not keep the listener
    /// from answering the next caller — the whole point of
    /// thread-per-connection (a wedged client used to freeze every read).
    #[test]
    fn listener_answers_while_one_connection_stalls() {
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || {
            listen(OpenCADStudio::new_for_test(), 0, listen_bound);
        });
        let mut port = 0;
        for _ in 0..50 {
            port = bound.load(Ordering::SeqCst);
            if port != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_ne!(port, 0, "listener never bound a port");

        // Stalls the thread that accepted it: no request line, ever.
        let _stalled = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET /api/v1/ready HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).expect("readiness answered");
        assert!(status_line.contains("200"), "{status_line}");
    }

    #[test]
    fn failure_codes_map_to_http_statuses() {
        assert_eq!(map_status(json!({"ok": true}), false), 200);
        assert_eq!(map_status(json!({"ok": true}), true), 201);
        assert_eq!(map_status(json!({"ok": false, "code": "entity_absent"}), false), 404);
        assert_eq!(map_status(json!({"ok": false, "code": "stale_state"}), false), 409);
        assert_eq!(map_status(json!({"ok": false, "code": "busy"}), false), 503);
        assert_eq!(map_status(json!({"ok": false, "code": "invalid_point"}), false), 400);
    }

    /// The generic `POST /api/v1/{op}` passthrough must reach the protocol-1
    /// envelope: ops like `user_select`/`operation`/`cancel` only exist
    /// there, and unstamped they died as "unknown op" on the legacy path.
    #[test]
    fn passthrough_reaches_envelope_only_ops() {
        let mut app = OpenCADStudio::new();
        app.automation_op(r#"{"op":"new"}"#);
        let mut document_id: Option<u64> = None;
        let mut counter: u64 = 0;
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/user_select", r#"{"request_id":"us-headless"}"#),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 409, "{body}");
        assert_eq!(body["code"], "gui_required", "{body}");

        // `operation` is a query: an unknown id answers `unknown_operation`,
        // not "unknown op".
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/operation", r#"{"request_id":"gone"}"#),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["code"], "unknown_operation", "{body}");
    }

    #[test]
    fn get_selection_reports_selected_entities_read_only() {
        let mut app = OpenCADStudio::new();
        let mut document_id: Option<u64> = None;
        let mut counter: u64 = 0;
        let (_, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/entities",
                r#"{"entities":[
                    {"type":"Line","start":[0,0],"end":[10,0]},
                    {"type":"Text","value":"PAGE-01","position":[1,1],"height":2.5}
                ]}"#,
            ),
            &mut document_id,
            &mut counter,
        );
        let handles = body["result"]["handles"].as_array().unwrap().clone();
        assert_eq!(handles.len(), 2);

        // The person "picks" the line and the text; the read mirrors the
        // selection order and matches the `query` field formats.
        app.automation_op(&format!(
            r#"{{"op":"select","handles":["{}","{}"]}}"#,
            handles[0].as_str().unwrap(),
            handles[1].as_str().unwrap()
        ));
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/get_selection", "{}"),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["status"], "completed");
        assert_eq!(body["result"]["count"], 2);
        assert_eq!(
            body["result"]["entities"][0]["handle"],
            handles[0].as_str().unwrap()
        );
        assert_eq!(body["result"]["entities"][0]["type"], "Line");
        assert_eq!(body["result"]["entities"][0]["text"], Value::Null);
        let sample = &body["result"]["entities"][1];
        assert_eq!(sample["type"], "Text");
        assert_eq!(sample["text"], "PAGE-01");
        assert_eq!(sample["value"], "PAGE-01");
        assert!(sample["bounds"].is_object(), "{sample}");
        // A second read is identical — the read is passive.
        let (_, again) = route(
            &mut app,
            &request("POST", "/api/v1/get_selection", "{}"),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(again, body);
    }

    #[test]
    fn rest_surface_round_trips_real_entities() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let mut document_id = None;
        let mut counter = 0;

        // Readiness exposes the version and session.
        let (status, body) = route(&mut app, &request("GET", "/api/v1/ready", ""), &mut document_id, &mut counter);
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true);
        assert!(body["session_id"].as_str().is_some());

        // Create: a REST resource POST returns 201 with handles.
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/entities",
                r#"{"entities":[
                    {"type":"Line","start":[0,0],"end":[10,0],"layer":"Walls"},
                    {"type":"Circle","center":[5,5],"radius":2},
                    {"type":"Text","value":"PAGE-01","position":[1,1],"height":2.5}
                ]}"#,
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        let handles = body["result"]["handles"].as_array().unwrap().clone();
        assert_eq!(handles.len(), 3);
        assert!(document_id.is_some(), "the server cached the document id");

        // Query through plain GET with query parameters.
        let mut get = request("GET", "/api/v1/entities", "");
        get.query = vec![("type".into(), "Line".into()), ("detail".into(), "full".into())];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 200);
        assert_eq!(body["entities"][0]["start"], json!([0.0, 0.0, 0.0]));

        // One entity by handle.
        let (status, body) = route(
            &mut app,
            &request("GET", &format!("/api/v1/entities/{}", handles[1].as_str().unwrap()), ""),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200);
        assert_eq!(body["count"], 1);
        assert_eq!(body["entities"][0]["type"], "Circle");

        // Transform through the dedicated endpoint.
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/entities/transform",
                &format!(
                    r#"{{"handles":["{}"],"action":"move","vector":[5,0]}}"#,
                    handles[1].as_str().unwrap()
                ),
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        let mut get = request("GET", "/api/v1/entities", "");
        get.query = vec![("type".into(), "Circle".into()), ("detail".into(), "full".into())];
        let (_, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(body["entities"][0]["center"], json!([10.0, 5.0, 0.0]));

        // XData write via PUT, read via GET.
        let handle = handles[0].as_str().unwrap().to_owned();
        let (status, body) = route(
            &mut app,
            &request(
                "PUT",
                &format!("/api/v1/entities/{handle}/xdata/SPM"),
                r#"[{"code":1000,"value":"PAGE-01"}]"#,
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        let mut get = request("GET", &format!("/api/v1/entities/{handle}/xdata"), "");
        get.query = vec![("app".into(), "SPM".into())];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["items"][0]["xdata"]["SPM"][0], "PAGE-01");

        // Unknown handles are 404 with the symbolic code preserved.
        let mut delete = request("DELETE", "/api/v1/entities", "");
        delete.query = vec![("handles".into(), "FF".into())];
        let (status, body) = route(&mut app, &delete, &mut document_id, &mut counter);
        assert_eq!(status, 404);
        assert_eq!(body["code"], "entity_absent");

        // The OpenAPI document is served and parses as JSON.
        let (status, body) = route(&mut app, &request("GET", "/api/v1/openapi", ""), &mut document_id, &mut counter);
        assert_eq!(status, 200);
        assert_eq!(body["openapi"], "3.0.3");

        // Unknown routes are 404, not panics.
        let (status, _) = route(&mut app, &request("GET", "/api/v1/nope", ""), &mut document_id, &mut counter);
        assert_eq!(status, 404);
    }

    #[test]
    fn rest_p1_document_surface_routes_end_to_end() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 1"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 20,0 5"}"#);
        let mut document_id = None;
        let mut counter = 0;

        // GET /state primes the document-id cache the mutations address.
        let (status, body) = route(&mut app, &request("GET", "/api/v1/state", ""), &mut document_id, &mut counter);
        assert_eq!(status, 200);
        let doc = document_id.expect("state caches the document id");
        assert_eq!(body["document_id"].as_u64(), Some(doc));

        // Where filters travel as the JSON-encoded `where` query parameter.
        let mut get = request("GET", "/api/v1/entities", "");
        get.query = vec![
            ("type".into(), "Circle".into()),
            ("detail".into(), "geometry".into()),
            ("where".into(), r#"[{"path":"/radius","op":"gt","value":2}]"#.into()),
        ];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["count"], 1);
        assert_eq!(body["entities"][0]["radius"], 5.0);

        // A malformed filter is refused before touching the dispatcher.
        let mut get = request("GET", "/api/v1/entities", "");
        get.query = vec![("where".into(), "not-json".into())];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 400);
        assert_eq!(body["code"], "invalid_where");

        // Sysvars read through GET with a `names` CSV, write through POST.
        let mut get = request("GET", "/api/v1/sysvars", "");
        get.query = vec![("names".into(), "ltscale,mirrtext".into())];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 200, "{body}");
        assert!(body["result"]["values"]["ltscale"].is_number());
        assert_eq!(body["result"]["values"]["mirrtext"], 0);

        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/sysvars", r#"{"set":{"ltscale":2.5}}"#),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        let mut get = request("GET", "/api/v1/sysvars", "");
        get.query = vec![("names".into(), "ltscale".into())];
        let (_, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(body["result"]["values"]["ltscale"], 2.5);

        // A layout is created (201) and its page setup patched via PUT.
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/layouts", r#"{"name":"PLAN"}"#),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        assert!(body["result"]["layouts"].as_array().unwrap().iter().any(|n| n == "PLAN"));
        let (status, body) = route(
            &mut app,
            &request(
                "PUT",
                "/api/v1/layouts/PLAN/page-setup",
                r#"{"paper":"ISO_A4_(210.00_x_297.00_MM)","orientation":"landscape","fit":true,"center":true}"#,
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["layout"], "PLAN");
        assert!(body["result"]["paper"].as_str().unwrap().contains("ISO_A4"));
        assert_eq!(body["result"]["scale"]["fit"], true);

        // Groups and selection sets address the drawing's entities (the
        // created layout also owns a sheet viewport, so filter to circles).
        let q = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#);
        let joined = q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| format!("\"{}\"", e["handle"].as_str().unwrap()))
            .collect::<Vec<_>>()
            .join(",");
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/groups", &format!(r#"{{"name":"FRAME","handles":[{joined}]}}"#)),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/selection-sets",
                &format!(r#"{{"name":"rest-set","handles":[{joined}]}}"#),
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        app.automation_op(r#"{"op":"select","clear":true}"#);
        // Default GET recalls without touching the selection; ?select=true
        // additionally makes the set the current selection.
        let (status, body) = route(
            &mut app,
            &request("GET", "/api/v1/selection-sets/rest-set", ""),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["handles"].as_array().unwrap().len(), 2);
        assert_eq!(body["result"]["selected"], 0);
        let mut get = request("GET", "/api/v1/selection-sets/rest-set", "");
        get.query = vec![("select".into(), "true".into())];
        let (status, body) = route(&mut app, &get, &mut document_id, &mut counter);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["selected"], 2);

        // Copy between documents: the body names the target document.
        let target = app.push_test_document();
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/entities/copy-to",
                &format!(r#"{{"handles":[{joined}],"document_id":{target}}}"#),
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["result"]["count"], 2);

        // Blocks: define → replace → delete; the drawing carries a stable
        // file identity GUID.
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/blocks",
                &format!(r#"{{"name":"MARK","base":[0,0,0],"handles":[{joined}]}}"#),
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        // Replace re-blocks fresh content under the same name — the define
        // consumed the original circles, SPM-style re-import draws new
        // content and wraps it again.
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/entities",
                r#"{"entities":[{"type":"Point","location":[9,9]}]}"#,
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        let fresh = body["result"]["handles"][0].as_str().unwrap().to_owned();
        let (status, body) = route(
            &mut app,
            &request(
                "POST",
                "/api/v1/blocks",
                &format!(r#"{{"name":"MARK","base":[0,0,0],"handles":["{fresh}"],"replace":true}}"#),
            ),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        let (status, body) = route(
            &mut app,
            &request("DELETE", "/api/v1/blocks/MARK", ""),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        assert!(body["result"]["erased"].as_u64().unwrap() >= 1);
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/file-identity", "{}"),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["identity"].as_str().unwrap().len(), 36);

        // An empty POST /documents opens a fresh
        // untitled document in its own tab — a copy-to target.
        let (status, body) = route(
            &mut app,
            &request("POST", "/api/v1/documents", "{}"),
            &mut document_id,
            &mut counter,
        );
        assert_eq!(status, 201, "{body}");
        let fresh = document_id.expect("fresh document cached");
        assert_ne!(fresh, doc, "a second document opened");

        // Close with discard: the dirty document is dropped, 200 returned.
        let mut delete = request("DELETE", &format!("/api/v1/documents/{doc}"), "");
        delete.query = vec![("discard".into(), "true".into())];
        let (status, body) = route(&mut app, &delete, &mut document_id, &mut counter);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["closed"], true);
    }
}
