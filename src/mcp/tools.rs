use super::cache::{
    ResourceStore, TempCaptureFile, cached_batch_result, encode_uri_segment, private_capture_dir,
    read_private_capture, summarize_batch_response,
};
use super::resources::{
    guide_resource, guide_resources, manifest_layout_differs,
    refuse_retired_capture_parameters, resolve_tile_base_bounds, spatial_world_bounds,
};
use super::session::{
    GuiClient, client, refresh_client_states, resolve_intent_session, sessions,
};
use super::transport::{CallError, CancelPump, random_id};
use super::{INSTRUCTIONS, with_bridge_identity};
use crate::mcp_ops::{batch_bindings, validate_execute_request};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Pinned content digest of [`tool_definitions`].
///
/// The `tool_schema_digest_is_pinned` test fails whenever the MCP tool surface
/// changes, so schema drift (new params, renamed tools) is always a conscious,
/// reviewed edit — and clients can detect a stale bridge by comparing digests.
///
/// `block_delete` gained its optional `force` parameter. With API v11
/// `grip_drag` became optional-`handle` (built-in entity grips) plus optional
/// `plugin` (dragging a plugin-declared custom grip, routed to its owner).
/// Sixteen generated intent tools now follow the four facades. `node_graph`
/// joined the execute ops; the action list gained `extraction_outdated`,
/// `print_all` and `recovery_accept`.
/// `input` gained `handles`, so a selection prompt can be handed a set
/// without a round trip through `select`; the dialog actions gained
/// `dialog_discard` and `dialog_cancel`.
/// The attribute form an attributed insertion raises is answerable from a
/// script, so the action list gained `dialog_answer`.
/// Table cells are driven from a script: `table_cell_dialog`,
/// `table_cell_warning`, `table_cell_tab` and `command_submit`.
/// `set_properties` lists its `name` selector and takes a colour as a
/// number or `{"rgb":[r,g,b]}`, which its example shows on a layer.
/// `command_escape` presses Esc once, where `cancel` ends the command.
/// The Block Definition dialog is driveable from a script: its `block_def_dialog`
/// fields cover mode, annotative, orientation, uniform scale, exploding, unit,
/// description and hyperlink. `dialog_ok`, `dialog_discard` and `dialog_cancel`
/// now answer the dirty-close (`BlockEditChanges`) prompt; the Write Block
/// dialog is driveable from a script: `wblock_dialog` fields cover source,
/// block, base point, unit, object mode and path, and `dialog_ok` applies it.
/// Native dock panels are driven through `panel_input` and `panel_button`:
/// form-only input, then the declared synchronous document-bound command.
/// The attribute form pages fifteen rows at a time: `dialog_page`.
/// Layout list/read operations joined the read facade; rename, activate and
/// delete joined layout creation and page-setup writes in the execute facade.
/// Atomic entity common-property writes and direct DGN import joined execute.
/// Fresh xref reads joined the read facade; six direct-reference lifecycle
/// writes joined execute with operation-local unknown-field rejection.
/// The Text Editor tab's windows are driveable from a script: `mtext_dialog`.
/// The enhanced attribute editor joined them: `attr_editor`.
/// Find and Replace is driveable from a script: `find_replace_dialog`.
/// `save` describes its `target_version` as the DWG version.
/// `ocs_capture` / `ocs_render` gained `layout`, `aspect_fit`, and `renderer` (`document_id` on `ocs_capture`) for headless off-screen capture.
/// The surface list of the Create Points > Surface commands: `surface_list_dialog`.
/// The windows of the Create Points tools: `points_tool_dialog`.
#[cfg(test)]
pub(super) const TOOL_SCHEMA_DIGEST: &str = "63d3ca6c1bae6ddcaa308ab7b7cea618af84555079f5fb9fb4b0c6d1e7f40de5";

pub(super) const READ_OPS: &[&str] = &[
    "state",
    "hello",
    "query",
    "civil_station_offset",
    "civil_surface_stats",
    "civil_surface_mesh",
    "records",
    "record_schema",
    "capabilities",
    "tools",
    "entities",
    "blocks",
    "layers",
    "layouts",
    "xrefs",
    "page_setup_get",
    "header",
    "properties",
    "measure",
    "snap",
    "grips",
    "history",
    "commands",
    "events",
    "operation",
    "batch_result",
    "guide",
    "entity_schema",
    "validate",
    "xdata_get",
    "audit",
    "text_search",
    "text_audit",
    "keytips",
    "plugins",
    "capture",
    "plugin_panels",
];
pub(super) const EXECUTE_OPS: &[&str] = &[
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
    "pointer",
    "key",
    "set_properties",
    "panel_input",
    "panel_button",
    "panel_action",
    "plugin_panel_action",
    "action",
    "embed_image",
    "dgn_import",
    "wblock",
    "plot",
    "entities_create",
    "entities_set",
    "xref_reload",
    "xref_unload",
    "xref_detach",
    "xref_bind",
    "xref_set_path",
    "xref_set_type",
    "entities_delete",
    "entities_transform",
    "node_graph",
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
    "plugin_call",
    "layout_create",
    "layout_rename",
    "layout_activate",
    "layout_delete",
    "page_setup_set",
    "file_identity",
    "save",
    "save_verified",
    "stop",
    "batch",
];
use crate::mcp_ops::MAX_BATCH_STEPS;
pub(super) const FACADE_TOOL_NAMES: &[&str] = &["ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"];

pub(super) fn available_tool_names() -> Vec<&'static str> {
    FACADE_TOOL_NAMES
        .iter()
        .copied()
        .chain(crate::mcp_intent::DESCRIPTORS.iter().map(|d| d.name))
        .collect()
}

pub(super) fn required_string<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Missing {key}"))
}

pub(super) fn normalize_intent_arguments(
    name: &str,
    arguments: &Value,
    clients: &HashMap<String, GuiClient>,
) -> Result<Value, CallError> {
    let Some(descriptor) = crate::mcp_intent::find_descriptor(name) else {
        return Ok(arguments.clone());
    };
    let mut normalized = arguments.clone();
    normalized["ocs_session_id"] = json!(resolve_intent_session(arguments, clients)?);
    if descriptor.kind == crate::mcp_intent::IntentKind::Execute
        && normalized.get("request_id").is_none()
    {
        normalized["request_id"] = json!(random_id()?);
    }
    Ok(normalized)
}

pub(super) fn resolve_batch_step(step: &Value, index: usize, results: &[Value]) -> Result<Value, String> {
    batch_bindings(step, index)?;
    let mut resolved = step.clone();
    if let Some(bindings) = resolved
        .as_object_mut()
        .ok_or("batch step must be an object")?
        .remove("bindings")
    {
        for (target, source) in bindings.as_object().expect("validated bindings") {
            let previous = results
                .get(source["step"].as_u64().unwrap() as usize)
                .ok_or("source step result unavailable")?;
            if previous["status"] != "completed" || previous["ok"] == false {
                return Err("binding source step has not completed successfully".into());
            }
            let path = source["path"].as_str().unwrap();
            resolved[target] = previous
                .pointer(path)
                .ok_or_else(|| format!("binding '{target}' path '{path}' not found"))?
                .clone();
        }
    }
    crate::mcp_ops::validate_request(&resolved, true)?;
    Ok(resolved)
}

pub(super) fn preflight_request(request: &Value) -> Value {
    let result =
        required_string(request, "op").and_then(|op| validate_execute_request(request, op));
    match result {
        Ok(validated) => {
            let deferred: Vec<Value> = request["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, step)| {
                    step.get("bindings")
                        .map(|bindings| json!({"step":index,"bindings":bindings}))
                })
                .collect();
            json!({"ok":true,"valid":true,"scope":"static","warnings":validated.warnings,"deferred_bindings":deferred,
                "not_checked":["live document revision, handles, blocks, styles and record property write rules","file access, command execution and non-creation geometry operations","values resolved from earlier batch results"],
                "executed":false})
        }
        Err(error) => {
            json!({"ok":false,"valid":false,"scope":"static","executed":false,"code":"validation_failed","error":error})
        }
    }
}

pub(super) fn compact_state(state: &Value) -> Value {
    let mut compact = Map::new();
    for key in [
        "session_id",
        "document_id",
        "revision",
        "geometry_revision",
        "camera_revision",
        "selection",
        "selection_revision",
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

pub(super) fn response_handles(response: &Value) -> Vec<String> {
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

pub(super) fn shape_execute_response(
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

pub(super) fn call_tool(
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
            let listed = sessions(launch).map_err(CallError::Infra)?;
            refresh_client_states(clients);
            Ok(Value::Array(listed))
        }
        "ocs_read" => {
            let op = arguments["op"].as_str().unwrap_or("state");
            if !READ_OPS.contains(&op) {
                return Err("Use ocs_execute for mutations".to_string().into());
            }
            if op == "entity_schema" {
                return match arguments["parameters"]["type"].as_str() {
                    Some(kind) => Ok(
                        json!({"ok":true,"schema":crate::entity_creation::schema(kind)?,"validation":"Use op validate with parameters.request to preflight; live document checks still run on execution."}),
                    ),
                    None => Ok(
                        json!({"ok":true,"types":crate::entity_creation::TYPES,"aliases":{"Polyline":"LwPolyline"},"next":"Set parameters.type for one creation schema and example."}),
                    ),
                };
            }
            if op == "validate" {
                return Ok(preflight_request(&arguments["parameters"]["request"]));
            }
            if op == "guide" {
                let mut result = match arguments["parameters"]["name"].as_str() {
                    Some(name) => guide_resource(name).map_err(CallError::Tool)?,
                    None => json!({"guides":guide_resources()}),
                };
                result["ok"] = json!(true);
                result["status"] = json!("completed");
                return Ok(result);
            }
            let session_id = required_string(arguments, "ocs_session_id")?;
            if op == "batch_result" {
                let request_id = required_string(&arguments["parameters"], "request_id")?;
                return cached_batch_result(clients, session_id, request_id);
            }
            if op == "capture" {
                // The GUI capture op needs a file path and lives behind the
                // ocs_capture tool; agents hitting the bare read op get
                // guidance instead of a cryptic missing-path failure.
                return Ok(json!({
                    "ok": false,
                    "status": "failed",
                    "code": "use_capture_tool",
                    "error": "Viewport captures run through the ocs_capture tool (scope, annotate, tiles). ocs_read exposes capture products already stored as resources, not new captures."
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
            let is_batch = op == "batch";
            let wait = arguments["wait_seconds"].as_f64().unwrap_or(30.0);
            let detail = arguments["response_detail"].as_str().unwrap_or("compact");
            let gui = client(clients, session_id)?;
            let mut response = if is_batch {
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
            if is_batch && detail == "summary" {
                Ok(summarize_batch_response(response, session_id))
            } else {
                shape_execute_response(response, detail, gui, pump).map_err(CallError::from)
            }
        }
        "ocs_capture" => {
            let session_id = required_string(arguments, "ocs_session_id")?;
            let path = private_capture_dir()?.join(format!("ocs-capture-{}.png", random_id()?));
            let _capture_guard = TempCaptureFile(path.clone());
            let scope = arguments["scope"].as_str().unwrap_or("viewport");
            let max_dimension = arguments["max_dimension"].as_u64().unwrap_or(1600);
            let delivery = arguments["delivery"].as_str().unwrap_or("inline");
            refuse_retired_capture_parameters(arguments).map_err(CallError::from)?;
            let tile = arguments.get("tile");
            let want_manifest = arguments["pyramid_manifest"].as_bool().unwrap_or(false);
            let requested_layout = arguments.get("layout").and_then(Value::as_str);
            let requested_renderer = arguments.get("renderer").and_then(Value::as_str);
            let requested_doc_id = arguments.get("document_id").and_then(Value::as_u64);

            if tile.is_some()
                && arguments
                    .get("focus_handles")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty())
            {
                return Err(CallError::Tool(
                    "tile cannot be combined with focus_handles; compute bounds from a capture or supply explicit 'bounds'".to_string(),
                ));
            }

            let mut tile_info: Option<(u32, u32, u32, [f64; 4])> = None;
            if let Some(tile_val) = tile {
                if let (Some(level), Some(x), Some(y)) = (
                    tile_val.get("level").and_then(Value::as_u64),
                    tile_val.get("x").and_then(Value::as_u64),
                    tile_val.get("y").and_then(Value::as_u64),
                ) {
                    let base_bounds = resolve_tile_base_bounds(
                        arguments,
                        resources,
                        session_id,
                        requested_layout,
                    )
                    .map_err(CallError::Tool)?;
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
            if let Some(layout) = requested_layout {
                req["layout"] = json!(layout);
            }
            if let Some(aspect_fit) = arguments.get("aspect_fit").and_then(Value::as_bool) {
                req["aspect_fit"] = json!(aspect_fit);
            }
            if let Some(renderer) = requested_renderer {
                req["renderer"] = json!(renderer);
            }
            if let Some(document_id) = requested_doc_id {
                req["document_id"] = json!(document_id);
            }
            let result = client(clients, session_id)?.request(req, 30.0, pump, mcp_id)?;
            if result["ok"].as_bool() != Some(true)
                || result["status"].as_str() != Some("completed")
            {
                // Failed capture response from the GUI: retryable, stays Tool.
                return Err(result.to_string().into());
            }
            let targets_other_doc = requested_doc_id.is_some_and(|doc_id| {
                clients
                    .get(session_id)
                    .and_then(|c| c.state["document_id"].as_u64())
                    .is_some_and(|active_id| active_id != doc_id)
            });
            let bytes = read_private_capture(&path).map_err(CallError::from)?;
            drop(_capture_guard);
            let mut meta = result.get("result").cloned().unwrap_or_else(|| json!({}));
            if let Some(obj) = meta.as_object_mut() {
                obj.remove("path");
                if let Some(request_id) = arguments["request_id"].as_str() {
                    obj.insert("request_id".into(), Value::String(request_id.into()));
                }
            }

            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let hash = crate::io::recovery::hex_digest(hasher.finalize());
            let b64 = BASE64.encode(&bytes);
            let bytes_len = bytes.len();

            let uri = resources.insert_with_latest(
                session_id,
                &hash,
                b64.clone(),
                bytes_len,
                meta.clone(),
                !targets_other_doc,
            );
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("uri".into(), Value::String(uri.clone()));
                if !targets_other_doc {
                    obj.insert(
                        "latest_uri".into(),
                        Value::String(format!("cad://session/{session_id}/snapshot/latest.png")),
                    );
                }
                obj.insert("hash".into(), Value::String(hash.clone()));
                obj.insert("bytes".into(), Value::from(bytes_len));
            }

            let effective_layout = meta
                .get("layout")
                .and_then(Value::as_str)
                .or(requested_layout)
                .map(str::to_string);
            let tile_layout = requested_layout.and(effective_layout.as_deref());

            let mut final_uri = uri.clone();
            if let Some((level, x, y, tb)) = tile_info {
                let tile_uri = ResourceStore::tile_uri_for(session_id, tile_layout, level, x, y);
                let manifest_uri = ResourceStore::manifest_uri_for(session_id, tile_layout);
                final_uri = tile_uri.clone();
                let pyr = json!({
                    "level": level,
                    "x": x,
                    "y": y,
                    "tile_world_bounds": tb,
                    "tile_uri": tile_uri,
                    "manifest_uri": manifest_uri,
                });
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("pyramid".into(), pyr);
                    obj.insert("uri".into(), Value::String(final_uri.clone()));
                }
                if !targets_other_doc {
                    resources.insert_tile_for_layout(
                        session_id,
                        tile_layout,
                        level,
                        x,
                        y,
                        b64.clone(),
                        bytes_len,
                        meta.clone(),
                    );
                }
            }

            let needs_manifest = if let Some(eff_lay) = tile_layout {
                want_manifest
                    || (!targets_other_doc
                        && tile_info.is_none()
                        && resources.get_layout_manifest(session_id, eff_lay).is_none())
            } else {
                let manifest_stale = manifest_layout_differs(
                    resources.pyramid_manifests.get(session_id),
                    effective_layout.as_deref(),
                );
                want_manifest
                    || (!targets_other_doc
                        && tile_info.is_none()
                        && (!resources.pyramid_manifests.contains_key(session_id)
                            || manifest_stale))
            };
            if needs_manifest {
                let bounds = spatial_world_bounds(&meta).unwrap_or([-100.0, -100.0, 100.0, 100.0]);
                let unit = meta
                    .get("_spatial")
                    .and_then(|s| s.get("unit"))
                    .and_then(Value::as_str)
                    .unwrap_or("Millimeters")
                    .to_string();
                let manifest = crate::app::control::vision::compute_pyramid_manifest(
                    session_id, &unit, bounds, 4, 512,
                );
                if let Ok(mut manifest_val) = serde_json::to_value(&manifest) {
                    if let Some(renderer) = meta
                        .get("renderer")
                        .and_then(Value::as_str)
                        .or(requested_renderer)
                    {
                        if let Some(obj) = manifest_val.as_object_mut() {
                            obj.insert("renderer".into(), Value::String(renderer.to_string()));
                        }
                    }
                    let manifest_for_meta = if let Some(eff_lay) = tile_layout {
                        if !targets_other_doc {
                            resources.insert_layout_manifest(session_id, eff_lay, manifest_val);
                            resources.get_layout_manifest(session_id, eff_lay).cloned()
                        } else {
                            let enc = encode_uri_segment(eff_lay);
                            if let Some(obj) = manifest_val.as_object_mut() {
                                obj.insert("layout".into(), Value::String(eff_lay.to_string()));
                                obj.insert(
                                    "tile_uri_template".into(),
                                    Value::String(format!(
                                        "cad://session/{session_id}/layout/{enc}/tile/{{level}}/{{x}}/{{y}}.png"
                                    )),
                                );
                            }
                            Some(manifest_val)
                        }
                    } else {
                        if let Some(layout) = effective_layout.as_deref() {
                            if let Some(obj) = manifest_val.as_object_mut() {
                                obj.insert("layout".into(), Value::String(layout.to_string()));
                            }
                        }
                        if !targets_other_doc {
                            resources.insert_pyramid_manifest(session_id, manifest_val.clone());
                        }
                        Some(manifest_val)
                    };
                    if want_manifest {
                        if let Some(mval) = manifest_for_meta {
                            if let Some(obj) = meta.as_object_mut() {
                                obj.insert("pyramid_manifest".into(), mval);
                            }
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
        _ => {
            let Some(descriptor) = crate::mcp_intent::find_descriptor(name) else {
                // Backstop: the dispatcher rejects unknown names first with -32602.
                return Err(CallError::Tool(format!("Unknown tool: {name}")));
            };
            let normalized = normalize_intent_arguments(name, arguments, clients)?;
            let session_id = normalized["ocs_session_id"]
                .as_str()
                .ok_or_else(|| CallError::Tool("missing normalized ocs_session_id".to_string()))?;
            let (facade, facade_arguments) =
                crate::mcp_intent::facade_arguments(descriptor, &normalized, session_id, None);
            call_tool(facade, &facade_arguments, clients, resources, pump, mcp_id)
        }
    }
}

#[allow(dead_code)]
pub(super) fn batch_step_schema() -> Value {
    crate::mcp_ops::batch_step_schema()
}

#[cfg(test)]
pub(super) fn execute_request_schema() -> Value {
    crate::mcp_ops::execute_request_schema()
}

// Local references resolve from the tool input schema root, not from request.
pub(super) fn execute_input_schema() -> Value {
    let (request_schema, defs, dialect) = crate::mcp_ops::execute_request_schema_parts();
    json!({
        "type": "object",
        "properties": {
            "ocs_session_id": {"type": "string", "minLength": 1, "description": "Value of session_id returned by ocs_sessions."},
            "request": request_schema,
            "wait_seconds": {"type": "number", "minimum": 0, "maximum": 60, "default": 30, "description": "Total time to wait for completion before returning."},
            "response_detail": {"type": "string", "enum": ["compact", "summary", "changed_entities", "full"], "default": "compact", "description": "compact returns only state needed for the next edit; summary also replaces batch step results and changes with counts, attention items and a read-only details request (non-batch operations use compact); changed_entities also returns current geometry for changed handles; full preserves the complete editor state."}
        },
        "required": ["ocs_session_id", "request"],
        "additionalProperties": false,
        "$defs": defs,
        "$schema": dialect
    })
}

pub(crate) fn read_output_schema() -> Value {
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

pub(crate) fn execute_output_schema() -> Value {
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

pub(crate) fn capture_input_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ocs_session_id":{"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."},
            "request_id":{"type":"string","description":"Optional client request id, accepted for schema compatibility and echoed in the result metadata. Captures are not idempotency-keyed."},
            "scope":{"type":"string","enum":["viewport","window"],"default":"viewport","description":"Capture only the drawing viewport by default, or the complete application window."},
            "max_dimension":{"type":"integer","minimum":256,"maximum":4096,"default":1600,"description":"Resize the longest image edge to at most this many pixels."},
            "delivery":{"type":"string","enum":["inline","resource","both"],"default":"inline","description":"Image delivery method: 'inline' embeds base64 in tool content, 'resource' returns an MCP cad:// URI reference without inlining image bytes, 'both' returns both inline image and cad:// URI."},
            "view":{"type":"string","enum":["current","extents","selection","region"],"default":"current","description":"Frame the camera before capture: extents fits all entities, selection fits selected entities, region fits explicit world bounds."},
            "bounds":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"World XY bounding box [min_x, min_y, max_x, max_y] to zoom and fit in view before capturing (used with view: 'region')."},
            "focus_handles":{"type":"array","items":{"type":"string"},"description":"Hex handles of entities to zoom and fit in view before capturing."},
            "highlight_handles":{"type":"array","items":{"type":"string"},"description":"Hex handles of entities to select/highlight before capturing."},
            "annotate":{"type":"boolean","default":false,"description":"Overlay Set-of-Marks numbered tags on visible entities for visual grounding."},
            "layout":{"type":"string","minLength":1,"description":"Capture a specific layout ('Model' or any paper-space layout name) headlessly off-screen without switching the active GUI tab, moving the live camera, or mutating selection. Defaults aspect_fit to true unless aspect_fit: false is explicitly passed."},
            "aspect_fit":{"type":"boolean","default":false,"description":"Render off-screen with output pixel dimensions fitted to the aspect ratio of the framed bounds/extents (longest edge = max_dimension) instead of the live GUI viewport aspect ratio. On layout captures, defaults to true unless explicitly set false."},
            "renderer":{"type":"string","enum":["auto","gpu","cpu"],"default":"auto","description":"Off-screen rendering backend: 'auto' uses headless wgpu (WYSIWYG canvas) when available and falls back to CPU SVG/resvg, 'gpu' requires headless wgpu, 'cpu' forces vector SVG/resvg rasterization."},
            "document_id":{"type":"integer","minimum":0,"description":"Optional open document id (from ocs_sessions) to capture headlessly off-screen without activating its tab."},
            "tile":{"type":"object","properties":{"level":{"type":"integer","minimum":0,"maximum":6,"description":"Pyramid zoom level (0 = full overview, 1 = 2x2 grid, 2 = 4x4 grid, etc.)."},"x":{"type":"integer","minimum":0,"description":"Tile column index (0-indexed, left-to-right)."},"y":{"type":"integer","minimum":0,"description":"Tile row index (0-indexed, top-to-bottom)."}},"required":["level","x","y"],"description":"Fetch a specific DeepZoom pyramid tile. Automatically computes tile world bounds and frames the camera."},
            "pyramid_manifest":{"type":"boolean","default":false,"description":"If true, generates and returns the complete multiscale pyramid manifest (levels, grid dimensions, tile world spans and resource URIs) in the response metadata."}
        },
        "required":["ocs_session_id"],
        "additionalProperties":true
    })
}

pub(crate) fn tool_definitions() -> Value {
    let mut tools = json!([
        {
            "name":"ocs_sessions",
            "description":"List real OpenCADStudio GUI sessions and documents. Launch the installed editor if none is running. On first use, announce the build to the user from the `bridge` object in each result (OpenCADStudio version, build_rev, tool_schema digest); announce again if a later call reports a different build.",
            "inputSchema":{"type":"object","properties":{"launch_if_none":{"type":"boolean","default":true,"description":"Launch OpenCADStudio when no live session exists."}},"additionalProperties":false},
            "outputSchema":{"type":"object","properties":{"result":{"type":"array","items":{"type":"object","properties":{"ok":{"const":true},"session_id":{"type":"string"},"document_id":{"type":"integer"},"revision":{"type":"integer"},"selection":{"type":"array","items":{"type":"string"}},"documents":{"type":"array"}},"required":["ok","session_id","document_id","revision","selection","documents"],"additionalProperties":true}}},"required":["result"],"additionalProperties":false},
            "title":"List OCS sessions","annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_read",
            "description":"Use entity_schema with parameters.type for creation fields/examples; omit type to list. Use validate with parameters.request to check an execute request without running it (no live document checks). Use guide with parameters.name to read workflow, architecture, capture, plugin-panels or batch-bindings guidance (omit name to list). Discover capabilities and record schemas, or read state, complete database records, command manifests, entities, blocks (semantic block instances with attributes, definition tags and dynamic properties), properties, kernel measurements and spatial relationships, history, events or operation status from a live OCS session. Use batch_result with parameters.request_id to retrieve retained batch details without executing steps; available only in the originating MCP connection, for up to 64 retained batches per session.",
            "inputSchema":{"type":"object","properties":{"ocs_session_id":{"type":"string","minLength":1,"description":"Value of session_id returned by ocs_sessions."},"op":{"type":"string","enum":READ_OPS,"default":"state"},"parameters":{"type":"object","description":"Operation-specific filters.","properties":{"layout":{"type":"string","minLength":1,"description":"Layout name for op: page_setup_get."},"request":{"type":"object","description":"Execute request to preflight without dispatch. request_id may be omitted. Batch bindings are checked structurally; result-dependent values require execution."},"name":{"type":"string","description":"Command name, block name/glob, or record name."},"names":{"type":"array","items":{"type":"string"},"description":"Block name or wildcard glob filters (e.g. ['*'], ['DOOR*']) for op: blocks."},"extract_attributes":{"type":"boolean","default":true,"description":"Extract ATTRIB values, ATTDEF defaults, and visible definition text tags (e.g. TAG, DIMS) for op: blocks."},"extract_dynamic_properties":{"type":"boolean","default":true,"description":"Extract dynamic block parameters, visibility states, and lookup values for op: blocks."},"include_anonymous":{"type":"boolean","default":false,"description":"Include unlinked internal anonymous blocks (*U..., *D..., *T...) in op: blocks."},"search":{"type":"string","description":"Case-insensitive command or record-type search."},"find":{"type":"string","description":"Text query string for text_search."},"match_case":{"type":"boolean","default":false,"description":"Case-sensitive text search."},"whole_word":{"type":"boolean","default":false,"description":"Match only whole words."},"ignore_accents":{"type":"boolean","description":"Ignore accents/diacritics in text (defaults to true when match_case is false)."},"scope":{"type":"string","enum":["all","active_space","blocks"],"default":"all","description":"Text search scope."},"system_spellcheck":{"type":"boolean","default":false,"description":"Use native OS spell-checker (Windows, macOS, Linux)."},"language":{"type":"string","description":"Language tag for spell-checker (e.g. 'fr-FR', 'en-US', 'de-DE', 'es-ES')."},"suggest":{"type":"boolean","default":true,"description":"Include suggested corrections for misspelled words."},"check_terms":{"type":"array","items":{"type":"string"},"description":"List of terms or suspect misspellings to flag in text_audit."},"dictionary":{"type":"array","items":{"type":"string"},"description":"Known valid words for text_audit dictionary check."},"pairs":{"type":"array","description":"Find/replace pairs for dry-run simulation in text_audit.","items":{"type":"object","properties":{"find":{"type":"string"},"replace":{"type":"string"}},"required":["find","replace"]}},"dry_run_pairs":{"type":"array","description":"Alias for pairs in text_audit.","items":{"type":"object","properties":{"find":{"type":"string"},"replace":{"type":"string"}},"required":["find","replace"]}},"document_id":{"type":"integer","minimum":0},"path":{"type":"string","description":"Optional intended output path for audit; extension determines target format."},"target_format":{"type":"string","enum":["dwg","dxf"],"description":"Intended output format for audit."},"target_version":{"type":"string","enum":["R14","2000","2004","2007","2010","2013","2018","AC1014","AC1015","AC1018","AC1021","AC1024","AC1027","AC1032"],"description":"Intended CAD output version for audit."},"collection":{"type":"string","description":"Record collection, all for records, or omit to discover collections and schema types."},"handle":{"type":"string"},"handles":{"type":"array","items":{"type":"string"},"description":"Exact entity or record handles."},"type":{"type":"string","description":"Entity or record type filter; for record_schema, returns its complete type graph and writable field paths."},"layer":{"type":"string","description":"Layer name filter for query; * and ? are wildcards."},"xdata_app":{"type":"string","description":"Only entities carrying XDATA of this application (query)."},"detail":{"type":"string","enum":["summary","geometry","full"],"default":"geometry","description":"Entity detail returned by query."},"fields":{"type":"array","items":{"type":"string"},"description":"Return only these entity fields plus handle."},"paths":{"type":"array","items":{"type":"string"},"description":"Project RFC 6901 JSON Pointer paths relative to record.properties."},"where":{"type":"array","description":"All property filters must match.","items":{"type":"object","properties":{"path":{"type":"string"},"op":{"type":"string","enum":["eq","ne","lt","lte","gt","gte","contains","starts_with","ends_with","in","exists","not_exists"],"default":"eq"},"value":{}},"required":["path"],"additionalProperties":false}},"near":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Rank planar curves by exact kernel distance to this world XY point."},"point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"World point whose object snap the snap op reports."},"from":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Base point for perpendicular and tangent snaps (snap op)."},"contains_point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3,"description":"Return closed planar curves containing this world XY point."},"bounds":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4,"description":"Filter entities whose world XY bounds overlap [min_x,min_y,max_x,max_y]."},"intersections":{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":2,"description":"Return exact kernel intersections between two planar curve handles."},"after":{"type":"integer","minimum":0,"description":"Event cursor."},"request_id":{"type":"string","description":"Operation id to query."},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":10000},"station":{"type":"number","description":"Alignment station for op: civil_station_offset."},"offset_distance":{"type":"number","description":"Signed perpendicular distance from the alignment, positive right of travel, for op: civil_station_offset."},"handle":{"type":"string"},"handles":{"type":"array","items":{"type":"string"},"description":"Exact entity or record handles."}},"additionalProperties":false}},"anyOf":[{"required":["ocs_session_id"]},{"required":["op"],"properties":{"op":{"enum":["guide","entity_schema","validate"]}}}],"additionalProperties":false},
            "outputSchema":read_output_schema(),
            "title":"Read OCS state","annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_execute",
            "description":"Execute semantic OCS actions, batch drafting, and mutations. Supports batch creation (entities_create), reference image underlays (embed_image), block definitions (block_define), and block reference insertions (INSERT). Read the architecture guide before floorplan vectorization.",
            "inputSchema":execute_input_schema(),
            "outputSchema":execute_output_schema(),
            "title":"Execute OCS action","annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}
        },
        {
            "name":"ocs_capture",
            "description":"Capture the actual current OCS drawing viewport or window as a bounded PNG with optional camera framing, entity highlighting, spatial coordinates and Set-of-Marks annotations. Works quietly in background and overlapped window states without stealing focus. Read the capture guide for annotations, resource delivery and tiling.",
            "inputSchema":capture_input_schema(),
            "title":"Capture OCS window","annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }
    ]);
    tools
        .as_array_mut()
        .expect("tool definitions array")
        .extend(crate::mcp_intent::tool_definitions());
    tools
}

pub(super) fn tool_result(value: Value) -> Value {
    if value.get("$resource").is_some() || value.get("$image").is_some() {
        let mut content = Vec::new();
        if let Some(res) = value.get("$resource") {
            let uri = res["uri"].as_str().unwrap_or("");
            content.push(json!({
                "type": "text",
                "text": format!("Captured viewport to MCP resource: {uri}")
            }));
            content.push(json!({
                "type": "resource_link",
                "name": "Captured viewport",
                "uri": uri,
                "mimeType": res["mimeType"].as_str().unwrap_or("image/png")
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

pub(super) fn error_result(message: impl ToString) -> Value {
    let message = message.to_string();
    json!({
        "content":[{"type":"text","text":message}],
        "isError":true
    })
}

/// Sha256 hex digest of the canonical [`tool_definitions`] JSON.
///
/// Published via `capabilities.experimental` and the in-band [`bridge_identity`]
/// stamp so MCP clients can detect a stale bridge (running an older binary
/// than the one on disk) without guessing.
pub(super) fn tool_schema_digest() -> String {
    let canonical = serde_json::to_string(&tool_definitions()).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    crate::io::recovery::hex_digest(hasher.finalize())
}
