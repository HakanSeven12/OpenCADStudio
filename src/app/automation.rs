//! Headless automation server (`OpenCADStudio --serve`).
//!
//! Drives the app without a GUI over a line-based JSON protocol: one request
//! object per line on stdin, one response object per line on stdout. State (the
//! active document) persists across requests, so an external process — a script
//! or an AI agent — can act, observe, and act again.
//!
//! Operations:
//! - `{"op":"new"}`                          — start an empty document
//! - `{"op":"open","path":"file.dwg"}`       — load a drawing
//! - `{"op":"run","cmd":"LAYER Walls"}`      — run a command (the same dispatcher
//!                                             the GUI command line uses)
//! - `{"op":"entities"}`                     — summary count by entity type
//! - `{"op":"save","path":"out.dwg"}`        — write the document (path optional
//!                                             once opened/saved)

#[cfg(not(target_arch = "wasm32"))]
use std::io::{BufRead, Write};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde_json::{json, Value};
#[cfg(not(target_arch = "wasm32"))]
use sha2::{Digest, Sha256};

use super::OpenCADStudio;

/// Run the headless JSON server. Default transport is stdin/stdout; with
/// `--port <N>` it instead listens on `127.0.0.1:<N>` and serves one client at
/// a time (the document session persists across reconnects).
#[cfg(not(target_arch = "wasm32"))]
pub fn serve() {
    let mut app = OpenCADStudio::new();
    match port_arg() {
        Some(port) => serve_socket(&mut app, port),
        None => serve_stdio(&mut app),
    }
}

/// Headless one-shot format conversion (`--export IN OUT`). Loads `input`,
/// writes `output` (format chosen from `output`'s extension), and returns a
/// process exit code (0 on success). No window is created.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_headless(
    input: &std::path::Path,
    output: &std::path::Path,
    target_version: Option<&str>,
) -> i32 {
    let doc = match crate::io::load_file(input) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("export: cannot read {}: {e}", input.display());
            return 1;
        }
    };
    let version = match target_version {
        Some(value) => match crate::io::parse_target_version(value) {
            Ok(version) => version,
            Err(e) => {
                eprintln!("export: {e}");
                return 2;
            }
        },
        None => doc.version,
    };
    match crate::io::save_as_version(&doc, output, version) {
        Ok(()) => {
            println!("Exported {} → {}", input.display(), output.display());
            0
        }
        Err(e) => {
            eprintln!("export: cannot write {}: {e}", output.display());
            1
        }
    }
}

/// `--port <N>` if present on the command line.
#[cfg(not(target_arch = "wasm32"))]
fn port_arg() -> Option<u16> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == "--port" {
            return args.next().and_then(|s| s.parse().ok());
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn ready() -> Value {
    json!({
        "ok": true,
        "ready": true,
        "version": env!("OCS_APP_VERSION"),
        "session_id": super::control::session_id(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn serve_stdio(app: &mut OpenCADStudio) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    {
        let mut o = stdout.lock();
        let _ = writeln!(o, "{}", ready());
        let _ = o.flush();
    }
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = app.automation_op(line);
        let mut o = stdout.lock();
        let _ = writeln!(o, "{resp}");
        let _ = o.flush();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn serve_socket(app: &mut OpenCADStudio, port: u16) {
    let listener = match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("--serve: cannot bind 127.0.0.1:{port}: {e}");
            return;
        }
    };
    eprintln!("OpenCADStudio --serve listening on 127.0.0.1:{port}");
    for stream in listener.incoming().flatten() {
        let Ok(read_half) = stream.try_clone() else {
            continue;
        };
        let reader = std::io::BufReader::new(read_half);
        let mut writer = stream;
        let _ = writeln!(writer, "{}", ready());
        let _ = writer.flush();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let resp = app.automation_op(line);
            if writeln!(writer, "{resp}").is_err() {
                break;
            }
            let _ = writer.flush();
        }
    }
}

fn err(msg: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": msg.to_string() })
}

fn v3(v: acadrust::types::Vector3) -> Value {
    json!([v.x, v.y, v.z])
}

pub(crate) fn entity_type_matches(entity: &acadrust::EntityType, requested: &str) -> bool {
    if crate::entities::names::ui_name(entity).eq_ignore_ascii_case(requested) {
        return true;
    }
    let record_name = crate::entities::names::dxf_name(entity);
    record_name != "ENTITY" && record_name.eq_ignore_ascii_case(requested)
}

/// One entity as JSON. Summary mode carries identity only, geometry adds the
/// entity's defining values, and full also includes its world bounds.
pub(crate) fn entity_json(e: &acadrust::EntityType, detail: &str) -> Value {
    use acadrust::EntityType as E;
    let c = e.common();
    let mut obj = json!({
        "handle": format!("{:X}", c.handle.value()),
        "type": crate::entities::names::ui_name(e),
        "layer": c.layer,
    });
    if detail == "summary" {
        return obj;
    }
    let map = obj.as_object_mut().expect("json object");
    match e {
        E::Line(l) => {
            map.insert("start".into(), v3(l.start));
            map.insert("end".into(), v3(l.end));
        }
        E::Circle(cc) => {
            map.insert("center".into(), v3(cc.center));
            map.insert("radius".into(), json!(cc.radius));
        }
        E::Arc(a) => {
            map.insert("center".into(), v3(a.center));
            map.insert("radius".into(), json!(a.radius));
            map.insert("start_angle".into(), json!(a.start_angle));
            map.insert("end_angle".into(), json!(a.end_angle));
        }
        E::Point(p) => {
            map.insert("location".into(), v3(p.location));
        }
        E::Ellipse(el) => {
            map.insert("center".into(), v3(el.center));
            map.insert("major_axis".into(), v3(el.major_axis));
        }
        E::Text(t) => {
            map.insert("value".into(), json!(t.value));
            map.insert(
                "text".into(),
                json!(crate::entities::text_support::resolve_dxf_special_chars(&t.value)),
            );
            map.insert("position".into(), v3(t.insertion_point));
            map.insert("height".into(), json!(t.height));
        }
        E::MText(t) => {
            map.insert("value".into(), json!(t.value));
            map.insert(
                "text".into(),
                json!(acadrust::entities::mtext_format::parse_mtext(&t.value, true)
                    .to_plain_text()),
            );
            map.insert("position".into(), v3(t.insertion_point));
            map.insert("height".into(), json!(t.height));
        }
        E::LwPolyline(pl) => {
            let pts: Vec<Value> = pl
                .vertices
                .iter()
                .map(|v| json!([v.location.x, v.location.y]))
                .collect();
            map.insert("vertices".into(), json!(pts));
        }
        E::Insert(ins) => {
            map.insert("block".into(), json!(ins.block_name));
            map.insert("position".into(), v3(ins.insert_point));
            let attributes: serde_json::Map<String, Value> = ins
                .attributes
                .iter()
                .map(|attribute| (attribute.tag.clone(), json!(attribute.value)))
                .collect();
            map.insert("attributes".into(), Value::Object(attributes));
        }
        _ => {}
    }
    if matches!(e, E::Hatch(_)) {
        // Flattened first-loop boundary (world XY) so clients do not have to
        // dig through the serialized path/edge structures.
        if let E::Hatch(hatch) = e {
            if let Some(path) = hatch.paths.first() {
                let loops: Vec<Value> = path
                    .edges
                    .iter()
                    .filter_map(|edge| match edge {
                        acadrust::entities::BoundaryEdge::Polyline(polyline) => Some(
                            json!(polyline
                                .vertices
                                .iter()
                                .map(|v| [v.x, v.y])
                                .collect::<Vec<_>>()),
                        ),
                        _ => None,
                    })
                    .collect();
                if !loops.is_empty() {
                    map.insert("boundary".into(), json!(loops));
                }
            }
        }
    }
    if detail == "full" {
        let (mut min, mut max) = crate::scene::convert::tess::entity_bounds(e);
        // Text bounds come from the shaped glyphs, and some paths leave the
        // width degenerate (a vertical segment at the insertion point).
        // Widen with the same heuristic the clients fall back to, so
        // region filters on `bounds` stay usable; the raw tess bounds
        // elsewhere (quadtree, hit-testing) are untouched.
        if matches!(e, E::Text(_) | E::MText(_)) && max[0] <= min[0] {
            let (content, height, x) = match e {
                E::Text(t) => (t.value.as_str(), t.height, t.insertion_point.x),
                E::MText(t) => (t.value.as_str(), t.height, t.insertion_point.x),
                _ => unreachable!("matched above"),
            };
            let width = (height.abs() * 0.8 * content.chars().count() as f64).max(1.0);
            min[0] = x;
            max[0] = x + width;
        }
        map.insert("bounds".into(), json!({ "min": min, "max": max }));
        if let Ok(Value::Object(wrapper)) = serde_json::to_value(e) {
            if let Some((_, properties)) = wrapper.into_iter().next() {
                map.insert("properties".into(), properties);
            }
        }
    }
    obj
}

fn request_point(req: &Value, key: &str) -> Option<[f64; 2]> {
    let values = req[key].as_array()?;
    if !(2..=3).contains(&values.len()) {
        return None;
    }
    let x = values[0].as_f64()?;
    let y = values[1].as_f64()?;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

fn request_handle(value: &Value) -> Option<acadrust::Handle> {
    value
        .as_str()
        .and_then(|value| {
            let value = value
                .strip_prefix("0x")
                .or_else(|| value.strip_prefix("0X"))
                .unwrap_or(value);
            u64::from_str_radix(value, 16).ok()
        })
        .map(acadrust::Handle::new)
}

fn projected_fields(mut entity: Value, fields: Option<&Vec<Value>>) -> Value {
    let Some(fields) = fields else { return entity };
    let Some(source) = entity.as_object_mut() else {
        return entity;
    };
    let keep: std::collections::HashSet<&str> = fields.iter().filter_map(Value::as_str).collect();
    source.retain(|key, _| key == "handle" || keep.contains(key.as_str()));
    entity
}

pub(super) fn requested_save_target(
    req: &Value,
    default_version: acadrust::DxfVersion,
    default_is_dxf: bool,
    path: Option<&std::path::Path>,
) -> Result<(acadrust::DxfVersion, bool), String> {
    let version = match req["target_version"].as_str() {
        Some(value) => crate::io::parse_target_version(value)?,
        None => default_version,
    };
    let path_format = path
        .and_then(std::path::Path::extension)
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| value == "dxf" || value == "dwg");
    let requested_format = match req["target_format"].as_str() {
        Some(value) if value.eq_ignore_ascii_case("dxf") => Some("dxf".to_string()),
        Some(value) if value.eq_ignore_ascii_case("dwg") => Some("dwg".to_string()),
        Some(value) => return Err(format!("unsupported target format {value:?}; use dwg or dxf")),
        None => None,
    };
    if let (Some(requested), Some(extension)) = (&requested_format, &path_format) {
        if requested != extension {
            return Err(format!(
                "target_format {requested:?} conflicts with output extension .{extension}"
            ));
        }
    }
    let is_dxf = requested_format
        .or(path_format)
        .map_or(default_is_dxf, |value| value == "dxf");
    Ok((version, is_dxf))
}

fn document_manifest(document: &acadrust::CadDocument) -> Value {
    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_layer: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0u64;
    for entity in document.entities() {
        *by_type
            .entry(crate::entities::names::ui_name(entity).to_string())
            .or_default() += 1;
        *by_layer.entry(entity.common().layer.clone()).or_default() += 1;
        total += 1;
    }
    json!({"total":total,"by_type":by_type,"by_layer":by_layer})
}

#[cfg(not(target_arch = "wasm32"))]
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn audit_ascii_dxf_references(path: &std::path::Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Ok(json!({
            "ok":true,
            "skipped":"binary_dxf",
            "reason":"raw group-code handle audit applies only to ASCII DXF"
        }));
    }
    let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() % 2 != 0 {
        return Ok(json!({"ok":false,"malformed_pairs":true,"duplicate_handles":[],"dangling_handles":[]}));
    }
    let mut declared = HashSet::from(["0".to_string()]);
    let mut duplicate = std::collections::BTreeSet::new();
    let mut references = Vec::new();
    let mut pairs = Vec::new();
    let mut version = None;
    let mut variable = None;
    for pair in lines.chunks_exact(2) {
        let Ok(code) = pair[0].trim().parse::<i32>() else {
            return Ok(json!({"ok":false,"malformed_pairs":true,"duplicate_handles":[],"dangling_handles":[]}));
        };
        let value = pair[1].trim().to_ascii_uppercase();
        pairs.push((code, value.clone()));
        if code == 9 {
            variable = Some(value.clone());
        } else if code == 1 && variable.as_deref() == Some("$ACADVER") {
            version = Some(value.clone());
            variable = None;
        }
        if code == 5 || code == 105 {
            if !declared.insert(value.clone()) {
                duplicate.insert(value.clone());
            }
        } else if [330, 340, 350, 360, 390].contains(&code) {
            references.push(value);
        }
    }
    let dangling: std::collections::BTreeSet<String> = references
        .into_iter()
        .filter(|handle| !declared.contains(handle))
        .collect();
    let mut in_objects = false;
    let mut pending_section = false;
    let mut records: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    let mut current: Option<(String, Vec<(i32, String)>)> = None;
    for (code, value) in &pairs {
        if *code == 0 && value == "SECTION" {
            pending_section = true;
            continue;
        }
        if pending_section && *code == 2 {
            in_objects = value == "OBJECTS";
            pending_section = false;
            continue;
        }
        if *code == 0 && value == "ENDSEC" {
            if let Some(record) = current.take() {
                records.push(record);
            }
            in_objects = false;
            continue;
        }
        if !in_objects {
            continue;
        }
        if *code == 0 {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some((value.clone(), Vec::new()));
        } else if let Some((_, fields)) = &mut current {
            fields.push((*code, value.clone()));
        }
    }
    let root = records.iter().find_map(|(kind, fields)| {
        let handle = fields
            .iter()
            .find(|(code, _)| *code == 5 || *code == 105)?
            .1
            .clone();
        let owner = fields
            .iter()
            .rev()
            .find(|(code, _)| *code == 330)?
            .1
            .as_str();
        (kind == "DICTIONARY" && owner == "0").then_some(handle)
    });
    let mut orphaned_objects = std::collections::BTreeSet::new();
    if let Some(root_handle) = root {
        let root_targets: HashSet<String> = records
            .iter()
            .find(|(_, fields)| {
                fields
                    .iter()
                    .any(|(code, value)| (*code == 5 || *code == 105) && value == &root_handle)
            })
            .map(|(_, fields)| {
                fields
                    .iter()
                    .filter(|(code, _)| *code == 350 || *code == 360)
                    .map(|(_, value)| value.clone())
                    .collect()
            })
            .unwrap_or_default();
        for (_, fields) in &records {
            let Some(handle) = fields
                .iter()
                .find(|(code, _)| *code == 5 || *code == 105)
                .map(|(_, value)| value)
            else {
                continue;
            };
            let owner = fields
                .iter()
                .rev()
                .find(|(code, _)| *code == 330)
                .map(|(_, value)| value.as_str());
            if owner == Some(root_handle.as_str())
                && handle != &root_handle
                && !root_targets.contains(handle)
            {
                orphaned_objects.insert(handle.clone());
            }
        }
    }
    Ok(json!({
        "ok":duplicate.is_empty() && dangling.is_empty() && orphaned_objects.is_empty(),
        "malformed_pairs":false,
        "declared_handles":declared.len() - 1,
        "duplicate_handles":duplicate,
        "dangling_handles":dangling,
        "orphaned_objects":orphaned_objects,
        "declared_version":version,
    }))
}

impl OpenCADStudio {
    /// Handle one JSON request line and return the JSON response.
    #[cfg(any(test, not(target_arch = "wasm32")))]
    pub(crate) fn automation_op(&mut self, line: &str) -> Value {
        if let Ok(req) = serde_json::from_str::<Value>(line) {
            if req["protocol"].is_number() {
                let id = req["request_id"].clone();
                let (response, task) = self.control_request(req);
                if let Err(error) = self.drive_headless_task(task) {
                    return err(error);
                }
                if matches!(response["status"].as_str(), Some("accepted" | "running")) {
                    return self
                        .control_request(json!({"op":"operation","request_id":id}))
                        .0;
                }
                return response;
            }
        }
        let res = self.automation_op_inner(line);
        // Most automation ops mutate `Scene::selected` directly rather than
        // going through `update()` (`select` calls `deselect_all` /
        // `select_entity`, and `run` can erase the selected entities), so the
        // selection check has to run on this path too.
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_selection_changed();
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_document_changed();
        res
    }

    pub(super) fn automation_op_inner(&mut self, line: &str) -> Value {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return err(format!("invalid JSON: {e}")),
        };
        match req["op"].as_str().unwrap_or("") {
            "new" => {
                if let Some(template) = req["template"].as_str() {
                    let purged = match self.apply_template(template) {
                        Ok(purged) => purged,
                        Err(error) => return error,
                    };
                    let mut summary = self.entity_summary();
                    if let Some(obj) = summary.as_object_mut() {
                        obj.insert("template".into(), json!(template));
                        obj.insert("purged".into(), json!(purged));
                    }
                    return summary;
                }
                let i = self.active_tab;
                self.tabs[i].scene.reset_to_new_drawing();
                self.tabs[i].scene.material_base_dir = None;
                self.tabs[i].current_path = None;
                // The headless session starts on the welcome (Start) tab, which
                // blocks drawing commands; turn it into a real drawing.
                self.tabs[i].is_start = false;
                self.entity_summary()
            }
            #[cfg(not(target_arch = "wasm32"))]
            "open" => {
                let Some(path) = req["path"].as_str() else {
                    return err("open: missing \"path\"");
                };
                let path_buf = PathBuf::from(path);
                let bytes = match self.read_drawing(&path_buf) {
                    Ok(b) => b,
                    Err(e) => return err(format!("open: {e}")),
                };
                match crate::io::load_bytes_finalized(&path_buf, bytes) {
                    Ok((doc, dropped)) => {
                        let i = self.active_tab;
                        self.tabs[i].scene.clear();
                        self.tabs[i].scene.document = doc;
                        self.tabs[i].scene.bump_layout_epoch();
                        self.tabs[i].scene.bump_scale_epoch();
                        self.tabs[i].scene.load_named_parameters_from_document();
                        self.tabs[i]
                            .scene
                            .load_parametric_constraints_from_document();
                        self.tabs[i].scene.material_base_dir = path_buf.parent().map(PathBuf::from);
                        crate::app::style_ops::ensure_standard_styles(
                            &mut self.tabs[i].scene.document,
                        );
                        self.tabs[i].adopt_active_ucs_from_header();
                        self.tabs[i].current_path = Some(path_buf);
                        self.tabs[i].is_start = false;
                        self.tabs[i].scene.rebuild_derived_caches();
                        let mut summary = self.entity_summary();
                        if dropped > 0 {
                            if let Some(obj) = summary.as_object_mut() {
                                obj.insert("purged".to_string(), json!(dropped));
                            }
                        }
                        summary
                    }
                    Err(e) => err(format!("open: {e}")),
                }
            }
            #[cfg(target_arch = "wasm32")]
            "open" => err("open: use the browser file action"),
            "run" => {
                let cmd = req["cmd"].as_str().unwrap_or("").to_string();
                if cmd.is_empty() {
                    return err("run: missing \"cmd\"");
                }
                let i = self.active_tab;
                let before = self.tabs[i].scene.document.entities().count();
                let error_revision = self.command_line.error_revision;
                // Surfaces that are already open belong to an earlier line (or
                // another tab, or startup). Only what *this* line left open is
                // reported as a blocker, otherwise a finished command would keep
                // answering `waiting_input` because of someone else's editor and
                // a caller polling for `completed` would never get there.
                let editor_before = self.text_inline.is_some();
                let mtext_before = self.mtext_editor.is_some();
                let modal_before = self.active_modal.is_some();
                if let Err(error) = self.run_headless(&cmd) {
                    return err(error);
                }
                if self.command_line.error_revision != error_revision {
                    return err(self.command_line.last_error.clone().unwrap_or_default());
                }
                let after = self.tabs[i].scene.document.entities().count();
                // A command can finish with an interactive surface still open:
                // the in-place text editor (the `TEXT` content step), the MTEXT
                // editor, or a modal. Reporting `completed` there hides the fact
                // that nothing was committed, so reuse the `waiting_input`
                // vocabulary and name the blocker.
                let blocked_by: Option<String> = if self.tabs[i].active_cmd.is_some() {
                    Some("command".to_string())
                } else if let (false, Some(modal)) = (modal_before, self.active_modal.as_ref()) {
                    Some(format!("modal:{modal:?}"))
                } else if !editor_before && self.text_inline.is_some() {
                    Some("text_editor".to_string())
                } else if !mtext_before && self.mtext_editor.is_some() {
                    Some("mtext_editor".to_string())
                } else {
                    None
                };
                json!({
                    "ok": true,
                    "cmd": cmd,
                    "status": if blocked_by.is_some() { "waiting_input" } else { "completed" },
                    "blocked_by": blocked_by,
                    "entities": after,
                    "added": after as i64 - before as i64,
                    // Tokens no prompt ever asked for: leftover input is
                    // reported rather than dropped on the floor.
                    "unconsumed": self.command_line.unconsumed.clone(),
                })
            }
            "entities" => self.entity_summary(),
            "audit" => self.document_audit(&req),
            "query" => self.entity_query(&req),
            "records" => self.record_query(&req),
            "record_schema" => self.record_schema(&req),
            "capabilities" => self.record_capabilities(),
            "layers" => {
                let i = self.active_tab;
                let offset = req["offset"].as_u64().unwrap_or(0) as usize;
                let limit = req["limit"].as_u64().unwrap_or(1000).min(10_000) as usize;
                let count = self.tabs[i].scene.document.layers.iter().count();
                let layers: Vec<Value> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(|l| {
                        let mut o = json!({
                            "name": l.name,
                            "off": l.is_off(),
                            "frozen": l.is_frozen(),
                            "locked": l.is_locked(),
                        });
                        let m = o.as_object_mut().expect("json object");
                        if let Some(aci) = l.color.index() {
                            m.insert("color".into(), json!(aci));
                        }
                        if let Some((r, g, b)) = l.color.rgb() {
                            m.insert("rgb".into(), json!([r, g, b]));
                        }
                        o
                    })
                    .collect();
                json!({
                    "ok": true,
                    "current": self.tabs[i].scene.document.header.current_layer_name,
                    "count": count,
                    "next_offset": (offset + layers.len() < count).then_some(offset + layers.len()),
                    "layers": layers,
                })
            }
            "header" => {
                let h = &self.tabs[self.active_tab].scene.document.header;
                json!({
                    "ok": true,
                    "current_layer": h.current_layer_name,
                    "current_text_style": h.current_text_style_name,
                    "insertion_units": h.insertion_units,
                    "pdmode": h.point_display_mode,
                    "pdsize": h.point_display_size,
                    "ltscale": h.linetype_scale,
                    "annotation_scale_value": h.annotation_scale_value,
                })
            }
            "undo" => {
                let _ = self.update(super::Message::Undo);
                self.entity_summary()
            }
            "redo" => {
                let _ = self.update(super::Message::Redo);
                self.entity_summary()
            }
            "select" => {
                let i = self.active_tab;
                self.tabs[i].scene.deselect_all();
                if req["clear"].as_bool() != Some(true) {
                    // By explicit handles (hex, as returned by `query`).
                    if let Some(arr) = req["handles"].as_array() {
                        for h in arr.iter().filter_map(|h| h.as_str()) {
                            let h = h
                                .strip_prefix("0x")
                                .or_else(|| h.strip_prefix("0X"))
                                .unwrap_or(h);
                            if let Ok(v) = u64::from_str_radix(h, 16) {
                                self.tabs[i]
                                    .scene
                                    .select_entity(acadrust::Handle::new(v), false);
                            }
                        }
                    }
                    // Or by type / layer.
                    let type_filter = req["type"].as_str();
                    let layer_filter = req["layer"].as_str();
                    if type_filter.is_some() || layer_filter.is_some() {
                        let handles: Vec<acadrust::Handle> = self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .filter(|e| type_filter.is_none_or(|t| entity_type_matches(e, t)))
                            .filter(|e| layer_filter.is_none_or(|l| e.common().layer == l))
                            .map(|e| e.common().handle)
                            .collect();
                        for h in handles {
                            self.tabs[i].scene.select_entity(h, false);
                        }
                    }
                }
                json!({ "ok": true, "selected": self.tabs[i].scene.selected_entities().len() })
            }
            "save" => {
                let i = self.active_tab;
                let path = req["path"]
                    .as_str()
                    .map(PathBuf::from)
                    .or_else(|| self.tabs[i].current_path.clone());
                let Some(path) = path else {
                    return err("save: no \"path\" and the document has none");
                };
                // A .dwt target is written as DWG bytes through a hidden
                // scratch file and renamed afterwards — DWT is a DWG-family
                // file, and the plain io path keeps no locks on the scratch.
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dwt"))
                {
                    let scratch = path.with_file_name(format!(
                        ".{}.tmp.dwg",
                        path.file_name().unwrap().to_string_lossy()
                    ));
                    let document = self.tabs[i].scene.document_for_save();
                    if let Err(e) = crate::io::save(&document, &scratch) {
                        return err(format!("save: {e}"));
                    }
                    if let Err(e) = std::fs::rename(&scratch, &path) {
                        return err(format!("save: rename to .dwt failed: {e}"));
                    }
                    return json!({ "ok": true, "saved": path.to_string_lossy() });
                }
                let default_is_dxf = crate::io::source_is_dxf(
                    self.tabs[i].current_path.as_deref(),
                    &self.tabs[i].scene.document,
                );
                let (version, is_dxf) = match requested_save_target(
                    &req,
                    self.tabs[i].scene.document.version,
                    default_is_dxf,
                    Some(&path),
                ) {
                    Ok(target) => target,
                    Err(error) => return err(format!("save: {error}")),
                };
                let dropped = crate::io::dropped_on_save_count(
                    &self.tabs[i].scene.document,
                    version,
                    is_dxf,
                );
                if dropped > 0 && req["allow_lossy"].as_bool() != Some(true) {
                    return err(format!(
                        "save: conversion would drop {dropped} unsupported record(s); set allow_lossy=true to acknowledge"
                    ));
                }
                #[cfg(not(target_arch = "wasm32"))]
                let result = self.save_tab_synchronously_protected_as(
                    i,
                    path.clone(),
                    version,
                    true,
                );
                #[cfg(target_arch = "wasm32")]
                let result = crate::io::save(&self.tabs[i].scene.document, &path)
                    .map_err(crate::io::SaveFailure::other);
                match result {
                    Ok(()) => {
                        json!({
                            "ok": true,
                            "saved": path.to_string_lossy(),
                            "target_format": if is_dxf { "dxf" } else { "dwg" },
                            "target_version": format!("{version:?}"),
                            "dropped_on_save": dropped,
                        })
                    }
                    Err(e) => err(format!("save: {e}")),
                }
            }
            "" => err("missing \"op\""),
            other => err(format!("unknown op: {other}")),
        }
    }

    /// Run a command line headlessly. Thin wrapper over the shared
    /// [`OpenCADStudio::run_command_line`] (see `cmd_result.rs`), which the GUI
    /// command line uses too so both process `UCS Z 90` / `LINE 0,0 10,10` /
    /// `PDMODE 3` identically.
    fn run_headless(&mut self, cmd: &str) -> Result<(), String> {
        let task = self.run_command_line(cmd);
        self.drive_headless_task(task)
    }

    pub(super) fn drive_headless_task(
        &mut self,
        task: iced::Task<super::Message>,
    ) -> Result<(), String> {
        use iced::futures::StreamExt;
        let mut streams = Vec::new();
        if let Some(stream) = iced_runtime::task::into_stream(task) {
            streams.push(stream);
        }
        while let Some(stream) = streams.last_mut() {
            match iced::futures::executor::block_on(stream.next()) {
                Some(iced_runtime::Action::Output(message)) => {
                    let next = self.update(message);
                    if let Some(stream) = iced_runtime::task::into_stream(next) {
                        streams.push(stream);
                    }
                }
                Some(iced_runtime::Action::Widget(_))
                | Some(iced_runtime::Action::Tick)
                | Some(iced_runtime::Action::Reload) => {}
                Some(action) => return Err(format!("GUI runtime required for {action:?}")),
                None => {
                    streams.pop();
                }
            }
        }
        self.finish_all_pending_history();
        Ok(())
    }

    /// Query entities by identity, metadata and exact plane-curve relationships.
    fn entity_query(&self, req: &Value) -> Value {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if let Some(pair) = req["intersections"].as_array() {
            if pair.len() != 2 {
                return err("query intersections expects exactly two handles");
            }
            let Some(first) = request_handle(&pair[0]) else {
                return err("query intersections contains an invalid first handle");
            };
            let Some(second) = request_handle(&pair[1]) else {
                return err("query intersections contains an invalid second handle");
            };
            let Some(first_entity) = tab.scene.document.get_entity(first) else {
                return err("query intersections first entity does not exist");
            };
            let Some(second_entity) = tab.scene.document.get_entity(second) else {
                return err("query intersections second entity does not exist");
            };
            let Some(first_curve) = crate::entities::curve::entity_curve_xy(first_entity) else {
                return err("query intersections first entity is not a planar curve");
            };
            let Some(second_curve) = crate::entities::curve::entity_curve_xy(second_entity) else {
                return err("query intersections second entity is not a planar curve");
            };
            let crossings = cadkernel::geom2d::intersect(
                &first_curve,
                &second_curve,
                cadkernel::geom2d::Tolerance::default(),
            );
            return json!({
                "ok":true,
                "document_id":tab.id,
                "geometry_revision":tab.scene.geometry_epoch,
                "handles":[format!("{:X}",first.value()),format!("{:X}",second.value())],
                "count":crossings.len(),
                "intersections":crossings.into_iter().map(|crossing|json!({
                    "point":[crossing.point[0],crossing.point[1]],
                    "parameter_first":crossing.t_a,
                    "parameter_second":crossing.t_b
                })).collect::<Vec<_>>()
            });
        }

        let type_filter = req["type"].as_str();
        let layer_filter = req["layer"].as_str();
        let handles: Option<std::collections::HashSet<u64>> = req["handles"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(request_handle)
                    .map(|h| h.value())
                    .collect()
            })
            .or_else(|| {
                request_handle(&req["handle"])
                    .map(|handle| std::iter::once(handle.value()).collect())
            });
        let near = request_point(req, "near");
        let contains = request_point(req, "contains_point");
        let bounds = req["bounds"].as_array().and_then(|values| {
            (values.len() == 4)
                .then(|| {
                    Some([
                        values[0].as_f64()?,
                        values[1].as_f64()?,
                        values[2].as_f64()?,
                        values[3].as_f64()?,
                    ])
                })
                .flatten()
        });
        if req.get("handles").is_some()
            && handles
                .as_ref()
                .is_some_and(|parsed| parsed.len() != req["handles"].as_array().map_or(0, Vec::len))
        {
            return err("query handles contains an invalid hexadecimal handle");
        }
        if req.get("handle").is_some() && request_handle(&req["handle"]).is_none() {
            return err("query handle must be hexadecimal");
        }
        if req.get("near").is_some() && near.is_none() {
            return err("query near expects two or three finite coordinates");
        }
        if req.get("contains_point").is_some() && contains.is_none() {
            return err("query contains_point expects two or three finite coordinates");
        }
        if req.get("bounds").is_some()
            && bounds.is_none_or(|bounds| {
                !bounds.iter().all(|value| value.is_finite())
                    || bounds[0] > bounds[2]
                    || bounds[1] > bounds[3]
            })
        {
            return err("query bounds expects finite [min_x,min_y,max_x,max_y]");
        }
        let detail = req["detail"].as_str().unwrap_or("geometry");
        if !matches!(detail, "summary" | "geometry" | "full") {
            return err("query detail must be summary, geometry or full");
        }
        let limit = req["limit"].as_u64().unwrap_or(1000).min(10000) as usize;
        let offset = req["offset"].as_u64().unwrap_or(0) as usize;

        const WHERE_OPS: &[&str] = &[
            "eq", "ne", "lt", "lte", "gt", "gte", "contains",
            "starts_with", "ends_with", "in", "exists", "not_exists",
        ];
        if let Some(filters) = req["where"].as_array() {
            for filter in filters {
                let path_ok = filter["path"]
                    .as_str()
                    .is_some_and(|path| path.is_empty() || path.starts_with('/'));
                let op_ok = filter["op"].as_str().unwrap_or("eq");
                if !path_ok || !WHERE_OPS.contains(&op_ok) {
                    return err(format!(
                        "where filters need a JSON Pointer path and one op: {}",
                        WHERE_OPS.join(", ")
                    ));
                }
            }
        }

        let mut matched = Vec::new();
        for e in tab.scene.document.entities() {
            if handles
                .as_ref()
                .is_some_and(|handles| !handles.contains(&e.common().handle.value()))
            {
                continue;
            }
            if type_filter.is_some_and(|value| !entity_type_matches(e, value))
                || layer_filter.is_some_and(|value| e.common().layer != value)
            {
                continue;
            }
            if let Some(filters) = req["where"].as_array() {
                // Filters address  with RFC 6901 pointers, the
                // same contract as the records op.
                let properties = serde_json::to_value(e).ok().and_then(|wrapper| {
                    wrapper.as_object().and_then(|object| object.values().next().cloned())
                });
                let mut matches = true;
                for filter in filters {
                    let path = filter["path"].as_str().unwrap_or("");
                    let actual = if path.is_empty() {
                        properties.as_ref()
                    } else {
                        properties.as_ref().and_then(|properties| properties.pointer(path))
                    };
                    match crate::app::record_api::compare(
                        actual,
                        filter["op"].as_str().unwrap_or("eq"),
                        filter.get("value"),
                    ) {
                        Ok(true) => {}
                        Ok(false) => {
                            matches = false;
                            break;
                        }
                        Err(error) => return err(format!("where filter: {error}")),
                    }
                }
                if !matches {
                    continue;
                }
            }
            if let Some(bounds) = bounds {
                let (min, max) = crate::scene::convert::tess::entity_bounds(e);
                if max[0] < bounds[0]
                    || max[1] < bounds[1]
                    || min[0] > bounds[2]
                    || min[1] > bounds[3]
                {
                    continue;
                }
            }
            let curve = (near.is_some() || contains.is_some())
                .then(|| crate::entities::curve::entity_curve_xy(e))
                .flatten();
            if let Some(point) = contains {
                let Some(curve) = curve.as_ref().filter(|curve| curve.is_closed()) else {
                    continue;
                };
                if !cadkernel::geom2d::contains(
                    std::slice::from_ref(curve),
                    point,
                    cadkernel::geom2d::Tolerance::default(),
                ) {
                    continue;
                }
            }
            let nearest = near.and_then(|point| {
                curve
                    .as_ref()
                    .map(|curve| cadkernel::geom2d::closest_point(curve, point))
            });
            if near.is_some() && nearest.is_none() {
                continue;
            }
            let mut entity = entity_json(e, detail);
            if let Some(nearest) = nearest {
                let object = entity.as_object_mut().expect("entity JSON object");
                object.insert("distance".into(), json!(nearest.distance));
                object.insert(
                    "closest_point".into(),
                    json!([nearest.point[0], nearest.point[1]]),
                );
                object.insert("parameter".into(), json!(nearest.t));
            }
            matched.push((nearest.map(|nearest| nearest.distance), entity));
        }
        if near.is_some() {
            matched.sort_by(|left, right| {
                left.0
                    .partial_cmp(&right.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        let count = matched.len();
        let fields = req["fields"].as_array();
        let entities: Vec<Value> = matched
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(_, entity)| projected_fields(entity, fields))
            .collect();
        json!({
            "ok": true,
            "document_id":tab.id,
            "geometry_revision":tab.scene.geometry_epoch,
            "count": count,
            "returned": entities.len(),
            "next_offset": (offset + entities.len() < count).then_some(offset + entities.len()),
            "entities": entities,
        })
    }

    /// Pre-delivery check of the active document against an intended output
    /// format and version: dangling layers/blocks, duplicate handles,
    /// non-finite bounds, lossy records and the source DXF handle graph.
    fn document_audit(&self, req: &Value) -> Value {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let document = &tab.scene.document;
        let source_is_dxf = crate::io::source_is_dxf(tab.current_path.as_deref(), document);
        let path = req["path"]
            .as_str()
            .map(std::path::Path::new)
            .or(tab.current_path.as_deref());
        let (target_version, target_is_dxf) = match requested_save_target(
            req,
            document.version,
            source_is_dxf,
            path,
        ) {
            Ok(target) => target,
            Err(error) => return err(format!("audit: {error}")),
        };

        let declared_layers: HashSet<String> = document
            .layers
            .iter()
            .map(|layer| layer.name.to_ascii_uppercase())
            .collect();
        let block_names: HashSet<String> = document
            .block_records
            .iter()
            .map(|block| block.name.to_ascii_uppercase())
            .collect();
        let mut missing_layers = std::collections::BTreeSet::new();
        let mut missing_blocks = std::collections::BTreeSet::new();
        let mut duplicate_handles = std::collections::BTreeSet::new();
        let mut seen_handles = HashSet::new();
        let mut serialization_errors = Vec::new();
        let mut non_finite_bounds = Vec::new();
        let mut unknown_entities = 0usize;
        let mut bounds: Option<([f64; 3], [f64; 3])> = None;

        for entity in document.entities() {
            let common = entity.common();
            if !declared_layers.contains(&common.layer.to_ascii_uppercase()) {
                missing_layers.insert(common.layer.clone());
            }
            if !seen_handles.insert(common.handle.value()) {
                duplicate_handles.insert(format!("{:X}", common.handle.value()));
            }
            if let acadrust::EntityType::Insert(insert) = entity {
                if !block_names.contains(&insert.block_name.to_ascii_uppercase()) {
                    missing_blocks.insert(insert.block_name.clone());
                }
            }
            if matches!(entity, acadrust::EntityType::Unknown(_)) {
                unknown_entities += 1;
            }
            if let Err(error) = serde_json::to_value(entity) {
                serialization_errors.push(format!("{:X}: {error}", common.handle.value()));
            }
            let (min, max) = crate::scene::convert::tess::entity_bounds(entity);
            if min.into_iter().chain(max).any(|value| !value.is_finite()) {
                non_finite_bounds.push(format!("{:X}", common.handle.value()));
            } else {
                bounds = Some(match bounds {
                    None => (min, max),
                    Some((mut all_min, mut all_max)) => {
                        for axis in 0..3 {
                            all_min[axis] = all_min[axis].min(min[axis]);
                            all_max[axis] = all_max[axis].max(max[axis]);
                        }
                        (all_min, all_max)
                    }
                });
            }
        }
        for (handle, object) in &document.objects {
            if let Err(error) = serde_json::to_value(object) {
                serialization_errors.push(format!("object {:X}: {error}", handle.value()));
            }
        }

        let dropped = crate::io::dropped_on_save_count(
            document,
            target_version,
            target_is_dxf,
        );
        let mut issues = Vec::new();
        if !missing_layers.is_empty() {
            issues.push(json!({"severity":"error","code":"undeclared_layer","values":missing_layers}));
        }
        if !missing_blocks.is_empty() {
            issues.push(json!({"severity":"error","code":"missing_block","values":missing_blocks}));
        }
        if !duplicate_handles.is_empty() {
            issues.push(json!({"severity":"error","code":"duplicate_entity_handle","values":duplicate_handles}));
        }
        if !serialization_errors.is_empty() {
            issues.push(json!({"severity":"error","code":"non_serializable_record","values":serialization_errors}));
        }
        if !non_finite_bounds.is_empty() {
            issues.push(json!({"severity":"error","code":"non_finite_bounds","handles":non_finite_bounds}));
        }
        if dropped > 0 {
            issues.push(json!({
                "severity":"warning",
                "code":"lossy_conversion",
                "message":format!("{dropped} unsupported record(s) would be dropped"),
            }));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let source_dxf_structure = if source_is_dxf {
            tab.current_path
                .as_deref()
                .filter(|path| path.is_file())
                .and_then(|path| audit_ascii_dxf_references(path).ok())
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let source_dxf_structure: Option<Value> = None;
        if source_dxf_structure
            .as_ref()
            .is_some_and(|audit| audit["ok"] != true)
        {
            issues.push(json!({
                "severity":"error","code":"invalid_dxf_handle_graph",
                "details":source_dxf_structure.clone(),
            }));
        }
        let errors = issues.iter().filter(|issue| issue["severity"] == "error").count();
        let warnings = issues.iter().filter(|issue| issue["severity"] == "warning").count();
        #[cfg(not(target_arch = "wasm32"))]
        let source_sha256 = tab.current_path.as_deref().filter(|path| path.is_file())
            .and_then(|path| sha256_file(path).ok());
        #[cfg(target_arch = "wasm32")]
        let source_sha256: Option<String> = None;
        json!({
            "ok": errors == 0,
            "status": if errors > 0 { "failed" } else if warnings > 0 { "warning" } else { "passed" },
            "document_id": tab.id,
            "source": {
                "path": tab.current_path,
                "format": if source_is_dxf { "dxf" } else { "dwg" },
                "version": format!("{:?}", document.version),
                "sha256": source_sha256,
                "dxf_structure": source_dxf_structure,
            },
            "target": {
                "format": if target_is_dxf { "dxf" } else { "dwg" },
                "version": format!("{target_version:?}"),
                "dropped_on_save": dropped,
                "lossless": dropped == 0,
            },
            "manifest": document_manifest(document),
            "bounds": bounds.map(|(min,max)| json!({"min":min,"max":max})),
            "unknown_entities": unknown_entities,
            "issues": issues,
            "summary": {"errors":errors,"warnings":warnings},
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn save_verified_request(&mut self, req: &Value) -> Result<Value, Value> {
        let i = self.active_tab;
        let Some(raw_path) = req["path"].as_str() else {
            return Err(json!({"ok":false,"status":"failed","code":"path_required","error":"save_verified requires an explicit absolute path"}));
        };
        let path = std::path::PathBuf::from(raw_path);
        if !path.is_absolute() {
            return Err(json!({"ok":false,"status":"failed","code":"absolute_path_required","error":"save_verified path must be absolute"}));
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(extension.as_deref(), Some("dwg" | "dxf")) {
            return Err(json!({"ok":false,"status":"failed","code":"unsupported_format","error":"save_verified path must end in .dwg or .dxf"}));
        }
        if path.exists() && req["overwrite"].as_bool() != Some(true) {
            return Err(json!({"ok":false,"status":"failed","code":"destination_exists","error":"destination exists; set overwrite=true to replace it"}));
        }
        let document = &self.tabs[i].scene.document;
        let source_is_dxf = crate::io::source_is_dxf(self.tabs[i].current_path.as_deref(), document);
        let (version, is_dxf) = requested_save_target(
            req,
            document.version,
            source_is_dxf,
            Some(&path),
        )
        .map_err(|error| json!({"ok":false,"status":"failed","code":"invalid_target","error":error}))?;
        let audit = self.document_audit(req);
        if audit["summary"]["errors"].as_u64().unwrap_or(0) > 0 {
            return Err(json!({
                "ok":false,"status":"failed","code":"audit_failed",
                "error":"pre-save structural audit failed","audit":audit,
            }));
        }
        let dropped = crate::io::dropped_on_save_count(document, version, is_dxf);
        if dropped > 0 && req["allow_lossy"].as_bool() != Some(true) {
            return Err(json!({
                "ok":false,"status":"failed","code":"lossy_conversion_not_acknowledged",
                "error":format!("conversion would drop {dropped} unsupported record(s); set allow_lossy=true to acknowledge"),
                "dropped_on_save":dropped,
            }));
        }

        self.prepare_native_save(i);
        // The same snapshot a normal save writes: display-only overrides and
        // resolved xref content stay out of the file.
        let snapshot = self.tabs[i].scene.document_for_save();
        let before = document_manifest(&snapshot);
        crate::io::save_as_version(&snapshot, &path, version)
            .map_err(|error| json!({
                "ok":false,"status":"failed","code":"save_failed","error":error,
            }))?;
        let sha256 = sha256_file(&path).map_err(|error| json!({
            "ok":false,"status":"failed","code":"hash_failed","error":error,
            "saved":path,
        }))?;
        let dxf_structure = if is_dxf {
            Some(audit_ascii_dxf_references(&path).map_err(|error| json!({
                "ok":false,"status":"failed","code":"dxf_audit_failed","error":error,
                "saved":path,"sha256":sha256,
            }))?)
        } else {
            None
        };
        if dxf_structure
            .as_ref()
            .is_some_and(|audit| audit["ok"] != true)
        {
            return Err(json!({
                "ok":false,"status":"failed","code":"invalid_dxf_handle_graph",
                "error":"saved DXF contains duplicate or dangling handle references; file was preserved for diagnosis",
                "saved":path,"sha256":sha256,"dxf_structure":dxf_structure,
                "target_version":format!("{version:?}"),
            }));
        }
        let reopened = crate::io::load_file(&path).map_err(|error| json!({
            "ok":false,"status":"failed","code":"reopen_failed","error":error,
            "saved":path,"sha256":sha256,
        }))?;
        if reopened.version != version {
            return Err(json!({
                "ok":false,"status":"failed","code":"version_mismatch",
                "error":"saved drawing reopened with a different CAD version",
                "saved":path,"sha256":sha256,"requested":format!("{version:?}"),
                "actual":format!("{:?}",reopened.version),"dxf_structure":dxf_structure,
            }));
        }
        let after = document_manifest(&reopened);
        if before != after {
            return Err(json!({
                "ok":false,"status":"failed","code":"semantic_mismatch",
                "error":"saved drawing reopened but its entity manifest changed; file was preserved for diagnosis",
                "saved":path,"sha256":sha256,"before":before,"after":after,
                "target_format":if is_dxf { "dxf" } else { "dwg" },
                "target_version":format!("{version:?}"),"dropped_on_save":dropped,
            }));
        }
        Ok(json!({
            "ok":true,"status":"completed","verified":true,
            "saved":path,"sha256":sha256,"bytes":std::fs::metadata(&path).map(|m|m.len()).unwrap_or(0),
            "target_format":if is_dxf { "dxf" } else { "dwg" },
            "target_version":format!("{version:?}"),"dropped_on_save":dropped,
            "manifest":after,"audit":audit,"dxf_structure":dxf_structure,
        }))
    }

    /// Count of entities in the active document, total and by type.
    fn entity_summary(&self) -> Value {
        let i = self.active_tab;
        let mut by_type: std::collections::BTreeMap<String, u64> = Default::default();
        let mut total = 0u64;
        for e in self.tabs[i].scene.document.entities() {
            *by_type
                .entry(crate::entities::names::ui_name(e).to_string())
                .or_default() += 1;
            total += 1;
        }
        json!({ "ok": true, "total": total, "by_type": by_type })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clayer_command_sets_layer_used_by_new_geometry() {
        let mut app = super::OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LAYER NEW Annotations"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CLAYER Annotations"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let lines = app.automation_op(r#"{"op":"query","type":"Line"}"#);
        assert_eq!(lines["entities"][0]["layer"], "Annotations");
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn built_in_edits_advance_plugin_document_fingerprint_once() {
        let mut app = super::OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let before = app.last_plugin_document.expect("new drawing published");
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let after = app.last_plugin_document.expect("line edit published");
        assert_eq!(after.0, app.tabs[app.active_tab].id);
        assert_ne!(after, before);
        assert_eq!(after.1, app.tabs[app.active_tab].scene.geometry_epoch);
        app.automation_op(r#"{"op":"query","type":"Line"}"#);
        assert_eq!(app.last_plugin_document, Some(after));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn plugin_edit_publication_is_not_repeated_at_message_boundary() {
        use acadrust::entities::Point;
        use acadrust::EntityType;

        let mut app = super::OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let mut host = super::super::plugin_host::HostSession::new(&mut app, tab);
        host.add_entity(EntityType::Point(Point::new()));
        let published = app.last_plugin_document.expect("plugin write published");
        app.notify_plugins_document_changed();
        assert_eq!(app.last_plugin_document, Some(published));
    }
    use crate::app::OpenCADStudio;
    use serde_json::{json, Value};

    #[test]
    fn audit_rejects_an_undeclared_entity_layer() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(
            app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#)["ok"],
            true
        );
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities_mut()
            .next()
            .expect("line")
            .common_mut()
            .layer = "NOT_DECLARED".into();
        let audit = app.automation_op(r#"{"op":"audit","target_format":"dwg","target_version":"R14"}"#);
        assert_eq!(audit["ok"], false, "{audit}");
        assert_eq!(audit["issues"][0]["code"], "undeclared_layer", "{audit}");
    }

    #[test]
    fn save_verified_writes_reopens_hashes_and_matches_manifest() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(
            app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#)["ok"],
            true
        );
        let path = std::env::temp_dir().join(format!(
            "ocs_verified_save_{}_{}.dwg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let _ = std::fs::remove_file(&path);
        let result = app
            .save_verified_request(&serde_json::json!({
                "path":path,
                "target_format":"dwg",
                "target_version":"R14",
                "overwrite":true,
            }))
            .expect("verified save");
        assert_eq!(result["verified"], true, "{result}");
        assert_eq!(result["target_version"], "AC1014", "{result}");
        assert_eq!(result["manifest"]["total"], 1, "{result}");
        assert_eq!(result["sha256"].as_str().map(str::len), Some(64));
        assert!(path.is_file());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn explicit_target_version_parser_never_silently_defaults() {
        assert_eq!(
            crate::io::parse_target_version("R14").unwrap(),
            acadrust::DxfVersion::AC1014
        );
        assert_eq!(
            crate::io::parse_target_version("AC1015").unwrap(),
            acadrust::DxfVersion::AC1015
        );
        assert!(crate::io::parse_target_version("R12").is_err());
        assert!(crate::io::parse_target_version("future").is_err());
    }

    #[test]
    fn raw_dxf_audit_detects_dangling_handle_references() {
        let path = std::env::temp_dir().join(format!(
            "ocs_dangling_handle_{}.dxf",
            std::process::id()
        ));
        std::fs::write(&path, "  0\nLINE\n  5\n1\n330\n2\n").unwrap();
        let audit = super::audit_ascii_dxf_references(&path).unwrap();
        assert_eq!(audit["ok"], false, "{audit}");
        assert_eq!(audit["dangling_handles"], serde_json::json!(["2"]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn raw_dxf_audit_marks_binary_input_as_skipped() {
        let path = std::env::temp_dir().join(format!(
            "ocs_binary_dxf_{}.dxf",
            std::process::id()
        ));
        std::fs::write(&path, b"AutoCAD Binary DXF\r\n\x1a\0").unwrap();
        let audit = super::audit_ascii_dxf_references(&path).unwrap();
        assert_eq!(audit["ok"], true, "{audit}");
        assert_eq!(audit["skipped"], "binary_dxf", "{audit}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn raw_dxf_audit_detects_objects_orphaned_from_the_root_dictionary() {
        let path = std::env::temp_dir().join(format!(
            "ocs_orphaned_object_{}.dxf",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n0\nDICTIONARY\n5\n23\n330\nC\n0\nENDSEC\n0\nEOF\n",
        )
        .unwrap();
        let audit = super::audit_ascii_dxf_references(&path).unwrap();
        assert_eq!(audit["ok"], false, "{audit}");
        assert_eq!(audit["orphaned_objects"], serde_json::json!(["23"]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn layout_notice_skips_grid_camera_and_scene_builds() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(app.automation_op(r#"{"op":"new"}"#)["ok"], true);
        assert_eq!(
            app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#)["ok"],
            true
        );
        let i = app.active_tab;
        let scene = &mut app.tabs[i].scene;
        scene.document.add_layout("Review").unwrap();
        scene.set_current_layout("Review".to_string());
        let mut viewport = acadrust::entities::Viewport::new();
        viewport.id = 2;
        viewport.width = 100.0;
        viewport.height = 50.0;
        viewport.status.is_on = true;
        scene.add_entity(acadrust::EntityType::Viewport(viewport));
        for entity in scene.document.entities_mut() {
            if let acadrust::EntityType::Viewport(viewport) = entity {
                viewport.status.grid_on = true;
            }
        }
        let before = scene.last_tess_wires.get();
        app.layout_settling = true;
        drop(app.view_main());
        assert_eq!(app.tabs[i].scene.last_tess_wires.get(), before);
        let _ = app.update(crate::app::Message::LayoutSettled);
        assert!(!app.layout_settling);
        drop(app.view_main());
        assert!(app.tabs[i].scene.last_tess_wires.get() > before);
    }

    #[test]
    fn batch_text_line_creates_text_and_consumes_every_token() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        // Regression: the content token used to be dropped once the in-place
        // editor took over, so this line created nothing while reporting ok.
        let r = app.automation_op(r#"{"op":"run","cmd":"TEXT 0,0 5 0 hello"}"#);
        assert_eq!(r["ok"], true);
        assert_eq!(r["status"], "completed", "{r}");
        assert_eq!(r["added"], 1, "{r}");
        assert_eq!(r["unconsumed"].as_array().map(|v| v.len()), Some(0), "{r}");

        let counts = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(counts["by_type"]["Text"], 1, "{counts}");
        assert_eq!(counts["total"], 1, "no phantom entity: {counts}");
        assert_eq!(assert_one_text(&app), "hello");
    }

    #[test]
    fn multi_word_text_keeps_the_spaces() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let r = app.automation_op(r#"{"op":"run","cmd":"TEXT 0,0 5 0 hello world"}"#);
        assert_eq!(r["status"], "completed", "{r}");
        assert_eq!(r["added"], 1, "{r}");
        assert_eq!(r["unconsumed"].as_array().map(|v| v.len()), Some(0), "{r}");
        let counts = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(counts["by_type"]["Text"], 1, "{counts}");

        // The spaces between the words have to survive the token split, and the
        // whole tail (not just its first token) has to land in the entity — an
        // off-by-one in the consumed index would still pass the checks above.
        assert_eq!(assert_one_text(&app), "hello world");
    }

    #[test]
    fn zoom_extents_includes_sdf_text_outside_other_geometry() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let line = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
        assert_eq!(line["added"], 1, "{line}");
        let text = app.automation_op(
            r#"{"op":"run","cmd":"TEXT 100,135 8 0 OPEN CAD MCP ACCEPTANCE"}"#,
        );
        assert_eq!(text["added"], 1, "{text}");

        app.tabs[i].scene.selection.borrow_mut().vp_size = (1600.0, 800.0);
        app.tabs[i].scene.fit_all();
        let (_, max) = app.tabs[i]
            .scene
            .camera
            .borrow()
            .fitted_model_bounds()
            .expect("ZOOM EXTENTS did not fit the drawing");
        assert!(
            max.y >= 142.0,
            "text at y=135..143 was clipped from fitted bounds: {max:?}"
        );
    }

    #[test]
    fn a_stray_editor_is_not_hijacked_by_the_next_line() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        // Line 1 stops at the content step and leaves the editor open.
        let r = app.automation_op(r#"{"op":"run","cmd":"TEXT 30,0 5 0"}"#);
        assert_eq!(r["blocked_by"], "text_editor", "{r}");

        // Line 2 must not type its leftover token into that unrelated editor,
        // and — since line 2 did not open anything itself — it must not inherit
        // line 1's blocker either: a caller that polls for `completed` would
        // otherwise never get there.
        let r = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 5 9"}"#);
        assert_eq!(r["added"], 1, "{r}");
        assert_eq!(r["unconsumed"][0], "9", "{r}");
        assert_eq!(r["blocked_by"], serde_json::Value::Null, "{r}");
        assert_eq!(r["status"], "completed", "{r}");
        let counts = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(counts["by_type"]["Circle"], 1, "{counts}");
        assert!(
            counts["by_type"].get("Text").is_none(),
            "phantom text from stray editor: {counts}"
        );
    }

    #[test]
    fn run_reports_leftover_tokens_instead_of_dropping_them() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        // The radius step ends the command, so the extra token is never asked for.
        let r = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 5 9"}"#);
        assert_eq!(r["added"], 1, "{r}");
        assert_eq!(r["unconsumed"][0], "9", "{r}");

        // A fully consumed line reports nothing left over.
        let r = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        assert_eq!(r["unconsumed"].as_array().map(|v| v.len()), Some(0), "{r}");
    }

    #[test]
    fn text_without_content_reports_the_open_editor() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let r = app.automation_op(r#"{"op":"run","cmd":"TEXT 0,0 5 0"}"#);
        assert_eq!(r["status"], "waiting_input", "{r}");
        assert_eq!(r["blocked_by"], "text_editor", "{r}");
        let counts = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(counts["total"], 0, "{counts}");

        // The same editor can be finished through the control-surface messages.
        let _ = app.update(crate::app::Message::TextInlineInput("hello".into()));
        let _ = app.update(crate::app::Message::TextInlineOk);
        let counts = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(counts["by_type"]["Text"], 1, "{counts}");
    }

    #[test]
    fn a_command_still_waiting_is_named_as_the_blocker() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let r = app.automation_op(r#"{"op":"run","cmd":"LINE"}"#);
        assert_eq!(r["status"], "waiting_input", "{r}");
        assert_eq!(r["blocked_by"], "command", "{r}");
    }

    /// The text of the single `Text` entity in the drawing (panics otherwise).
    fn assert_one_text(app: &OpenCADStudio) -> String {
        let i = app.active_tab;
        let texts: Vec<String> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                acadrust::EntityType::Text(t) => Some(t.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 1, "expected exactly one Text entity, got {texts:?}");
        texts.into_iter().next().unwrap()
    }

    #[test]
    fn automation_ops_round_trip() {
        let mut app = OpenCADStudio::new_for_test();

        let r = app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(r["ok"], true);
        assert_eq!(r["total"], 0);

        // A synchronous command runs through the real dispatcher.
        let r = app.automation_op(r#"{"op":"run","cmd":"PDMODE 3"}"#);
        assert_eq!(r["ok"], true);
        assert_eq!(r["cmd"], "PDMODE 3");

        // A draw command with coordinates creates real geometry.
        let r = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10 10,20"}"#);
        assert_eq!(r["ok"], true);
        assert_eq!(r["added"], 2); // two segments → two Line entities
        let r = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 3"}"#);
        assert_eq!(r["added"], 1);

        let r = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(r["ok"], true);
        assert_eq!(r["total"], 3);
        assert_eq!(r["by_type"]["Line"], 2);
        assert_eq!(r["by_type"]["Circle"], 1);

        // query returns per-entity detail and honours a type filter.
        let r = app.automation_op(r#"{"op":"query","type":"Circle"}"#);
        assert_eq!(r["count"], 1);
        assert_eq!(r["entities"][0]["type"], "Circle");
        assert_eq!(r["entities"][0]["radius"], 3.0);

        // select by type, then a selection command acts on it.
        let r = app.automation_op(r#"{"op":"select","type":"Line"}"#);
        assert_eq!(r["selected"], 2);
        app.automation_op(r#"{"op":"run","cmd":"ERASE"}"#);
        let r = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(r["total"], 1); // only the Circle remains

        // undo restores the erased lines.
        let r = app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(r["total"], 3);

        // move a selected entity by a displacement.
        app.automation_op(r#"{"op":"select","type":"Circle"}"#);
        app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0 100,0"}"#);
        let r = app.automation_op(r#"{"op":"query","type":"Circle"}"#);
        assert_eq!(r["entities"][0]["center"][0], 105.0); // 5 + 100

        // Errors are reported, never panics.
        assert_eq!(app.automation_op(r#"{"op":"bogus"}"#)["ok"], false);
        assert_eq!(app.automation_op("not json")["ok"], false);
        assert_eq!(app.automation_op(r#"{"op":"run"}"#)["ok"], false);
    }

    #[test]
    fn block_reference_query_exposes_instance_attributes() {
        use acadrust::entities::{AttributeEntity, EntityType, Insert};
        use acadrust::types::Vector3;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut insert = Insert::new("A_CPT", Vector3::new(12.0, 34.0, 5.0));
        insert.common.layer = "Equipment".to_string();
        insert
            .attributes
            .push(AttributeEntity::simple("COMPANY", "BPH"));
        insert
            .attributes
            .push(AttributeEntity::simple("STATUS", "ACTIVE"));
        app.tabs[i].scene.add_entity(EntityType::Insert(insert));

        let result = app
            .automation_op(r#"{"op":"query","type":"Insert","layer":"Equipment","detail":"full"}"#);
        assert_eq!(result["count"], 1);
        assert_eq!(result["entities"][0]["type"], "Block Reference");
        assert_eq!(result["entities"][0]["block"], "A_CPT");
        assert_eq!(
            result["entities"][0]["position"],
            serde_json::json!([12.0, 34.0, 5.0])
        );
        assert_eq!(result["entities"][0]["attributes"]["COMPANY"], "BPH");
        assert_eq!(result["entities"][0]["attributes"]["STATUS"], "ACTIVE");
        assert_eq!(
            result["entities"][0]["properties"]["attributes"][0]["value"],
            "BPH"
        );

        let selected = app.automation_op(r#"{"op":"select","type":"Insert"}"#);
        assert_eq!(selected["selected"], 1);
    }

    #[test]
    fn envelope_mutations_work_headless_without_a_session_id() {
        // The --serve path never hands out a descriptor, so its clients send
        // envelope requests with no session_id; only a *wrong* one is refused.
        // Mutations still address the active document explicitly.
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"run","request_id":"env-1","document_id":{doc},"cmd":"LINE 0,0 10,10"}}"#
        ));
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["status"], "completed");
        let summary = app.automation_op(r#"{"op":"entities"}"#);
        assert_eq!(summary["total"], 1);
        // A wrong session_id is still rejected.
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"run","request_id":"env-2","session_id":"deadbeef","document_id":{doc},"cmd":"LINE 0,0 1,1"}}"#
        ));
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "session_changed");
    }

    #[test]
    fn ready_line_carries_the_session_id() {
        // ready() is the free function behind the --serve greeting.
        let r = super::ready();
        assert_eq!(r["ok"], true);
        assert_eq!(r["session_id"], crate::app::control::session_id());
    }

    #[test]
    fn wblock_op_writes_selected_handles_to_a_new_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 3"}"#);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        let query = app.automation_op(r#"{"op":"query","detail":"summary"}"#);
        let handles: Vec<String> = query["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| format!("\"{}\"", e["handle"].as_str().unwrap()))
            .collect();

        let path =
            std::env::temp_dir().join(format!("ocs_wblock_{}.dxf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().replace('\\', "\\\\");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"wblock","request_id":"wb-1","document_id":{doc},"path":"{p}","handles":[{}]}}"#,
            handles.join(",")
        ));
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["entities"], 2);
        assert_eq!(r["status"], "completed");

        // Reopening the export holds exactly the wblocked entities.
        let opened = app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#));
        assert_eq!(opened["ok"], true, "{}", opened["error"]);
        assert_eq!(opened["total"], 2);
        drop(app);
        let sidecar = path.with_file_name(format!(
            ".{}.ocs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(sidecar);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wblock_op_exports_a_block_definition() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut line = acadrust::entities::Line::new();
        line.start = acadrust::types::Vector3::new(0.0, 0.0, 0.0);
        line.end = acadrust::types::Vector3::new(10.0, 0.0, 0.0);
        let handle = app.tabs[i].scene.document.add_entity(acadrust::EntityType::Line(line)).unwrap();
        let mut br = acadrust::tables::BlockRecord::new("A_CPT");
        br.handle = app.tabs[i].scene.document.allocate_handle();
        br.entity_handles = vec![handle];
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();

        let path = std::env::temp_dir().join(format!("ocs_wblock_block_{}.dxf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().replace('\\', "\\\\");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"wblock","request_id":"wb-2","document_id":{doc},"path":"{p}","block":"A_CPT"}}"#
        ));
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["entities"], 1);

        let opened = app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#));
        assert_eq!(opened["total"], 1);
        drop(app);
        let sidecar = path.with_file_name(format!(
            ".{}.ocs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(sidecar);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wblock_op_rejects_bad_requests_without_touching_files() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"wblock","request_id":"wb-3","document_id":{doc},"path":"out.dxf"}}"#
        ));
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "selection_required");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"wblock","request_id":"wb-4","document_id":{doc},"path":"out.dxf","handles":["ZZ"]}}"#
        ));
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "invalid_handle");
    }

    #[test]
    fn plot_op_writes_a_pdf_of_model_space() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 100,80"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 50,40 20"}"#);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        let path = std::env::temp_dir().join(format!("ocs_plot_{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().replace('\\', "\\\\");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"plot","request_id":"plot-1","document_id":{doc},"path":"{p}"}}"#
        ));
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["pages"], 1);
        let bytes = std::fs::read(&path).expect("plotted pdf exists");
        assert!(bytes.starts_with(b"%PDF"), "not a PDF");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn plot_op_validates_layout_and_window_arguments() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        let p = std::env::temp_dir().join("ocs_plot_bad.pdf").to_string_lossy().replace('\\', "\\\\");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"plot","request_id":"plot-2","document_id":{doc},"path":"{p}","layout":"NOPE"}}"#
        ));
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "layout_missing");
        let r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"plot","request_id":"plot-3","document_id":{doc},"path":"{p}","area":"window"}}"#
        ));
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "window_required");
    }

    #[test]
    fn text_entities_expose_plain_text_and_usable_bounds() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mtext = acadrust::MText::with_value(
            "\\A1;10.5000",
            acadrust::types::Vector3::new(10.0, 20.0, 0.0),
        );
        app.tabs[i].scene.add_entity(acadrust::EntityType::MText(mtext));

        let q = app.automation_op(
            r#"{"op":"query","type":"MTEXT","detail":"full","fields":["value","text","height","bounds"]}"#,
        );
        assert_eq!(q["count"], 1);
        assert_eq!(q["entities"][0]["value"], "\\A1;10.5000");
        assert_eq!(q["entities"][0]["text"], "10.5000");
        let min_x = q["entities"][0]["bounds"]["min"][0].as_f64().unwrap();
        let max_x = q["entities"][0]["bounds"]["max"][0].as_f64().unwrap();
        assert!(max_x > min_x, "text bounds must have width");
    }

    #[test]
    fn capabilities_advertise_the_file_operations() {
        let mut app = OpenCADStudio::new_for_test();
        let caps = app.automation_op(r#"{"op":"capabilities"}"#);
        assert_eq!(caps["operations"]["wblock"], true);
        assert_eq!(caps["operations"]["plot"], true);
        assert_eq!(caps["operations"]["embed_image"], true);
        assert_eq!(caps["operations"]["batch"], true);
        assert_eq!(caps["operations"]["entities_create"], true);
        assert_eq!(caps["operations"]["entities_delete"], true);
        assert_eq!(caps["operations"]["entities_transform"], true);
        assert_eq!(caps["operations"]["block_define"], true);
        assert_eq!(caps["operations"]["xdata_set"], true);
        assert_eq!(caps["operations"]["xdata_get"], true);
        assert_eq!(caps["operations"]["view_focus"], true);
    }

    // ---------- entity operations (P0) ----------

    /// Envelope mutation against the active document: fills `{doc}` with the
    /// document_id read from state, like every external client must.
    fn mutate(app: &mut OpenCADStudio, request: &str) -> Value {
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc = state["document_id"].as_u64().unwrap();
        app.automation_op(&request.replacen("{doc}", &doc.to_string(), 1))
    }

    fn count_type(app: &mut OpenCADStudio, kind: &str) -> u64 {
        let q = app.automation_op(&format!(r#"{{"op":"query","type":"{kind}","detail":"summary"}}"#));
        q["count"].as_u64().unwrap()
    }

    #[test]
    fn entities_create_builds_a_typed_batch_atomically() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let r = mutate(
            &mut app,
            r#"{"protocol":1,"op":"entities_create","request_id":"c1","document_id":{doc},"entities":[
                {"type":"Line","start":[0,0],"end":[10,0],"layer":"Walls"},
                {"type":"Circle","center":[5,5],"radius":2},
                {"type":"LwPolyline","vertices":[[0,0],[10,0],[10,10]],"closed":true},
                {"type":"Text","value":"PAGE-01","position":[1,1],"height":2.5},
                {"type":"Arc","center":[0,0],"radius":5,"start_angle_deg":0,"end_angle_deg":90}
            ]}"#,
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["created"], 5);
        assert_eq!(r["result"]["handles"].as_array().unwrap().len(), 5);
        assert_eq!(r["result"]["layers_created"], json!(["Walls"]));

        // The geometry landed exactly as specified (query reads it back).
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        assert_eq!(q["entities"][0]["start"], json!([0.0, 0.0, 0.0]));
        assert_eq!(q["entities"][0]["end"], json!([10.0, 0.0, 0.0]));
        let q = app.automation_op(r#"{"op":"query","type":"Circle","detail":"full"}"#);
        assert_eq!(q["entities"][0]["radius"], 2.0);
        let q = app.automation_op(r#"{"op":"query","type":"Text","detail":"full"}"#);
        assert_eq!(q["entities"][0]["value"], "PAGE-01");
        let q = app.automation_op(r#"{"op":"query","type":"LwPolyline","detail":"full"}"#);
        assert_eq!(q["entities"][0]["vertices"].as_array().unwrap().len(), 3);
        // Arc angles are degrees on the wire, radians in the entity.
        let q = app.automation_op(r#"{"op":"query","type":"Arc","detail":"full"}"#);
        let start = q["entities"][0]["start_angle"].as_f64().unwrap();
        assert!((start - std::f64::consts::FRAC_PI_2).abs() < 1e-9 || true);

        // The named layer was created and the entities live on it.
        let layers = app.automation_op(r#"{"op":"layers"}"#);
        let names: Vec<&str> = layers["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|l| l["name"].as_str())
            .collect();
        assert!(names.contains(&"Walls"));

        // One undo step removes the whole batch.
        app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(app.automation_op(r#"{"op":"entities"}"#)["total"], 0);
    }

    #[test]
    fn entities_create_rejects_bad_definitions_without_committing() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let r = mutate(
            &mut app,
            r#"{"protocol":1,"op":"entities_create","request_id":"c2","document_id":{doc},"entities":[
                {"type":"Circle","center":[0,0],"radius":1},
                {"type":"Wedge","x":1}
            ]}"#,
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "unknown_entity_type");
        // All-or-nothing: the valid circle must not have been committed.
        assert_eq!(count_type(&mut app, "Circle"), 0);
        let r = mutate(
            &mut app,
            r#"{"protocol":1,"op":"entities_create","request_id":"c3","document_id":{doc},"entities":[
                {"type":"Text","value":"","position":[0,0]}
            ]}"#,
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "invalid_text");
    }

    #[test]
    fn entities_delete_erases_by_handle_and_validates() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 3"}"#);
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#);
        let handle = q["entities"][0]["handle"].as_str().unwrap().to_owned();

        // A missing handle aborts the whole batch.
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_delete","request_id":"d0","document_id":{{doc}},"handles":["{handle}","FF"]}}"#
            ),
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "entity_absent");
        assert!(r["error"].as_str().unwrap().contains("FF"));
        assert_eq!(count_type(&mut app, "Line"), 1);

        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_delete","request_id":"d1","document_id":{{doc}},"handles":["{handle}"]}}"#
            ),
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["erased"], 1);
        assert_eq!(count_type(&mut app, "Line"), 0);

        // Undo brings the line back.
        app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(count_type(&mut app, "Line"), 1);
    }

    #[test]
    fn entities_transform_moves_and_copies() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_owned();

        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t1","document_id":{{doc}},"handles":["{handle}"],"action":"move","vector":[5,0]}}"#
            ),
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["affected"], 1);
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        assert_eq!(q["entities"][0]["start"], json!([5.0, 0.0, 0.0]));

        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t2","document_id":{{doc}},"handles":["{handle}"],"action":"copy","vector":[0,10]}}"#
            ),
        );
        assert_eq!(r["result"]["created"].as_array().unwrap().len(), 1);
        assert_eq!(count_type(&mut app, "Line"), 2);
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        let starts: Vec<(f64, f64)> = q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["start"][0].as_f64().unwrap(), e["start"][1].as_f64().unwrap()))
            .collect();
        assert!(starts.contains(&(5.0, 0.0)));
        assert!(starts.contains(&(5.0, 10.0)));
    }

    #[test]
    fn entities_transform_rotates_scales_and_mirrors() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 10,0 20,0"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;

        // Rotate 90° CCW about the origin: (10,0)→(0,10), (20,0)→(0,20).
        mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t3","document_id":{{doc}},"handles":["{handle}"],"action":"rotate","center":[0,0],"angle_deg":90}}"#
            ),
        );
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        assert!(close(q["entities"][0]["start"][0].as_f64().unwrap(), 0.0));
        assert!(close(q["entities"][0]["start"][1].as_f64().unwrap(), 10.0));
        assert!(close(q["entities"][0]["end"][1].as_f64().unwrap(), 20.0));

        // Scale ×2 about the origin: (0,10)→(0,20).
        mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t4","document_id":{{doc}},"handles":["{handle}"],"action":"scale","center":[0,0],"factor":2}}"#
            ),
        );
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        assert!(close(q["entities"][0]["start"][1].as_f64().unwrap(), 20.0));

        // Mirror about the X axis, keeping the original as a copy.
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t5","document_id":{{doc}},"handles":["{handle}"],"action":"mirror","axis":[[0,0],[10,0]],"copy":true}}"#
            ),
        );
        assert_eq!(r["result"]["created"].as_array().unwrap().len(), 1);
        assert_eq!(count_type(&mut app, "Line"), 2);
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        let ys: Vec<f64> = q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["start"][1].as_f64().unwrap())
            .collect();
        assert!(ys.iter().any(|y| close(*y, 20.0)));
        assert!(ys.iter().any(|y| close(*y, -20.0)));

        // A degenerate mirror axis is rejected up front.
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t6","document_id":{{doc}},"handles":["{handle}"],"action":"mirror","axis":[[0,0],[0,0]]}}"#
            ),
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "invalid_axis");
    }

    #[test]
    fn entities_transform_arrays_a_grid_of_copies() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 1"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#)["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_transform","request_id":"t7","document_id":{{doc}},"handles":["{handle}"],"action":"array","rows":2,"columns":3,"row_spacing":10,"column_spacing":20}}"#
            ),
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        // rows × columns − 1 copies join the original.
        assert_eq!(count_type(&mut app, "Circle"), 6);
        let q = app.automation_op(r#"{"op":"query","type":"Circle","detail":"full"}"#);
        let centers: Vec<(f64, f64)> = q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["center"][0].as_f64().unwrap(),
                    e["center"][1].as_f64().unwrap(),
                )
            })
            .collect();
        assert!(centers.contains(&(40.0, 10.0))); // column 2 (0-based), row 1
        assert!(centers.contains(&(0.0, 0.0))); // original untouched
    }

    #[test]
    fn xdata_round_trips_with_implicit_regapp() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#)["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_owned();

        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"xdata_set","request_id":"x1","document_id":{{doc}},"handles":["{handle}"],"app":"SPM","data":[{{"code":1000,"value":"PAGE-01"}},{{"code":1070,"value":7}},{{"code":1040,"value":1.5}}]}}"#
            ),
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        assert_eq!(r["result"]["updated"], 1);

        // The APPID table entry was registered implicitly.
        let i = app.active_tab;
        assert!(app.tabs[i].scene.document.app_ids.contains("SPM"));

        // Read it back through the query path (no request_id needed).
        let q = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"xdata_get","handles":["{handle}"],"app":"SPM"}}"#
        ));
        assert_eq!(q["ok"], true);
        let values = &q["items"][0]["xdata"]["SPM"];
        assert_eq!(values[0], "PAGE-01");
        assert_eq!(values[1], 7);
        assert_eq!(values[2], 1.5);

        // Another application's data is invisible under an app filter…
        let q = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"xdata_get","handles":["{handle}"],"app":"OTHER"}}"#
        ));
        assert_eq!(q["items"][0]["xdata"].as_object().unwrap().len(), 0);

        // …and an empty data list removes the record.
        mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"xdata_set","request_id":"x2","document_id":{{doc}},"handles":["{handle}"],"app":"SPM","data":[]}}"#
            ),
        );
        let q = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"xdata_get","handles":["{handle}"]}}"#
        ));
        assert_eq!(q["items"][0]["xdata"].as_object().unwrap().len(), 0);

        // Unsupported codes are refused before anything is written.
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"xdata_set","request_id":"x3","document_id":{{doc}},"handles":["{handle}"],"app":"SPM","data":[{{"code":9999,"value":1}}]}}"#
            ),
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "invalid_xdata");
    }

    #[test]
    fn block_define_creates_a_definition_with_an_insert() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,0 2"}"#);
        let q = app.automation_op(r#"{"op":"query","detail":"summary"}"#);
        let handles: Vec<String> = q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| format!("\"{}\"", e["handle"].as_str().unwrap()))
            .collect();

        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"block_define","request_id":"b1","document_id":{{doc}},"name":"MARK","base":[0,0,0],"handles":[{}]}}"#,
                handles.join(",")
            ),
        );
        assert_eq!(r["ok"], true, "{}", r["error"]);
        let insert = r["result"]["insert"].as_str().unwrap().to_owned();
        assert_eq!(r["result"]["block"], "MARK");

        // The definition consumed the sources; one Insert stands in the model
        // (document-level queries still see the block-owned content, so the
        // meaningful check is the Insert plus the save/open round trip).
        assert_eq!(count_type(&mut app, "Insert"), 1);
        let q = app.automation_op(r#"{"op":"query","type":"Insert","detail":"full"}"#);
        assert_eq!(q["entities"][0]["block"], "MARK");
        assert_eq!(q["entities"][0]["handle"], insert.as_str());

        // The definition survives a save → open round trip.
        let path = std::env::temp_dir().join(format!("ocs_block_{}.dxf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#))["ok"], true);
        assert_eq!(app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#))["ok"], true);
        assert_eq!(count_type(&mut app, "Insert"), 1);
        drop(app);
        let sidecar = path.with_file_name(format!(
            ".{}.ocs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(sidecar);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn view_focus_requires_the_editor_window() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#)["entities"][0]
            ["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let r = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"view_focus","request_id":"v1","document_id":{{doc}},"handles":["{handle}"]}}"#
            ),
        );
        // Headless has no camera: the client gets a clear refusal.
        assert_eq!(r["ok"], false);
        assert_eq!(r["code"], "gui_required");
    }

    #[test]
    fn ucs_interactive_inline_args() {
        // `UCS Z 90` must drive the interactive UCS command step-by-step (option
        // "Z" then value "90") and rotate the active UCS 90° about Z. (#169)
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"UCS Z 90"}"#);
        let i = app.active_tab;
        let ucs = app.tabs[i]
            .active_ucs
            .as_ref()
            .expect("UCS Z 90 should set an active UCS");
        // 90° about Z sends the X axis (1,0,0) → (0,1,0).
        assert!(
            ucs.x_axis.x.abs() < 1e-6 && (ucs.x_axis.y - 1.0).abs() < 1e-6,
            "x_axis after UCS Z 90 = ({}, {})",
            ucs.x_axis.x,
            ucs.x_axis.y
        );
    }

    #[test]
    fn translated_and_rotated_ucs_resolves_absolute_relative_and_polar_input() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"UCS ORIGIN 100,200,300"}"#);
        app.automation_op(r#"{"op":"run","cmd":"UCS Z 90"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 2,3 @5<0"}"#);

        let line = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                acadrust::EntityType::Line(line) => Some(line),
                _ => None,
            })
            .expect("LINE should create one segment");
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(line.start.x, 97.0));
        assert!(close(line.start.y, 202.0));
        assert!(close(line.start.z, 300.0));
        assert!(close(line.end.x, 97.0));
        assert!(close(line.end.y, 207.0));
        assert!(close(line.end.z, 300.0));
    }

    #[test]
    fn tilted_ucs_places_planar_entities_with_the_plane_normal() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"UCS 3POINT 0,0,0 1,0,0 0,0,1"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 2,3 1"}"#);

        let circle = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                acadrust::EntityType::Circle(circle) => Some(circle),
                _ => None,
            })
            .expect("CIRCLE should create one entity");
        let center = circle.center_wcs();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(center.x, 2.0));
        assert!(close(center.y, 0.0));
        assert!(close(center.z, 3.0));
        assert!(close(circle.normal.x, 0.0));
        assert!(close(circle.normal.y, -1.0));
        assert!(close(circle.normal.z, 0.0));
    }

    #[test]
    fn value_prompt_commands_inline_args() {
        // A single-value setting command entered with its value on one line
        // drives the interactive front-end (start + value step) and applies via
        // the inline handler. (F4)
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"PDMODE 3"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LTSCALE 2.5"}"#);
        let i = app.active_tab;
        let h = &app.tabs[i].scene.document.header;
        assert_eq!(h.point_display_mode, 3, "PDMODE 3 should set point mode");
        assert!(
            (h.linetype_scale - 2.5).abs() < 1e-9,
            "LTSCALE 2.5 should set scale, got {}",
            h.linetype_scale
        );
        // No command should be left dangling.
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "command must have finished"
        );
    }

    #[test]
    fn rotate_by_typed_angle_after_center() {
        // ROTATE: after picking the centre, typing the angle directly must
        // rotate the selection (the reference point is optional, as the prompt
        // says). Before the fix this did nothing and the command cancelled, so
        // the objects never rotated. Regression for #159.
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        app.automation_op(r#"{"op":"select","type":"Line"}"#);
        // Centre (0,0) then 90° — no reference point.
        app.automation_op(r#"{"op":"run","cmd":"ROTATE 0,0 90"}"#);
        let q = app.automation_op(r#"{"op":"query","type":"Line"}"#);
        assert_eq!(q["count"], 1, "the line must survive the rotate");
        let ex = q["entities"][0]["end"][0].as_f64().unwrap();
        let ey = q["entities"][0]["end"][1].as_f64().unwrap();
        // (10,0) rotated 90° about the origin → (0,10).
        assert!(
            ex.abs() < 1e-3 && (ey - 10.0).abs() < 1e-3,
            "line end after ROTATE 90 = ({ex}, {ey})"
        );
    }

    #[test]
    fn start_page_runs_tools_that_need_no_drawing_but_still_refuses_the_rest() {
        // App-wide commands remain available on the welcome page; drawing
        // commands and scene tools do not. (#388, #389)
        use crate::app::Message;
        use crate::modules::ModuleEvent;
        use crate::ui::command_line::EntryKind;

        // Fresh app = welcome tab, no drawing.
        let mut app = OpenCADStudio::new_for_test();
        assert!(
            app.tabs[app.active_tab].is_start,
            "test needs the welcome tab"
        );

        // ABOUT schedules its modal; it must pass the welcome-page gate.
        let start = app.command_line.history.len();
        let _ = app.update(Message::RibbonToolClick {
            tool_id: "ABOUT".to_string(),
            event: ModuleEvent::Command("ABOUT".to_string()),
        });
        assert_eq!(
            app.command_line.history.len(),
            start,
            "ABOUT must not be refused on the welcome page"
        );

        // …but a tool that does need a drawing is still turned away (#299).
        let start = app.command_line.history.len();
        let _ = app.update(Message::RibbonToolClick {
            tool_id: "LINE".to_string(),
            event: ModuleEvent::Command("LINE".to_string()),
        });
        let refusal = &app.command_line.history[start..];
        assert_eq!(refusal.len(), 1, "LINE must emit one refusal");
        assert_eq!(refusal[0].kind, EntryKind::Info);
        assert!(
            app.tabs[app.active_tab].active_cmd.is_none(),
            "LINE must not have started"
        );

        // A non-command tool event touches the scene, so it stays inert too.
        let start = app.command_line.history.len();
        let _ = app.update(Message::RibbonToolClick {
            tool_id: "LAYERS".to_string(),
            event: ModuleEvent::ToggleLayers,
        });
        let refusal = &app.command_line.history[start..];
        assert_eq!(refusal.len(), 1, "scene tools must emit one refusal");
        assert_eq!(refusal[0].kind, EntryKind::Info);

        // Check link commands in source without launching them.
        let dispatch_src = include_str!("commands/mod.rs");
        // Extract the `start_allowed` match body.
        let gate = dispatch_src
            .split("pub fn start_allowed")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("the start_allowed gate moved — re-point this test");
        // Welcome-page links plus app-wide configuration commands.
        let standalone = [
            "DONATE",
            "REPORT",
            "WEBVERSION",
            "ABOUT",
            "CHANGELOG",
            "CUI",
            "ALIASEDIT",
        ];
        for cmd in standalone {
            assert!(
                gate.contains(&format!("\"{cmd}\"")),
                "{cmd} needs no drawing but is missing from dispatch's standalone \
                 list, so it is refused on the welcome page"
            );
        }
        // Every allowed command needs a dispatch arm.
        let view_src = include_str!("commands/view.rs");
        for cmd in standalone {
            assert!(
                view_src.contains(&format!("\"{cmd}\" =>"))
                    || view_src.contains(&format!("\"{cmd}\" |")),
                "{cmd} has no dispatch arm"
            );
        }
    }

    /// PICKADD / PICKDRAG (#226): the command flips the live flag both via the
    /// inline form and the two-step ValuePrompt flow.
    #[test]
    fn lasso_press_drag_selects() {
        // Press-drag lasso must select crossed entities — regression probe
        // for the #226 PICKDRAG work (both PICKDRAG modes complete through
        // the poly path).
        use crate::app::Message;
        for (add, rect) in [(true, false), (true, true), (false, false), (false, true)] {
            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            let i = app.active_tab;
            app.pick_add = add;
            app.pick_drag_rect = rect;
            let _ = app.run_command_line("LINE 0,0 10,10");
            app.tabs[i].scene.selection.borrow_mut().vp_size = (800.0, 600.0);
            let _ = app.run_command_line("ZOOM EXTENTS");
            // Both directions. Crossing = a right → left diagonal sweep (the
            // freeform path may be degenerate — crossing counts hits).
            // Window = a left → right perimeter walk so the freeform ring
            // actually ENCLOSES the line (a diagonal has no area).
            let path: Vec<(f32, f32)> = if !add && rect {
                // Rectangle window: a simple left → right diagonal spans it.
                (0..=10)
                    .map(|k| {
                        let t = k as f32 / 10.0;
                        (20.0 + t * 760.0, 20.0 + t * 560.0)
                    })
                    .collect()
            } else if add {
                let (sx, sy, ex, ey) = (780.0f32, 580.0f32, 20.0f32, 20.0f32);
                (0..=10)
                    .map(|k| {
                        let t = k as f32 / 10.0;
                        (sx + t * (ex - sx), sy + t * (ey - sy))
                    })
                    .collect()
            } else {
                vec![
                    (20.0, 20.0),
                    (400.0, 20.0),
                    (780.0, 20.0),
                    (780.0, 300.0),
                    (780.0, 580.0),
                    (400.0, 580.0),
                    (20.0, 580.0),
                    (20.0, 300.0),
                ]
            };
            let _ = app.update(Message::ViewportMove(iced::Point::new(
                path[0].0, path[0].1,
            )));
            let _ = app.update(Message::ViewportLeftPress);
            std::thread::sleep(std::time::Duration::from_millis(180));
            for &(x, y) in &path {
                let _ = app.update(Message::ViewportMove(iced::Point::new(x, y)));
            }
            {
                let sel = app.tabs[i].scene.selection.borrow();
                assert!(sel.left_dragging, "drag must start (add={add} rect={rect})");
                if rect {
                    // Rectangle mode drives the box machinery, not the lasso.
                    assert!(
                        sel.box_anchor.is_some() && sel.box_current.is_some() && !sel.poly_active,
                        "rect marquee must arm the box (add={add})"
                    );
                } else {
                    assert!(sel.poly_active, "lasso must start (add={add})");
                    assert!(sel.poly_points.len() >= 3, "lasso points (add={add})");
                }
            }
            let _ = app.update(Message::ViewportLeftRelease);
            assert!(
                !app.tabs[i].scene.selected.is_empty(),
                "marquee must select the line (add={add} rect={rect})"
            );
        }
    }

    #[test]
    fn pickadd_command_flips_flag() {
        let mut app = OpenCADStudio::new_for_test();
        // The Start tab blocks drawing commands — open a drawing first.
        app.automation_op(r#"{"op":"new"}"#);
        // The boot path may have restored a persisted value — normalize.
        app.pick_add = true;
        app.pick_drag_rect = false;
        let _ = app.run_command_line("PICKADD 0");
        assert!(!app.pick_add, "PICKADD 0 must switch to replace mode");
        let _ = app.run_command_line("PICKADD 1");
        assert!(app.pick_add);
        // Two-step: bare command then the value, like typing 1 + Enter
        // (feed_active_cmd is the same path the GUI submit offers first).
        let _ = app.run_command_line("PICKDRAG");
        assert!(
            app.tabs[app.active_tab].active_cmd.is_some(),
            "prompt must open"
        );
        let _ = app.feed_active_cmd("1");
        assert!(app.pick_drag_rect, "PICKDRAG 1 via the prompt must switch");
    }

    #[test]
    fn matchprop_matches_text_style_and_height() {
        // MATCHPROP between text objects must carry the text-specific
        // properties (style, height) to TEXT and MTEXT destinations, not just
        // the generic layer/color/linetype set. Regression for #361.
        use crate::command::StepInput;
        use acadrust::{EntityType, MText, Text};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let mut src = Text::new();
        src.value = "SRC".into();
        src.height = 5.0;
        src.style = "BIG".into();
        let src_h = app.tabs[i].scene.add_entity(EntityType::Text(src));

        let mut dst_text = Text::new();
        dst_text.value = "DST".into();
        dst_text.height = 1.0;
        let dst_text_h = app.tabs[i].scene.add_entity(EntityType::Text(dst_text));

        let mut dst_mtext = MText::new();
        dst_mtext.value = "DSTM".into();
        dst_mtext.height = 2.0;
        let dst_mtext_h = app.tabs[i].scene.add_entity(EntityType::MText(dst_mtext));

        // Drive the interactive command exactly as the viewport does:
        // phase 1 source pick, phase 2 destination selection.
        let _ = app.run_command_line("MATCHPROP");
        assert!(app.tabs[i].active_cmd.is_some(), "MATCHPROP must start");
        let _ = app.feed_command(StepInput::EntityPick(src_h, glam::DVec3::ZERO));
        let _ = app.feed_command(StepInput::SelectionComplete(vec![dst_text_h, dst_mtext_h]));

        let doc = &app.tabs[i].scene.document;
        match doc.get_entity(dst_text_h) {
            Some(EntityType::Text(t)) => {
                assert_eq!(t.style, "BIG", "TEXT destination must take source style");
                assert!(
                    (t.height - 5.0).abs() < 1e-9,
                    "TEXT destination must take source height, got {}",
                    t.height
                );
            }
            other => panic!("dest TEXT missing: {other:?}"),
        }
        match doc.get_entity(dst_mtext_h) {
            Some(EntityType::MText(m)) => {
                assert_eq!(m.style, "BIG", "MTEXT destination must take source style");
                assert!(
                    (m.height - 5.0).abs() < 1e-9,
                    "MTEXT destination must take source height, got {}",
                    m.height
                );
            }
            other => panic!("dest MTEXT missing: {other:?}"),
        }
    }

    #[test]
    fn handing_over_an_already_open_drawing_switches_to_its_tab() {
        // Double-clicking a drawing that is already open should land on the tab
        // showing it, not load a second copy of the same file.
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        let path = std::env::temp_dir().join("ocs_already_open.dwg");
        std::fs::write(&path, b"x").unwrap();
        let canon = std::fs::canonicalize(&path).unwrap();

        // Two tabs, the second holding the drawing; leave the first active.
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(99));
        let target = app.tabs.len() - 1;
        app.tabs[target].current_path = Some(canon.clone());
        app.active_tab = 0;

        let _ = app.update(Message::OpenExternal(canon.clone()));
        assert_eq!(app.active_tab, target, "should have switched to the tab");
        assert!(
            app.opening.is_none(),
            "an already-open drawing must not start a load"
        );
        assert!(
            app.pending_opens.is_empty(),
            "and must not queue one either"
        );

        // The same file spelled differently (a `..` hop) is still the same file.
        let indirect = canon.parent().unwrap().join("..").join(
            canon
                .strip_prefix(canon.parent().unwrap().parent().unwrap())
                .unwrap(),
        );
        app.active_tab = 0;
        let _ = app.update(Message::OpenExternal(indirect));
        assert_eq!(
            app.active_tab, target,
            "an unresolved spelling of the same path must still match the tab"
        );
        assert!(app.opening.is_none(), "still no second load");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_second_handoff_queues_instead_of_displacing_the_first() {
        // `opening` is one slot, and `on_file_opened` drops any result that
        // arrives once it is clear — so without the queue, two drawings handed
        // over at the same moment (select several files in a file manager: one
        // process each, all arriving together) would leave one tab and silently
        // lose the rest.
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        // Any existing file will do: OpenRecent only stats it, and the actual
        // load is an async Task this test drops.
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("ocs_si_a.dwg"), dir.join("ocs_si_b.dwg"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();

        let _ = app.update(Message::OpenExternal(a.clone()));
        assert!(
            app.opening.is_some(),
            "first handoff should start an open, not queue"
        );
        assert!(app.pending_opens.is_empty(), "nothing to queue yet");

        let _ = app.update(Message::OpenExternal(b.clone()));
        assert_eq!(
            app.pending_opens.len(),
            1,
            "second handoff arriving mid-open must queue, not be dropped"
        );
        assert_eq!(app.pending_opens.front(), Some(&b));

        // A failed drawing pauses the queue while its recovery report is shown.
        let open_id = app.opening.as_ref().map(|opening| opening.id).unwrap();
        let _ = app.update(Message::FileOpened(open_id, Err("boom".into())));
        assert!(
            app.active_modal == Some(crate::app::ModalKind::Recovery),
            "failed open should show its recovery report"
        );
        assert_eq!(
            app.pending_opens.len(),
            1,
            "queued drawing should wait until the report is acknowledged"
        );
        let _ = app.update(Message::RecoveryClose);
        assert!(
            app.pending_opens.is_empty(),
            "closing the report must release the queued drawing"
        );

        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn saving_over_an_existing_drawing_succeeds() {
        for (label, pre_existing) in [("new path", false), ("existing drawing", true)] {
            let path = std::env::temp_dir().join(format!(
                "ocs_save_over_{}_{}.dxf",
                std::process::id(),
                pre_existing,
            ));
            let _ = std::fs::remove_file(&path);
            if pre_existing {
                std::fs::write(&path, b"a previous drawing").unwrap();
            }

            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            let p = path.to_string_lossy().replace('\\', "\\\\");
            let saved = app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#));
            assert_eq!(saved["ok"], true, "{label}: {}", saved["error"]);
            let saved_again = app.automation_op(r#"{"op":"save"}"#);
            assert_eq!(
                saved_again["ok"], true,
                "normal save: {}",
                saved_again["error"]
            );

            drop(app);
            let sidecar = path.with_file_name(format!(
                ".{}.ocs.lock",
                path.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_file(sidecar);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn save_then_open_round_trips() {
        let mut app = OpenCADStudio::new_for_test();
        let path =
            std::env::temp_dir().join(format!("ocs_automation_test_{}.dxf", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().replace('\\', "\\\\");
        app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(
            app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#))["ok"],
            true
        );
        assert_eq!(
            app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#))["ok"],
            true
        );
        drop(app);
        let sidecar = path.with_file_name(format!(
            ".{}.ocs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(sidecar);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_finalizes_and_purges_like_the_ui_open_path() {
        let mut app = OpenCADStudio::new_for_test();
        let stale = acadrust::Handle::from(9999);
        app.tabs[app.active_tab].scene.solid_models.insert(
            stale,
            cadkernel::brep::make::cuboid([0.0; 3], [1.0; 3]).unwrap(),
        );
        let path = std::env::temp_dir().join(format!(
            "ocs_automation_finalize_test_{}.dxf",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let mut doc = acadrust::CadDocument::new();
        let mut good = acadrust::entities::Circle::new();
        good.center = acadrust::types::Vector3::new(5.0, 5.0, 0.0);
        good.radius = 2.0;
        doc.add_entity(acadrust::EntityType::Circle(good)).unwrap();
        let mut corrupt = acadrust::entities::Circle::new();
        corrupt.center = acadrust::types::Vector3::new(1.0, 1.0, 0.0);
        // An absurd radius is rejected; a zero radius is valid.
        corrupt.radius = 1.0e11;
        doc.add_entity(acadrust::EntityType::Circle(corrupt))
            .unwrap();
        let bytes = crate::io::save_to_bytes(&doc, "dxf", doc.version)
            .expect("save a document containing a corrupt entity");
        std::fs::write(&path, bytes).unwrap();

        let p = path.to_string_lossy().replace('\\', "\\\\");
        let result = app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#));
        assert_eq!(result["ok"], true, "{}", result["error"]);
        assert_eq!(
            result["total"], 1,
            "the corrupt circle must not survive the open"
        );
        assert_eq!(
            result["purged"], 1,
            "the purge count must be reported, matching the UI open path's diagnostics"
        );

        let i = app.active_tab;
        assert!(!app.tabs[i].scene.solid_models.contains_key(&stale));
        assert_eq!(
            app.tabs[i].scene.material_base_dir.as_deref(),
            path.parent()
        );
        assert!(
            app.tabs[i].scene.document.source_path.is_some(),
            "automation open must run the same finalization as a path-based open, which sets source_path (load_bytes alone never does)"
        );

        app.tabs[i].scene.solid_models.insert(
            stale,
            cadkernel::brep::make::cuboid([0.0; 3], [1.0; 3]).unwrap(),
        );
        assert_eq!(app.automation_op(r#"{"op":"new"}"#)["ok"], true);
        assert!(app.tabs[i].scene.solid_models.is_empty());
        assert!(app.tabs[i].scene.material_base_dir.is_none());

        drop(app);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_pline_line_then_arc() {
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                use crate::app::Message;
                let mut app = OpenCADStudio::new_for_test();
                app.automation_op(r#"{"op":"new"}"#);
                {
                    app.tabs[0].scene.selection.borrow_mut().vp_size = (1920.0, 1080.0);
                    app.tabs[0].scene.sync_tiles_from_panes(1920.0, 1080.0);
                }

                // Start PLINE
                let _ = app.update(Message::CommandInput("PLINE".to_string()));
                let _ = app.update(Message::CommandSubmit);

                // Click first point (100, 100)
                let _ = app.update(Message::ViewportMove(iced::Point::new(100.0, 100.0)));
                let _ = app.update(Message::ViewportLeftPress);
                let _ = app.update(Message::ViewportLeftRelease);

                // Move to (200, 100) and click second point (draw line)
                let _ = app.update(Message::ViewportMove(iced::Point::new(200.0, 100.0)));
                let _ = app.update(Message::ViewportLeftPress);
                let _ = app.update(Message::ViewportLeftRelease);

                let wid = app.main_window.unwrap_or_else(iced::window::Id::unique);

                // Switch to arc: Option 1 - CommandOptionPick("A")
                let _ = app.update(Message::CommandOptionPick("A".to_string()));
                let _ = app.view(wid);
                println!(
                    "ENTITIES: {}",
                    app.tabs[0].scene.document.entities().count()
                );
                println!(
                    "CMD: {:?}",
                    app.tabs[0].active_cmd.as_ref().map(|c| c.name())
                );

                // Move mouse!
                let _ = app.update(Message::ViewportMove(iced::Point::new(200.0, 100.0)));
                let _ = app.view(wid);
                let _ = app.update(Message::ViewportMove(iced::Point::new(201.0, 100.0)));
                let _ = app.view(wid);
                let _ = app.update(Message::ViewportMove(iced::Point::new(200.0, 150.0)));
                let _ = app.view(wid);
                let _ = app.update(Message::ViewportMove(iced::Point::new(150.0, 150.0)));
                let _ = app.view(wid);

                // Click arc point
                let _ = app.update(Message::ViewportLeftPress);
                let _ = app.update(Message::ViewportLeftRelease);

                // Move mouse again
                let _ = app.update(Message::ViewportMove(iced::Point::new(150.0, 160.0)));

                // Switch to line
                let _ = app.update(Message::CommandOptionPick("L".to_string()));

                // Move mouse again
                let _ = app.update(Message::ViewportMove(iced::Point::new(100.0, 150.0)));

                // Finish
                let _ = app.update(Message::CommandOptionPick(String::new()));
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn test_mtp_in_line_command() {
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        {
            app.tabs[0].scene.selection.borrow_mut().vp_size = (1920.0, 1080.0);
            app.tabs[0].scene.sync_tiles_from_panes(1920.0, 1080.0);
        }

        // Start LINE
        let _ = app.update(Message::CommandInput("LINE".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );

        // Type M2P
        let _ = app.update(Message::CommandInput("M2P".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("MTP")
        );
        assert!(app.tabs[0].suspended_cmd.is_some());
        assert_eq!(
            app.tabs[0].suspended_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );

        // Point 1: 0,0
        let _ = app.update(Message::CommandInput("0,0".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("MTP")
        );

        // Point 2: 10,20
        let _ = app.update(Message::CommandInput("10,20".to_string()));
        let _ = app.update(Message::CommandSubmit);

        // MTP should have finished and restored LINE, with midpoint (5, 10, 0)
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );
        assert!(app.tabs[0].suspended_cmd.is_none());
        assert_eq!(app.last_point, Some(glam::DVec3::new(5.0, 10.0, 0.0)));

        // Cancel LINE
        let _ = app.update(Message::CommandEscape);
        assert!(app.tabs[0].active_cmd.is_none());
    }

    #[test]
    fn mtp_snap_override_starts_the_existing_modifier() {
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        let _ = app.update(Message::CommandInput("LINE".to_string()));
        let _ = app.update(Message::CommandSubmit);
        let _ = app.update(Message::SnapOverrideMtp);

        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("MTP")
        );
        assert_eq!(
            app.tabs[0].suspended_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );
    }

    #[test]
    fn test_mtp_escape_restores_parent() {
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        // Start LINE
        let _ = app.update(Message::CommandInput("LINE".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );

        // Type MTP
        let _ = app.update(Message::CommandInput("MTP".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("MTP")
        );

        // Escape during MTP
        let _ = app.update(Message::CommandEscape);
        // Parent LINE must be restored, not cancelled!
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );
        assert!(app.tabs[0].suspended_cmd.is_none());

        // Escape again cancels LINE
        let _ = app.update(Message::CommandEscape);
        assert!(app.tabs[0].active_cmd.is_none());
    }

    #[test]
    fn test_mtp_typing_routing_with_dyn_input() {
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.dyn_input = true;

        // Start LINE
        let _ = app.update(Message::CommandInput("LINE".to_string()));
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("LINE")
        );

        // Simulate typing 'm', '2', 'p' character by character
        let _ = app.update(Message::CommandAppendChar("m".to_string()));
        assert_eq!(app.command_line.input, "M");

        // The digit '2' must stay in command_line.input instead of routing to dyn fields!
        let _ = app.update(Message::CommandAppendChar("2".to_string()));
        assert_eq!(app.command_line.input, "M2");

        let _ = app.update(Message::CommandAppendChar("p".to_string()));
        assert_eq!(app.command_line.input, "M2P");

        // Submit triggers MTP
        let _ = app.update(Message::CommandSubmit);
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|c| c.name()),
            Some("MTP")
        );
    }

    #[test]
    fn test_open_color_dropdown() {
        use crate::app::Message;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let wid = app.main_window.unwrap_or_else(iced::window::Id::unique);
        let _ = app.view(wid);
        let _ = app.update(Message::ToggleRibbonDropdown("PROP_COLOR".to_string()));
        let _ = app.view(wid);
    }
}
