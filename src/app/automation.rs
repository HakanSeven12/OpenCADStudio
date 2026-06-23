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

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use super::OpenCADStudio;

/// Run the headless JSON server. Default transport is stdin/stdout; with
/// `--port <N>` it instead listens on `127.0.0.1:<N>` and serves one client at
/// a time (the document session persists across reconnects).
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
pub fn export_headless(input: &std::path::Path, output: &std::path::Path) -> i32 {
    let doc = match crate::io::load_file(input) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("export: cannot read {}: {e}", input.display());
            return 1;
        }
    };
    match crate::io::save(&doc, output) {
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
fn port_arg() -> Option<u16> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == "--port" {
            return args.next().and_then(|s| s.parse().ok());
        }
    }
    None
}

fn ready() -> Value {
    json!({ "ok": true, "ready": true, "version": env!("CARGO_PKG_VERSION") })
}

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

/// One entity as JSON: handle, type, layer, plus basic geometry for the common
/// types (others carry only the common fields).
fn entity_json(e: &acadrust::EntityType) -> Value {
    use acadrust::EntityType as E;
    let c = e.common();
    let mut obj = json!({
        "handle": format!("{:X}", c.handle.value()),
        "type": crate::entities::names::ui_name(e),
        "layer": c.layer,
    });
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
            map.insert("position".into(), v3(t.insertion_point));
            map.insert("height".into(), json!(t.height));
        }
        E::MText(t) => {
            map.insert("value".into(), json!(t.value));
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
        }
        _ => {}
    }
    obj
}

impl OpenCADStudio {
    /// Handle one JSON request line and return the JSON response.
    pub(crate) fn automation_op(&mut self, line: &str) -> Value {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return err(format!("invalid JSON: {e}")),
        };
        match req["op"].as_str().unwrap_or("") {
            "new" => {
                let i = self.active_tab;
                self.tabs[i].scene.document = acadrust::CadDocument::new();
                self.tabs[i].current_path = None;
                // The headless session starts on the welcome (Start) tab, which
                // blocks drawing commands; turn it into a real drawing.
                self.tabs[i].is_start = false;
                self.tabs[i].scene.bump_geometry();
                self.entity_summary()
            }
            "open" => {
                let Some(path) = req["path"].as_str() else {
                    return err("open: missing \"path\"");
                };
                let bytes = match std::fs::read(path) {
                    Ok(b) => b,
                    Err(e) => return err(format!("open: {e}")),
                };
                let name = PathBuf::from(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string());
                match crate::io::load_bytes(&name, bytes) {
                    Ok(doc) => {
                        let i = self.active_tab;
                        self.tabs[i].scene.document = doc;
                        self.tabs[i].adopt_active_ucs_from_header();
                        self.tabs[i].current_path = Some(PathBuf::from(path));
                        self.tabs[i].is_start = false;
                        self.tabs[i].scene.bump_geometry();
                        self.entity_summary()
                    }
                    Err(e) => err(format!("open: {e}")),
                }
            }
            "run" => {
                let cmd = req["cmd"].as_str().unwrap_or("").to_string();
                if cmd.is_empty() {
                    return err("run: missing \"cmd\"");
                }
                let i = self.active_tab;
                let before = self.tabs[i].scene.document.entities().count();
                self.run_headless(&cmd);
                let after = self.tabs[i].scene.document.entities().count();
                json!({
                    "ok": true,
                    "cmd": cmd,
                    "entities": after,
                    "added": after as i64 - before as i64,
                })
            }
            "entities" => self.entity_summary(),
            "query" => self.entity_query(&req),
            "layers" => {
                let i = self.active_tab;
                let layers: Vec<Value> = self
                    .tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
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
                            if let Ok(v) = u64::from_str_radix(h.trim_start_matches("0x"), 16) {
                                self.tabs[i].scene.select_entity(acadrust::Handle::new(v), false);
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
                            .filter(|e| {
                                type_filter.is_none_or(|t| {
                                    crate::entities::names::ui_name(e).eq_ignore_ascii_case(t)
                                })
                            })
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
                match crate::io::save(&self.tabs[i].scene.document, &path) {
                    Ok(()) => {
                        self.tabs[i].current_path = Some(path.clone());
                        json!({ "ok": true, "saved": path.to_string_lossy() })
                    }
                    Err(e) => err(format!("save: {e}")),
                }
            }
            "" => err("missing \"op\""),
            other => err(format!("unknown op: {other}")),
        }
    }

    /// Run a command headlessly. Single-word and inline-argument commands
    /// (`PDMODE 3`, `LAYER Walls`) dispatch as-is. For an interactive tool with
    /// coordinate arguments (`LINE 0,0 10,10`) the first word starts the tool
    /// and the remaining tokens are fed as points / option keywords, then the
    /// command is terminated as if Enter were pressed.
    fn run_headless(&mut self, cmd: &str) {
        let i = self.active_tab;
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        if tokens.len() <= 1 {
            let _ = self.dispatch_command(cmd);
            return;
        }
        let _ = self.dispatch_command(tokens[0]);
        if self.tabs[i].active_cmd.is_none() {
            // Not an interactive tool — an inline-argument command.
            let _ = self.dispatch_command(cmd);
            return;
        }
        self.last_point = None;
        for tok in &tokens[1..] {
            if self.tabs[i].active_cmd.is_none() {
                break;
            }
            self.feed_active_cmd(tok);
        }
        // Terminate a still-open command (LINE / PLINE finish on Enter).
        if self.tabs[i].active_cmd.is_some() {
            if let Some(r) = self.tabs[i].active_cmd.as_mut().map(|c| c.on_enter()) {
                let _ = self.apply_cmd_result(r);
            }
        }
    }

    /// Feed one token to the active command. When the command is picking an
    /// existing entity, the token is a hex handle; otherwise a coordinate point
    /// or an option keyword.
    fn feed_active_cmd(&mut self, token: &str) {
        let i = self.active_tab;
        // Object-pick step: the token is a handle (as returned by `query`).
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.needs_entity_pick())
        {
            if let Ok(v) = u64::from_str_radix(token.trim_start_matches("0x"), 16) {
                let handle = acadrust::Handle::new(v);
                let pt = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|e| {
                        let bb = e.as_entity().bounding_box();
                        glam::Vec3::new(
                            ((bb.min.x + bb.max.x) * 0.5) as f32,
                            ((bb.min.y + bb.max.y) * 0.5) as f32,
                            0.0,
                        )
                    })
                    .unwrap_or(glam::Vec3::ZERO);
                if let Some(r) = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|c| c.on_entity_pick(handle, pt))
                {
                    let _ = self.apply_cmd_result(r);
                }
            }
            return;
        }
        if let Some((mut pt, kind)) = super::helpers::parse_coord(token) {
            if matches!(kind, super::helpers::CoordKind::Relative) {
                if let Some(base) = self.last_point {
                    pt += base;
                }
            }
            self.last_point = Some(pt);
            if let Some(r) = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt)) {
                let _ = self.apply_cmd_result(r);
            }
        } else if let Some(r) = self.tabs[i]
            .active_cmd
            .as_mut()
            .and_then(|c| c.on_text_input(token))
        {
            let _ = self.apply_cmd_result(r);
        }
    }

    /// List entities (handle, type, layer, basic geometry), optionally filtered
    /// by `type` and/or `layer`, capped by `limit` (default 1000).
    fn entity_query(&self, req: &Value) -> Value {
        let i = self.active_tab;
        let type_filter = req["type"].as_str();
        let layer_filter = req["layer"].as_str();
        let limit = req["limit"].as_u64().unwrap_or(1000) as usize;

        let mut entities = Vec::new();
        let mut matched = 0u64;
        for e in self.tabs[i].scene.document.entities() {
            if let Some(tf) = type_filter {
                if !crate::entities::names::ui_name(e).eq_ignore_ascii_case(tf) {
                    continue;
                }
            }
            if let Some(lf) = layer_filter {
                if e.common().layer != lf {
                    continue;
                }
            }
            matched += 1;
            if entities.len() < limit {
                entities.push(entity_json(e));
            }
        }
        json!({
            "ok": true,
            "count": matched,
            "returned": entities.len(),
            "entities": entities,
        })
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
    use crate::app::OpenCADStudio;

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
    fn save_then_open_round_trips() {
        let mut app = OpenCADStudio::new_for_test();
        let path = std::env::temp_dir().join("ocs_automation_test.dxf");
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
        let _ = std::fs::remove_file(&path);
    }
}
