//! Structured external-reference reads and lifecycle mutations.
//!
//! The dispatcher integration deliberately lives in `control/mod.rs`; this
//! module owns only request validation, shared `ReferenceEntry` serialization,
//! and calls into the same engine paths used by XREF and its palette.

use crate::app::control::ref_status_name;
use crate::app::{Message, OpenCADStudio};
use crate::io::xref_model::{Pathtype, RefKind, RefStatus, RefType, ReferenceEntry};
use document::{Document, Handle};
use iced::Task;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn failure(code: &str, error: impl ToString) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":error.to_string()})
}

/// Strict operation-local fields on every transport, not just MCP preflight.
/// Envelope and transport keys are shared with the other protocol-1 writes.
fn validate_fields(req: &Value, fields: &[&str]) -> Result<(), Value> {
    let object = req
        .as_object()
        .ok_or_else(|| failure("invalid_request", "xref operations expect an object"))?;
    for key in object.keys() {
        if fields.contains(&key.as_str())
            || matches!(
                key.as_str(),
                "op" | "request_id"
                    | "document_id"
                    | "revision"
                    | "geometry_revision"
                    | "camera_revision"
                    | "selection"
                    | "client_id"
                    | "steps"
                    | "protocol"
                    | "session_id"
                    | "token"
                    | "active_only"
                    | "bindings"
            )
        {
            continue;
        }
        let operation = req["op"].as_str().unwrap_or("xref");
        return Err(failure(
            "unknown_xref_field",
            format!("Unknown {operation} field '{key}'"),
        ));
    }
    Ok(())
}

fn kind_name(kind: RefKind) -> &'static str {
    match kind {
        RefKind::DwgXref => "dwg_xref",
        RefKind::Image => "image",
        RefKind::Underlay => "underlay",
        RefKind::PointCloud => "point_cloud",
    }
}

fn path_format(path: &str) -> Option<String> {
    Path::new(&path.replace('\\', "/"))
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}

fn reference_format(document: &Document, entry: &ReferenceEntry) -> Option<String> {
    if entry.kind == RefKind::Underlay {
        if let Some(document::objects::ObjectType::UnderlayDefinition(definition)) =
            document.objects.get(&Handle::new(entry.key))
        {
            return Some(
                match definition.underlay_type {
                    document::entities::UnderlayType::Pdf => "pdf",
                    document::entities::UnderlayType::Dwf => "dwf",
                    document::entities::UnderlayType::Dgn => "dgn",
                }
                .to_string(),
            );
        }
    }
    path_format(&entry.saved_path)
}

fn modified_unix_ms(modified: Option<std::time::SystemTime>) -> Option<i64> {
    modified.and_then(|time| match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_millis()).ok(),
        Err(error) => i64::try_from(error.duration().as_millis())
            .ok()
            .and_then(|millis| millis.checked_neg()),
    })
}

/// Serialize the shared manager row without reconstituting reference state from
/// the legacy `(name, path, status)` automation cache.
pub(super) fn reference_json(document: &Document, entry: &ReferenceEntry) -> Value {
    let reference_type = (entry.kind == RefKind::DwgXref).then(|| match entry.ref_type {
        RefType::Attach => "attach",
        RefType::Overlay => "overlay",
    });
    json!({
        "id": format!("{:X}", entry.key),
        "name": entry.name.clone(),
        "kind": kind_name(entry.kind),
        "format": reference_format(document, entry),
        "reference_type": reference_type,
        "status": ref_status_name(entry.status),
        "saved_path": entry.saved_path.clone(),
        "resolved_path": entry.found_at.clone(),
        "parent_id": entry.parent_key.map(|key| format!("{key:X}")),
        "nested": entry.parent_key.is_some(),
        "size_bytes": entry.size_bytes,
        "modified_unix_ms": modified_unix_ms(entry.modified),
    })
}

fn reference_id(req: &Value) -> Result<u64, Value> {
    let id = req
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| failure("invalid_handle", "Supply a hexadecimal reference id"))?;
    u64::from_str_radix(id.trim_start_matches("0x").trim_start_matches("0X"), 16)
        .map_err(|_| failure("invalid_handle", "Expected a hexadecimal reference id"))
}

fn reference_type(req: &Value, default: RefType) -> Result<RefType, Value> {
    match req.get("reference_type") {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("attach") => Ok(RefType::Attach),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("overlay") => Ok(RefType::Overlay),
        Some(_) => Err(failure(
            "invalid_reference_type",
            "reference_type must be attach or overlay",
        )),
    }
}

fn path_type(value: &Value) -> Result<Pathtype, Value> {
    match value {
        Value::String(value) if value.eq_ignore_ascii_case("full") => Ok(Pathtype::Full),
        Value::String(value) if value.eq_ignore_ascii_case("relative") => Ok(Pathtype::Relative),
        Value::String(value) if value.eq_ignore_ascii_case("none") => Ok(Pathtype::None),
        _ => Err(failure(
            "invalid_path_type",
            "path_type must be full, relative or none",
        )),
    }
}

fn kind_unsupported(operation: &str, entry: &ReferenceEntry) -> Value {
    let mut response = failure(
        "xref_kind_unsupported",
        format!(
            "{operation} does not support {} rows",
            kind_name(entry.kind)
        ),
    );
    response["kind"] = json!(kind_name(entry.kind));
    response["operation"] = json!(operation);
    response
}

fn source_missing(path: impl Into<String>, diagnostics: Vec<String>) -> Value {
    let mut response = failure(
        "xref_source_missing",
        "The reference source file was not found",
    );
    response["path"] = json!(path.into());
    response["diagnostics"] = json!(diagnostics);
    response
}

fn load_failed(info: &crate::io::xref::XrefInfo) -> Value {
    let code = if info.status == crate::io::xref::XrefStatus::NotFound {
        "xref_source_missing"
    } else {
        "xref_load_failed"
    };
    let mut response = failure(code, format!("Could not reload xref '{}'", info.name));
    response["path"] = json!(info.path.clone());
    response["diagnostics"] = json!(info.diagnostics.clone());
    response
}

fn path_failure(error: crate::io::xref::ApplyPathtypeError) -> Value {
    use crate::io::xref::ApplyPathtypeError;
    use crate::io::xref_model::PathtypeError;
    match error {
        ApplyPathtypeError::MissingReference => {
            failure("xref_missing", "The reference no longer exists")
        }
        ApplyPathtypeError::Conversion(PathtypeError::AcrossDrives) => failure(
            "xref_path_across_drives",
            "Cannot make the reference path relative across drives",
        ),
        ApplyPathtypeError::Conversion(PathtypeError::UnsavedHost) => failure(
            "xref_path_unsaved_host",
            "Save the drawing first to resolve reference paths",
        ),
        ApplyPathtypeError::Store(error) => failure("xref_path_failed", error),
    }
}

impl OpenCADStudio {
    fn fresh_reference_entries(&mut self, i: usize) -> Vec<ReferenceEntry> {
        self.refresh_xref_report(i)
    }

    fn direct_reference(&mut self, req: &Value) -> Result<ReferenceEntry, Value> {
        let id = reference_id(req)?;
        let i = self.active_tab;
        let entries = self.fresh_reference_entries(i);
        let entry = entries
            .iter()
            .find(|entry| entry.key == id)
            .cloned()
            .ok_or_else(|| {
                failure(
                    "xref_missing",
                    format!("Reference {id:X} does not exist in the active document"),
                )
            })?;
        if entry.parent_key.is_some() {
            return Err(failure(
                "xref_nested_read_only",
                format!(
                    "Nested reference '{}' is read-only; edit it in its host drawing",
                    entry.name
                ),
            ));
        }
        Ok(entry)
    }

    fn reference_after(&mut self, i: usize, id: u64) -> Result<Value, Value> {
        let base_dir = self.tabs[i]
            .current_path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let entry = crate::io::xref::collect_direct_entry_with_prev(
            &self.tabs[i].scene.document,
            base_dir,
            self.tabs[i].xref_unloaded.as_set(),
            &self.tabs[i].xref_stat_cache.0,
            id,
        )
        .ok_or_else(|| {
            failure(
                "xref_missing",
                "The reference disappeared during the operation",
            )
        })?;
        Ok(reference_json(&self.tabs[i].scene.document, &entry))
    }

    fn saved_host_dir(&self, i: usize) -> Result<PathBuf, Value> {
        self.tabs[i]
            .current_path
            .as_deref()
            .and_then(Path::parent)
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                failure(
                    "xref_path_unsaved_host",
                    "Save the drawing first to resolve reference paths",
                )
            })
    }

    fn reject_pending_ref_edit(&mut self, i: usize, error: Value) -> Value {
        self.discard_last_undo_entry(i);
        error
    }

    /// Fresh unified reference read. The scan refreshes the legacy state report
    /// but never opens document history or marks the drawing dirty.
    pub(crate) fn control_xrefs(&mut self, req: &Value) -> Value {
        if let Err(error) = validate_fields(req, &[]) {
            return error;
        }
        let i = self.active_tab;
        let entries = self.fresh_reference_entries(i);
        let document = &self.tabs[i].scene.document;
        let xrefs = entries
            .iter()
            .map(|entry| reference_json(document, entry))
            .collect::<Vec<_>>();
        json!({"ok":true, "count":xrefs.len(), "xrefs":xrefs})
    }

    pub(super) fn control_xref_reload(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id"])?;
        let target = self.direct_reference(req)?;
        let i = self.active_tab;
        let id = target.key;
        if target.kind == RefKind::Image {
            return Err(kind_unsupported("xref_reload", &target));
        }
        let base_dir = self.saved_host_dir(i)?;
        if target.found_at.is_none() {
            return Err(source_missing(target.saved_path, Vec::new()));
        }
        match target.kind {
            RefKind::DwgXref => {
                self.push_undo_snapshot(i, "XREF-RELOAD");
                let handles = [Handle::new(id)].into_iter().collect();
                let infos = crate::io::xref::resolve_xrefs_for_keys(
                    &mut self.tabs[i].scene.document,
                    &base_dir,
                    &handles,
                );
                let Some(info) = infos
                    .iter()
                    .find(|info| info.name.eq_ignore_ascii_case(&target.name))
                    .cloned()
                else {
                    return Err(self.reject_pending_ref_edit(
                        i,
                        failure("xref_load_failed", "The selected xref was not reloaded"),
                    ));
                };
                if !matches!(
                    &info.status,
                    crate::io::xref::XrefStatus::Loaded | crate::io::xref::XrefStatus::Recovered
                ) {
                    return Err(self.reject_pending_ref_edit(i, load_failed(&info)));
                }
                self.tabs[i].xref_unloaded.remove(&id);
                self.tabs[i].xref_stat_cache.remove(&id);
                let fresh = crate::io::xref::collect_entries_with_prev(
                    &self.tabs[i].scene.document,
                    &base_dir,
                    self.tabs[i].xref_unloaded.as_set(),
                    &self.tabs[i].xref_stat_cache.0,
                );
                if let Some(modified) = fresh
                    .iter()
                    .find(|entry| entry.key == id && entry.status == RefStatus::Loaded)
                    .and_then(|entry| entry.modified)
                {
                    self.tabs[i].xref_stat_cache.insert(id, modified);
                }
                self.post_ref_op(i);
                let reference = self.reference_after(i, id)?;
                self.set_control_result(json!({
                    "reference": reference,
                    "diagnostics": info.diagnostics,
                }));
            }
            RefKind::Underlay => {
                self.push_undo_snapshot(i, "XREF-RELOAD");
                let handle = Handle::new(id);
                let Some(document::objects::ObjectType::UnderlayDefinition(definition)) =
                    self.tabs[i].scene.document.objects.get_mut(&handle)
                else {
                    return Err(self.reject_pending_ref_edit(
                        i,
                        failure("xref_missing", "The underlay definition no longer exists"),
                    ));
                };
                definition.unloaded = false;
                self.tabs[i].xref_unloaded.remove(&id);
                self.tabs[i].xref_stat_cache.remove(&id);
                self.tabs[i].scene.reseed_underlays();
                self.post_ref_op(i);
                let reference = self.reference_after(i, id)?;
                self.set_control_result(json!({"reference": reference, "diagnostics": []}));
            }
            RefKind::PointCloud => {
                self.push_undo_snapshot(i, "XREF-RELOAD");
                crate::io::xref::set_point_cloud_loaded(
                    &mut self.tabs[i].scene.document,
                    Handle::new(id),
                    true,
                );
                self.tabs[i].xref_unloaded.remove(&id);
                self.tabs[i].xref_stat_cache.remove(&id);
                self.tabs[i].scene.bump_geometry();
                self.post_ref_op(i);
                let reference = self.reference_after(i, id)?;
                self.set_control_result(json!({"reference": reference, "diagnostics": []}));
            }
            RefKind::Image => unreachable!("image reload rejected above"),
        }
        Ok(Task::none())
    }

    pub(super) fn control_xref_unload(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id"])?;
        let target = self.direct_reference(req)?;
        let i = self.active_tab;
        let id = target.key;
        // `unload_reference` writes UnderlayDefinition.unloaded and the point
        // cloud definition's is_loaded flag. Drawing and raster loadedness is
        // instead held in DocumentTab::xref_unloaded for this session.
        let session_only = matches!(target.kind, RefKind::DwgXref | RefKind::Image);
        if target.status == RefStatus::Unloaded {
            let reference = reference_json(&self.tabs[i].scene.document, &target);
            self.set_control_result(json!({
                "reference": reference,
                "session_only": session_only,
                "unchanged": true,
            }));
            return Ok(Task::none());
        }
        self.push_undo_snapshot(i, "XREF-UNLOAD");
        let name = match crate::io::xref::unload_reference(&mut self.tabs[i].scene.document, id) {
            Ok(name) => name,
            Err(error) => {
                return Err(self.reject_pending_ref_edit(i, failure("xref_unload_failed", error)))
            }
        };
        self.tabs[i].xref_unloaded.add(id);
        self.tabs[i].scene.reseed_underlays();
        self.tabs[i].scene.bump_geometry();
        self.post_ref_op(i);
        let reference = self.reference_after(i, id)?;
        self.set_control_result(json!({
            "name": name,
            "reference": reference,
            "session_only": session_only,
            "unchanged": false,
        }));
        Ok(Task::none())
    }

    pub(super) fn control_xref_detach(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id"])?;
        let target = self.direct_reference(req)?;
        let i = self.active_tab;
        let id = target.key;
        self.push_undo_snapshot(i, "XREF-DETACH");
        let name = match crate::io::xref::detach_reference(&mut self.tabs[i].scene.document, id) {
            Ok(name) => name,
            Err(error) => {
                return Err(self.reject_pending_ref_edit(i, failure("xref_detach_failed", error)))
            }
        };
        self.tabs[i].xref_unloaded.remove(&id);
        self.tabs[i].xref_stat_cache.remove(&id);
        self.post_ref_op(i);
        self.set_control_result(json!({
            "detached": true,
            "id": format!("{id:X}"),
            "name": name,
        }));
        Ok(Task::none())
    }

    pub(super) fn control_xref_bind(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id"])?;
        let target = self.direct_reference(req)?;
        if target.kind != RefKind::DwgXref {
            return Err(kind_unsupported("xref_bind", &target));
        }
        let i = self.active_tab;
        let id = target.key;
        let host_dir = self.saved_host_dir(i)?;
        if target.found_at.is_none() {
            return Err(source_missing(target.saved_path, Vec::new()));
        }
        self.push_undo_snapshot(i, "XREF-BIND");
        let outcome = match crate::io::xref::bind_reference(
            &mut self.tabs[i].scene.document,
            id,
            &host_dir,
            &host_dir,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(self.reject_pending_ref_edit(i, failure("xref_bind_failed", error)))
            }
        };
        self.tabs[i].xref_unloaded.remove(&id);
        self.tabs[i].xref_stat_cache.remove(&id);
        self.post_ref_op(i);
        self.set_control_result(json!({
            "id": format!("{id:X}"),
            "block": outcome.name,
            "unremapped": outcome.unremapped,
        }));
        Ok(Task::none())
    }

    pub(super) fn control_xref_set_path(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id", "path", "path_type"])?;
        let has_path = req.get("path").is_some();
        let has_path_type = req.get("path_type").is_some();
        if has_path == has_path_type
            || req.get("path").is_some_and(Value::is_null)
            || req.get("path_type").is_some_and(Value::is_null)
        {
            return Err(failure(
                "invalid_request",
                "Supply exactly one of path or path_type",
            ));
        }
        let requested_path_type = req.get("path_type").map(path_type).transpose()?;
        let new_path = if has_path {
            Some(
                req.get("path")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| failure("path_required", "path must be a non-empty string"))?
                    .to_string(),
            )
        } else {
            None
        };
        let target = self.direct_reference(req)?;
        let i = self.active_tab;
        let id = target.key;
        let old_path = target.saved_path.clone();
        if new_path.as_deref() == Some(old_path.as_str()) {
            let reference = reference_json(&self.tabs[i].scene.document, &target);
            self.set_control_result(json!({
                "old_path": old_path.clone(),
                "saved_path": old_path,
                "reload_required": false,
                "unchanged": true,
                "reference": reference,
            }));
            return Ok(Task::none());
        }

        let host = requested_path_type.map(|_| self.tabs[i].current_path.clone());
        if requested_path_type.is_some() {
            self.saved_host_dir(i)?;
        }
        self.push_undo_snapshot(i, "XREF-PATH");
        let saved_path = if let Some(new_path) = new_path {
            match crate::io::xref::set_ref_path(&mut self.tabs[i].scene.document, id, &new_path) {
                Ok(_) => new_path,
                Err(error) => {
                    return Err(self.reject_pending_ref_edit(i, failure("xref_path_failed", error)))
                }
            }
        } else {
            let path_type = requested_path_type.expect("exactly one path field");
            let host = host.flatten().unwrap_or_else(|| PathBuf::from("."));
            match crate::io::xref::apply_pathtype_typed(
                &mut self.tabs[i].scene.document,
                id,
                path_type,
                &host,
            ) {
                Ok(path) => path,
                Err(error) => {
                    return Err(self.reject_pending_ref_edit(i, path_failure(error)));
                }
            }
        };
        if saved_path == old_path {
            self.drop_unchanged_pending_history(i);
            let reference = reference_json(&self.tabs[i].scene.document, &target);
            self.set_control_result(json!({
                "old_path": old_path,
                "saved_path": saved_path,
                "reload_required": false,
                "unchanged": true,
                "reference": reference,
            }));
            return Ok(Task::none());
        }
        self.post_ref_op(i);
        let reference = self.reference_after(i, id)?;
        self.set_control_result(json!({
            "old_path": old_path,
            "saved_path": saved_path,
            "reload_required": true,
            "unchanged": false,
            "reference": reference,
        }));
        Ok(Task::none())
    }

    pub(super) fn control_xref_set_type(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        validate_fields(req, &["id", "reference_type"])?;
        let requested = reference_type(req, RefType::Attach)?;
        if req.get("reference_type").is_none()
            || req.get("reference_type").is_some_and(Value::is_null)
        {
            return Err(failure(
                "invalid_reference_type",
                "reference_type must be attach or overlay",
            ));
        }
        let target = self.direct_reference(req)?;
        if target.kind != RefKind::DwgXref {
            return Err(kind_unsupported("xref_set_type", &target));
        }
        let i = self.active_tab;
        let id = target.key;
        if target.ref_type == requested {
            let reference = reference_json(&self.tabs[i].scene.document, &target);
            self.set_control_result(json!({"reference": reference, "unchanged": true}));
            return Ok(Task::none());
        }
        self.push_undo_snapshot(
            i,
            if requested == RefType::Overlay {
                "XREF-OVERLAY"
            } else {
                "XREF-ATTACH"
            },
        );
        if let Err(error) =
            crate::io::xref::set_ref_type(&mut self.tabs[i].scene.document, id, requested)
        {
            return Err(self.reject_pending_ref_edit(i, failure("xref_type_failed", error)));
        }
        self.post_ref_op(i);
        let reference = self.reference_after(i, id)?;
        self.set_control_result(json!({"reference": reference, "unchanged": false}));
        Ok(Task::none())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn scratch(tag: &str) -> Scratch {
        let serial = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ocs_control_xref_{tag}_{}_{}",
            std::process::id(),
            serial
        ));
        std::fs::create_dir_all(&dir).expect("create xref test directory");
        Scratch(dir)
    }

    fn envelope(app: &OpenCADStudio, mut request: Value) -> Value {
        request["protocol"] = json!(1);
        request["document_id"] = json!(app.tabs[app.active_tab].id);
        if request.get("request_id").is_none() {
            request["request_id"] = json!(format!(
                "xref-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        }
        request
    }

    fn call(app: &mut OpenCADStudio, request: Value) -> Value {
        let request = envelope(app, request);
        app.automation_op(&request.to_string())
    }

    fn list(app: &mut OpenCADStudio) -> Vec<Value> {
        let response = call(app, json!({"op":"xrefs"}));
        assert_eq!(response["ok"], true, "{response}");
        response["xrefs"].as_array().expect("xref rows").clone()
    }

    fn write_dwg(dir: &Path, name: &str, line_end: f64) -> PathBuf {
        let mut source = Document::new();
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::ZERO;
        line.end = document::types::Vector3::new(line_end, 0.0, 0.0);
        line.common.owner_handle = source.header.model_space_block_handle;
        source
            .add_entity(document::EntityType::Line(line))
            .expect("add source line");
        let bytes = crate::io::save_to_bytes(&source, "dwg", source.version).expect("encode DWG");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write source DWG");
        path
    }

    fn app_in(dir: &Path) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let opened = app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(opened["ok"], true, "{opened}");
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        app.tabs[i].dirty = false;
        app
    }

    /// Build a placed source reference without opening history. The structured
    /// attach operation is intentionally withheld until its commit can report
    /// insertion failure atomically.
    fn placed_reference(app: &mut OpenCADStudio, source: &Path) -> (String, String) {
        let request = crate::modules::insert::xattach::XrefAttachRequest {
            path: source.to_string_lossy().into_owned(),
            overlay: false,
            path_type: Pathtype::Relative,
        };
        let i = app.active_tab;
        let host = app.tabs[i].current_path.clone();
        let name = crate::modules::insert::xattach::prepare_xref_definition(
            &mut app.tabs[i].scene,
            &request,
            host.as_deref(),
        );
        let id = app.tabs[i]
            .scene
            .document
            .block_records
            .get(&name)
            .expect("fixture definition")
            .handle
            .value();
        let insert = app.tabs[i].scene.add_entity(document::EntityType::Insert(
            document::entities::Insert::new(&name, document::types::Vector3::ZERO),
        ));
        app.tabs[i].dirty = false;
        (format!("{id:X}"), format!("{:X}", insert.value()))
    }

    fn history_depth(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn assert_quiet_rejection(app: &mut OpenCADStudio, request: Value, code: &str) {
        let i = app.active_tab;
        let before = (
            app.tabs[i].edit_revision,
            app.tabs[i].scene.geometry_epoch,
            app.tabs[i].dirty,
            history_depth(app),
        );
        let response = call(app, request);
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["status"], "failed", "{response}");
        assert_eq!(response["code"], code, "{response}");
        let after = (
            app.tabs[i].edit_revision,
            app.tabs[i].scene.geometry_epoch,
            app.tabs[i].dirty,
            history_depth(app),
        );
        assert_eq!(after, before, "{response}");
    }

    #[test]
    fn empty_read_is_passive_and_embedded_images_are_not_references() {
        let scratch = scratch("empty");
        let mut app = app_in(&scratch.0);
        let embedded = crate::io::ole_embed::EmbeddedImage {
            bytes: vec![1, 2, 3],
            pixel_width: 1,
            pixel_height: 1,
            name: "embedded.png".into(),
        };
        crate::io::ole_embed::add_embedded_image(
            &mut app.tabs[app.active_tab].scene.document,
            &embedded,
            document::types::Vector3::ZERO,
            1.0,
        )
        .expect("add embedded image");
        let i = app.active_tab;
        let before = (
            app.tabs[i].edit_revision,
            app.tabs[i].scene.geometry_epoch,
            app.tabs[i].dirty,
            history_depth(&app),
        );
        assert!(list(&mut app).is_empty());
        let after = (
            app.tabs[i].edit_revision,
            app.tabs[i].scene.geometry_epoch,
            app.tabs[i].dirty,
            history_depth(&app),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn unknown_and_unsupported_mutations_do_not_open_history() {
        let scratch = scratch("errors");
        let mut app = app_in(&scratch.0);
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_detach", "id":"FFFFFFFFFFFFFFF"}),
            "xref_missing",
        );

        let image = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let handle = document.allocate_handle();
            let mut definition =
                document::objects::ImageDefinition::with_dimensions("missing.png", 1, 1);
            definition.handle = handle;
            document.objects.insert(
                handle,
                document::objects::ObjectType::ImageDefinition(definition),
            );
            handle.value()
        };
        let id = format!("{image:X}");
        assert_eq!(list(&mut app)[0]["kind"], "image");
        for op in ["xref_reload", "xref_bind", "xref_set_type"] {
            let mut request = json!({"op":op, "id":id.clone()});
            if op == "xref_set_type" {
                request["reference_type"] = json!("overlay");
            }
            assert_quiet_rejection(&mut app, request, "xref_kind_unsupported");
        }
    }

    #[test]
    fn image_session_unload_is_undoable_even_when_already_dirty() {
        let scratch = scratch("image-undo");
        let mut app = app_in(&scratch.0);
        let image = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let handle = document.allocate_handle();
            let mut definition =
                document::objects::ImageDefinition::with_dimensions("missing.png", 1, 1);
            definition.handle = handle;
            document.objects.insert(
                handle,
                document::objects::ObjectType::ImageDefinition(definition),
            );
            format!("{:X}", handle.value())
        };
        let i = app.active_tab;
        app.tabs[i].dirty = true;
        let before = history_depth(&app);
        let unloaded = call(&mut app, json!({"op":"xref_unload","id":image.clone()}));
        assert_eq!(unloaded["ok"], true, "{unloaded}");
        assert_eq!(unloaded["result"]["unchanged"], false);
        assert_eq!(history_depth(&app), before + 1);
        assert_eq!(list(&mut app)[0]["status"], "unloaded");

        assert_eq!(call(&mut app, json!({"op":"undo"}))["ok"], true);
        assert_eq!(list(&mut app)[0]["status"], "unreferenced");
        assert_eq!(call(&mut app, json!({"op":"redo"}))["ok"], true);
        assert_eq!(list(&mut app)[0]["status"], "unloaded");
        assert_eq!(history_depth(&app), before + 1);
    }

    #[test]
    fn nested_rows_are_read_only() {
        let scratch = scratch("nested");
        let inner = write_dwg(&scratch.0, "inner.dwg", 4.0);
        let mut middle = Document::new();
        let mut child = document::tables::BlockRecord::new("INNER");
        child.flags.is_xref = true;
        child.xref_path = inner.to_string_lossy().into_owned();
        child.handle = middle.allocate_handle();
        middle.block_records.add(child).expect("child xref");
        let bytes =
            crate::io::save_to_bytes(&middle, "dwg", middle.version).expect("encode middle");
        let middle_path = scratch.0.join("middle.dwg");
        std::fs::write(&middle_path, bytes).expect("write middle");

        let mut app = app_in(&scratch.0);
        let parent = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let mut parent = document::tables::BlockRecord::new("MIDDLE");
            parent.flags.is_xref = true;
            parent.xref_path = middle_path.to_string_lossy().into_owned();
            parent.handle = document.allocate_handle();
            let id = parent.handle.value();
            document.block_records.add(parent).expect("parent xref");
            id
        };
        let rows = list(&mut app);
        let nested = rows
            .iter()
            .find(|row| row["nested"] == true)
            .expect("nested row")
            .clone();
        assert_eq!(nested["parent_id"], json!(format!("{parent:X}")));
        assert_eq!(nested["saved_path"], json!(inner.to_string_lossy()));
        assert!(nested["resolved_path"].as_str().is_some());
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_unload", "id":nested["id"]}),
            "xref_nested_read_only",
        );
    }

    #[test]
    fn path_conversion_rejections_are_stable_and_atomic() {
        let scratch = scratch("path-errors");
        let mut app = app_in(&scratch.0);
        let id = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let mut record = document::tables::BlockRecord::new("DISTANT");
            record.flags.is_xref = true;
            record.xref_path = "D:/library/site.dwg".into();
            record.handle = document.allocate_handle();
            let id = record.handle.value();
            document.block_records.add(record).expect("distant xref");
            format!("{id:X}")
        };
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_set_path", "id":id.clone(), "path_type":"relative"}),
            "xref_path_across_drives",
        );
        app.tabs[app.active_tab].current_path = None;
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_set_path", "id":id, "path_type":"relative"}),
            "xref_path_unsaved_host",
        );
    }

    #[test]
    fn missing_non_dwg_sources_fail_before_mutation() {
        let scratch = scratch("missing-sources");
        let mut app = app_in(&scratch.0);
        let underlay = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let handle = document.allocate_handle();
            let mut definition = document::objects::UnderlayDefinition::pdf("missing.pdf", "1");
            definition.handle = handle;
            document.objects.insert(
                handle,
                document::objects::ObjectType::UnderlayDefinition(definition),
            );
            handle.value()
        };
        let cloud = {
            use document::objects::{
                ClassObject, ClassObjectData, ObjectType, PointCloudDefinition,
            };
            let document = &mut app.tabs[app.active_tab].scene.document;
            let handle = document.allocate_handle();
            let mut definition = ClassObject::new(ClassObjectData::PointCloudDefinitionEx(
                PointCloudDefinition {
                    class_version: 1,
                    source_filename: "missing.rcs".into(),
                    is_loaded: true,
                    point_count: 0,
                    extents_min: document::types::Vector3::ZERO,
                    extents_max: document::types::Vector3::ZERO,
                },
            ));
            definition.handle = handle;
            document
                .objects
                .insert(handle, ObjectType::ClassObject(definition));
            handle.value()
        };
        for id in [underlay, cloud] {
            assert_quiet_rejection(
                &mut app,
                json!({"op":"xref_reload","id":format!("{id:X}")}),
                "xref_source_missing",
            );
        }
    }

    #[test]
    fn strict_fields_and_path_choice_are_atomic_on_native_requests() {
        let scratch = scratch("strict-fields");
        let source = write_dwg(&scratch.0, "site.dwg", 10.0);
        let mut app = app_in(&scratch.0);
        let (id, _) = placed_reference(&mut app, &source);
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_set_path","id":id.clone(),"path":"other.dwg","path_typ":"full"}),
            "unknown_xref_field",
        );
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_set_path","id":id.clone(),"path":"other.dwg","path_type":"full"}),
            "invalid_request",
        );
        for request in [
            json!({"op":"xref_set_path","id":id.clone(),"path":null}),
            json!({"op":"xref_set_path","id":id.clone(),"path":null,"path_type":"full"}),
            json!({"op":"xref_set_path","id":id.clone(),"path":"site.dwg","path_type":null}),
        ] {
            assert_quiet_rejection(&mut app, request, "invalid_request");
        }
        assert_quiet_rejection(
            &mut app,
            json!({"op":"xref_set_type","id":id,"reference_type":"overlay","stray":true}),
            "unknown_xref_field",
        );
    }

    #[test]
    fn path_edit_returns_a_direct_row_hidden_by_nested_deduplication() {
        let scratch = scratch("deduplicated-direct");
        let inner = write_dwg(&scratch.0, "inner.dwg", 4.0);
        let other = write_dwg(&scratch.0, "other.dwg", 8.0);
        let mut middle = Document::new();
        let mut child = document::tables::BlockRecord::new("INNER");
        child.flags.is_xref = true;
        child.xref_path = inner.to_string_lossy().into_owned();
        child.handle = middle.allocate_handle();
        middle.block_records.add(child).expect("child xref");
        let bytes =
            crate::io::save_to_bytes(&middle, "dwg", middle.version).expect("encode middle");
        let middle_path = scratch.0.join("middle.dwg");
        std::fs::write(&middle_path, bytes).expect("write middle");

        let mut app = app_in(&scratch.0);
        let id = {
            let document = &mut app.tabs[app.active_tab].scene.document;
            let mut parent = document::tables::BlockRecord::new("MIDDLE");
            parent.flags.is_xref = true;
            parent.xref_path = middle_path.to_string_lossy().into_owned();
            parent.handle = document.allocate_handle();
            document.block_records.add(parent).expect("parent xref");

            let mut direct = document::tables::BlockRecord::new("DIRECT");
            direct.flags.is_xref = true;
            direct.xref_path = other.to_string_lossy().into_owned();
            direct.handle = document.allocate_handle();
            let id = direct.handle.value();
            document.block_records.add(direct).expect("direct xref");
            format!("{id:X}")
        };
        let before = history_depth(&app);
        let changed = call(
            &mut app,
            json!({"op":"xref_set_path","id":id.clone(),"path":inner.to_string_lossy()}),
        );
        assert_eq!(changed["ok"], true, "{changed}");
        assert_eq!(changed["result"]["reference"]["id"], json!(id.clone()));
        assert_eq!(
            changed["result"]["saved_path"],
            json!(inner.to_string_lossy())
        );
        assert_eq!(history_depth(&app), before + 1);
        assert!(list(&mut app).iter().all(|row| row["id"] != id));
    }

    #[test]
    fn drawing_reference_lifecycle_uses_single_undo_steps() {
        let scratch = scratch("lifecycle");
        let source = write_dwg(&scratch.0, "site.dwg", 10.0);
        let mut app = app_in(&scratch.0);
        let (id, insert) = placed_reference(&mut app, &source);

        let mut depth = history_depth(&app);
        let change = envelope(
            &app,
            json!({"op":"xref_set_type", "id":id.clone(), "reference_type":"overlay"}),
        );
        let changed = app.automation_op(&change.to_string());
        assert_eq!(changed["result"]["reference"]["reference_type"], "overlay");
        assert_eq!(changed["result"]["unchanged"], false);
        assert_eq!(history_depth(&app), depth + 1);
        let replay = app.automation_op(&change.to_string());
        assert_eq!(replay, changed);
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        let unchanged = call(
            &mut app,
            json!({"op":"xref_set_type", "id":id.clone(), "reference_type":"overlay"}),
        );
        assert_eq!(unchanged["result"]["unchanged"], true);
        assert_eq!(history_depth(&app), depth);

        let absolute = source.to_string_lossy().into_owned();
        let path = call(
            &mut app,
            json!({"op":"xref_set_path", "id":id.clone(), "path":absolute.clone()}),
        );
        assert_eq!(path["result"]["reload_required"], true);
        assert_eq!(path["result"]["saved_path"], json!(absolute.clone()));
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        let same_path = call(
            &mut app,
            json!({"op":"xref_set_path", "id":id.clone(), "path":absolute.clone()}),
        );
        assert_eq!(same_path["result"]["unchanged"], true);
        assert_eq!(history_depth(&app), depth);

        let no_path = call(
            &mut app,
            json!({"op":"xref_set_path", "id":id.clone(), "path_type":"none"}),
        );
        assert_eq!(no_path["result"]["saved_path"], "site.dwg");
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        let relative = call(
            &mut app,
            json!({"op":"xref_set_path", "id":id.clone(), "path_type":"relative"}),
        );
        assert_eq!(relative["result"]["saved_path"], "./site.dwg");
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;

        let unloaded = call(&mut app, json!({"op":"xref_unload", "id":id.clone()}));
        assert_eq!(unloaded["result"]["reference"]["status"], "unloaded");
        assert_eq!(unloaded["result"]["session_only"], true);
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        let unchanged = call(&mut app, json!({"op":"xref_unload", "id":id.clone()}));
        assert_eq!(unchanged["result"]["unchanged"], true);
        assert_eq!(history_depth(&app), depth);

        assert_eq!(call(&mut app, json!({"op":"undo"}))["ok"], true);
        let restored = list(&mut app);
        assert_eq!(restored[0]["status"], "loaded");
        assert_eq!(call(&mut app, json!({"op":"redo"}))["ok"], true);
        let unloaded_again = list(&mut app);
        assert_eq!(unloaded_again[0]["status"], "unloaded");
        assert_eq!(history_depth(&app), depth);

        write_dwg(&scratch.0, "site.dwg", 99.0);
        let reloaded = call(&mut app, json!({"op":"xref_reload", "id":id.clone()}));
        assert_eq!(reloaded["ok"], true, "{reloaded}");
        assert_eq!(reloaded["result"]["reference"]["status"], "loaded");
        assert!(reloaded["result"]["diagnostics"].is_array());
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        assert_eq!(call(&mut app, json!({"op":"undo"}))["ok"], true);
        let unloaded_before_reload = list(&mut app);
        assert_eq!(unloaded_before_reload[0]["status"], "unloaded");
        assert_eq!(call(&mut app, json!({"op":"redo"}))["ok"], true);
        let loaded_after_redo = list(&mut app);
        assert_eq!(loaded_after_redo[0]["status"], "loaded");
        let document = &app.tabs[app.active_tab].scene.document;
        let record = document.block_records.get("site").expect("xref block");
        assert!(document.entities().any(|entity| matches!(
            entity,
            document::EntityType::Line(line)
                if line.common.owner_handle == record.handle && line.end.x == 99.0
        )));

        let bound = call(&mut app, json!({"op":"xref_bind", "id":id.clone()}));
        assert_eq!(bound["ok"], true, "{bound}");
        assert_eq!(bound["result"]["block"], "site");
        assert!(bound["result"]["unremapped"].is_number());
        assert_eq!(history_depth(&app), depth + 1);
        depth += 1;
        assert!(list(&mut app).is_empty());
        let record = app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get("site")
            .expect("bound local block");
        assert!(!record.flags.is_xref && !record.flags.is_xref_overlay);
        assert!(record.xref_path.is_empty());

        assert_eq!(call(&mut app, json!({"op":"undo"}))["ok"], true);
        assert_eq!(list(&mut app).len(), 1);
        assert_eq!(history_depth(&app), depth - 1);
        assert_eq!(call(&mut app, json!({"op":"redo"}))["ok"], true);
        assert!(list(&mut app).is_empty());
        assert_eq!(call(&mut app, json!({"op":"undo"}))["ok"], true);

        let before_detach = history_depth(&app);
        let detached = call(&mut app, json!({"op":"xref_detach", "id":id.clone()}));
        assert_eq!(detached["ok"], true, "{detached}");
        assert_eq!(detached["result"]["detached"], true);
        assert_eq!(history_depth(&app), before_detach + 1);
        assert!(list(&mut app).is_empty());
        let insert_handle = u64::from_str_radix(insert.as_str(), 16).unwrap();
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(Handle::new(insert_handle))
            .is_none());
    }

    #[test]
    fn failed_reload_preserves_content_and_history() {
        let scratch = scratch("reload-error");
        let source = write_dwg(&scratch.0, "site.dwg", 12.0);
        let mut app = app_in(&scratch.0);
        let (id, _) = placed_reference(&mut app, &source);
        let block = app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get("site")
            .expect("xref block")
            .handle;
        let original = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == block)
            .cloned()
            .collect::<Vec<_>>();
        assert!(!original.is_empty());
        std::fs::write(source, b"not a DWG").expect("corrupt source");
        let before = (
            history_depth(&app),
            app.tabs[app.active_tab].edit_revision,
            app.tabs[app.active_tab].dirty,
        );
        let failed = call(&mut app, json!({"op":"xref_reload", "id":id.clone()}));
        assert_eq!(failed["ok"], false, "{failed}");
        assert_eq!(failed["code"], "xref_load_failed", "{failed}");
        assert!(failed["diagnostics"].is_array());
        let after = (
            history_depth(&app),
            app.tabs[app.active_tab].edit_revision,
            app.tabs[app.active_tab].dirty,
        );
        assert_eq!(after, before);
        let retained = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == block)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(retained, original);
    }
}
