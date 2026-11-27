#[cfg(unix)]
use super::session::geteuid;
use super::session::GuiClient;
use super::tools::compact_state;
use super::transport::{CallError, iso8601_now};
use crate::mcp_ops::validate_execute_request;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const CACHE_TTL_MS: u64 = 3_600_000;
/// Freshness for resources/*, which vary as sessions open and snapshots
/// land. Tools and discovery are static by comparison, so they keep the hour.
pub(super) const RESOURCE_TTL_MS: u64 = 60_000;
pub(super) const TASK_TTL_MS: u64 = 3_600_000;

pub(super) struct BatchExecution {
    pub(super) id: String,
    pub(super) request: Value,
    pub(super) steps: Vec<Value>,
    pub(super) next: usize,
    pub(super) active: Option<String>,
    pub(super) pending: Option<Value>,
    pub(super) results: Vec<Value>,
    pub(super) changes: Vec<Value>,
    pub(super) state: Option<Value>,
    pub(super) terminal: Option<Value>,
}

pub(super) struct McpTask {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: Value,
    pub(super) created_at: String,
    pub(super) last_updated_at: String,
    pub(super) result: Option<Value>,
    pub(super) error: Option<Value>,
    /// Cooperative cancel acknowledged: polls report `cancelled` and late
    /// completions are discarded (checked before result/error).
    pub(super) cancelled: bool,
}

pub(super) struct StoredTask {
    pub(super) expires_at: Instant,
    pub(super) task: McpTask,
}

#[derive(Default)]
pub(super) struct TaskStore {
    pub(super) tasks: VecDeque<StoredTask>,
}

impl TaskStore {
    pub(super) fn insert(&mut self, task: McpTask) {
        self.insert_at(task, Instant::now());
    }

    pub(super) fn insert_at(&mut self, task: McpTask, now: Instant) {
        self.evict_expired_at(now);
        self.tasks.push_back(StoredTask {
            expires_at: now + Duration::from_millis(TASK_TTL_MS),
            task,
        });
    }

    pub(super) fn get_mut(&mut self, id: &str) -> Option<&mut McpTask> {
        self.get_mut_at(id, Instant::now())
    }

    pub(super) fn get_mut_at(&mut self, id: &str, now: Instant) -> Option<&mut McpTask> {
        self.evict_expired_at(now);
        self.tasks
            .iter_mut()
            .find(|stored| stored.task.id == id)
            .map(|stored| &mut stored.task)
    }

    pub(super) fn evict_expired_at(&mut self, now: Instant) {
        self.tasks.retain(|stored| stored.expires_at > now);
    }
}

#[derive(Clone, Debug)]
pub(super) struct CachedSnapshot {
    pub(super) session_id: String,
    pub(super) hash: String,
    pub(super) created_at: String,
    pub(super) mime_type: String,
    pub(super) data_base64: String,
    pub(super) bytes_len: usize,
    pub(super) metadata: Value,
}

pub(super) fn encode_uri_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn decode_uri_segment(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            let val = u8::from_str_radix(hex, 16).ok()?;
            out.push(val);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Default)]
pub(super) struct ResourceStore {
    pub(super) snapshots: HashMap<String, CachedSnapshot>,
    pub(super) order: VecDeque<String>,
    pub(super) latest: HashMap<String, String>,
    pub(super) tiles: HashMap<String, CachedSnapshot>,
    pub(super) tile_order: VecDeque<String>,
    pub(super) pyramid_manifests: HashMap<String, Value>,
    pub(super) layout_manifests: HashMap<(String, String), Value>,
}

pub(super) const MAX_STORED_SNAPSHOTS: usize = 32;
pub(super) const MAX_STORED_TILES: usize = 64;

impl ResourceStore {
    pub(super) fn insert(
        &mut self,
        session_id: &str,
        hash: &str,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
    ) -> String {
        self.insert_with_latest(session_id, hash, data_base64, bytes_len, metadata, true)
    }

    pub(super) fn insert_with_latest(
        &mut self,
        session_id: &str,
        hash: &str,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
        update_latest: bool,
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
        if update_latest {
            self.latest.insert(session_id.to_string(), uri.clone());
        }
        uri
    }

    pub(super) fn insert_tile_entry(
        &mut self,
        session_id: &str,
        uri: String,
        hash: String,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
    ) -> String {
        let now = iso8601_now();
        let snapshot = CachedSnapshot {
            session_id: session_id.to_string(),
            hash,
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

    pub(super) fn insert_tile(
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
        let hash = format!("tile-{level}-{x}-{y}");
        self.insert_tile_entry(session_id, uri, hash, data_base64, bytes_len, metadata)
    }

    pub(super) fn tile_uri_for(session_id: &str, layout: Option<&str>, level: u32, x: u32, y: u32) -> String {
        match layout.map(str::trim).filter(|s| !s.is_empty()) {
            Some(lay) => {
                let enc = encode_uri_segment(lay);
                format!("cad://session/{session_id}/layout/{enc}/tile/{level}/{x}/{y}.png")
            }
            None => format!("cad://session/{session_id}/tile/{level}/{x}/{y}.png"),
        }
    }

    pub(super) fn manifest_uri_for(session_id: &str, layout: Option<&str>) -> String {
        match layout.map(str::trim).filter(|s| !s.is_empty()) {
            Some(lay) => {
                let enc = encode_uri_segment(lay);
                format!("cad://session/{session_id}/layout/{enc}/pyramid/manifest.json")
            }
            None => format!("cad://session/{session_id}/pyramid/manifest.json"),
        }
    }

    pub(super) fn insert_tile_for_layout(
        &mut self,
        session_id: &str,
        layout: Option<&str>,
        level: u32,
        x: u32,
        y: u32,
        data_base64: String,
        bytes_len: usize,
        metadata: Value,
    ) -> String {
        match layout.map(str::trim).filter(|s| !s.is_empty()) {
            Some(lay) => {
                let enc = encode_uri_segment(lay);
                let uri = Self::tile_uri_for(session_id, Some(lay), level, x, y);
                let hash = format!("tile-{enc}-{level}-{x}-{y}");
                self.insert_tile_entry(session_id, uri, hash, data_base64, bytes_len, metadata)
            }
            None => self.insert_tile(session_id, level, x, y, data_base64, bytes_len, metadata),
        }
    }

    pub(super) fn insert_pyramid_manifest(&mut self, session_id: &str, manifest: Value) -> String {
        let uri = format!("cad://session/{session_id}/pyramid/manifest.json");
        self.pyramid_manifests
            .insert(session_id.to_string(), manifest);
        uri
    }

    pub(super) fn insert_layout_manifest(
        &mut self,
        session_id: &str,
        layout: &str,
        mut manifest: Value,
    ) -> String {
        let enc = encode_uri_segment(layout);
        let uri = Self::manifest_uri_for(session_id, Some(layout));
        if let Some(obj) = manifest.as_object_mut() {
            obj.insert("layout".into(), Value::String(layout.to_string()));
            obj.insert(
                "tile_uri_template".into(),
                Value::String(format!(
                    "cad://session/{session_id}/layout/{enc}/tile/{{level}}/{{x}}/{{y}}.png"
                )),
            );
        }
        self.layout_manifests.insert(
            (session_id.to_string(), layout.to_ascii_lowercase()),
            manifest,
        );
        uri
    }

    pub(super) fn get_layout_manifest(&self, session_id: &str, layout: &str) -> Option<&Value> {
        self.layout_manifests
            .get(&(session_id.to_string(), layout.to_ascii_lowercase()))
    }

    pub(super) fn get_by_uri(&self, uri: &str) -> Option<&CachedSnapshot> {
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

    pub(super) fn list_resources(&self, active_session_ids: &[String]) -> Vec<Value> {
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
            let mut session_layouts: Vec<(&str, &Value)> = self
                .layout_manifests
                .iter()
                .filter_map(|((sess, key_layout), manifest)| {
                    (sess == session_id).then_some((key_layout.as_str(), manifest))
                })
                .collect();
            session_layouts.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (key_layout, manifest) in session_layouts {
                let layout_name = manifest
                    .get("layout")
                    .and_then(Value::as_str)
                    .unwrap_or(key_layout);
                list.push(json!({
                    "uri": Self::manifest_uri_for(session_id, Some(layout_name)),
                    "name": format!("Layout Pyramid Manifest ({session_id}: {layout_name})"),
                    "description": format!("Multiscale DeepZoom pyramidal tiling manifest for session {session_id}, layout {layout_name}."),
                    "mimeType": "application/json"
                }));
            }
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

pub(super) struct TempCaptureFile(pub(super) PathBuf);

impl Drop for TempCaptureFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) fn private_capture_dir() -> Result<PathBuf, String> {
    let base = match crate::config::config_dir() {
        Some(dir) => dir.join("capture-tmp"),
        None => std::env::temp_dir().join(format!("ocs-capture-{}", std::process::id())),
    };
    let dir = base.join(format!("pid-{}", std::process::id()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        if !base.exists() {
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&base);
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|error| error.to_string())?;
        let meta = std::fs::symlink_metadata(&dir).map_err(|error| error.to_string())?;
        // SAFETY: geteuid() takes no pointers and always succeeds.
        let my_uid = unsafe { geteuid() };
        if meta.file_type().is_symlink() || !meta.file_type().is_dir() || meta.uid() != my_uid {
            return Err("private capture directory failed ownership/symlink check".to_string());
        }
        if (meta.mode() & 0o077) != 0 {
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            let verified = std::fs::symlink_metadata(&dir).map_err(|error| error.to_string())?;
            if (verified.mode() & 0o077) != 0 {
                return Err("private capture directory failed permission check".to_string());
            }
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let meta = std::fs::symlink_metadata(&dir).map_err(|error| error.to_string())?;
        if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
            return Err("capture directory must not be a symlink".to_string());
        }
    }
    Ok(dir)
}

pub(super) fn read_private_capture(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err("refusing to read symlinked or non-regular capture output".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid() takes no pointers and always succeeds.
        if meta.uid() != unsafe { geteuid() } {
            return Err("capture output ownership mismatch".to_string());
        }
    }
    std::fs::read(path).map_err(|error| error.to_string())
}

/// `stale_state` on a batch's first step: the GUI refused before running
/// anything. On a later step, earlier steps have already run.
pub(super) fn stale_before_first_step(batch: &BatchExecution, response: &Value) -> bool {
    batch.next == 1 && response["code"] == "stale_state"
}

pub(super) fn batch_step_id(id: &str, step: usize) -> String {
    let hash = id
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("batch-{hash:016x}-{step}")
}

pub(super) fn batch_result(batch: &BatchExecution, status: &str, ok: bool) -> Value {
    let mut result = json!({
        "ok":ok,
        "status":status,
        "request_id":batch.id,
        "completed_steps":batch.next,
        "total_steps":batch.steps.len(),
        "next_step":(batch.next < batch.steps.len()).then_some(batch.next),
        "results":batch.results,
        "changes":batch.changes,
        "state":batch.state
    });
    if let Some(pending) = &batch.pending {
        result["active_step"] = pending.clone();
    }
    result
}

/// Project only the outgoing response; retained batch results stay lossless.
pub(super) fn summarize_batch_response(mut response: Value, session_id: &str) -> Value {
    let mut attention = Vec::new();
    let mut failed_steps = 0;
    if let Some(results) = response["results"].as_array() {
        for (index, step) in results.iter().enumerate() {
            let failed = step["ok"].as_bool() == Some(false)
                || matches!(step["status"].as_str(), Some("failed" | "cancelled"));
            if failed {
                failed_steps += 1;
            }
            let awaiting_input = response["status"] == "waiting_input"
                && index + 1 == results.len()
                && step["status"] == "waiting_input";
            if failed || awaiting_input {
                // Error and interactive payloads may carry recovery guidance.
                attention.push(step.clone());
            } else if step
                .get("warnings")
                .is_some_and(|v| !v.is_null() && v != &json!([]))
            {
                attention.push(json!({
                    "step":step["step"], "op":step["op"],
                    "status":step["status"], "warnings":step["warnings"]
                }));
            }
        }
    }
    let change_count = response["changes"].as_array().map_or(0, Vec::len);
    let request_id = response["request_id"].clone();
    if let Some(object) = response.as_object_mut() {
        object.remove("results");
        object.remove("changes");
        object.insert("change_count".into(), json!(change_count));
        object.insert("failed_steps".into(), json!(failed_steps));
        object.insert("attention".into(), json!(attention));
        object.insert(
            "details".into(),
            json!({
                "tool":"ocs_read", "arguments":{
                    "ocs_session_id":session_id, "op":"batch_result",
                    "parameters":{"request_id":request_id}
                }
            }),
        );
        if let Some(state) = object.get_mut("state") {
            *state = compact_state(state);
        }
    }
    response
}

pub(super) fn cached_batch_result(
    clients: &HashMap<String, GuiClient>,
    session_id: &str,
    request_id: &str,
) -> Result<Value, CallError> {
    // Do not connect, poll, resume or recreate a batch on this read-only path.
    let batch = clients.get(session_id)
        .and_then(|gui| gui.batches.iter().find(|batch| batch.id == request_id))
        .ok_or_else(|| CallError::Tool(
            "Batch result absent or expired in this MCP connection; do not replay a mutation to retrieve it".into()
        ))?;
    let mut result = batch
        .terminal
        .clone()
        .unwrap_or_else(|| batch_result(batch, "running", true));
    let validation = validate_execute_request(&batch.request, "batch")?;
    if !validation.warnings.is_empty() {
        result["warnings"] = json!(validation.warnings);
    }
    Ok(result)
}

pub(super) fn trim_batches(batches: &mut VecDeque<BatchExecution>) {
    while batches.len() > 64 {
        batches.pop_front();
    }
}
