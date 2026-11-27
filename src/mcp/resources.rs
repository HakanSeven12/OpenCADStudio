use super::cache::{ResourceStore, TempCaptureFile, decode_uri_segment, private_capture_dir, read_private_capture};
use super::session::{GuiClient, client};
use super::transport::{CancelPump, random_id};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub(super) const GUIDES: &[(&str, &str, &str)] = &[
    (
        "workflow",
        "CAD automation workflow",
        include_str!("../../docs/automation/guides/workflow.md"),
    ),
    (
        "architecture",
        "Architectural vectorization",
        include_str!("../../docs/automation/guides/architecture.md"),
    ),
    (
        "capture",
        "Captures and visual verification",
        include_str!("../../docs/automation/guides/capture.md"),
    ),
    (
        "plugin-panels",
        "Plugin panel surfaces",
        include_str!("../../docs/automation/guides/plugin-panels.md"),
    ),
    (
        "batch-bindings",
        "Dependent batch steps",
        include_str!("../../docs/automation/batch-bindings.md"),
    ),
];

pub(super) fn guide_resources() -> Vec<Value> {
    GUIDES.iter().map(|(name, title, _)| json!({
        "uri":format!("cad://guides/{name}"), "name":name, "title":title, "mimeType":"text/markdown"
    })).collect()
}

pub(super) fn guide_resource(name: &str) -> Result<Value, String> {
    let (_, _, text) = GUIDES.iter().find(|(key, _, _)| *key == name)
        .ok_or_else(|| format!("Unknown guide '{name}'; use ocs_read op guide without parameters.name to list guides"))?;
    Ok(
        json!({"contents":[{"uri":format!("cad://guides/{name}"),"mimeType":"text/markdown","text":text}]}),
    )
}

pub(super) fn parse_tile_subpath(trimmed: &str) -> Option<(u32, u32, u32, bool)> {
    let is_json = trimmed.ends_with(".json");
    let core = trimmed.trim_end_matches(".png").trim_end_matches(".json");
    let parts: Vec<&str> = core.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let level = parts[0].parse::<u32>().ok()?;
    let x = parts[1].parse::<u32>().ok()?;
    let y = parts[2].parse::<u32>().ok()?;
    Some((level, x, y, is_json))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn serve_tile_resource(
    uri: &str,
    session_id: &str,
    layout_opt: Option<&str>,
    level: u32,
    x: u32,
    y: u32,
    is_json: bool,
    clients: &mut HashMap<String, GuiClient>,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
    mcp_id: Option<&Value>,
) -> Result<Value, String> {
    let png_uri = ResourceStore::tile_uri_for(session_id, layout_opt, level, x, y);
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

    let manifest = match layout_opt {
        Some(lay) => Some(resources.get_layout_manifest(session_id, lay).ok_or_else(|| {
            format!("No cached pyramid manifest for layout '{lay}'; capture layout '{lay}' first")
        })?),
        None => resources.pyramid_manifests.get(session_id),
    };
    let manifest_layout = layout_opt
        .or_else(|| {
            manifest
                .and_then(|m| m.get("layout"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let manifest_renderer = manifest
        .and_then(|m| m.get("renderer"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let base_bounds = match manifest {
        Some(m) => m
            .get("world_bounds")
            .and_then(Value::as_array)
            .and_then(|arr| bounds_from_array(arr, true))
            .ok_or_else(|| "Cached pyramid manifest is missing valid world_bounds".to_string())?,
        None => [-100.0, -100.0, 100.0, 100.0],
    };

    let tile_bounds = crate::app::control::vision::compute_tile_bounds(base_bounds, level, x, y)?;
    let path = private_capture_dir()?.join(format!("ocs-tile-{}.png", random_id()?));
    let _capture_guard = TempCaptureFile(path.clone());
    let mut req = json!({
        "op": "capture",
        "path": path.to_string_lossy(),
        "scope": "viewport",
        "view": "region",
        "bounds": tile_bounds,
        "max_dimension": 512,
    });
    if let Some(ref lay) = manifest_layout {
        req["layout"] = json!(lay);
        req["aspect_fit"] = json!(true);
    }
    if let Some(ref rend) = manifest_renderer {
        req["renderer"] = json!(rend);
    }

    let result = client(clients, session_id)?.request(req, 20.0, pump, mcp_id)?;
    if result["ok"].as_bool() != Some(true) || result["status"].as_str() != Some("completed") {
        return Err(result.to_string());
    }
    let bytes = read_private_capture(&path)?;
    drop(_capture_guard);
    let b64 = BASE64.encode(&bytes);
    let bytes_len = bytes.len();
    let manifest_uri = ResourceStore::manifest_uri_for(session_id, layout_opt);
    let mut meta = result.get("result").cloned().unwrap_or_else(|| json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("path");
        obj.insert("uri".into(), Value::String(png_uri.clone()));
        obj.insert(
            "pyramid".into(),
            json!({
                "level": level,
                "x": x,
                "y": y,
                "tile_world_bounds": tile_bounds,
                "tile_uri": png_uri,
                "manifest_uri": manifest_uri,
            }),
        );
    }
    resources.insert_tile_for_layout(
        session_id,
        layout_opt,
        level,
        x,
        y,
        b64.clone(),
        bytes_len,
        meta.clone(),
    );
    if is_json {
        let meta_text = serde_json::to_string_pretty(&meta).unwrap_or_else(|_| meta.to_string());
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
    Ok(json!({
        "contents": [
            {
                "uri": png_uri,
                "mimeType": "image/png",
                "blob": b64
            }
        ]
    }))
}

pub(super) fn read_resource(
    uri: &str,
    clients: &mut HashMap<String, GuiClient>,
    resources: &mut ResourceStore,
    pump: &mut CancelPump,
    mcp_id: Option<&Value>,
) -> Result<Value, String> {
    if let Some(name) = uri.strip_prefix("cad://guides/") {
        return guide_resource(name);
    }
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
                let manifest_val = serde_json::to_value(&manifest).map_err(|e| e.to_string())?;
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
            } else if let Some(layout_rest) = subpath.strip_prefix("layout/") {
                let Some((enc_layout, tail)) = layout_rest.split_once('/') else {
                    return Err(format!("Unsupported layout resource URI: {uri}"));
                };
                let Some(layout) = decode_uri_segment(enc_layout).filter(|s| !s.trim().is_empty())
                else {
                    return Err(format!("Invalid layout encoding in URI: {uri}"));
                };
                if tail == "pyramid/manifest.json" {
                    let manifest = resources
                        .get_layout_manifest(session_id, &layout)
                        .ok_or_else(|| {
                            format!("No cached pyramid manifest for layout '{layout}'; capture layout '{layout}' first")
                        })?;
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
                } else if let Some(trimmed) = tail.strip_prefix("tile/") {
                    if let Some((level, x, y, is_json)) = parse_tile_subpath(trimmed) {
                        return serve_tile_resource(
                            uri,
                            session_id,
                            Some(&layout),
                            level,
                            x,
                            y,
                            is_json,
                            clients,
                            resources,
                            pump,
                            mcp_id,
                        );
                    }
                }
                return Err(format!("Unsupported layout resource URI: {uri}"));
            } else if let Some(trimmed) = subpath.strip_prefix("tile/") {
                if let Some((level, x, y, is_json)) = parse_tile_subpath(trimmed) {
                    return serve_tile_resource(
                        uri, session_id, None, level, x, y, is_json, clients, resources, pump,
                        mcp_id,
                    );
                }
            } else if subpath == "viewport.png" {
                let path = private_capture_dir()?.join(format!("ocs-resource-{}.png", random_id()?));
                let _capture_guard = TempCaptureFile(path.clone());
                let req = json!({
                    "op": "capture",
                    "path": path.to_string_lossy(),
                    "scope": "viewport",
                    "max_dimension": 1600,
                });
                let result = client(clients, session_id)?.request(req, 15.0, pump, mcp_id)?;
                if result["ok"].as_bool() != Some(true)
                    || result["status"].as_str() != Some("completed")
                {
                    return Err(result.to_string());
                }
                let bytes = read_private_capture(&path)?;
                drop(_capture_guard);

                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let hash = crate::io::recovery::hex_digest(hasher.finalize());
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

/// The capture used to compute a visual diff against the previous frame.
/// That comparison was never trustworthy — its baseline reset whenever the
/// camera moved and the on-screen command line counted as changed pixels —
/// so it was removed rather than repaired, and a request that still asks for
/// it is refused by name instead of quietly returning a plain capture.
pub(super) fn refuse_retired_capture_parameters(arguments: &Value) -> Result<(), String> {
    const RETIRED: [&str; 3] = ["diff", "diff_mode", "reset_diff_baseline"];
    match RETIRED.iter().find(|key| arguments.get(**key).is_some()) {
        Some(key) => Err(format!(
            "ocs_capture no longer computes a visual diff; remove '{key}' (retired: {}) and compare two captures yourself",
            RETIRED.join(", ")
        )),
        None => Ok(()),
    }
}

pub(super) fn capture_layout_matches(entry: &Value, requested_layout: Option<&str>) -> bool {
    match requested_layout {
        None => true,
        Some(req_lay) => entry
            .get("layout")
            .and_then(Value::as_str)
            .is_some_and(|lay| lay.eq_ignore_ascii_case(req_lay)),
    }
}

pub(super) fn manifest_layout_differs(manifest: Option<&Value>, effective_layout: Option<&str>) -> bool {
    match (effective_layout, manifest) {
        (Some(eff_lay), Some(manifest)) => manifest
            .get("layout")
            .and_then(Value::as_str)
            .is_none_or(|lay| !lay.eq_ignore_ascii_case(eff_lay)),
        (None, Some(manifest)) => manifest.get("layout").is_some(),
        _ => false,
    }
}

pub(super) fn bounds_from_array(arr: &[Value], exact_len: bool) -> Option<[f64; 4]> {
    let valid_len = if exact_len {
        arr.len() == 4
    } else {
        arr.len() >= 4
    };
    if !valid_len {
        return None;
    }
    Some([
        arr[0].as_f64()?,
        arr[1].as_f64()?,
        arr[2].as_f64()?,
        arr[3].as_f64()?,
    ])
}

pub(super) fn spatial_world_bounds(meta: &Value) -> Option<[f64; 4]> {
    let wb = meta.get("_spatial")?.get("world_bounds")?;
    let min = wb.get("min")?.as_array()?;
    let max = wb.get("max")?.as_array()?;
    if min.len() < 2 || max.len() < 2 {
        return None;
    }
    Some([
        min[0].as_f64()?,
        min[1].as_f64()?,
        max[0].as_f64()?,
        max[1].as_f64()?,
    ])
}

pub(super) fn resolve_tile_base_bounds(
    arguments: &Value,
    resources: &ResourceStore,
    session_id: &str,
    requested_layout: Option<&str>,
) -> Result<[f64; 4], String> {
    const DEFAULT_BOUNDS: [f64; 4] = [-100.0, -100.0, 100.0, 100.0];
    if let Some(bounds_arr) = arguments.get("bounds").and_then(Value::as_array) {
        return bounds_from_array(bounds_arr, false)
            .filter(|b| b.iter().all(|v| v.is_finite()))
            .ok_or_else(|| {
                "bounds must be a 4-element finite numeric array [min_x, min_y, max_x, max_y]"
                    .to_string()
            });
    }
    if let Some(req_lay) = requested_layout {
        if let Some(bounds) = resources
            .get_layout_manifest(session_id, req_lay)
            .and_then(|m| m.get("world_bounds").and_then(Value::as_array))
            .and_then(|arr| bounds_from_array(arr, true))
        {
            return Ok(bounds);
        }
        if let Some(bounds) = resources
            .pyramid_manifests
            .get(session_id)
            .filter(|m| capture_layout_matches(m, Some(req_lay)))
            .and_then(|m| m.get("world_bounds").and_then(Value::as_array))
            .and_then(|arr| bounds_from_array(arr, true))
        {
            return Ok(bounds);
        }
        if let Some(bounds) = resources
            .latest
            .get(session_id)
            .and_then(|latest_uri| resources.snapshots.get(latest_uri))
            .filter(|snap| capture_layout_matches(&snap.metadata, Some(req_lay)))
            .and_then(|snap| spatial_world_bounds(&snap.metadata))
        {
            return Ok(bounds);
        }
        if let Some(bounds) = resources
            .order
            .iter()
            .rev()
            .filter_map(|uri| resources.snapshots.get(uri))
            .find(|snap| {
                snap.session_id == session_id
                    && capture_layout_matches(&snap.metadata, Some(req_lay))
            })
            .and_then(|snap| spatial_world_bounds(&snap.metadata))
        {
            return Ok(bounds);
        }
        return Err(format!(
            "No cached bounds or pyramid manifest for layout '{req_lay}'; capture layout '{req_lay}' first or supply explicit 'bounds'"
        ));
    }
    if let Some(manifest) = resources
        .pyramid_manifests
        .get(session_id)
        .filter(|m| capture_layout_matches(m, None))
    {
        if let Some(bounds) = manifest
            .get("world_bounds")
            .and_then(Value::as_array)
            .and_then(|arr| bounds_from_array(arr, true))
        {
            return Ok(bounds);
        }
    }
    if let Some(snap) = resources
        .latest
        .get(session_id)
        .and_then(|latest_uri| resources.snapshots.get(latest_uri))
        .filter(|snap| capture_layout_matches(&snap.metadata, None))
    {
        if let Some(bounds) = spatial_world_bounds(&snap.metadata) {
            return Ok(bounds);
        }
    }
    Ok(DEFAULT_BOUNDS)
}
