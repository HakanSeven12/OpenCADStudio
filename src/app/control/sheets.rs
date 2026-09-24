//! Sheet-level and environment automation operations — layouts, page setups,
//! system variables, the document lifecycle and saved selection sets.

use super::*;

impl OpenCADStudio {
    /// `close` — close a document tab. A dirty
    /// document is refused unless `"discard":true`; discarding clears the
    /// dirty flag so the close path skips the unsaved-changes dialog.
    pub(super) fn control_close(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let index = if let Some(id) = req["document_id"].as_u64() {
            self.tabs
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| failure("document_closed", "Document absent"))?
        } else {
            self.active_tab
        };
        if self.tabs[index].is_start {
            self.set_control_result(json!({ "closed": false, "reason": "start_page" }));
            return Ok(Task::none());
        }
        if self.tabs[index].dirty && !req["discard"].as_bool().unwrap_or(false) {
            return Err(failure(
                "document_dirty",
                "Save the document or pass discard:true",
            ));
        }
        self.tabs[index].dirty = false;
        let remaining = self.tabs.len();
        // The close may queue async work (autosave cleanup) — return the
        // task so the runtime drives it.
        let close_task = self.update(Message::TabClose(index));
        let closed_document = req["document_id"].as_u64();
        self.set_control_result(json!({
            "closed": true,
            "documents_before": remaining,
            "documents_after": self.tabs.len(),
            "closed_document_id": closed_document,
        }));
        Ok(close_task)
    }

    /// `sysvar` — read/write the curated system-variable registry
    /// (read and write).
    /// `{"op":"sysvar","get":["LTSCALE","EXTMIN"]}` reads; `{"op":"sysvar",
    /// "set":{"MIRRTEXT":0}}` writes (one undo step, validated per name).
    pub(super) fn control_sysvar(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        if let Some(set) = req["set"].as_object() {
            self.push_undo_snapshot(self.active_tab, "SYSVAR");
            let mut applied = serde_json::Map::new();
            for (name, value) in set {
                set_sysvar(&mut self.tabs[self.active_tab].scene.document, name, value)
                    .map_err(|e| {
                        // Nothing committed yet — the snapshot is empty.
                        e
                    })?;
                applied.insert(name.to_ascii_lowercase(), value.clone());
            }
            self.set_control_result(json!({ "set": applied }));
        }
        if let Some(get) = req["get"].as_array() {
            let scene = &self.tabs[self.active_tab].scene;
            let mut values = serde_json::Map::new();
            for name in get {
                let name = name.as_str().unwrap_or("");
                values.insert(
                    name.to_ascii_lowercase(),
                    read_sysvar(scene, name).ok_or_else(|| {
                        failure(
                            "unknown_sysvar",
                            format!(
                                "Unknown system variable '{name}'. Readable: {}",
                                READABLE_SYSVARS.join(", ")
                            ),
                        )
                    })?,
                );
            }
            self.set_control_result(json!({ "values": values }));
        }
        if req.get("get").is_none() && req.get("set").is_none() {
            return Err(failure(
                "get_or_set_required",
                "Supply get:[names] and/or set:{name:value}",
            ));
        }
        Ok(Task::none())
    }

    /// `layout_create` — add a layout with the default A4 page setup.
    pub(super) fn control_layout_create(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        if name.is_empty() {
            return Err(failure("name_required", "Supply a layout name"));
        }
        let i = self.active_tab;
        if self.tabs[i].scene.layout_names().iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            return Err(failure(
                "layout_exists",
                format!("Layout '{name}' already exists"),
            ));
        }
        self.push_undo_snapshot(i, "LAYOUTCREATE");
        self.tabs[i]
            .scene
            .document
            .add_layout(&name)
            .map_err(|e| failure("layout_failed", e))?;
        let plot_style = self
            .active_plot_style
            .as_ref()
            .map(|style| style.name.clone())
            .unwrap_or_default();
        let layout_flags = i16::from(
            self.tabs[i].scene.document.header.paper_space_linetype_scaling,
        ) | (i16::from(self.tabs[i].scene.document.header.paper_space_limit_check) << 1);
        for obj in self.tabs[i].scene.document.objects.values_mut() {
            if let acadrust::objects::ObjectType::Layout(layout) = obj {
                if layout.name == name {
                    layout.flags = layout_flags;
                    crate::scene::apply_default_page_setup(layout, &plot_style);
                    break;
                }
            }
        }
        self.tabs[i].scene.ensure_sheet_viewport(&name);
        self.tabs[i].dirty = true;
        let layouts = self.tabs[i].scene.layout_names();
        self.set_control_result(json!({ "layout": name, "layouts": layouts }));
        Ok(Task::none())
    }

    /// `page_setup_set` — write a layout's plot configuration
    /// Explicit fields win; everything else
    /// keeps the layout's stored setup. Paper names resolve through the
    /// catalog; scales are `"fit"` or `"paper:drawing"` like `"1:100"`.
    pub(super) fn control_page_setup_set(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let layout = string(req, "layout")?.to_owned();
        let i = self.active_tab;
        if !self.tabs[i].scene.layout_names().iter().any(|n| n.eq_ignore_ascii_case(&layout)) {
            return Err(failure(
                "layout_missing",
                format!("Layout '{layout}' does not exist"),
            ));
        }
        let mut ps = self
            .tabs[i]
            .scene
            .plot_settings_for(&layout)
            .unwrap_or_else(|| acadrust::objects::PlotSettings::new(String::new()));
        if let Some(paper) = req["paper"].as_str().filter(|p| !p.is_empty()) {
            let resolved = crate::io::paper_catalog::resolve(paper)
                .ok_or_else(|| failure("invalid_paper", format!("Unknown paper '{paper}'")))?;
            ps.paper_size = resolved.canonical.to_string();
            let (width, height) = resolved.portrait_mm();
            ps.paper_width = width;
            ps.paper_height = height;
            ps.rotation = if req["orientation"].as_str().is_some_and(|o| o.eq_ignore_ascii_case("portrait")) {
                acadrust::objects::PlotRotation::None
            } else {
                acadrust::objects::PlotRotation::Degrees90
            };
        }
        if let Some(fit) = req["fit"].as_bool() {
            if fit {
                ps.set_scale_to_fit();
                ps.flags.use_standard_scale = true;
            } else {
                ps.flags.use_standard_scale = false;
            }
        }
        if let Some(scale) = req["scale"].as_str().filter(|s| !s.is_empty()) {
            let parsed = scale.split_once(':').and_then(|(a, b)| {
                let numerator = a.trim().parse::<f64>().ok()?;
                let denominator = b.trim().parse::<f64>().ok()?;
                (numerator > 0.0 && denominator > 0.0).then_some((numerator, denominator))
            });
            let Some((numerator, denominator)) = parsed else {
                return Err(failure(
                    "invalid_scale",
                    "scale must look like \"1:100\" (paper:drawing, both > 0)",
                ));
            };
            ps.scale_numerator = numerator;
            ps.scale_denominator = denominator;
            ps.standard_scale_factor = numerator / denominator;
            ps.flags.use_standard_scale = false;
        }
        if let Some(center) = req["center"].as_bool() {
            ps.flags.plot_centered = center;
        }
        if let Some(style) = req["plot_style"].as_str() {
            ps.current_style_sheet = style.to_owned();
            ps.flags.plot_plot_styles = !style.is_empty();
        }
        if let Some(window) = req["window"].as_array().filter(|v| v.len() == 4) {
            let coord = |k: usize| window[k].as_f64().unwrap_or(0.0);
            ps.set_plot_window(coord(0), coord(1), coord(2), coord(3));
            ps.plot_type = acadrust::objects::PlotType::Window;
        }
        self.push_undo_snapshot(i, "PAGESETUP");
        if !self.tabs[i].scene.set_layout_plot_settings(&layout, &ps) {
            return Err(failure("layout_missing", format!("Layout '{layout}' vanished")));
        }
        self.tabs[i].dirty = true;
        self.set_control_result(json!({
            "layout": layout,
            "paper": ps.paper_size,
            "scale": json!({
                "numerator": ps.scale_numerator,
                "denominator": ps.scale_denominator,
                "fit": ps.flags.use_standard_scale,
            }),
            "plot_style": ps.current_style_sheet,
        }));
        Ok(Task::none())
    }

    /// `entities_copy_to` — copy entities into another open document
    /// Referenced layer definitions travel along;
    /// the new handles are returned.
    pub(super) fn control_entities_copy_to(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let handles = crate::app::control::entities::hex_handles(req, "handles")?;
        let target = req["document_id"]
            .as_u64()
            .ok_or_else(|| failure("document_required", "Supply the target document_id"))?;
        let source_index = self.active_tab;
        let target_index = self
            .tabs
            .iter()
            .position(|t| t.id == target)
            .ok_or_else(|| failure("document_closed", "Target document absent"))?;
        crate::app::control::entities::require_existing(
            &self.tabs[source_index].scene.document,
            &handles,
        )?;

        // Clone out of the source first so the two tabs never borrow together.
        let cloned: Vec<acadrust::EntityType> = handles
            .iter()
            .filter_map(|handle| self.tabs[source_index].scene.document.get_entity(*handle))
            .map(|entity| entity.clone())
            .collect();

        // Copy the referenced layer definitions out of the source before the
        // target borrow starts (the two tabs never borrow together).
        let mut missing_layers: Vec<acadrust::tables::Layer> = Vec::new();
        {
            let source_document = &self.tabs[source_index].scene.document;
            for entity in &cloned {
                let layer = entity.common().layer.clone();
                if layer.is_empty()
                    || layer == "0"
                    || source_document.layers.contains(&layer)
                    || missing_layers.iter().any(|l| l.name == layer)
                {
                    continue;
                }
                if let Some(source_layer) = source_document.layers.get(&layer) {
                    missing_layers.push(source_layer.clone());
                }
            }
        }

        self.push_undo_snapshot(target_index, "COPYTO");
        let mut created = Vec::new();
        {
            let scene = &mut self.tabs[target_index].scene;
            crate::io::linetypes::populate_document(&mut scene.document);
            for layer_record in &missing_layers {
                if !scene.document.layers.contains(&layer_record.name) {
                    let mut layer_record = layer_record.clone();
                    layer_record.handle = scene.document.allocate_handle();
                    let _ = scene.document.layers.add(layer_record);
                }
            }
            for entity in cloned {
                created.push(format!("{:X}", scene.add_entity_clone(entity).value()));
            }
        }
        self.tabs[target_index].dirty = true;
        self.post_ref_op(target_index);
        let created_count = created.len();
        self.set_control_result(json!({
            "document_id": target,
            "created": created,
            "count": created_count,
        }));
        Ok(Task::none())
    }

    /// `group_create` — attach a named group to the listed entities
    /// (stored in the drawing's group dictionary).
    pub(super) fn control_group_create(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        if name.is_empty() {
            return Err(failure("name_required", "Supply a group name"));
        }
        let handles = crate::app::control::entities::hex_handles(req, "handles")?;
        let i = self.active_tab;
        crate::app::control::entities::require_existing(&self.tabs[i].scene.document, &handles)?;
        self.push_undo_snapshot(i, "GROUPCREATE");
        let handle = self.tabs[i].scene.create_group(name.clone(), handles);
        self.tabs[i].dirty = true;
        self.set_control_result(json!({
            "group": name,
            "handle": format!("{:X}", handle.value()),
        }));
        Ok(Task::none())
    }

    /// `selection_set_save` — remember a named handle set for this session
    /// (session-scoped by design).
    pub(super) fn control_selection_set_save(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        if name.is_empty() {
            return Err(failure("name_required", "Supply a set name"));
        }
        let handles = crate::app::control::entities::hex_handles(req, "handles")?;
        let stored: Vec<String> = handles
            .iter()
            .map(|h| format!("{:X}", h.value()))
            .collect();
        self.control
            .selection_sets
            .insert(name.clone(), handles);
        let count = self
            .control
            .selection_sets
            .get(&name)
            .map(|set| set.len())
            .unwrap_or(0);
        self.set_control_result(json!({ "name": name, "handles": stored, "count": count }));
        Ok(Task::none())
    }

    /// `selection_set_load` — recall a saved set. With `"select":true`
    /// (default) the entities become the current selection.
    pub(super) fn control_selection_set_load(
        &mut self,
        req: &Value,
    ) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?.to_owned();
        let handles = self
            .control
            .selection_sets
            .get(&name)
            .cloned()
            .ok_or_else(|| failure("selection_set_missing", format!("No set named '{name}'")))?;
        let stored: Vec<String> = handles
            .iter()
            .map(|h| format!("{:X}", h.value()))
            .collect();
        let select = req["select"].as_bool().unwrap_or(true);
        let mut selected = 0;
        if select {
            let i = self.active_tab;
            let scene = &mut self.tabs[i].scene;
            scene.deselect_all();
            for handle in &handles {
                if scene.document.get_entity(*handle).is_some() {
                    scene.select_entity(*handle, false);
                    selected += 1;
                }
            }
        }
        self.set_control_result(json!({
            "name": name,
            "handles": stored,
            "selected": selected,
        }));
        Ok(Task::none())
    }

    /// `new` with `"template"` — the web build reads no files from a path, so
    /// a template is refused there rather than silently ignored.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn apply_template(&mut self, _template: &str) -> Result<usize, Value> {
        Err(failure(
            "invalid_request",
            "starting from a template needs the desktop build",
        ))
    }

    /// `new` with `"template"` — start an untitled drawing from a DWG/DXF/DWT
    /// file. Tables and styles come from the template; the document keeps no
    /// source path.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn apply_template(&mut self, template: &str) -> Result<usize, Value> {
        let path = std::path::PathBuf::from(template);
        let bytes = self
            .read_drawing(&path)
            .map_err(|e| failure("template_missing", e))?;
        // DWT is a DWG-family file, but the io dispatcher keys on the file
        // extension — hand it a .dwg name and point the document's source
        // path back at the real template.
        let load_path = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dwt"))
        {
            path.with_extension("dwg")
        } else {
            path.clone()
        };
        let (mut document, purged) = crate::io::load_bytes_finalized(&load_path, bytes)
            .map_err(|e| failure("template_failed", e))?;
        document.source_path = Some(path.to_string_lossy().into_owned());
        let i = self.active_tab;
        {
            let scene = &mut self.tabs[i].scene;
            scene.clear();
            scene.document = document;
            scene.load_named_parameters_from_document();
            scene.load_parametric_constraints_from_document();
            scene.material_base_dir = None;
            scene.rebuild_derived_caches();
        }
        crate::app::style_ops::ensure_standard_styles(&mut self.tabs[i].scene.document);
        self.tabs[i].adopt_active_ucs_from_header();
        self.tabs[i].current_path = None;
        self.tabs[i].is_start = false;
        self.tabs[i].dirty = true;
        Ok(purged)
    }

    /// `file_identity` — stable per-document GUID (the drawing's SPMDOCUNIQUE value,
    /// Catalog §1.6). The identity lives on a single invisible marker text
    /// at the origin carrying the XData application "SPMDOCUNIQUE" — the
    /// same marker SPM.ACAD writes — so it survives save/open round trips.
    /// Idempotent get-or-create; `"renew":true` rewrites the marker with a
    /// fresh GUID instead of adding a second one.
    pub(super) fn control_file_identity(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let renew = req["renew"].as_bool().unwrap_or(false);
        let i = self.active_tab;
        let marker = find_file_identity_marker(&self.tabs[i].scene.document);
        if !renew {
            if let Some(handle) = marker {
                if let Some(identity) = marker_identity(&self.tabs[i].scene.document, handle) {
                    self.set_control_result(json!({ "identity": identity, "created": false }));
                    return Ok(Task::none());
                }
            }
        }
        let identity = new_guid_v4();
        self.push_undo_snapshot(i, "FILEIDENTITY");
        match marker {
            Some(handle) => {
                crate::scene::view::dispatch::set_entity_xdata(
                    &mut self.tabs[i].scene.document,
                    handle,
                    FILE_IDENTITY_APP,
                    Some(vec![acadrust::xdata::XDataValue::String(identity.clone())]),
                );
            }
            None => {
                let mut text = acadrust::entities::Text::with_value(
                    FILE_IDENTITY_APP,
                    acadrust::types::Vector3::ZERO,
                )
                .with_height(0.0001);
                text.common.invisible = true;
                let handle = self.tabs[i].scene.add_entity(acadrust::EntityType::Text(text));
                crate::scene::view::dispatch::set_entity_xdata(
                    &mut self.tabs[i].scene.document,
                    handle,
                    FILE_IDENTITY_APP,
                    Some(vec![acadrust::xdata::XDataValue::String(identity.clone())]),
                );
            }
        }
        self.tabs[i].dirty = true;
        self.post_ref_op(i);
        self.set_control_result(json!({ "identity": identity, "created": true }));
        Ok(Task::none())
    }
}

/// The XData application name that carries a drawing's stable identity.
const FILE_IDENTITY_APP: &str = "SPMDOCUNIQUE";

/// The handle of the entity carrying the file identity, if present.
fn find_file_identity_marker(document: &acadrust::CadDocument) -> Option<acadrust::Handle> {
    document
        .entities()
        .find(|entity| {
            entity
                .common()
                .extended_data
                .get_record(FILE_IDENTITY_APP)
                .is_some()
        })
        .map(|entity| entity.common().handle)
}

/// The identity GUID from the marker entity's XData.
fn marker_identity(document: &acadrust::CadDocument, handle: acadrust::Handle) -> Option<String> {
    let record = document
        .get_entity(handle)?
        .common()
        .extended_data
        .get_record(FILE_IDENTITY_APP)?;
    match record.values.first() {
        Some(acadrust::xdata::XDataValue::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// A random RFC 4122 version-4 GUID string, from the OS entropy pool.
fn new_guid_v4() -> String {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS entropy unavailable");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The readable registry; `WRITABLE_SYSVARS` is the subset `set` accepts.
const READABLE_SYSVARS: &[&str] = &[
    "celtscale", "clayer", "ctextstyle", "extmax", "extmin", "filletrad",
    "insunits", "ltscale", "mirrtext", "osmode", "pdmode", "pdsize",
    "textsize",
];

fn read_sysvar(scene: &crate::scene::Scene, name: &str) -> Option<Value> {
    let header = &scene.document.header;
    let value = match name.to_ascii_lowercase().as_str() {
        "ltscale" => json!(header.linetype_scale),
        "pdmode" => json!(header.point_display_mode),
        "pdsize" => json!(header.point_display_size),
        "celtscale" => json!(header.current_entity_linetype_scale),
        "textsize" => json!(header.text_height),
        "filletrad" => json!(header.fillet_radius),
        "mirrtext" => json!(i16::from(header.mirror_text)),
        "insunits" => json!(header.insertion_units),
        "osmode" => json!(header.object_snap_mode),
        "clayer" => json!(header.current_layer_name),
        "ctextstyle" => json!(header.current_text_style_name),
        "extmin" => {
            let (min, _) = scene.model_space_extents()?;
            json!([min.x, min.y, min.z])
        }
        "extmax" => {
            let (_, max) = scene.model_space_extents()?;
            json!([max.x, max.y, max.z])
        }
        _ => return None,
    };
    Some(value)
}

fn set_sysvar(document: &mut acadrust::CadDocument, name: &str, value: &Value) -> Result<(), Value> {
    let failure = |message: &str| crate::app::control::failure("invalid_sysvar_value", message);
    let header = &mut document.header;
    match name.to_ascii_lowercase().as_str() {
        "ltscale" => {
            header.linetype_scale = positive(value, "ltscale", failure)?;
        }
        "pdmode" => {
            header.point_display_mode = integer(value, "pdmode", failure)?;
        }
        "pdsize" => {
            header.point_display_size = value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                failure("pdsize needs a finite number")
            })?;
        }
        "celtscale" => {
            header.current_entity_linetype_scale = positive(value, "celtscale", failure)?;
        }
        "textsize" => {
            header.text_height = positive(value, "textsize", failure)?;
        }
        "filletrad" => {
            header.fillet_radius = value
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(|| failure("filletrad needs a finite number >= 0"))?;
        }
        "mirrtext" => {
            header.mirror_text = value.as_i64().unwrap_or(1) != 0;
        }
        "insunits" => {
            header.insertion_units = integer(value, "insunits", failure)?;
        }
        "osmode" => {
            header.object_snap_mode = value
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| failure("osmode needs an integer"))?;
        }
        "clayer" => {
            let name = value.as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
                failure("clayer needs a layer name")
            })?;
            let handle = document
                .layers
                .get(name)
                .map(|layer| layer.handle)
                .ok_or_else(|| {
                    failure(&format!("Layer '{name}' does not exist"))
                })?;
            header.current_layer_name = name.to_owned();
            header.current_layer_handle = handle;
        }
        "ctextstyle" => {
            let name = value.as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
                failure("ctextstyle needs a text style name")
            })?;
            let handle = document
                .text_styles
                .get(name)
                .map(|style| style.handle)
                .ok_or_else(|| {
                    failure(&format!("Text style '{name}' does not exist"))
                })?;
            header.current_text_style_name = name.to_owned();
            header.current_text_style_handle = handle;
        }
        other => {
            return Err(crate::app::control::failure(
                "unknown_sysvar",
                format!(
                    "System variable '{other}' is read-only or unknown. Writable: {}",
                    "celtscale, clayer, ctextstyle, filletrad, insunits, ltscale, mirrtext, osmode, pdmode, pdsize, textsize"
                ),
            ))
        }
    }
    Ok(())
}

fn positive(
    value: &Value,
    name: &str,
    failure: impl Fn(&str) -> Value,
) -> Result<f64, Value> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| failure(&format!("{name} needs a finite number > 0")))
}

fn integer(
    value: &Value,
    name: &str,
    failure: impl Fn(&str) -> Value,
) -> Result<i16, Value> {
    value
        .as_i64()
        .and_then(|v| i16::try_from(v).ok())
        .ok_or_else(|| failure(&format!("{name} needs a 16-bit integer")))
}
