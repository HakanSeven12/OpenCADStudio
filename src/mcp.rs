//! Client-neutral MCP adapter for the live desktop editor.
//!
//! `OpenCADStudio --mcp` speaks MCP over stdio. All drawing work is forwarded
//! to the authenticated GUI control bridge; this module contains no geometry.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime},
};
#[cfg(any(windows, target_os = "macos"))]
use std::collections::HashSet;

const PROTOCOL_VERSION: &str = "2025-11-25";
const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
/// Pinned content digest of [`tool_definitions`].
///
/// The `tool_schema_digest_is_pinned` test fails whenever the MCP tool surface
/// changes, so schema drift (new params, renamed tools) is always a conscious,
/// reviewed edit — and clients can detect a stale bridge by comparing digests.
#[cfg(test)]
const TOOL_SCHEMA_DIGEST: &str = "2ec01401ca96bc0574a9be41958d24dbd9c27a497dce80064c8afcca5258256e";
const MAX_REQUEST: usize = 1_048_576;
const MAX_RESPONSE: u64 = 16 * 1024 * 1024;
const CACHE_TTL_MS: u64 = 3_600_000;
/// Freshness for resources/*, which vary as sessions open and snapshots
/// land. Tools and discovery are static by comparison, so they keep the hour.
const RESOURCE_TTL_MS: u64 = 60_000;
const TASK_TTL_MS: u64 = 3_600_000;
pub(crate) const INSTRUCTIONS: &str = "Call ocs_sessions, then pass its session_id as ocs_session_id to ocs_read, ocs_execute and ocs_capture. Read capabilities to discover the complete CAD automation surface. Call record_schema to discover every record type, property path, JSON type, enum, unit, constraint and write rule before editing unfamiliar data. Use records to inspect every serializable entity, object, table, header and document record; filter with RFC 6901 JSON Pointer paths. Use set_properties for atomic, type-checked record edits and preserve document_id, revision and request_id. Use commands with parameters.name for a command manifest. Use batch when several steps are known, and request changed_entities when resulting geometry is needed. For interactive work, call start and follow state.command.accepts, options and input_example. To have the person at the screen pick entities for you, call user_select and keep polling until it completes; running means they are still picking. A run.cmd contains the command name followed by prompt answers separated by spaces; points use x,y or x,y,z. After a timeout, query the existing operation and never replay a mutation with a new request_id. waiting_input and running are not completion. Let OCS and its geometry kernel calculate geometry; use query near, contains_point and intersections for exact relationships. ARCHITECTURAL VECTORIZATION DIRECTIVE: When converting or vectorizing a floorplan from an image or sketch: 1. Attach reference images as Xref underlays via embed_image on layer _XREF and lock it. 2. NEVER draw loose lines or arcs for doors or windows; always query records (collection: 'block_records') and insert Block References (type: 'INSERT') on A-DOOR and A-GLAZ. If a block is missing, draft standard geometry at origin (0,0) and register it with block_define before inserting. 3. Categorize layers cleanly: A-WALL-EXTR, A-WALL-INTR, A-WALL-HATCH, A-DOOR, A-GLAZ, A-ANNO-TEXT, A-ANNO-DIMS. 4. Always verify drafted geometry using ocs_capture with annotate: true (Set-of-Marks entity IDs) and diff: true (visual dirty streaming). ocs_capture operates quietly in background and overlapped window states without stealing user focus. Viewports and captured snapshots are available as MCP resources under cad://session/{session_id}/viewport.png and cad://session/{session_id}/snapshot/{hash}.png; ocs_capture accepts delivery: 'resource' to avoid large inline base64 payloads, supplies standardized spatial grounding in _spatial, supports diff: true for streaming dirty visual regions, and provides multiscale DeepZoom pyramidal tiling via tile: {level, x, y} or cad://session/{session_id}/pyramid/manifest.json and cad://session/{session_id}/tile/{level}/{x}/{y}.png. Before delivery call audit with the intended target_format and target_version; use save_verified with an explicit absolute path to save, reopen, hash and compare the semantic manifest. When you first connect, announce the build you are working with to the user from the `bridge` object on ocs_sessions states and hello/capabilities responses (OpenCADStudio version, build_rev, tool_schema digest); repeat the announcement if a later handshake reports a different build.";
const READ_OPS: &[&str] = &[
    "state",
    "hello",
    "query",
    "records",
    "record_schema",
    "capabilities",
    "tools",
    "entities",
    "layers",
    "header",
    "properties",
    "measure",
    "snap",
    "grips",
    "history",
    "commands",
    "events",
    "operation",
    "xdata_get",
    "audit",
    "text_search",
    "text_audit",
    "capture",
];
const EXECUTE_OPS: &[&str] = &[
    "new",
    "open",
    "activate",
    "switch_document",
    "run",
    "start",
    "input",
    "cancel",
    "undo",
    "redo",
    "select",
    "property",
    "grip_drag",
    "click",
    "set_properties",
    "action",
    "embed_image",
    "wblock",
    "plot",
    "entities_create",
    "entities_delete",
    "entities_transform",
    "text_replace",
    "block_define",
    "block_delete",
    "xdata_set",
    "view_focus",
    "entities_copy_to",
    "group_create",
    "selection_set_save",
    "selection_set_load",
    "user_select",
    "getpoint",
    "close",
    "sysvar",
    "layout_create",
    "page_setup_set",
    "file_identity",
    "save",
    "save_verified",
    "stop",
    "batch",
];
const BATCH_STEP_OPS: &[&str] = &[
    "new",
    "open",
    "activate",
    "switch_document",
    "run",
    "start",
    "input",
    "cancel",
    "undo",
    "redo",
    "select",
    "property",
    "grip_drag",
    "click",
    "set_properties",
    "action",
    "embed_image",
    "wblock",
    "plot",
    "entities_create",
    "entities_delete",
    "entities_transform",
    "text_replace",
    "block_define",
    "block_delete",
    "xdata_set",
    "view_focus",
    "entities_copy_to",
    "group_create",
    "selection_set_save",
    "selection_set_load",
    "user_select",
    "getpoint",
    "close",
    "sysvar",
    "layout_create",
    "page_setup_set",
    "file_identity",
    "save",
    "stop",
];
const MAX_BATCH_STEPS: usize = 64;

#[derive(Clone, Deserialize)]
struct Descriptor {
    session_id: String,
    port: u16,
    token: String,
    /// GUI process id when the descriptor writer knows it. Used to skip dead
    /// sessions without a (possibly hanging) TCP probe; absent on legacy
    /// files, which keep the old probe path.
    #[serde(default)]
    pid: Option<u64>,
}

/// True when `pid` currently exists. Linux checks /proc (no spawn, no new
/// dependencies); other platforms go through one shared snapshot per
/// [`descriptors`] pass (see below) instead of per-pid spawns.
#[cfg(target_os = "linux")]
fn pid_alive(pid: u64) -> bool {
    // Fail open where /proc is unavailable (containers, chroots): without it
    // every pid would read "dead" and live sessions would be dropped en masse.
    Path::new("/proc").exists() && Path::new(&format!("/proc/{pid}")).exists()
}

/// One snapshot of all live PIDs. `None` on any failure so callers fail open
/// (treat every descriptor as alive) instead of dropping live sessions
/// because enumeration broke.
#[cfg(windows)]
fn live_pids_snapshot() -> Option<HashSet<u64>> {
    let output = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut set = HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split("\",\"");
        fields.next()?;
        if let Some(pid) = fields
            .next()
            .and_then(|field| field.trim_matches('"').parse::<u64>().ok())
        {
            set.insert(pid);
        }
    }
    (!set.is_empty()).then_some(set)
}

/// macOS has no /proc: same one-snapshot approach as Windows via `ps`.
#[cfg(target_os = "macos")]
fn live_pids_snapshot() -> Option<HashSet<u64>> {
    let output = Command::new("ps").args(["-Ao", "pid="]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut set = HashSet::new();
    for token in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Ok(pid) = token.parse::<u64>() {
            set.insert(pid);
        }
    }
    (!set.is_empty()).then_some(set)
}

/// Process snapshots cost a spawn (~100ms), but `ocs_sessions` can run every
/// agent turn — cache each snapshot briefly. Failures cache as `None`, which
/// keeps the fail-open behavior below.
#[cfg(any(windows, target_os = "macos"))]
const PID_SNAPSHOT_TTL: Duration = Duration::from_secs(10);

#[cfg(any(windows, target_os = "macos"))]
std::thread_local! {
    static PID_SNAPSHOT_CACHE: std::cell::RefCell<(Instant, Option<HashSet<u64>>)> =
        std::cell::RefCell::new((Instant::now() - PID_SNAPSHOT_TTL - Duration::from_secs(1), None));
}

#[cfg(any(windows, target_os = "macos"))]
fn live_pids_cached() -> Option<HashSet<u64>> {
    PID_SNAPSHOT_CACHE.with(|cache| {
        {
            let (stamp, cached) = &*cache.borrow();
            if stamp.elapsed() < PID_SNAPSHOT_TTL {
                return cached.clone();
            }
        }
        let fresh = live_pids_snapshot();
        *cache.borrow_mut() = (Instant::now(), fresh.clone());
        fresh
    })
}

/// Drop the cached PID snapshot so the next discovery pass enumerates
/// fresh. Called after spawning a GUI: without this, the pre-spawn snapshot
/// does not contain the newborn pid, and the corpse cleanup below would
/// delete its just-written descriptor as "dead on arrival".
#[cfg(any(windows, target_os = "macos"))]
fn live_pids_invalidate() {
    PID_SNAPSHOT_CACHE.with(|cache| {
        *cache.borrow_mut() = (
            Instant::now() - PID_SNAPSHOT_TTL - Duration::from_secs(1),
            None,
        );
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
fn live_pids_invalidate() {}

/// Descriptors younger than this are never deleted, even when their pid is
/// missing from the snapshot: the snapshot may predate the spawn, and slow
/// starters write late. Deletion stays for genuinely old corpses.
const DESCRIPTOR_GRACE: Duration = Duration::from_secs(120);

/// Whether a pid missing from the snapshot authorizes deletion. Pure
/// predicate so the staleness rule is unit-testable: only old files go.
fn stale_snapshot_may_delete(missing_from_snapshot: bool, file_age: Duration) -> bool {
    missing_from_snapshot && file_age >= DESCRIPTOR_GRACE
}

struct GuiClient {
    descriptor: Descriptor,
    state: Value,
    client_id: String,
    batches: VecDeque<BatchExecution>,
    /// MCP request id (as string) -> GUI request id for ops still pending
    /// server-side. Lets an idle-arriving `notifications/cancelled` find
    /// the GUI op to dismiss. Entries leave on terminal responses and
    /// dismissals; abandoned entries mirror GUI sessions that outlive
    /// interest (pre-existing behavior, two small strings each).
    inflight: HashMap<String, String>,
    /// GUI request ids dismissed by cancel/timeout: late completions and
    /// re-polls answer `cancelled` without touching the GUI. Capped.
    dismissed: VecDeque<String>,
}

/// Reader-thread inbox for stdin lines. The reader never writes: the main
/// loop (and, during waits, the wait loop) is the only consumer, so stdout
/// stays single-writer. Non-cancel lines met during a wait are stowed in
/// `backlog` and handled in order once the wait ends.
struct CancelPump {
    rx: Receiver<Result<String, String>>,
    backlog: VecDeque<String>,
}

enum PumpEvent {
    Cancelled,
    Quiet,
}

impl CancelPump {
    fn next(&mut self) -> Option<String> {
        if let Some(line) = self.backlog.pop_front() {
            return Some(line);
        }
        match self.rx.recv() {
            Ok(Ok(line)) => Some(line),
            Ok(Err(error)) => {
                eprintln!("MCP input error: {error}");
                None
            }
            Err(_) => None,
        }
    }

    /// Drain newly arrived lines for up to `quantum`: consume a cancel naming
    /// `mkey`, stow everything else in order. Never blocks past the quantum.
    fn wait_line(&mut self, quantum: Duration, mkey: Option<&str>) -> PumpEvent {
        let deadline = Instant::now() + quantum;
        loop {
            match self.rx.try_recv() {
                Ok(Ok(line)) => {
                    if mkey.is_some_and(|key| is_cancel_for(&line, key)) {
                        return PumpEvent::Cancelled;
                    }
                    self.backlog.push_back(line);
                }
                // Reader gone or input broken: end the wait quietly; the
                // main loop observes the closed channel right after.
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => return PumpEvent::Quiet,
                Err(TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return PumpEvent::Quiet;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

/// MCP request-id key for cancel matching. String and number ids stay
/// distinct (`7` vs `"7"`): both sides stringify the same JSON value.
fn mcp_key(id: &Value) -> String {
    id.to_string()
}

fn is_cancel_for(line: &str, mkey: &str) -> bool {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    if message.get("method").and_then(Value::as_str) != Some("notifications/cancelled") {
        return false;
    }
    message
        .get("params")
        .and_then(|params| params.get("requestId"))
        .is_some_and(|id| id.to_string() == mkey)
}

fn wait_deadline(op: &str, wait_seconds: f64) -> Duration {
    let capped = if matches!(op, "user_select" | "getpoint") {
        wait_seconds.clamp(0.0, INTERACTIVE_MAX_WAIT.as_secs_f64())
    } else {
        wait_seconds.clamp(0.0, 60.0)
    };
    Duration::from_secs_f64(capped)
}

/// Only a wait that actually reaches the interactive ceiling dismisses the
/// prompt. Shorter waits return `running` so the agent can re-poll; without
/// this distinction the first short poll would kill every interactive op.
fn interactive_timeout(wait_seconds: f64) -> bool {
    wait_seconds >= INTERACTIVE_MAX_WAIT.as_secs_f64()
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
    /// Cooperative cancel acknowledged: polls report `cancelled` and late
    /// completions are discarded (checked before result/error).
    cancelled: bool,
}

struct StoredTask {
    expires_at: Instant,
    task: McpTask,
}

#[derive(Default)]
struct TaskStore {
    tasks: VecDeque<StoredTask>,
}

impl TaskStore {
    fn insert(&mut self, task: McpTask) {
        self.insert_at(task, Instant::now());
    }

    fn insert_at(&mut self, task: McpTask, now: Instant) {
        self.evict_expired_at(now);
        self.tasks.push_back(StoredTask {
            expires_at: now + Duration::from_millis(TASK_TTL_MS),
            task,
        });
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut McpTask> {
        self.get_mut_at(id, Instant::now())
    }

    fn get_mut_at(&mut self, id: &str, now: Instant) -> Option<&mut McpTask> {
        self.evict_expired_at(now);
        self.tasks
            .iter_mut()
            .find(|stored| stored.task.id == id)
            .map(|stored| &mut stored.task)
    }

    fn evict_expired_at(&mut self, now: Instant) {
        self.tasks.retain(|stored| stored.expires_at > now);
    }
}

#[derive(Clone, Debug)]
struct CachedSnapshot {
    session_id: String,
    hash: String,
    created_at: String,
    mime_type: String,
    data_base64: String,
    bytes_len: usize,
    metadata: Value,
}

#[derive(Default)]
struct ResourceStore {
    snapshots: HashMap<String, CachedSnapshot>,
    order: VecDeque<String>,
    latest: HashMap<String, String>,
    last_captures: HashMap<String, (image::RgbaImage, String)>,
    tiles: HashMap<String, CachedSnapshot>,
    tile_order: VecDeque<String>,
    pyramid_manifests: HashMap<String, Value>,
}

const MAX_STORED_SNAPSHOTS: usize = 32;
const MAX_STORED_TILES: usize = 64;

impl ResourceStore {
    fn insert(
        &mut self,
        session_id: &str,
        hash: &str,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
    ) -> String {
        let uri = format!("cad://session/{session_id}/snapshot/{hash}.png");
        let now = iso8601_now();
        let snapshot = CachedSnapshot {
            session_id: session_id.to_string(),
            hash: hash.to_string(),
            created_at: now,
            mime_type: "image/png".to_string(),
            data_base64,
            bytes_len,
            metadata,
        };

        if !self.snapshots.contains_key(&uri) {
            if self.order.len() >= MAX_STORED_SNAPSHOTS {
                if let Some(oldest) = self.order.pop_front() {
                    self.snapshots.remove(&oldest);
                }
            }
            self.order.push_back(uri.clone());
        }
        self.snapshots.insert(uri.clone(), snapshot);
        self.latest.insert(session_id.to_string(), uri.clone());
        uri
    }

    fn insert_tile(
        &mut self,
        session_id: &str,
        level: u32,
        x: u32,
        y: u32,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
    ) -> String {
        let uri = format!("cad://session/{session_id}/tile/{level}/{x}/{y}.png");
        let now = iso8601_now();
        let snapshot = CachedSnapshot {
            session_id: session_id.to_string(),
            hash: format!("tile-{level}-{x}-{y}"),
            created_at: now,
            mime_type: "image/png".to_string(),
            data_base64,
            bytes_len,
            metadata,
        };

        if !self.tiles.contains_key(&uri) {
            if self.tile_order.len() >= MAX_STORED_TILES {
                if let Some(oldest) = self.tile_order.pop_front() {
                    self.tiles.remove(&oldest);
                }
            }
            self.tile_order.push_back(uri.clone());
        }
        self.tiles.insert(uri.clone(), snapshot);
        uri
    }

    fn insert_pyramid_manifest(&mut self, session_id: &str, manifest: Value) -> String {
        let uri = format!("cad://session/{session_id}/pyramid/manifest.json");
        self.pyramid_manifests.insert(session_id.to_string(), manifest);
        uri
    }

    fn get_by_uri(&self, uri: &str) -> Option<&CachedSnapshot> {
        if uri.ends_with("/snapshot/latest.png") {
            if let Some(session_id) = uri
                .strip_prefix("cad://session/")
                .and_then(|s| s.strip_suffix("/snapshot/latest.png"))
            {
                if let Some(target_uri) = self.latest.get(session_id) {
                    return self.snapshots.get(target_uri);
                }
            }
            return None;
        }
        if uri.contains("/tile/") {
            return self.tiles.get(uri);
        }
        self.snapshots.get(uri)
    }

    fn list_resources(&self, active_session_ids: &[String]) -> Vec<Value> {
        let mut list = Vec::new();
        for session_id in active_session_ids {
            list.push(json!({
                "uri": format!("cad://session/{session_id}/viewport.png"),
                "name": format!("Active Viewport ({session_id})"),
                "description": format!("Live render of the active drawing viewport for session {session_id}."),
                "mimeType": "image/png"
            }));
            list.push(json!({
                "uri": format!("cad://session/{session_id}/pyramid/manifest.json"),
                "name": format!("Pyramid Manifest ({session_id})"),
                "description": format!("Multiscale DeepZoom pyramidal tiling manifest for session {session_id}."),
                "mimeType": "application/json"
            }));
            if let Some(latest_uri) = self.latest.get(session_id) {
                if let Some(snap) = self.snapshots.get(latest_uri) {
                    list.push(json!({
                        "uri": format!("cad://session/{session_id}/snapshot/latest.png"),
                        "name": format!("Latest Capture ({session_id})"),
                        "description": format!("Most recent viewport capture ({} bytes, captured at {})", snap.bytes_len, snap.created_at),
                        "mimeType": "image/png"
                    }));
                }
            }
        }
        for uri in &self.order {
            if let Some(snap) = self.snapshots.get(uri) {
                list.push(json!({
                    "uri": uri,
                    "name": format!("Snapshot {}", &snap.hash[..snap.hash.len().min(8)]),
                    "description": format!("Captured frame for session {} ({} bytes, captured at {})", snap.session_id, snap.bytes_len, snap.created_at),
                    "mimeType": "image/png"
                }));
            }
        }
        for uri in &self.tile_order {
            if let Some(tile) = self.tiles.get(uri) {
                list.push(json!({
                    "uri": uri,
                    "name": format!("Tile {}", tile.hash),
                    "description": format!("Pyramidal tile for session {} ({} bytes)", tile.session_id, tile.bytes_len),
                    "mimeType": "image/png"
                }));
            }
        }
        list
    }
}

/// (mtime, byte length) of this executable. A rebuild changes at least one of
/// the two, so a mismatch means this bridge serves a stale tool schema.
fn exe_fingerprint() -> Option<(SystemTime, u64)> {
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// True when both fingerprints are known and differ (unknown counts as same
/// to avoid false-positive exits on locked-down filesystems).
fn exe_superseded(baseline: &Option<(SystemTime, u64)>) -> bool {
    match (baseline, &exe_fingerprint()) {
        (Some(before), Some(now)) => before != now,
        _ => false,
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

/// Tool-call failure split for error routing: Tool-domain failures carry
/// model-actionable guidance and stay `isError` results (SEP-1303); bridge
/// and GUI infrastructure failures (nothing the model can fix) become
/// JSON-RPC errors. `From<String>` defaults to Tool so existing sites keep
/// working unchanged; infrastructure sites opt in explicitly.
#[derive(Debug)]
enum CallError {
    Tool(String),
    Infra(String),
    /// Client cancelled the in-flight request: send nothing back.
    Cancelled,
}

/// Interactive picks may legitimately take a human minutes; everything else
/// keeps the 60 s clamp. This bounds a single call; agents re-poll with
/// small waits by design.
const INTERACTIVE_MAX_WAIT: Duration = Duration::from_secs(600);

impl From<String> for CallError {
    fn from(message: String) -> Self {
        CallError::Tool(message)
    }
}

impl From<&str> for CallError {
    fn from(message: &str) -> Self {
        CallError::Tool(message.to_string())
    }
}

// Collapses back to String where the caller maps everything to one code
// anyway (resources/read not-found codes); the routing decision is made
// by the caller, not the classification.
impl From<CallError> for String {
    fn from(error: CallError) -> Self {
        error.message().to_string()
    }
}

impl CallError {
    fn message(&self) -> &str {
        match self {
            CallError::Tool(message) | CallError::Infra(message) => message,
            CallError::Cancelled => "cancelled by client",
        }
    }

    fn infra(message: impl ToString) -> Self {
        CallError::Infra(message.to_string())
    }
}

fn exchange(descriptor: &Descriptor, request: Value, timeout: Duration) -> Result<Value, CallError> {
    let mut object = request
        .as_object()
        .cloned()
        .ok_or_else(|| CallError::infra("GUI request must be an object"))?;
    object.insert("token".into(), Value::String(descriptor.token.clone()));
    object.insert(
        "session_id".into(),
        Value::String(descriptor.session_id.clone()),
    );
    object.insert("protocol".into(), Value::from(1));
    let mut wire = serde_json::to_vec(&Value::Object(object))
        .map_err(CallError::infra)?;
    wire.push(b'\n');
    if wire.len() > MAX_REQUEST {
        // Caller-caused (batch too big): model-actionable, stays Tool.
        return Err("Request exceeds 1 MiB".to_string().into());
    }

    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], descriptor.port)),
        timeout,
    )
    .map_err(CallError::infra)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(CallError::infra)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(CallError::infra)?;
    stream.write_all(&wire).map_err(CallError::infra)?;

    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE + 1)
        .read_to_string(&mut response)
        .map_err(CallError::infra)?;
    if response.is_empty() || response.len() as u64 > MAX_RESPONSE {
        return Err(CallError::infra(
            "No valid OCS response; query request_id before retrying a mutation",
        ));
    }
    serde_json::from_str(response.trim_end()).map_err(CallError::infra)
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

    #[cfg(any(windows, target_os = "macos"))]
    let live_pids = live_pids_cached();
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
        // A dead GUI leaves its descriptor file behind; its TCP port may hang
        // instead of refusing, which used to stall discovery past client
        // timeouts. Skip (and delete) pid-verified corpses before probing.
        // Deletion waits out DESCRIPTOR_GRACE: the PID snapshot may predate
        // a spawn, and a newborn pid missing from it must never read "dead".
        // Unknown file age counts as newborn (fail open, probe instead).
        let file_age = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::ZERO);
        let pid_dead = match descriptor.pid {
            Some(pid) => {
                #[cfg(target_os = "linux")]
                {
                    !pid_alive(pid)
                }
                #[cfg(any(windows, target_os = "macos"))]
                {
                    match &live_pids {
                        None => false,
                        Some(set) => {
                            stale_snapshot_may_delete(!set.contains(&pid), file_age)
                        }
                    }
                }
                #[cfg(not(any(
                    target_os = "linux",
                    windows,
                    target_os = "macos"
                )))]
                {
                    let _ = pid;
                    false
                }
            }
            None => false,
        };
        if pid_dead {
            let _ = std::fs::remove_file(&path);
            continue;
        }
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

/// A launch already in flight keeps its claim here so concurrent
/// `ocs_sessions(launch_if_none: true)` calls wait for the same GUI instead
/// of spawning one window each. Content is `{"pid":..,"started":unix_secs}`.
const STARTUP_LOCK_FILE: &str = "starting.lock";
/// Claims older than this are abandoned (crashed starter, previous boot).
const STARTUP_LOCK_TTL_SECS: u64 = 60;

fn startup_lock_path_for(directory: &Path) -> PathBuf {
    directory.join(STARTUP_LOCK_FILE)
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_startup_lock_at(directory: &Path, pid: u64, started: u64) -> Result<(), String> {
    let text = serde_json::to_string(&json!({"pid": pid, "started": started}))
        .map_err(|error| error.to_string())?;
    std::fs::write(startup_lock_path_for(directory), text).map_err(|error| error.to_string())
}

fn read_startup_lock_at(directory: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(startup_lock_path_for(directory)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some((
        value.get("pid")?.as_u64()?,
        value.get("started")?.as_u64()?,
    ))
}

fn claim_pid_alive(pid: u64) -> bool {
    #[cfg(target_os = "linux")]
    {
        pid_alive(pid)
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        live_pids_cached().is_none_or(|set| set.contains(&pid))
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        let _ = pid;
        true
    }
}

/// True when this caller owns the startup claim and may spawn the GUI.
/// False means a live starter holds a fresh claim: wait for its descriptor
/// instead of spawning another window. Stale claims and claims from dead
/// pids are reclaimed. Fail open (claim granted) when the directory is
/// unusable so a broken lock can never wedge launching.
fn try_claim_startup_lock(directory: &Path, pid: u64) -> bool {
    let now = now_unix_secs();
    if let Some((owner, started)) = read_startup_lock_at(directory) {
        let fresh = now.saturating_sub(started) < STARTUP_LOCK_TTL_SECS;
        if fresh && claim_pid_alive(owner) {
            return false;
        }
    }
    write_startup_lock_at(directory, pid, now).is_ok()
}

fn release_startup_lock(directory: &Path) {
    let _ = std::fs::remove_file(startup_lock_path_for(directory));
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

fn automation_dir() -> Result<PathBuf, String> {
    let base = crate::config::config_dir()
        .ok_or_else(|| "No user configuration directory".to_string())?;
    Ok(base.join("automation"))
}

fn sessions(launch_if_none: bool) -> Result<Vec<Value>, String> {
    let mut available = descriptors()?;
    if available.is_empty() && launch_if_none {
        let directory = automation_dir()?;
        // A concurrent caller may already be starting the GUI: wait for its
        // descriptor instead of spawning another window.
        let claimed = try_claim_startup_lock(&directory, std::process::id() as u64);
        let mut child = if claimed {
            let child = start_gui()?;
            // The pre-spawn PID snapshot cannot contain the newborn GUI:
            // drop it so the next discovery pass enumerates fresh instead
            // of deleting the just-written descriptor as a corpse.
            live_pids_invalidate();
            Some(child)
        } else {
            None
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(child) = child.as_mut() {
                if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                    if claimed {
                        release_startup_lock(&directory);
                    }
                    return Err(format!("OpenCADStudio exited while starting ({status})"));
                }
            }
            thread::sleep(Duration::from_millis(200));
            available = descriptors()?;
            if !available.is_empty() {
                break;
            }
        }
        if claimed {
            release_startup_lock(&directory);
        }
        if available.is_empty() {
            return Err("OpenCADStudio is still starting; call ocs_sessions again".into());
        }
    }
    Ok(available
        .into_iter()
        .map(|(_, state)| with_bridge_identity(state))
        .collect())
}

fn insert_default(object: &mut Map<String, Value>, key: &str, value: Value) {
    if !object.contains_key(key) {
        object.insert(key.into(), value);
    }
}

impl GuiClient {
    fn connect(session_id: &str) -> Result<Self, CallError> {
        let mut matching: Vec<_> = descriptors()
            .map_err(CallError::Infra)?
            .into_iter()
            .filter(|(descriptor, _)| descriptor.session_id == session_id)
            .collect();
        if matching.len() != 1 {
            return Err(CallError::infra(format!(
                "Choose session_id from ocs_sessions; found {} matching sessions",
                matching.len()
            )));
        }
        let (descriptor, state) = matching.remove(0);
        Ok(Self {
            descriptor,
            state,
            client_id: random_id().map_err(CallError::Infra)?,
            batches: VecDeque::new(),
            inflight: HashMap::new(),
            dismissed: VecDeque::new(),
        })
    }

    /// Best-effort dismiss of a GUI-pending interactive op: the GUI resolves
    /// a pending `user_select`/`getpoint` as cancelled on `op == "cancel"`
    /// (see `app::control`), and late completions are dropped bridge-side
    /// via `dismissed`. Failures are ignored: dismissal races a GUI that may
    /// already be gone, and the outcome (Cancelled/timeout) stands either way.
    fn dismiss(&mut self, gui_id: Option<&str>) {
        let Some(gui_id) = gui_id else { return };
        let cancel = json!({
            "op": "cancel",
            "request_id": random_id().unwrap_or_else(|_| format!("cancel-{}", iso8601_now())),
            "client_id": self.client_id.clone(),
            "document_id": self.state["document_id"].clone(),
            "revision": self.state["revision"].clone(),
        });
        let _ = exchange(&self.descriptor, cancel, Duration::from_secs(5));
        if !self.dismissed.iter().any(|id| id == gui_id) {
            if self.dismissed.len() >= 128 {
                self.dismissed.pop_front();
            }
            self.dismissed.push_back(gui_id.to_string());
        }
    }

    fn request(
        &mut self,
        request: Value,
        wait_seconds: f64,
        pump: &mut CancelPump,
        mcp_id: Option<&Value>,
    ) -> Result<Value, CallError> {
        let mut object = request
            .as_object()
            .cloned()
            .ok_or_else(|| "request must be an object".to_string())?;
        let op = object
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "request must contain op".to_string())?
            .to_string();
        // The GUI requires request_id on every non-query op, capture
        // included: without one the GUI rejects the request outright, so
        // the bridge always mints one (a client-supplied id wins).
        if !READ_OPS.contains(&op.as_str()) || op == "capture" {
            insert_default(&mut object, "request_id", Value::String(random_id()?));
        }
        // Capture reads the active tab, so it needs the document the
        // GUI requires on every non-query op (revision stays absent:
        // the GUI only checks revisions callers supply).
        if op == "capture" {
            insert_default(
                &mut object,
                "document_id",
                self.state["document_id"].clone(),
            );
        }
        if !READ_OPS.contains(&op.as_str()) {
            insert_default(
                &mut object,
                "client_id",
                Value::String(self.client_id.clone()),
            );
            if op != "entities_copy_to" {
                insert_default(
                    &mut object,
                    "document_id",
                    self.state["document_id"].clone(),
                );
            }
            insert_default(&mut object, "revision", self.state["revision"].clone());
            if ["input", "property", "run", "action", "save", "save_verified", "undo", "redo"].contains(&op.as_str())
            {
                insert_default(&mut object, "selection", self.state["selection"].clone());
            }
        }

        let request_id = object.get("request_id").cloned();
        // Re-polls for dismissed interactions answer `cancelled` without
        // touching the GUI: the prompt is already gone.
        if op == "operation" {
            if let Some(polled) = request_id.as_ref().and_then(Value::as_str) {
                if self.dismissed.iter().any(|id| id == polled) {
                    return Ok(json!({"ok":false,"status":"cancelled","request_id":polled}));
                }
            }
        }
        let mut response = exchange(
            &self.descriptor,
            Value::Object(object),
            Duration::from_secs(15),
        )?;
        let gui_id = request_id.as_ref().and_then(Value::as_str).map(str::to_string);
        let mkey = mcp_id.map(mcp_key);
        // Track the GUI op while it stays pending so an idle-arriving
        // cancel can find and dismiss it. Terminal outcomes remove the
        // entry; deadline exits keep it (the agent re-polls and re-tracks).
        if let (Some(key), Some(gui)) = (mkey.as_ref(), gui_id.as_ref()) {
            self.inflight.insert(key.clone(), gui.clone());
        }
        let interactive = matches!(op.as_str(), "user_select" | "getpoint");
        let deadline = Instant::now() + wait_deadline(&op, wait_seconds);
        while matches!(response["status"].as_str(), Some("accepted" | "running"))
            && Instant::now() < deadline
        {
            let Some(request_id) = request_id.clone() else {
                break;
            };
            match pump.wait_line(Duration::from_millis(50), mkey.as_deref()) {
                PumpEvent::Cancelled => {
                    self.dismiss(gui_id.as_deref());
                    if let Some(key) = mkey.as_ref() {
                        self.inflight.remove(key);
                    }
                    return Err(CallError::Cancelled);
                }
                PumpEvent::Quiet => {}
            }
            response = exchange(
                &self.descriptor,
                json!({"op":"operation","request_id":request_id}),
                Duration::from_secs(15),
            )?;
        }
        let terminal = !matches!(response["status"].as_str(), Some("accepted" | "running"));
        if terminal {
            if let Some(key) = mkey.as_ref() {
                self.inflight.remove(key);
            }
            if interactive && response["status"].as_str() == Some("cancelled") {
                if let Some(gui) = gui_id.as_ref() {
                    if !self.dismissed.iter().any(|id| id == gui) {
                        if self.dismissed.len() >= 128 {
                            self.dismissed.pop_front();
                        }
                        self.dismissed.push_back(gui.clone());
                    }
                }
            }
        } else if interactive && interactive_timeout(wait_seconds) {
            // Only the ceiling itself dismisses: short waits keep the
            // classic running response so agents can re-poll. Hitting the
            // ten-minute ceiling means nobody is coming: dismiss the prompt
            // and say so plainly instead of parking it forever.
            self.dismiss(gui_id.as_deref());
            if let Some(key) = mkey.as_ref() {
                self.inflight.remove(key);
            }
            return Err(CallError::Tool(
                "timed out waiting for user (10 min); the prompt was dismissed".into(),
            ));
        }
        if response.get("state").is_some() {
            self.state = response["state"].clone();
        } else if matches!(op.as_str(), "hello" | "state") && response["ok"].as_bool() == Some(true)
        {
            self.state = response.clone();
        }
        Ok(response)
    }

    fn execute_batch(
        &mut self,
        request: Value,
        wait_seconds: f64,
        pump: &mut CancelPump,
        mcp_id: Option<&Value>,
    ) -> Result<Value, CallError> {
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

            // Cancel between steps: dismiss a pending interactive step so
            // no late answer fires, then abandon the batch (cancel = stop).
            if let PumpEvent::Cancelled =
                pump.wait_line(Duration::ZERO, mcp_id.map(mcp_key).as_deref())
            {
                self.dismiss(batch.active.as_deref());
                return Err(CallError::Cancelled);
            }
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
                    pump,
                    mcp_id,
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
                    pump,
                    mcp_id,
                )
            };
            let response = match response {
                Ok(response) => response,
                // Cancelled abandons the batch outright: resuming would
                // re-poll a dismissed step.
                Err(CallError::Cancelled) => return Err(CallError::Cancelled),
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
) -> Result<&'a mut GuiClient, CallError> {
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

fn validate_execute_request(
    request: &Value,
    op: &str,
) -> Result<crate::mcp_ops::ValidationResult, String> {
    if op == "batch" {
        let steps = request["steps"].as_array().ok_or_else(|| {
            r#"Missing steps for batch. Example request: {"op":"batch","request_id":"draw-1","steps":[{"op":"run","cmd":"LINE 0,0 10,0"}]}"#.to_string()
        })?;
        if steps.is_empty() || steps.len() > MAX_BATCH_STEPS {
            return Err(format!(
                "batch steps must contain 1 to {MAX_BATCH_STEPS} operations"
            ));
        }
        let mut warnings = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let step_obj = step
                .as_object()
                .ok_or_else(|| format!("batch step {index} must be an object"))?;
            if step_obj.contains_key("request_id") {
                return Err(format!(
                    "batch step {index} must omit request_id; the batch assigns idempotency keys"
                ));
            }
            let step_op = step_obj
                .get("op")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("batch step {index} is missing op"))?;
            if !BATCH_STEP_OPS.contains(&step_op) {
                return Err(format!("Unknown operation {step_op} in batch step {index}"));
            }
            let step_val = crate::mcp_ops::validate_request(step, true)
                .map_err(|error| format!("batch step {index}: {error}"))?;
            for w in step_val.warnings {
                warnings.push(format!("batch step {index}: {w}"));
            }
        }
        return Ok(crate::mcp_ops::ValidationResult { warnings });
    }
    crate::mcp_ops::validate_request(request, false)
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
    pump: &mut CancelPump,
) -> Result<Value, String> {
    if detail == "changed_entities" {
        let handles = response_handles(&response);
        if !handles.is_empty() {
            let entities = gui.request(
                json!({"op":"query","handles":handles,"detail":"geometry","limit":MAX_BATCH_STEPS * 100}),
                30.0,
                pump,
                None,
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

fn read_resource(
    uri: &str,
    clients: &mut HashMap<String, GuiClient>,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
    mcp_id: Option<&Value>,
) -> Result<Value, String> {
    if !uri.starts_with("cad://") {
        return Err(format!("Unsupported resource URI scheme: {uri}"));
    }
    // Check cached snapshot image: cad://session/{session_id}/snapshot/{hash}.png or latest.png
    if let Some(snapshot) = resources.get_by_uri(uri) {
        return Ok(json!({
            "contents": [
                {
                    "uri": uri,
                    "mimeType": snapshot.mime_type,
                    "blob": snapshot.data_base64
                }
            ]
        }));
    }
    // Check snapshot metadata json: cad://session/{session_id}/snapshot/{hash}.json
    if uri.ends_with(".json") {
        let png_uri = format!("{}.png", uri.trim_end_matches(".json"));
        if let Some(snapshot) = resources.get_by_uri(&png_uri) {
            return Ok(json!({
                "contents": [
                    {
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&snapshot.metadata)
                            .unwrap_or_else(|_| snapshot.metadata.to_string())
                    }
                ]
            }));
        }
    }
    // Live viewport capture: cad://session/{session_id}/viewport.png, pyramid manifest, tiles, or state.json
    if let Some(rest) = uri.strip_prefix("cad://session/") {
        if let Some((session_id, subpath)) = rest.split_once('/') {
            if subpath == "pyramid/manifest.json" {
                if let Some(manifest) = resources.pyramid_manifests.get(session_id) {
                    let text = serde_json::to_string_pretty(manifest)
                        .unwrap_or_else(|_| manifest.to_string());
                    return Ok(json!({
                        "contents": [
                            {
                                "uri": uri,
                                "mimeType": "application/json",
                                "text": text
                            }
                        ]
                    }));
                }
                let mut bounds = [-100.0, -100.0, 100.0, 100.0];
                let mut unit = "Millimeters".to_string();
                if let Some(latest_uri) = resources.latest.get(session_id) {
                    if let Some(snap) = resources.snapshots.get(latest_uri) {
                        if let Some(spatial) = snap.metadata.get("_spatial") {
                            if let Some(u) = spatial.get("unit").and_then(Value::as_str) {
                                unit = u.to_string();
                            }
                            if let Some(wb) = spatial.get("world_bounds") {
                                if let (Some(min), Some(max)) = (
                                    wb.get("min").and_then(Value::as_array),
                                    wb.get("max").and_then(Value::as_array),
                                ) {
                                    if min.len() >= 2 && max.len() >= 2 {
                                        if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                            min[0].as_f64(),
                                            min[1].as_f64(),
                                            max[0].as_f64(),
                                            max[1].as_f64(),
                                        ) {
                                            bounds = [x0, y0, x1, y1];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let manifest = crate::app::control::vision::compute_pyramid_manifest(
                    session_id, &unit, bounds, 4, 512,
                );
                let manifest_val =
                    serde_json::to_value(&manifest).map_err(|e| e.to_string())?;
                resources.insert_pyramid_manifest(session_id, manifest_val.clone());
                let text = serde_json::to_string_pretty(&manifest_val)
                    .unwrap_or_else(|_| manifest_val.to_string());
                return Ok(json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text
                        }
                    ]
                }));
            } else if subpath.starts_with("tile/") {
                let trimmed = subpath.strip_prefix("tile/").unwrap_or("");
                let is_json = trimmed.ends_with(".json");
                let core = trimmed.trim_end_matches(".png").trim_end_matches(".json");
                let parts: Vec<&str> = core.split('/').collect();
                if parts.len() == 3 {
                    if let (Ok(level), Ok(x), Ok(y)) = (
                        parts[0].parse::<u32>(),
                        parts[1].parse::<u32>(),
                        parts[2].parse::<u32>(),
                    ) {
                        let png_uri =
                            format!("cad://session/{session_id}/tile/{level}/{x}/{y}.png");
                        if is_json {
                            if let Some(tile_snap) = resources.tiles.get(&png_uri) {
                                let meta_text = serde_json::to_string_pretty(&tile_snap.metadata)
                                    .unwrap_or_else(|_| tile_snap.metadata.to_string());
                                return Ok(json!({
                                    "contents": [
                                        {
                                            "uri": uri,
                                            "mimeType": "application/json",
                                            "text": meta_text
                                        }
                                    ]
                                }));
                            }
                        }

                        if let Some(tile_snap) = resources.tiles.get(&png_uri) {
                            return Ok(json!({
                                "contents": [
                                    {
                                        "uri": png_uri,
                                        "mimeType": "image/png",
                                        "blob": tile_snap.data_base64
                                    }
                                ]
                            }));
                        }

                        let mut base_bounds = [-100.0, -100.0, 100.0, 100.0];
                        if let Some(manifest) = resources.pyramid_manifests.get(session_id) {
                            if let Some(arr) =
                                manifest.get("world_bounds").and_then(Value::as_array)
                            {
                                if arr.len() == 4 {
                                    if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                        arr[0].as_f64(),
                                        arr[1].as_f64(),
                                        arr[2].as_f64(),
                                        arr[3].as_f64(),
                                    ) {
                                        base_bounds = [x0, y0, x1, y1];
                                    }
                                }
                            }
                        }
                        let tile_bounds = crate::app::control::vision::compute_tile_bounds(
                            base_bounds, level, x, y,
                        )?;
                        let path = std::env::temp_dir()
                            .join(format!("ocs-tile-{session_id}-{level}-{x}-{y}.png"));
                        let req = json!({
                            "op": "capture",
                            "path": path.to_string_lossy(),
                            "scope": "viewport",
                            "view": "region",
                            "bounds": tile_bounds,
                            "max_dimension": 512,
                        });
                        let result =
                            client(clients, session_id)?.request(req, 20.0, pump, mcp_id)?;
                        if result["ok"].as_bool() != Some(true)
                            || result["status"].as_str() != Some("completed")
                        {
                return Err(result.to_string().into());
                        }
                        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                        let _ = std::fs::remove_file(path);
                        let b64 = BASE64.encode(&bytes);
                        let bytes_len = bytes.len();
                        let mut meta =
                            result.get("result").cloned().unwrap_or_else(|| json!({}));
                        if let Some(obj) = meta.as_object_mut() {
                            obj.remove("path");
                            obj.insert(
                                "pyramid".into(),
                                json!({
                                    "level": level,
                                    "x": x,
                                    "y": y,
                                    "tile_world_bounds": tile_bounds,
                                    "tile_uri": png_uri,
                                    "manifest_uri": format!("cad://session/{session_id}/pyramid/manifest.json")
                                }),
                            );
                        }
                        resources.insert_tile(
                            session_id,
                            level,
                            x,
                            y,
                            b64.clone(),
                            bytes_len,
                            meta.clone(),
                        );
                        if is_json {
                            let meta_text = serde_json::to_string_pretty(&meta)
                                .unwrap_or_else(|_| meta.to_string());
                            return Ok(json!({
                                "contents": [
                                    {
                                        "uri": uri,
                                        "mimeType": "application/json",
                                        "text": meta_text
                                    }
                                ]
                            }));
                        }
                        return Ok(json!({
                            "contents": [
                                {
                                    "uri": png_uri,
                                    "mimeType": "image/png",
                                    "blob": b64
                                }
                            ]
                        }));
                    }
                }
            } else if subpath == "viewport.png" {
                let path = std::env::temp_dir().join(format!("ocs-resource-{}.png", random_id()?));
                let req = json!({
                    "op": "capture",
                    "path": path.to_string_lossy(),
                    "scope": "viewport",
                    "max_dimension": 1600,
                });
                let result = client(clients, session_id)?
                    .request(req, 15.0, pump, mcp_id)?;
                if result["ok"].as_bool() != Some(true)
                    || result["status"].as_str() != Some("completed")
                {
                    return Err(result.to_string());
                }
                let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                let _ = std::fs::remove_file(path);

                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let hash = format!("{:x}", hasher.finalize());
                let b64 = BASE64.encode(&bytes);
                let bytes_len = bytes.len();
                let meta = result.get("result").cloned().unwrap_or_else(|| json!({}));

                resources.insert(session_id, &hash, b64.clone(), bytes_len, meta);

                return Ok(json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "image/png",
                            "blob": b64
                        }
                    ]
                }));
            } else if subpath == "state.json" {
                let cli = client(clients, session_id)?;
                let state_text = serde_json::to_string_pretty(&cli.state)
                    .unwrap_or_else(|_| cli.state.to_string());
                return Ok(json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": state_text
                        }
                    ]
                }));
            }
        }
    }

    Err(format!("Resource not found: {uri}"))
}

fn call_tool(
    name: &str,
    arguments: &Value,
    clients: &mut HashMap<String, GuiClient>,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
    mcp_id: Option<&Value>,
) -> Result<Value, CallError> {
    match name {
        "ocs_sessions" => {
            let launch = arguments["launch_if_none"].as_bool().unwrap_or(true);
            Ok(Value::Array(sessions(launch).map_err(CallError::Infra)?))
        }
        "ocs_read" => {
            let session_id = required_string(arguments, "ocs_session_id")?;
            let op = arguments["op"].as_str().unwrap_or("state");
            if !READ_OPS.contains(&op) {
                return Err("Use ocs_execute for mutations".to_string().into());
            }
            if op == "capture" {
                // The GUI capture op needs a file path and lives behind the
                // ocs_capture tool; agents hitting the bare read op get
                // guidance instead of a cryptic missing-path failure.
                return Ok(json!({
                    "ok": false,
                    "status": "failed",
                    "code": "use_capture_tool",
                    "error": "Viewport captures run through the ocs_capture tool (scope, annotate, tiles, diff). ocs_read exposes capture products already stored as resources, not new captures."
                }));
            }
            if op == "tools" {
                return Ok(json!({
                    "ok": true,
                    "status": "completed",
                    "tools": tool_definitions(),
                    "instructions": INSTRUCTIONS
                }));
            }
            let mut request = arguments["parameters"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            request.insert("op".into(), Value::String(op.into()));
            let response =
                client(clients, session_id)?.request(Value::Object(request), 30.0, pump, mcp_id)?;
            if matches!(op, "hello" | "capabilities") {
                Ok(with_bridge_identity(response))
            } else {
                Ok(response)
            }
        }
        "ocs_execute" => {
            let session_id = required_string(arguments, "ocs_session_id")?;
            let request = arguments["request"]
                .as_object()
                .cloned()
                .map(Value::Object)
                .ok_or_else(|| "Missing request object".to_string())?;
            let op = required_string(&request, "op")?;
            if !EXECUTE_OPS.contains(&op) {
                return Err(CallError::Tool(format!("Unknown mutation operation: {op}")));
            }
            let request_id = required_string(&request, "request_id")?;
            if request_id.len() > 128 {
                return Err("request_id must not exceed 128 bytes".to_string().into());
            }
            let val_res = validate_execute_request(&request, op)?;
            let wait = arguments["wait_seconds"].as_f64().unwrap_or(30.0);
            let detail = arguments["response_detail"].as_str().unwrap_or("compact");
            let gui = client(clients, session_id)?;
            let mut response = if op == "batch" {
                gui.execute_batch(request, wait, pump, mcp_id)?
            } else {
                gui.request(request, wait, pump, mcp_id)?
            };
            if !val_res.warnings.is_empty() {
                if let Some(object) = response.as_object_mut() {
                    object.insert(
                        "warnings".into(),
                        Value::Array(val_res.warnings.into_iter().map(Value::String).collect()),
                    );
                }
            }
            shape_execute_response(response, detail, gui, pump).map_err(CallError::from)
        }
        "ocs_capture" => {
            let session_id = required_string(arguments, "ocs_session_id")?;
            let path = std::env::temp_dir().join(format!("ocs-capture-{}.png", random_id()?));
            let scope = arguments["scope"].as_str().unwrap_or("viewport");
            let max_dimension = arguments["max_dimension"].as_u64().unwrap_or(1600);
            let delivery = arguments["delivery"].as_str().unwrap_or("inline");
            let diff = arguments["diff"].as_bool().unwrap_or(false);
            let diff_mode = arguments["diff_mode"].as_str().unwrap_or("highlight");
            let reset_baseline = arguments["reset_diff_baseline"].as_bool().unwrap_or(false);
            let tile = arguments.get("tile");
            let want_manifest = arguments["pyramid_manifest"].as_bool().unwrap_or(false);

            if reset_baseline {
                resources.last_captures.remove(session_id);
            }

            let mut tile_info: Option<(u32, u32, u32, [f64; 4])> = None;
            if let Some(tile_val) = tile {
                if let (Some(level), Some(x), Some(y)) = (
                    tile_val.get("level").and_then(Value::as_u64),
                    tile_val.get("x").and_then(Value::as_u64),
                    tile_val.get("y").and_then(Value::as_u64),
                ) {
                    let mut base_bounds = [-100.0, -100.0, 100.0, 100.0];
                    if let Some(bounds_arr) = arguments.get("bounds").and_then(Value::as_array) {
                        if bounds_arr.len() >= 4 {
                            if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                bounds_arr[0].as_f64(),
                                bounds_arr[1].as_f64(),
                                bounds_arr[2].as_f64(),
                                bounds_arr[3].as_f64(),
                            ) {
                                base_bounds = [x0, y0, x1, y1];
                            }
                        }
                    } else if let Some(manifest) = resources.pyramid_manifests.get(session_id) {
                        if let Some(arr) = manifest.get("world_bounds").and_then(Value::as_array) {
                            if arr.len() == 4 {
                                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                    arr[0].as_f64(),
                                    arr[1].as_f64(),
                                    arr[2].as_f64(),
                                    arr[3].as_f64(),
                                ) {
                                    base_bounds = [x0, y0, x1, y1];
                                }
                            }
                        }
                    } else if let Some(latest_uri) = resources.latest.get(session_id) {
                        if let Some(snap) = resources.snapshots.get(latest_uri) {
                            if let Some(spatial) = snap.metadata.get("_spatial") {
                                if let Some(wb) = spatial.get("world_bounds") {
                                    if let (Some(min), Some(max)) = (
                                        wb.get("min").and_then(Value::as_array),
                                        wb.get("max").and_then(Value::as_array),
                                    ) {
                                        if min.len() >= 2 && max.len() >= 2 {
                                            if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                                min[0].as_f64(),
                                                min[1].as_f64(),
                                                max[0].as_f64(),
                                                max[1].as_f64(),
                                            ) {
                                                base_bounds = [x0, y0, x1, y1];
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let tb = crate::app::control::vision::compute_tile_bounds(
                        base_bounds,
                        level as u32,
                        x as u32,
                        y as u32,
                    )?;
                    tile_info = Some((level as u32, x as u32, y as u32, tb));
                }
            }

            let mut req = json!({
                "op": "capture",
                "path": path.to_string_lossy(),
                "scope": scope,
                "max_dimension": if tile_info.is_some() && arguments.get("max_dimension").is_none() { 512 } else { max_dimension },
            });
            // The client's id wins when supplied (and is echoed back in
            // metadata); otherwise request() mints one for the GUI below.
            if let Some(request_id) = arguments.get("request_id").and_then(Value::as_str) {
                req["request_id"] = json!(request_id);
            }
            if let Some((_, _, _, tb)) = tile_info {
                req["view"] = json!("region");
                req["bounds"] = json!(tb);
            } else {
                if let Some(view) = arguments.get("view").and_then(Value::as_str) {
                    req["view"] = json!(view);
                }
                if let Some(bounds) = arguments.get("bounds").and_then(Value::as_array) {
                    req["bounds"] = json!(bounds);
                }
            }
            if let Some(focus) = arguments.get("focus_handles").and_then(Value::as_array) {
                req["focus_handles"] = json!(focus);
            }
            if let Some(hl) = arguments.get("highlight_handles").and_then(Value::as_array) {
                req["highlight_handles"] = json!(hl);
            }
            if let Some(annotate) = arguments.get("annotate").and_then(Value::as_bool) {
                req["annotate"] = json!(annotate);
            }
            let result = client(clients, session_id)?
                .request(req, 30.0, pump, mcp_id)?;
            if result["ok"].as_bool() != Some(true)
                || result["status"].as_str() != Some("completed")
            {
                // Failed capture response from the GUI: retryable, stays Tool.
                return Err(result.to_string().into());
            }
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let _ = std::fs::remove_file(path);
            let mut meta = result.get("result").cloned().unwrap_or_else(|| json!({}));
            if let Some(obj) = meta.as_object_mut() {
                obj.remove("path");
                if let Some(request_id) = arguments["request_id"].as_str() {
                    obj.insert("request_id".into(), Value::String(request_id.into()));
                }
            }

            let mut final_bytes = bytes;
            if diff {
                let current_img = image::load_from_memory(&final_bytes)
                    .map_err(|e| format!("Failed to decode capture for visual diff: {e}"))?
                    .to_rgba8();

                let mut p2w = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                if let Some(arr) = meta
                    .get("_spatial")
                    .and_then(|s| s.get("pixel_to_world_matrix"))
                    .and_then(Value::as_array)
                {
                    if arr.len() == 6 {
                        for (i, v) in arr.iter().enumerate() {
                            if let Some(num) = v.as_f64() {
                                p2w[i] = num;
                            }
                        }
                    }
                }

                if let Some((prev_img, prev_hash)) = resources.last_captures.get(session_id) {
                    let diff_res = crate::app::control::vision::compute_visual_diff(prev_img, &current_img, &p2w);
                    let diff_info = json!({
                        "is_baseline": false,
                        "baseline_hash": prev_hash,
                        "changed": diff_res.changed,
                        "change_percentage": (diff_res.change_percentage * 100.0).round() / 100.0,
                        "changed_pixels": diff_res.changed_pixels,
                        "dirty_pixel_bounds": diff_res.dirty_pixel_bounds,
                        "dirty_world_bounds": diff_res.dirty_world_bounds,
                        "patch_resolution": diff_res.patch_resolution,
                        "mode": diff_mode,
                    });
                    if let Some(obj) = meta.as_object_mut() {
                        obj.insert("diff".into(), diff_info);
                    }

                    if diff_res.changed {
                        match diff_mode {
                            "crop" => {
                                if let Some(crop) = &diff_res.cropped_patch {
                                    final_bytes = crate::app::control::vision::encode_image_png(crop)?;
                                }
                            }
                            "highlight" => {
                                final_bytes = crate::app::control::vision::encode_image_png(&diff_res.diff_overlay)?;
                            }
                            _ => {}
                        }
                    }
                } else {
                    let diff_info = json!({
                        "is_baseline": true,
                        "changed": false,
                        "change_percentage": 0.0,
                        "changed_pixels": 0,
                        "message": "Baseline capture established; subsequent captures with diff: true will report dirty regions.",
                        "mode": diff_mode,
                    });
                    if let Some(obj) = meta.as_object_mut() {
                        obj.insert("diff".into(), diff_info);
                    }
                }

                let mut base_hasher = Sha256::new();
                let full_png = crate::app::control::vision::encode_image_png(&current_img)?;
                base_hasher.update(&full_png);
                let current_full_hash = format!("{:x}", base_hasher.finalize());
                resources.last_captures.insert(session_id.to_string(), (current_img, current_full_hash));
            }

            let mut hasher = Sha256::new();
            hasher.update(&final_bytes);
            let hash = format!("{:x}", hasher.finalize());
            let b64 = BASE64.encode(&final_bytes);
            let bytes_len = final_bytes.len();

            let uri = resources.insert(session_id, &hash, b64.clone(), bytes_len, meta.clone());
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("uri".into(), Value::String(uri.clone()));
                obj.insert("latest_uri".into(), Value::String(format!("cad://session/{session_id}/snapshot/latest.png")));
                obj.insert("hash".into(), Value::String(hash.clone()));
                obj.insert("bytes".into(), Value::from(bytes_len));
            }

            let mut final_uri = uri.clone();
            if let Some((level, x, y, tb)) = tile_info {
                let tile_uri = resources.insert_tile(
                    session_id,
                    level,
                    x,
                    y,
                    b64.clone(),
                    bytes_len,
                    meta.clone(),
                );
                final_uri = tile_uri.clone();
                let pyr = json!({
                    "level": level,
                    "x": x,
                    "y": y,
                    "tile_world_bounds": tb,
                    "tile_uri": tile_uri,
                    "manifest_uri": format!("cad://session/{session_id}/pyramid/manifest.json")
                });
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("pyramid".into(), pyr);
                    obj.insert("uri".into(), Value::String(final_uri.clone()));
                }
            }

            if want_manifest
                || (!resources.pyramid_manifests.contains_key(session_id) && tile_info.is_none())
            {
                let mut bounds = [-100.0, -100.0, 100.0, 100.0];
                let mut unit = "Millimeters".to_string();
                if let Some(spatial) = meta.get("_spatial") {
                    if let Some(u) = spatial.get("unit").and_then(Value::as_str) {
                        unit = u.to_string();
                    }
                    if let Some(wb) = spatial.get("world_bounds") {
                        if let (Some(min), Some(max)) = (
                            wb.get("min").and_then(Value::as_array),
                            wb.get("max").and_then(Value::as_array),
                        ) {
                            if min.len() >= 2 && max.len() >= 2 {
                                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                                    min[0].as_f64(),
                                    min[1].as_f64(),
                                    max[0].as_f64(),
                                    max[1].as_f64(),
                                ) {
                                    bounds = [x0, y0, x1, y1];
                                }
                            }
                        }
                    }
                }
                let manifest = crate::app::control::vision::compute_pyramid_manifest(
                    session_id, &unit, bounds, 4, 512,
                );
                if let Ok(manifest_val) = serde_json::to_value(&manifest) {
                    resources.insert_pyramid_manifest(session_id, manifest_val.clone());
                    if want_manifest {
                        if let Some(obj) = meta.as_object_mut() {
                            obj.insert("pyramid_manifest".into(), manifest_val);
                        }
                    }
                }
            }

            match delivery {
                "resource" => Ok(json!({
                    "$resource": {
                        "uri": final_uri,
                        "mimeType": "image/png",
                        "hash": hash,
                        "bytes": bytes_len,
                    },
                    "metadata": meta,
                })),
                "both" => Ok(json!({
                    "$image": b64,
                    "$resource": {
                        "uri": final_uri,
                        "mimeType": "image/png",
                        "hash": hash,
                        "bytes": bytes_len,
                    },
                    "metadata": meta,
                })),
                _ => Ok(json!({
                    "$image": b64,
                    "metadata": meta,
                })),
            }
        }
        // Backstop: the dispatcher rejects unknown names first with -32602.
        _ => Err(CallError::Tool(format!("Unknown tool: {name}"))),
    }
}

#[allow(dead_code)]
fn batch_step_schema() -> Value {
    crate::mcp_ops::batch_step_schema()
}

fn execute_request_schema() -> Value {
    crate::mcp_ops::execute_request_schema()
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

pub(crate) fn tool_definitions() -> Value {
    json!([
        {
            "name":"ocs_sessions",
            "description":"List real OpenCADStudio GUI sessions and documents. Launch the installed editor if none is running. On first use, announce the build to the user from the `bridge` object in each result (OpenCADStudio version, build_rev, tool_schema digest); announce again if a later call reports a different build.",
            "inputSchema":{"type":"object","properties":{"launch_if_none":{"type":"boolean","default":true,"description":"Launch OpenCADStudio when no live session exists."}},"additionalProperties":false},
            "outputSchema":{"type":"object","properties":{"result":{"type":"array","items":{"type":"object","properties":{"ok":{"const":true},"session_id":{"type":"string"},"document_id":{"type":"integer"},"revision":{"type":"integer"},"selection":{"type":"array","items":{"type":"string"}},"documents":{"type":"array"}},"required":["ok","session_id","document_id","revision","selection","documents"],"additionalProperties":true}}},"required":["result"],"additionalProperties":false},
            "title":"List OCS sessions","annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_read",
            "description":"Discover capabilities and record schemas, or read state, complete database records, command manifests, entities, properties, kernel measurements and spatial relationships, history, events or operation status from a live OCS session.",
            "inputSchema":{"type":"object","properties":{"ocs_session_id":{"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."},"op":{"type":"string","enum":READ_OPS,"default":"state"},"parameters":{"type":"object","description":"Operation-specific filters.","properties":{"name":{"type":"string","description":"Command name or record name."},"search":{"type":"string","description":"Case-insensitive command or record-type search."},"find":{"type":"string","description":"Text query string for text_search."},"match_case":{"type":"boolean","default":false,"description":"Case-sensitive text search."},"whole_word":{"type":"boolean","default":false,"description":"Match only whole words."},"ignore_accents":{"type":"boolean","description":"Ignore accents/diacritics in text (defaults to true when match_case is false)."},"scope":{"type":"string","enum":["all","active_space","blocks"],"default":"all","description":"Text search scope."},"system_spellcheck":{"type":"boolean","default":false,"description":"Use native OS spell-checker (Windows, macOS, Linux)."},"language":{"type":"string","description":"Language tag for spell-checker (e.g. 'fr-FR', 'en-US', 'de-DE', 'es-ES')."},"suggest":{"type":"boolean","default":true,"description":"Include suggested corrections for misspelled words."},"check_terms":{"type":"array","items":{"type":"string"},"description":"List of terms or suspect misspellings to flag in text_audit."},"dictionary":{"type":"array","items":{"type":"string"},"description":"Known valid words for text_audit dictionary check."},"pairs":{"type":"array","description":"Find/replace pairs for dry-run simulation in text_audit.","items":{"type":"object","properties":{"find":{"type":"string"},"replace":{"type":"string"}},"required":["find","replace"]}},"dry_run_pairs":{"type":"array","description":"Alias for pairs in text_audit.","items":{"type":"object","properties":{"find":{"type":"string"},"replace":{"type":"string"}},"required":["find","replace"]}},"document_id":{"type":"integer","minimum":0},"path":{"type":"string","description":"Optional intended output path for audit; extension determines target format."},"target_format":{"type":"string","enum":["dwg","dxf"],"description":"Intended output format for audit."},"target_version":{"type":"string","enum":["R14","2000","2004","2007","2010","2013","2018","AC1014","AC1015","AC1018","AC1021","AC1024","AC1027","AC1032"],"description":"Intended CAD output version for audit."},"collection":{"type":"string","description":"Record collection, all for records, or omit to discover collections and schema types."},"handle":{"type":"string"},"handles":{"type":"array","items":{"type":"string"},"description":"Exact entity or record handles."},"type":{"type":"string","description":"Entity or record type filter; for record_schema, returns its complete type graph and writable field paths."},"layer":{"type":"string","description":"Layer name filter for query."},"detail":{"type":"string","enum":["summary","geometry","full"],"default":"geometry","description":"Entity detail returned by query."},"fields":{"type":"array","items":{"type":"string"},"description":"Return only these entity fields plus handle."},"paths":{"type":"array","items":{"type":"string"},"description":"Project RFC 6901 JSON Pointer paths relative to record.properties."},"where":{"type":"array","description":"All property filters must match.","items":{"type":"object","properties":{"path":{"type":"string"},"op":{"type":"string","enum":["eq","ne","lt","lte","gt","gte","contains","starts_with","ends_with","in","exists","not_exists"],"default":"eq"},"value":{}},"required":["path"],"additionalProperties":false}},"near":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Rank planar curves by exact kernel distance to this world XY point."},"point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"World point whose object snap the snap op reports."},"from":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Base point for perpendicular and tangent snaps (snap op)."},"contains_point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Return closed planar curves containing this world XY point."},"bounds":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"Filter entities whose world XY bounds overlap [min_x,min_y,max_x,max_y]."},"intersections":{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":2,"description":"Return exact kernel intersections between two planar curve handles."},"after":{"type":"integer","minimum":0,"description":"Event cursor."},"request_id":{"type":"string","description":"Operation id to query."},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":10000}},"additionalProperties":false}},"required":["ocs_session_id"],"additionalProperties":false},
            "outputSchema":read_output_schema(),
            "title":"Read OCS state","annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_execute",
            "description":"Execute semantic OCS actions, batch drafting, and mutations. Supports batch creation (entities_create), reference image underlays (embed_image), block definitions (block_define), and block reference insertions (INSERT). MANDATORY: For architectural drafting, all doors and windows must be inserted as CAD blocks (type: INSERT), never loose lines.",
            "inputSchema":{"type":"object","properties":{"ocs_session_id":{"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."},"request":execute_request_schema(),"wait_seconds":{"type":"number","minimum":0,"maximum":60,"default":30,"description":"Total time to wait for completion before returning."},"response_detail":{"type":"string","enum":["compact","changed_entities","full"],"default":"compact","description":"compact returns only state needed for the next edit; changed_entities also returns current geometry for changed handles; full preserves the complete editor state."}},"required":["ocs_session_id","request"],"additionalProperties":false},
            "outputSchema":execute_output_schema(),
            "title":"Execute OCS action","annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_capture",
            "description":"Capture the actual current OCS drawing viewport or window as a bounded PNG with optional camera framing, entity highlighting, spatial coordinates and Set-of-Marks annotations. Works quietly in background and overlapped window states without stealing focus. MANDATORY FOR CAD ACCURACY: Use annotate: true to render Set-of-Marks entity IDs directly on the drawing for precision alignment feedback, and diff: true to stream visual changes between steps.",
            "inputSchema":{
                "type":"object",
                "properties":{
                    "ocs_session_id":{"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."},
                    "request_id":{"type":"string","description":"Optional client request id, accepted for schema compatibility and echoed in the result metadata. Captures are not idempotency-keyed."},
                    "scope":{"type":"string","enum":["viewport","window"],"default":"viewport","description":"Capture only the drawing viewport by default, or the complete application window."},
                    "max_dimension":{"type":"integer","minimum":256,"maximum":4096,"default":1600,"description":"Resize the longest image edge to at most this many pixels."},
                    "delivery":{"type":"string","enum":["inline","resource","both"],"default":"inline","description":"Image delivery method: 'inline' embeds base64 in tool content, 'resource' returns an MCP cad:// URI reference without inlining image bytes, 'both' returns both inline image and cad:// URI."},
                    "diff":{"type":"boolean","default":false,"description":"If true, calculates visual differential compared to the previous capture of this session, returning dirty bounds and changed areas."},
                    "diff_mode":{"type":"string","enum":["highlight","crop","metadata_only"],"default":"highlight","description":"Visual diff representation: 'highlight' returns full image with changed areas highlighted in vibrant green and unchanged background dimmed; 'crop' returns only the cropped dirty bounding box (saving maximum bandwidth/tokens); 'metadata_only' leaves image untouched but populates the diff metadata."},
                    "reset_diff_baseline":{"type":"boolean","default":false,"description":"If true, discards any previous diff baseline for this session and establishes this capture as the new baseline."},
                    "view":{"type":"string","enum":["current","extents","selection","region"],"default":"current","description":"Frame the camera before capture: extents fits all entities, selection fits selected entities, region fits explicit world bounds."},
                    "bounds":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"World XY bounding box [min_x, min_y, max_x, max_y] to zoom and fit in view before capturing (used with view: 'region')."},
                    "focus_handles":{"type":"array","items":{"type":"string"},"description":"Hex handles of entities to zoom and fit in view before capturing."},
                    "highlight_handles":{"type":"array","items":{"type":"string"},"description":"Hex handles of entities to select/highlight before capturing."},
                    "annotate":{"type":"boolean","default":false,"description":"Overlay Set-of-Marks numbered tags on visible entities for visual grounding."},
                    "tile":{"type":"object","properties":{"level":{"type":"integer","minimum":0,"maximum":6,"description":"Pyramid zoom level (0 = full overview, 1 = 2x2 grid, 2 = 4x4 grid, etc.)."},"x":{"type":"integer","minimum":0,"description":"Tile column index (0-indexed, left-to-right)."},"y":{"type":"integer","minimum":0,"description":"Tile row index (0-indexed, top-to-bottom)."}},"required":["level","x","y"],"description":"Fetch a specific DeepZoom pyramid tile. Automatically computes tile world bounds and frames the camera."},
                    "pyramid_manifest":{"type":"boolean","default":false,"description":"If true, generates and returns the complete multiscale pyramid manifest (levels, grid dimensions, tile world spans and resource URIs) in the response metadata."}
                },
                "required":["ocs_session_id"],
                "additionalProperties":true
            },
            "title":"Capture OCS window","annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }
    ])
}

fn tool_result(value: Value) -> Value {
    if value.get("$resource").is_some() || value.get("$image").is_some() {
        let mut content = Vec::new();
        if let Some(res) = value.get("$resource") {
            let uri = res["uri"].as_str().unwrap_or("");
            content.push(json!({
                "type": "text",
                "text": format!("Captured viewport to MCP resource: {uri}")
            }));
            content.push(json!({
                "type": "resource",
                "resource": {
                    "uri": uri,
                    "mimeType": res["mimeType"].as_str().unwrap_or("image/png")
                }
            }));
        }
        if let Some(meta) = value.get("metadata") {
            let meta_text = serde_json::to_string_pretty(meta).unwrap_or_else(|_| meta.to_string());
            content.push(json!({"type":"text","text":meta_text}));
        }
        if let Some(image) = value.get("$image").and_then(Value::as_str) {
            content.push(json!({"type":"image","data":image,"mimeType":"image/png"}));
        }
        let structured = value.get("metadata").cloned().unwrap_or_else(|| json!({}));
        return json!({
            "content": content,
            "structuredContent": structured,
            "isError": false
        });
    }
    let is_err = value["ok"].as_bool() == Some(false);
    let mut result = json!({
        "content":[{"type":"text","text":value.to_string()}],
        "isError": is_err,
    });
    // Strict clients validate structuredContent against outputSchema, which
    // only describes successful results. Omitting it on errors keeps the real
    // message visible instead of masked by a schema complaint.
    if !is_err {
        result["structuredContent"] = if value.is_object() {
            value.clone()
        } else {
            json!({"result":value.clone()})
        };
    }
    result
}

fn error_result(message: impl ToString) -> Value {
    let message = message.to_string();
    json!({
        "content":[{"type":"text","text":message}],
        "isError":true
    })
}

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
        object
            .entry("bridge")
            .or_insert_with(bridge_identity);
    }
    value
}

/// Sha256 hex digest of the canonical [`tool_definitions`] JSON.
///
/// Published via `capabilities.experimental` and the in-band [`bridge_identity`]
/// stamp so MCP clients can detect a stale bridge (running an older binary
/// than the one on disk) without guessing.
fn tool_schema_digest() -> String {
    let canonical = serde_json::to_string(&tool_definitions()).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
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
        return Some(rpc_error(Value::Null, -32600, "Invalid request: expected a JSON-RPC object"));
    }
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
            // Deterministic order: HashMap iteration is random, and stable
            // ordering keeps client caches and prompt caches hitting.
            session_ids.sort();
            let list = resources.list_resources(&session_ids);
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
                Ok(contents) => response(id, protocol_result(contents, modern, Some(RESOURCE_TTL_MS))),
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
            protocol_result(json!({"tools":tool_definitions()}), modern, Some(CACHE_TTL_MS)),
        ),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(rpc_error(id, -32602, "Missing tool name"));
            };
            // Unknown tools are Protocol Errors (-32602), never isError
            // results; the valid names ride along so the model can recover.
            if !["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"].contains(&name) {
                return Some(rpc_error(
                    id,
                    -32602,
                    format!(
                        "Unknown tool: {name}. Available tools: ocs_sessions, ocs_read, ocs_execute, ocs_capture"
                    ),
                ));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let called = call_tool(name, &arguments, clients, resources, pump, Some(&id));
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
            response(id, protocol_result(poll_task(task, clients, resources, pump), true, None))
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
    if let Err(_) = std::fs::create_dir_all(&base_dir) {
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
    let Some(key) = params
        .get("requestId")
        .map(|id| id.to_string())
    else {
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
        for line in crate::io::line_read::lines_capped(
            stdin.lock(),
            crate::io::line_read::MAX_LINE_BYTES,
        ) {
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
                        handle_message(
                            message,
                            &mut clients,
                            &mut tasks,
                            &mut resources,
                            &mut pump,
                        )
                    }
                }
                Err(error) => Some(rpc_error(Value::Null, -32700, error)),
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
        if superseded {
            eprintln!("MCP bridge superseded by rebuild; exiting for respawn");
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            names,
            ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"]
        );
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
            assert!(
                tool["inputSchema"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("ocs_session_id"))
            );
        }
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
                .find(|v| v["properties"]["op"]["enum"][0] == op_name)
                .cloned()
                .unwrap_or_else(|| panic!("variant for {op_name} exists"))
        };
        let run_var = find_variant("run");
        assert_eq!(run_var["properties"]["cmd"]["type"], "string");
        assert!(READ_OPS.contains(&"capabilities"));
        assert!(READ_OPS.contains(&"records"));
        assert!(READ_OPS.contains(&"record_schema"));
        assert!(READ_OPS.contains(&"audit"));
        assert!(READ_OPS.contains(&"text_search"));
        assert!(READ_OPS.contains(&"text_audit"));
        assert!(EXECUTE_OPS.contains(&"set_properties"));
        assert!(EXECUTE_OPS.contains(&"save_verified"));
        assert!(EXECUTE_OPS.contains(&"text_replace"));
        assert!(BATCH_STEP_OPS.contains(&"text_replace"));
        let save_var = find_variant("save_verified");
        assert_eq!(
            save_var["properties"]["target_version"]["enum"][0],
            "R14"
        );
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
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);
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
        for name in ["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"] {
            assert!(message.contains(name), "{message}");
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
            assert!(templates.iter().any(|t| t["uriTemplate"]
                .as_str()
                .unwrap()
                .contains("{level}")));
            assert!(templates.iter().any(|t| t["uriTemplate"]
                .as_str()
                .unwrap()
                .contains("{hash}")));
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
                    ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"]
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
        task.arguments =
            json!({"ocs_session_id":"00000000","op":"state","wait_seconds":0});
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
        assert!(guided["error"]
            .as_str()
            .unwrap()
            .contains("ocs_capture"));
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
        assert!(res_arr.iter().any(|r| r["uri"] == "cad://session/sess1/snapshot/latest.png"));

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
        assert_eq!(res_res["structuredContent"]["_spatial"]["unit"], "Millimeters");
        assert_eq!(
            res_res["structuredContent"]["_spatial"]["pixel_to_world_matrix"],
            json!([0.05, 0.0, 0.0, -0.05, -20.0, 15.0])
        );
        let content = res_res["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert!(content[0]["text"].as_str().unwrap().contains("cad://session/s1/snapshot/hash123.png"));
        assert_eq!(content[1]["type"], "resource");
        assert_eq!(content[1]["resource"]["uri"], "cad://session/s1/snapshot/hash123.png");
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
        assert!(both_content.iter().any(|c| c["type"] == "resource"));
        assert!(both_content.iter().any(|c| c["type"] == "image"));
    }

    #[test]
    fn capture_tool_schema_and_diff_metadata_handling() {
        // 1. Verify ocs_capture schema has diff parameters
        let tools = tool_definitions();
        let capture_tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "ocs_capture")
            .expect("ocs_capture tool exists");
        let props = &capture_tool["inputSchema"]["properties"];
        assert_eq!(props["diff"]["type"], "boolean");
        assert_eq!(props["diff"]["default"], false);
        assert_eq!(props["diff_mode"]["type"], "string");
        assert_eq!(props["diff_mode"]["default"], "highlight");
        let diff_modes = props["diff_mode"]["enum"].as_array().unwrap();
        assert!(diff_modes.iter().any(|v| v == "highlight"));
        assert!(diff_modes.iter().any(|v| v == "crop"));
        assert!(diff_modes.iter().any(|v| v == "metadata_only"));
        assert_eq!(props["reset_diff_baseline"]["type"], "boolean");
        assert_eq!(props["delivery"]["type"], "string");

        // 2. Test tool_result with diff metadata
        let diff_capture_res = json!({
            "$resource": {
                "uri": "cad://session/s1/snapshot/diffhash789.png",
                "mimeType": "image/png",
                "hash": "diffhash789",
                "bytes": 512
            },
            "metadata": {
                "width": 800,
                "height": 600,
                "_spatial": {
                    "crs": "CAD_WCS",
                    "unit": "Millimeters",
                    "pixel_to_world_matrix": [0.05, 0.0, 0.0, -0.05, -20.0, 15.0],
                    "world_to_pixel_matrix": [20.0, 0.0, 0.0, -20.0, 400.0, 300.0]
                },
                "diff": {
                    "is_baseline": false,
                    "baseline_hash": "basehash123",
                    "changed": true,
                    "change_percentage": 2.5,
                    "changed_pixels": 12000,
                    "dirty_pixel_bounds": [100, 150, 300, 350],
                    "dirty_world_bounds": [-15.0, -2.5, -5.0, 7.5],
                    "patch_resolution": [201, 201],
                    "mode": "crop"
                }
            }
        });

        let structured = tool_result(diff_capture_res);
        assert_eq!(structured["isError"], false);
        assert_eq!(structured["structuredContent"]["diff"]["changed"], true);
        assert_eq!(structured["structuredContent"]["diff"]["mode"], "crop");
        assert_eq!(
            structured["structuredContent"]["diff"]["dirty_pixel_bounds"],
            json!([100, 150, 300, 350])
        );
        assert_eq!(
            structured["structuredContent"]["diff"]["dirty_world_bounds"],
            json!([-15.0, -2.5, -5.0, 7.5])
        );
        assert_eq!(structured["structuredContent"]["_spatial"]["crs"], "CAD_WCS");

        let content = structured["content"].as_array().unwrap();
        let text_block = content.iter().find(|c| {
            c["type"] == "text"
                && c["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("\"dirty_pixel_bounds\"")
        });
        assert!(text_block.is_some(), "Metadata text block must format diff parameters");
    }

    #[test]
    fn test_pyramid_tiling_protocol_and_resources() {
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
        assert!(man_contents[0]["text"].as_str().unwrap().contains("cad://session/sess1/tile/{level}/{x}/{y}.png"));

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
        assert_eq!(structured["structuredContent"]["pyramid"]["tile_uri"], tile_uri);
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
    fn gui_failures_are_mcp_errors() {
        let result = tool_result(json!({
            "ok":false,
            "status":"failed",
            "code":"stale_state",
            "error":"Refresh state before editing"
        }));
        assert_eq!(result["isError"], true);
        assert!(result.get("structuredContent").is_none());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stale_state"));
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
        assert!(batch_res.warnings[0].contains("batch step 0: unknown parameter 'stray' ignored for op 'run'"));
    }

    #[test]
    fn pre_dispatch_validator_enforces_required_fields_and_rules() {
        // Missing required field
        let missing_cmd = json!({"op": "run", "request_id": "r1"});
        let err = crate::mcp_ops::validate_request(&missing_cmd, false).unwrap_err();
        assert!(err.contains("Missing cmd for run. Example request:"), "{err}");

        // Required-when rule (scale requires factor)
        let scale_missing_factor = json!({
            "op": "entities_transform",
            "request_id": "r2",
            "handles": ["2A"],
            "action": "scale"
        });
        let err2 = crate::mcp_ops::validate_request(&scale_missing_factor, false).unwrap_err();
        assert!(err2.contains("Missing factor for entities_transform. Example request:"), "{err2}");

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
        assert!(err4.contains("operation 'stop' cannot be used in a batch step"), "{err4}");
    }

    #[test]
    fn schema_size_and_defs_structure() {
        let schema = execute_request_schema();
        assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert!(schema["$defs"]["handle"].is_object());
        assert!(schema["$defs"]["point"].is_object());
        assert!(schema["$defs"]["window"].is_object());
        assert!(schema["$defs"]["batch_step"].is_object());
        let json_str = schema.to_string();
        // Generates cleanly without exponential blowup: compact ~15KB to 45KB
        assert!(json_str.len() < 45_000, "schema size is {} bytes", json_str.len());
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
            assert!(synced, "sync_agent_tool_schemas must succeed when directory exists");
            assert!(base_dir.join("ocs_capture.json").exists());
            assert!(base_dir.join("ocs_execute.json").exists());
            assert!(base_dir.join("ocs_read.json").exists());
            assert!(base_dir.join("ocs_sessions.json").exists());
            assert!(base_dir.join("instructions.md").exists());
        }
    }

    #[test]
    fn read_op_tools_returns_current_schemas_and_instructions() {        let mut clients = HashMap::new();
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
        ).expect("ocs_read op: tools must succeed");
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
}
