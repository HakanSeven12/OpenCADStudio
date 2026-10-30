#[cfg(not(target_arch = "wasm32"))]
use crate::io::plot_style::PlotStyleTableExt as _;
use super::*;
use crate::scene::model::object::PropValue;

fn control_color(value: &Value) -> Result<codec::types::Color, Value> {
    if let Some(index) = value.as_i64().and_then(|v| i16::try_from(v).ok()) {
        return Ok(codec::types::Color::from_index(index));
    }
    if let Some(rgb) = value
        .get("rgb")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 3)
    {
        let component = |i: usize| rgb[i].as_u64().and_then(|v| u8::try_from(v).ok());
        return match (component(0), component(1), component(2)) {
            (Some(r), Some(g), Some(b)) => Ok(codec::types::Color::from_rgb(r, g, b)),
            _ => Err(failure(
                "invalid_color",
                "RGB components must be 0 through 255",
            )),
        };
    }
    Err(failure(
        "invalid_color",
        "Use an index number or {rgb:[r,g,b]}",
    ))
}

fn control_lineweight(value: &Value) -> Result<codec::types::LineWeight, Value> {
    let raw = value
        .as_i64()
        .and_then(|v| i16::try_from(v).ok())
        .ok_or_else(|| failure("invalid_lineweight", "Use the raw lineweight value"))?;
    let candidate = codec::types::LineWeight::from_value(raw);
    crate::ui::properties::lw_options()
        .iter()
        .any(|v| v.0 == candidate)
        .then_some(candidate)
        .ok_or_else(|| failure("invalid_lineweight", "Value is not a standard lineweight"))
}

fn color_value(color: codec::types::Color) -> Value {
    if let Some((r, g, b)) = color.true_color_rgb() {
        json!({"rgb":[r,g,b]})
    } else {
        json!(color.index())
    }
}
/// The id a row answers to: its parameter and component for a dynamic row,
/// the palette's static field name otherwise.
pub(crate) fn row_id(p: &crate::scene::model::object::Property) -> String {
    match &p.value {
        PropValue::BlockNumber { key, .. } | PropValue::BlockChoice { key, .. } => {
            crate::scene::block_parameter_properties::field_id(key)
        }
        _ => p.field.to_owned(),
    }
}
/// One Properties-panel row as the control protocol's JSON (`id`, `label`,
/// `kind`, `value`, `options`). The node graph moves values in this form too,
/// so a port's value round-trips through [`OpenCADStudio::set_property_value`].
pub(crate) fn property_json(p: &crate::scene::model::object::Property) -> Value {
    let (kind, value, options) = match &p.value {
        PropValue::ReadOnly(v) | PropValue::ReadOnlyWithTooltip { value: v, .. } => {
            ("readonly", json!(v), Value::Null)
        }
        PropValue::EditText(v) => ("number", json!(v), Value::Null),
        PropValue::PlainText(v) => ("text", json!(v), Value::Null),
        PropValue::Hyperlink(v) => ("hyperlink", json!(v), Value::Null),
        PropValue::Choice { selected, options }
        | PropValue::ChoiceWithTooltip {
            selected, options, ..
        } => ("choice", json!(selected), json!(options)),
        PropValue::EditChoice { value, options } => {
            ("editable_choice", json!(value), json!(options))
        }
        PropValue::LayerChoice(v) => ("layer", json!(v), Value::Null),
        PropValue::LinetypeChoice(v) => ("linetype", json!(v), Value::Null),
        PropValue::BoolToggle { value, .. } => ("bool", json!(value), Value::Null),
        PropValue::ColorChoice(value) | PropValue::NamedColorChoice { color: value, .. } => {
            ("color", color_value(*value), Value::Null)
        }
        PropValue::ColorVaries => ("color", Value::Null, Value::Null),
        PropValue::LwChoice(value) | PropValue::FieldLwChoice { value, .. } => (
            "lineweight",
            json!(value.value()),
            json!(crate::ui::properties::lw_options()
                .iter()
                .map(|v| v.0.value())
                .collect::<Vec<_>>()),
        ),
        PropValue::LwVaries | PropValue::FieldLwVaries { .. } => (
            "lineweight",
            Value::Null,
            json!(crate::ui::properties::lw_options()
                .iter()
                .map(|v| v.0.value())
                .collect::<Vec<_>>()),
        ),
        PropValue::AttrText { tag, value } => {
            ("attribute", json!({"tag":tag,"value":value}), Value::Null)
        }
        PropValue::BlockNumber { value, .. } => ("number", json!(value), Value::Null),
        PropValue::BlockChoice {
            selected, options, ..
        } => (
            "choice",
            options
                .get(*selected)
                .map(|label| json!(label))
                .unwrap_or(Value::Null),
            json!(options),
        ),
        other => ("specialized", json!(format!("{other:?}")), Value::Null),
    };
    let mut row = json!({"id":row_id(p),"label":p.label,"kind":kind,"value":value,"options":options});
    if let PropValue::BlockNumber { key, description, .. }
    | PropValue::BlockChoice { key, description, .. } = &p.value
    {
        row["description"] = json!(description);
        row["component"] =
            json!(crate::scene::block_parameter_properties::component_id(&key.component));
        row["parameter"] = json!(format!("{:X}", key.parameter.value()));
    }
    row
}
pub(super) const NAMES: &[&str] = &[
    "close_modal",
    "recovery_accept",
    "conversion_loss_save_anyway",
    "conversion_loss_back",
    // Native-only: the "changed outside" dialog does not exist in the web
    // build, so neither does its action set.
    #[cfg(not(target_arch = "wasm32"))]
    "external_change_reload",
    #[cfg(not(target_arch = "wasm32"))]
    "external_change_overwrite",
    #[cfg(not(target_arch = "wasm32"))]
    "external_change_save_as",
    #[cfg(not(target_arch = "wasm32"))]
    "external_change_cancel",
    "pdf_dialog_ok",
    "txt2mtxt_settings",
    "view_background",
    "image_adjust",
    "import_file_dialog",
    "pdf_import_pick",
    "pdf_import_dialog",
    "dgn_import_dialog",
    "tds_import_dialog",
    "tds_material_dialog",
    "tds_map_dialog",
    "extraction_outdated",
    "print_all",
    "data_link_manager",
    "data_extraction",
    "refedit_dialog",
    "pdf_password_dialog",
    "dim_inspect_dialog",
    "hyperlink_dialog",
    "spell_dialog",
    "pdf_shx_settings",
    "pdf_layer_toggle",
    "pc_manager_toggle",
    "pc_manager",
    "count_palette",
    "blocks_palette",
    "pdf_page_select",
    "pc_colormap",
    "pc_section",
    "ribbon_tab",
    "ribbon_dropdown",
    "keytips",
    "dialog_ok",
    "dialog_discard",
    "dialog_cancel",
    "block_def_dialog",
    "table_insert_dialog",
    "attdef_dialog",
    "field_dialog",
    "attman_dialog",
    "sheet_set",
    "close_document",
    "toggle_properties",
    "toggle_layers",
    "toggle_grid",
    "toggle_snap",
    "toggle_ortho",
    "grip_menu_pick",
    "mtext_insert",
    "mtext_commit",
    "mtext_cancel",
    "text_input",
    "text_commit",
    "layer_visible",
    "layer_locked",
    "layer_frozen",
    "layer_current",
    "view_home",
    "zoom_extents",
    "undo",
    "redo",
];

/// Parse the `at` placement point for `embed_image`: `[x,y]` or `[x,y,z]`.
fn embed_point(req: &Value) -> Result<codec::types::Vector3, Value> {
    let values = req["at"]
        .as_array()
        .filter(|v| (2..=3).contains(&v.len()))
        .ok_or_else(|| failure("invalid_point", "Expected at:[x,y] or [x,y,z]"))?;
    let mut p = [0.0_f64; 3];
    for (i, v) in values.iter().enumerate() {
        p[i] = v
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure("invalid_point", "Expected finite coordinates"))?;
    }
    Ok(codec::types::Vector3::new(p[0], p[1], p[2]))
}
fn parse_point_2d(val: &Value) -> Result<[f64; 2], Value> {
    let arr = val
        .as_array()
        .filter(|v| v.len() >= 2)
        .ok_or_else(|| failure("invalid_point", "Expected [x, y]"))?;
    let x = arr[0]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| failure("invalid_point", "Expected finite coordinates"))?;
    let y = arr[1]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| failure("invalid_point", "Expected finite coordinates"))?;
    Ok([x, y])
}

fn parse_calibrate(
    cal: &Value,
    pixel_width: u32,
    pixel_height: u32,
) -> Result<(codec::types::Vector3, f64, f64), Value> {
    let pt_a = cal
        .get("point_a")
        .or_else(|| cal.get("pixel_a"))
        .or_else(|| cal.get("p1"))
        .or_else(|| cal.get("source_a"));
    let pt_b = cal
        .get("point_b")
        .or_else(|| cal.get("pixel_b"))
        .or_else(|| cal.get("p2"))
        .or_else(|| cal.get("source_b"));
    let dist = cal
        .get("distance")
        .or_else(|| cal.get("real_distance"))
        .or_else(|| cal.get("length"))
        .and_then(Value::as_f64);

    if let (Some(pa), Some(pb), Some(d_real)) = (pt_a, pt_b, dist) {
        if !d_real.is_finite() || d_real <= 0.0 {
            return Err(failure(
                "invalid_distance",
                "Calibration distance must be positive",
            ));
        }
        let p1 = parse_point_2d(pa)?;
        let p2 = parse_point_2d(pb)?;
        let d_px = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
        if d_px <= 1e-6 {
            return Err(failure(
                "invalid_points",
                "Calibration reference points must be distinct",
            ));
        }
        let scale = d_real / d_px;
        let width = pixel_width as f64 * scale;

        let align_to = cal
            .get("align_to")
            .or_else(|| cal.get("origin"))
            .or_else(|| cal.get("target_a"))
            .map(parse_point_2d)
            .transpose()?
            .unwrap_or([0.0, 0.0]);

        let x_ll = align_to[0] - p1[0] * scale;
        let y_ll = align_to[1] - (pixel_height as f64 - p1[1]) * scale;
        return Ok((codec::types::Vector3::new(x_ll, y_ll, 0.0), width, scale));
    }

    if let (Some(src_pts), Some(tgt_pts)) = (
        cal.get("source_points").and_then(Value::as_array),
        cal.get("target_points").and_then(Value::as_array),
    ) {
        if src_pts.len() >= 2 && tgt_pts.len() >= 2 {
            let p_s1 = parse_point_2d(&src_pts[0])?;
            let p_s2 = parse_point_2d(&src_pts[1])?;
            let p_t1 = parse_point_2d(&tgt_pts[0])?;
            let p_t2 = parse_point_2d(&tgt_pts[1])?;
            let d_px = ((p_s2[0] - p_s1[0]).powi(2) + (p_s2[1] - p_s1[1]).powi(2)).sqrt();
            let d_cad = ((p_t2[0] - p_t1[0]).powi(2) + (p_t2[1] - p_t1[1]).powi(2)).sqrt();
            if d_px <= 1e-6 {
                return Err(failure(
                    "invalid_points",
                    "Source calibration points must be distinct",
                ));
            }
            let scale = d_cad / d_px;
            let width = pixel_width as f64 * scale;
            let x_ll = p_t1[0] - p_s1[0] * scale;
            let y_ll = p_t1[1] - (pixel_height as f64 - p_s1[1]) * scale;
            return Ok((codec::types::Vector3::new(x_ll, y_ll, 0.0), width, scale));
        }
    }

    Err(failure(
        "invalid_calibrate",
        "calibrate requires point_a:[x,y], point_b:[x,y], distance:number, and optional align_to:[x,y]",
    ))
}

impl OpenCADStudio {
    /// `embed_image` — pack the picture at `path` into an OLE2FRAME or linked RasterImage
    /// placed with its lower-left corner at `at` (default width ≈ pixel_width/100).
    /// Supports automatic reference scaling and calibration via `calibrate`:
    /// `{"point_a": [px1, py1], "point_b": [px2, py2], "distance": 6500.0, "align_to": [0, 0]}`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_embed_image(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        let path = string(req, "path")?;
        let image = crate::io::ole_embed::EmbeddedImage::from_file(std::path::Path::new(&path))
            .map_err(|e| failure("embed_failed", e))?;

        let (at, width, scale) = if let Some(cal) = req.get("calibrate") {
            parse_calibrate(cal, image.pixel_width, image.pixel_height)?
        } else if let (Some(src_pts), Some(tgt_pts)) = (
            req.get("source_points").and_then(Value::as_array),
            req.get("target_points").and_then(Value::as_array),
        ) {
            let dummy = json!({"source_points": src_pts, "target_points": tgt_pts});
            parse_calibrate(&dummy, image.pixel_width, image.pixel_height)?
        } else {
            let default_width = (image.pixel_width as f64 / 100.0).max(1.0);
            let width = req["width"]
                .as_f64()
                .filter(|w| *w > 0.0)
                .unwrap_or(default_width);
            let scale = width / image.pixel_width as f64;
            (embed_point(req)?, width, scale)
        };

        let final_at = if req.get("calibrate").is_none() && req.get("source_points").is_none() {
            at
        } else {
            embed_point(req).unwrap_or(at)
        };
        let final_width = req["width"].as_f64().filter(|w| *w > 0.0).unwrap_or(width);
        let height = final_width * image.pixel_height as f64 / image.pixel_width as f64;

        let layer_name = req["layer"].as_str().filter(|l| !l.is_empty());
        let lock_layer = req["lock_layer"].as_bool().unwrap_or(false);

        if req["linked"].as_bool().unwrap_or(false) {
            self.push_undo_snapshot(i, "IMAGEATTACH");
            let handle = {
                let document = &mut self.tabs[i].scene.document;
                let mut entity = codec::entities::RasterImage::with_size(
                    path,
                    final_at,
                    image.pixel_width as f64,
                    image.pixel_height as f64,
                    final_width,
                    height,
                );
                if let Some(l_name) = layer_name {
                    entity.common.layer = l_name.to_string();
                    if !document.layers.contains(l_name) {
                        let mut layer_record = codec::tables::Layer::new(l_name.to_string());
                        layer_record.handle = document.allocate_handle();
                        if lock_layer {
                            layer_record.flags.locked = true;
                        }
                        let _ = document.layers.add(layer_record);
                    } else if lock_layer {
                        if let Some(l) = document.layers.get_mut(l_name) {
                            l.flags.locked = true;
                        }
                    }
                }
                let handle = self.tabs[i]
                    .scene
                    .add_entity(codec::EntityType::RasterImage(entity));
                if handle.is_null() {
                    return Err(failure("embed_failed", "Could not add image"));
                }
                handle
            };
            self.tabs[i].scene.populate_images_from_document();
            if layer_name.is_some() {
                self.refresh_layer_panel();
            }
            self.post_ref_op(i);
            self.set_control_result(json!({
                "handle": format!("{:X}", handle.value()),
                "kind": "RasterImage",
                "path": path,
                "at": [final_at.x, final_at.y, final_at.z],
                "width": final_width,
                "height": height,
                "scale": scale,
                "linked": true,
                "layer": layer_name.unwrap_or("0"),
            }));
            return Ok(Task::none());
        }

        self.push_undo_snapshot(i, "IMAGEEMBED");
        let handle = crate::io::ole_embed::add_embedded_image(
            &mut self.tabs[i].scene.document,
            &image,
            final_at,
            final_width,
        )
        .map_err(|e| failure("embed_failed", e))?;

        if let Some(l_name) = layer_name {
            let document = &mut self.tabs[i].scene.document;
            if !document.layers.contains(l_name) {
                let mut layer_record = codec::tables::Layer::new(l_name.to_string());
                layer_record.handle = document.allocate_handle();
                if lock_layer {
                    layer_record.flags.locked = true;
                }
                let _ = document.layers.add(layer_record);
            } else if lock_layer {
                if let Some(l) = document.layers.get_mut(l_name) {
                    l.flags.locked = true;
                }
            }
            if let Some(ent) = document.get_entity_mut(handle) {
                ent.common_mut().layer = l_name.to_string();
            }
            self.refresh_layer_panel();
        }

        self.post_ref_op(i);
        self.set_control_result(json!({
            "handle": format!("{:X}", handle.value()),
            "kind": "Ole2Frame",
            "path": path,
            "at": [final_at.x, final_at.y, final_at.z],
            "width": final_width,
            "height": height,
            "scale": scale,
            "linked": false,
            "layer": layer_name.unwrap_or("0"),
        }));
        Ok(Task::none())
    }

    /// `wblock` — write the named block's entities or the listed entity handles
    /// out to a standalone DWG/DXF file. The open document is not modified, so
    /// this needs no undo snapshot. Format follows the path extension.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_wblock(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        if self.tabs[self.active_tab].block_test.is_some() {
            return Err(failure(
                "temporary_document",
                "Test Block is temporary and cannot be saved",
            ));
        }
        let path = string(req, "path")?;
        let document = &self.tabs[self.active_tab].scene.document;
        // A template base keeps its tables/styles in the exported file
        // (Catalog §2.1: clone from a .dwt/.dwg template).
        let template_base = match req["template"].as_str().filter(|t| !t.is_empty()) {
            Some(template) => Some(
                crate::io::load_file(std::path::Path::new(template))
                    .map_err(|e| failure("template_missing", e))?,
            ),
            None => None,
        };
        let extracted = if let Some(block) = req["block"].as_str() {
            match &template_base {
                Some(base) => {
                    let mut base = base.clone();
                    crate::modules::insert::wblock::extract_block_into(document, block, &mut base)?;
                    Ok(base)
                }
                None => crate::modules::insert::wblock::extract_block_to_doc(document, block),
            }
        } else if let Some(listed) = req["handles"].as_array() {
            let handles: Result<Vec<codec::Handle>, Value> = listed
                .iter()
                .map(|v| {
                    u64::from_str_radix(
                        v.as_str()
                            .unwrap_or("")
                            .trim_start_matches("0x")
                            .trim_start_matches("0X"),
                        16,
                    )
                    .map(codec::Handle::new)
                    .map_err(|_| {
                        failure(
                            "invalid_handle",
                            format!("Expected a hexadecimal handle, got {v}"),
                        )
                    })
                })
                .collect();
            match &template_base {
                Some(base) => {
                    let mut base = base.clone();
                    crate::modules::insert::wblock::extract_entities_into(
                        document, &handles?, &mut base,
                    )?;
                    Ok(base)
                }
                None => {
                    crate::modules::insert::wblock::extract_entities_to_doc(document, &handles?)
                }
            }
        } else {
            return Err(failure(
                "selection_required",
                "Supply \"handles\" or \"block\"",
            ));
        };
        let mut extracted = extracted.map_err(|e| failure("wblock_failed", e))?;
        // CF-01.2: optionally shift the export so its overall bounds minimum
        // lands on the origin (SPM.ACAD normalizes cloned drawings to 0,0,0).
        let normalized = req["normalize"].as_bool().unwrap_or(false);
        if normalized {
            crate::modules::insert::wblock::normalize_to_origin(&mut extracted);
        }
        let entities = extracted.entities().count();
        // A .dwt target is written as DWG bytes under a hidden scratch name,
        // then renamed (DWT is a DWG-family file).
        let mut target = std::path::PathBuf::from(path);
        let mut scratch = None;
        if target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dwt"))
        {
            scratch = Some(target.clone());
            target = target.with_file_name(format!(
                ".{}.tmp.dwg",
                target.file_name().unwrap().to_string_lossy()
            ));
        }
        crate::io::save(&extracted, &target).map_err(|e| failure("save_failed", e))?;
        if let Some(final_path) = scratch {
            std::fs::rename(&target, &final_path)
                .map_err(|e| failure("save_failed", format!("rename to .dwt failed: {e}")))?;
        }
        self.set_control_result(json!({
            "path": path,
            "entities": entities,
            "normalized": normalized,
        }));
        Ok(Task::none())
    }

    /// `plot` — render the current drawing to a PDF with every choice taken
    /// from the request instead of the Plot dialog. Reuses the GUI pipeline
    /// (area jobs over `plot_scene_content`, saved page setups for layouts);
    /// dialog/layout/camera state is snapshotted and restored around the
    /// build, exactly like the print-all path. Explicitly supplied request
    /// fields win over a layout's stored page setup; unspecified fields keep
    /// the stored setup (layouts) or the app's plot defaults (Model).
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn control_plot(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        use crate::io::pdf_export::PdfPageInput;

        let path = string(req, "path")?;
        let lower = path.to_ascii_lowercase();
        // SVG shares the plot page with PDF: the writer follows the extension.
        let is_svg = lower.ends_with(".svg");
        if !is_svg && !lower.ends_with(".pdf") {
            return Err(failure("invalid_path", "plot writes .pdf or .svg files"));
        }
        self.stamp_plot_fields();
        let i = self.active_tab;
        let names = self.tabs[i].scene.layout_names();
        let requested = req["layout"].as_str().unwrap_or("Model").to_owned();
        let targets: Vec<String> = if requested.eq_ignore_ascii_case("all") {
            names
        } else {
            vec![names
                .iter()
                .find(|name| name.eq_ignore_ascii_case(&requested))
                .cloned()
                .ok_or_else(|| {
                    failure(
                        "layout_missing",
                        format!("Layout '{requested}' does not exist"),
                    )
                })?]
        };

        // Optional plot style table: a CTB path or a folder-discovered name.
        let style = req["plot_style"].as_str().filter(|s| !s.is_empty());
        let loaded_style = match style {
            Some(name) => Some(
                crate::io::plot_style::PlotStyleTable::load(std::path::Path::new(name))
                    .or_else(|_| crate::io::plot_style::PlotStyleTable::load_named(name))
                    .map_err(|e| failure("plot_style_missing", e))?,
            ),
            None => None,
        };

        // Request → dialog vocabulary, before any stored setup is loaded so
        // the explicit choices can be re-applied on top of it.
        let mut request = self.plot_dialog.clone();
        if let Some(paper) = req["paper"].as_str().filter(|p| !p.is_empty()) {
            request.paper = crate::io::paper_catalog::resolve(paper)
                .ok_or_else(|| failure("invalid_paper", format!("Unknown paper '{paper}'")))?
                .canonical
                .to_string();
            request.paper_width_mm = 0.0;
            request.paper_height_mm = 0.0;
        }
        if let Some(orientation) = req["orientation"].as_str() {
            request.orientation = if orientation.eq_ignore_ascii_case("Portrait") {
                "Portrait".into()
            } else {
                "Landscape".into()
            };
        }
        let explicit_scale = req.get("scale").is_some();
        if let Some(scale) = req["scale"].as_str().filter(|s| !s.is_empty()) {
            request.scale = scale.to_owned();
            request.fit_to_paper = false;
        } else if req.get("fit").is_some() {
            request.fit_to_paper = req["fit"].as_bool().unwrap_or(true);
        }
        let explicit_center = req.get("center").is_some();
        request.center = req["center"].as_bool().unwrap_or(true);
        if let Some(offset) = req["offset_x"].as_f64() {
            request.offset_x = offset.to_string();
        }
        if let Some(offset) = req["offset_y"].as_f64() {
            request.offset_y = offset.to_string();
        }
        if req.get("upside_down").is_some() {
            request.upside_down = req["upside_down"].as_bool().unwrap_or(false);
        }
        for (key, field) in [
            ("transparency", 0),
            ("lineweights", 1),
            ("merge_lines", 2),
            ("stamp", 3),
        ] {
            if let Some(flag) = req[key].as_bool() {
                match field {
                    0 => request.transparency = flag,
                    1 => request.lineweights = flag,
                    2 => request.merge_lines = flag,
                    _ => request.stamp = flag,
                }
            }
        }
        if let Some(table) = &loaded_style {
            self.active_plot_style = Some(table.clone());
            request.style_name = table.name.clone();
            request.apply_plot_styles = true;
            request.style_missing = false;
        }
        let area_explicit = req.get("area").is_some();
        let area = req["area"].as_str().unwrap_or("").to_ascii_lowercase();
        match area.as_str() {
            "" => request.area = "Extents".into(),
            "extents" => request.area = "Extents".into(),
            "display" => request.area = "Display".into(),
            "limits" => request.area = "Limits".into(),
            "layout" => request.area = "Layout".into(),
            "window" => {
                let values = req["window"]
                    .as_array()
                    .filter(|v| v.len() == 4)
                    .ok_or_else(|| {
                        failure(
                            "window_required",
                            "area \"window\" needs window:[x0,y0,x1,y1]",
                        )
                    })?;
                let coord = |k: usize| {
                    values[k].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                        failure("invalid_window", "window must be four finite numbers")
                    })
                };
                request.area = "Window".into();
                request.window = Some((coord(0)?, coord(1)?, coord(2)?, coord(3)?));
            }
            other => return Err(failure("invalid_area", format!("Unknown area '{other}'"))),
        }

        // Snapshot everything the pipeline reads or mutates, like print-all.
        let original_layout = self.tabs[i].scene.current_layout.clone();
        let original_viewport = self.tabs[i].scene.active_viewport;
        let original_psltscale = self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling;
        let original_plimcheck = self.tabs[i].scene.document.header.paper_space_limit_check;
        let original_dialog = self.plot_dialog.clone();
        let original_window = self.plot_window;
        let original_style = self.active_plot_style.clone();
        let original_camera = self.tabs[i].scene.camera.borrow().clone();
        let original_camera_generation = self.tabs[i].scene.camera_generation;

        let build = |app: &mut Self| -> Result<Vec<PdfPageInput>, String> {
            let mut pages = Vec::with_capacity(targets.len());
            for name in &targets {
                let is_model = name.eq_ignore_ascii_case("Model");
                {
                    let scene = &mut app.tabs[i].scene;
                    scene.current_layout = name.clone();
                    scene.active_viewport = None;
                    scene.load_current_layout_state();
                }
                let mut page_dialog = request.clone();
                page_dialog.paper_space = !is_model;
                if is_model {
                    if page_dialog.area == "Layout" {
                        page_dialog.area = "Extents".into();
                    }
                } else if let Some(page_setup) = app.tabs[i].scene.plot_settings_for(name) {
                    // Stored page setup first, then the explicit request
                    // fields on top (a layout plot's behaviour).
                    app.plot_dialog.paper_space = true;
                    app.plot_dialog.window = None;
                    app.load_plotsettings_into_dialog(&page_setup);
                    page_dialog = app.plot_dialog.clone();
                    page_dialog.paper_space = true;
                    if area_explicit {
                        page_dialog.area = request.area.clone();
                    }
                    if request.paper_width_mm == 0.0 && !request.paper.is_empty() {
                        page_dialog.paper = request.paper.clone();
                        page_dialog.paper_width_mm = 0.0;
                        page_dialog.paper_height_mm = 0.0;
                    }
                    if req.get("orientation").is_some() {
                        page_dialog.orientation = request.orientation.clone();
                    }
                    if explicit_scale {
                        page_dialog.scale = request.scale.clone();
                        page_dialog.fit_to_paper = request.fit_to_paper;
                    }
                    if explicit_center {
                        page_dialog.center = request.center;
                    }
                    if req.get("offset_x").is_some() {
                        page_dialog.offset_x = request.offset_x.clone();
                    }
                    if req.get("offset_y").is_some() {
                        page_dialog.offset_y = request.offset_y.clone();
                    }
                    if req.get("upside_down").is_some() {
                        page_dialog.upside_down = request.upside_down;
                    }
                    // Explicit output flags win over the stored page setup too,
                    // like paper, scale and area above.
                    if req["transparency"].is_boolean() {
                        page_dialog.transparency = request.transparency;
                    }
                    if req["lineweights"].is_boolean() {
                        page_dialog.lineweights = request.lineweights;
                    }
                    if req["merge_lines"].is_boolean() {
                        page_dialog.merge_lines = request.merge_lines;
                    }
                    if req["stamp"].is_boolean() {
                        page_dialog.stamp = request.stamp;
                    }
                    // The requested table wins over the layout's own, which
                    // the stored page setup just loaded (or failed to find).
                    if let Some(table) = &loaded_style {
                        app.active_plot_style = Some(table.clone());
                        page_dialog.style_name = table.name.clone();
                        page_dialog.apply_plot_styles = true;
                        page_dialog.style_missing = false;
                        page_dialog.style_error = None;
                    }
                }
                app.plot_dialog = page_dialog;
                if app.plot_dialog.area == "Display" {
                    app.tabs[i].scene.restore_saved_camera();
                }
                if app.plot_dialog.style_missing && app.plot_dialog.apply_plot_styles {
                    return Err(format!(
                        "Plot style table '{}' is not loaded.",
                        app.plot_dialog.style_name
                    ));
                }
                let page = match app.plot_dialog.area.as_str() {
                    "Layout" => Some(app.layout_plot_page_for("Layout")),
                    "Display" => app.display_plot_job(),
                    "Extents" => app.extents_plot_job(),
                    "Limits" => app.limits_plot_job(),
                    "Window" => {
                        if let Some((x0, y0, x1, y1)) = app.plot_dialog.window {
                            app.plot_window = Some((x0, y0, x1, y1));
                        }
                        app.window_plot_job()
                    }
                    _ => None,
                };
                let page = page.ok_or_else(|| format!("Layout '{name}' plot area is empty."))?;
                pages.push(page);
            }
            Ok(pages)
        };
        let built = build(self);
        self.tabs[i].scene.current_layout = original_layout;
        self.tabs[i].scene.active_viewport = original_viewport;
        self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling = original_psltscale;
        self.tabs[i].scene.document.header.paper_space_limit_check = original_plimcheck;
        self.plot_dialog = original_dialog;
        self.plot_window = original_window;
        self.active_plot_style = original_style;
        *self.tabs[i].scene.camera.borrow_mut() = original_camera;
        self.tabs[i].scene.camera_generation = original_camera_generation;
        let pages = built.map_err(|e| failure("plot_failed", e))?;
        if req["per_page"].as_bool().unwrap_or(false) {
            // One file per layout: <stem>-<Layout>.pdf/.svg beside the requested
            // path (sheet-by-sheet delivery).
            let stem = path
                .strip_suffix(".pdf")
                .or_else(|| path.strip_suffix(".PDF"))
                .or_else(|| path.strip_suffix(".svg"))
                .or_else(|| path.strip_suffix(".SVG"))
                .unwrap_or(path)
                .to_owned();
            let ext = if is_svg { "svg" } else { "pdf" };
            let mut files = Vec::with_capacity(pages.len());
            for (name, page) in targets.iter().zip(&pages) {
                let safe: String = name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let file = format!("{stem}-{safe}.{ext}");
                let result = if is_svg {
                    crate::io::svg_export::export_svg_pages(
                        std::slice::from_ref(page),
                        std::path::Path::new(&file),
                        loaded_style.as_ref(),
                    )
                } else {
                    crate::io::pdf_export::export_pdf_pages(
                        std::slice::from_ref(page),
                        std::path::Path::new(&file),
                        loaded_style.as_ref(),
                    )
                };
                result.map_err(|e| failure("plot_failed", e))?;
                files.push(json!({"layout": name, "path": file}));
            }
            self.set_control_result(json!({ "files": files, "pages": pages.len() }));
            return Ok(Task::none());
        }
        // Pages may drop the style when a stored page setup overwrites the
        // dialog's style fields, so the explicit request style rides along as
        // the export-level fallback.
        let result = if is_svg {
            crate::io::svg_export::export_svg_pages(
                &pages,
                std::path::Path::new(path),
                loaded_style.as_ref(),
            )
        } else {
            crate::io::pdf_export::export_pdf_pages(
                &pages,
                std::path::Path::new(path),
                loaded_style.as_ref(),
            )
        };
        result.map_err(|e| failure("plot_failed", e))?;
        self.set_control_result(json!({
            "path": path,
            "pages": pages.len(),
            "page_sizes": pages
                .iter()
                .map(|page| json!([page.paper_w, page.paper_h]))
                .collect::<Vec<_>>(),
        }));
        Ok(Task::none())
    }

    pub(super) fn control_properties(&mut self) -> Value {
        self.refresh_properties();
        json!({"ok":true,"sections":self.tabs[self.active_tab].properties.sections.iter().map(|s|json!({"title":s.title,"properties":s.props.iter().map(property_json).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    /// The grips of the current selection, as the viewport shows them.
    pub(super) fn control_grips(&self) -> Value {
        let tab = &self.tabs[self.active_tab];
        let grips = tab
            .selected_grip_handles
            .iter()
            .zip(&tab.selected_grips)
            .map(|(owner, grip)| {
                json!({
                    "handle": format!("{:X}", owner.value()),
                    "grip_id": grip.id,
                    "point": [grip.world.x, grip.world.y, grip.world.z],
                    "shape": format!("{:?}", grip.shape),
                    "moves_entity": grip.is_midpoint,
                    "axis": grip.axis.map(|a| [a.x, a.y, a.z]),
                })
            })
            .collect::<Vec<_>>();
        // API v11: plugin-declared custom grips of the active tab, each marked
        // with its owning plugin. `moves_entity` is false by construction —
        // the host never moves anything for a custom grip; any edit is the
        // plugin's own, at settle.
        #[cfg(target_arch = "wasm32")]
        let plugin_grips: Vec<Value> = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        let visible = crate::plugin::grip_points::visibility(&self.disabled_plugins);
        #[cfg(not(target_arch = "wasm32"))]
        let plugin_grips = tab
            .plugin_grips
            .query_all(&visible)
            .iter()
            .map(|grip| {
                json!({
                    "plugin": grip.plugin,
                    "grip_id": grip.grip_id,
                    "handle": grip.anchor.map(|handle| format!("{:X}", handle.value())),
                    "point": [grip.point.x, grip.point.y, grip.point.z],
                    "shape": format!("{:?}", grip.shape),
                    "moves_entity": false,
                    "axis": grip.axis.map(|a| [a.x, a.y, a.z]),
                })
            })
            .collect::<Vec<_>>();
        json!({"ok": true, "grips": grips, "plugin_grips": plugin_grips})
    }

    /// Drag one grip of an entity to a world point through the viewport's
    /// own grip gesture: select the entity, engage the grip, move, place.
    /// Object snaps, grid, ortho, polar and tracking are off for the move so
    /// the grip lands on `point`.
    pub(super) fn control_grip_drag(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        // API v11: a `plugin` names the owner of a custom grip — the drag
        // routes to the plugin through the same live machinery the viewport
        // uses (begin → move → settle; preview frames flow; the settle is
        // the plugin's own one-undo-step write).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(plugin) = req["plugin"].as_str() {
            let grip_id = req["grip_id"]
                .as_u64()
                .ok_or_else(|| failure("invalid_request", "Missing grip_id"))?;
            let to = point(req)?;
            return self.control_plugin_grip_drag(i, plugin, grip_id, to.to_array());
        }
        let target = handle(req)?;
        let grip_id = req["grip_id"]
            .as_u64()
            .ok_or_else(|| failure("invalid_request", "Missing grip_id"))?
            as usize;
        let to = point(req)?;
        self.perform_grip_drag(i, target, grip_id, to.to_array())
            .map_err(|(code, message)| failure(code, message))
    }

    /// The MCP/automation entry for a custom-grip drag (API v11/v12): drive
    /// begin → move → settle through the same lifecycle the viewport runs,
    /// so end-to-end tests exercise exactly one pipeline — including the
    /// v11.1 hardening (deliveries never inline command frames; the settle
    /// is issued without waiting; the watchdog bounds a plugin that never
    /// answers). The op waits, bounded by the settle floor, for the final
    /// result: the settle is the plugin's own one-undo-step write window,
    /// and cancel paths are identical to interactive drags.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_plugin_grip_drag(
        &mut self,
        i: usize,
        plugin: &str,
        grip_id: u64,
        to: [f64; 3],
    ) -> Result<Task<Message>, Value> {
        let candidate = {
            let visible = crate::plugin::grip_points::visibility(&self.disabled_plugins);
            self.tabs[i]
                .plugin_grips
                .find(plugin, grip_id, &visible)
                .ok_or_else(|| {
                    failure(
                        "unknown_grip",
                        "Read plugin_grips for the declaring plugin's grip ids",
                    )
                })?
        };
        // begin → move → settle(issued) → drive pump ticks until the settle
        // finalizes (or the watchdog abandons it). Bounded — the UI route
        // never waits, this automation route may.
        let _ = self.begin_plugin_grip_drag(i, &candidate);
        let _ = self.move_plugin_grip_drag(i, glam::DVec3::new(to[0], to[1], to[2]));
        let _ = self.settle_plugin_grip_drag();
        let completed =
            self.drive_plugin_grip_settle_to_completion(std::time::Duration::from_secs(12));
        if !completed {
            return Err(failure(
                "plugin_settle_timeout",
                "The plugin did not settle its grip drag in time; the delivery was abandoned",
            ));
        }
        Ok(iced::Task::none())
    }

    /// The shared programmatic grip-drag core — one pipeline, three
    /// consumers: the interactive viewport itself (the same
    /// `grip_edit_for_hit` / `on_viewport_move` / `commit_active_grip_edit`
    /// machinery), the MCP `grip_drag` op, and the HostApi `grip_drag`
    /// (API v11). Selects `handle`, engages its grip `grip_id`, moves to
    /// `to` with snaps / ortho / polar / tracking off, and commits: exactly
    /// one undo step labeled like the interactive grip step, byte-exact
    /// restore on undo. Refusal parity with the interactive path: unknown
    /// entity, reference-only designation (gripless), locked layer, unknown
    /// grip id, point out of view.
    pub(in crate::app) fn perform_grip_drag(
        &mut self,
        i: usize,
        target: codec::Handle,
        grip_id: usize,
        to: [f64; 3],
    ) -> Result<Task<Message>, (&'static str, &'static str)> {
        if self.tabs.len() <= i {
            return Err(("invalid_request", "Tab does not exist"));
        }
        if self.tabs[i].scene.document.get_entity(target).is_none() {
            return Err(("entity_absent", "No entity with that handle"));
        }
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(target, false);
        self.refresh_properties();
        let (world, moves_entity) = self.tabs[i]
            .selected_grip_handles
            .iter()
            .zip(&self.tabs[i].selected_grips)
            .find(|(owner, grip)| **owner == target && grip.id == grip_id)
            .map(|(_, grip)| (grip.world, grip.is_midpoint))
            .ok_or_else(|| {
                if self.tabs[i].scene.is_reference_only(target) {
                    ("reference_only", "The entity is reference-only")
                } else if self.tabs[i].scene.is_layer_locked(target) {
                    ("layer_locked", "The entity's layer is locked")
                } else {
                    ("unknown_grip", "Read grips for the entity's grip ids")
                }
            })?;
        let edit = self.grip_edit_for_hit(i, target, grip_id, moves_entity, world);
        self.tabs[i].active_grip = Some(edit);

        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let tile = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let local = iced::Rectangle {
            width: tile.width,
            height: tile.height,
            ..iced::Rectangle::default()
        };
        let Some(screen) = self.tabs[i]
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(to[0], to[1], to[2]), local)
        else {
            self.cancel_active_grip_edit();
            return Err(("invalid_point", "The point is not in view"));
        };
        let saved = (
            self.snapper.snap_enabled,
            self.snapper.snap3d_enabled,
            self.snapper.grid_snap_on,
            self.snapper.otrack_enabled,
            self.ortho_mode,
            self.polar_mode,
        );
        self.snapper.snap_enabled = false;
        self.snapper.snap3d_enabled = false;
        self.snapper.grid_snap_on = false;
        self.snapper.otrack_enabled = false;
        self.ortho_mode = false;
        self.polar_mode = false;
        let _ = self.on_viewport_move(iced::Point::new(screen.x + tile.x, screen.y + tile.y));
        (
            self.snapper.snap_enabled,
            self.snapper.snap3d_enabled,
            self.snapper.grid_snap_on,
            self.snapper.otrack_enabled,
            self.ortho_mode,
            self.polar_mode,
        ) = saved;
        Ok(self.commit_active_grip_edit())
    }

    /// Click the viewport where `point` (world) shows, as the left button
    /// would, holding Ctrl and/or Shift when `modifiers` names them.
    /// Where a world point shows on the drawing canvas, through the camera
    /// the cursor edits with: an entered viewport's, else the active tile's.
    fn control_world_to_canvas(&self, at: glam::DVec3) -> Result<iced::Point, Value> {
        let scene = &self.tabs[self.active_tab].scene;
        let (vw, vh) = scene.selection.borrow().view.vp_size;
        let (camera, bounds) = scene.viewport_edit_frame((vw, vh)).unwrap_or_else(|| {
            (
                scene.camera.borrow().clone(),
                scene.active_model_tile_bounds(vw, vh),
            )
        });
        let local = iced::Rectangle {
            width: bounds.width,
            height: bounds.height,
            ..iced::Rectangle::default()
        };
        camera
            .project(at, local)
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .map(|p| iced::Point::new(p.x + bounds.x, p.y + bounds.y))
            .ok_or_else(|| failure("invalid_point", "The point is not in view"))
    }

    pub(super) fn control_click(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let at = point(req)?;
        let modifiers = req["modifiers"].as_str().unwrap_or("").to_ascii_lowercase();
        let cursor = self.control_world_to_canvas(at)?;
        let saved = (self.ctrl_down, self.shift_down);
        self.ctrl_down = modifiers.contains("ctrl");
        self.shift_down = modifiers.contains("shift");
        let tasks = [
            self.update(Message::ViewportMove(cursor)),
            self.update(Message::ViewportLeftPress),
            self.update(Message::ViewportLeftRelease),
        ];
        (self.ctrl_down, self.shift_down) = saved;
        Ok(Task::batch(tasks))
    }

    /// Replay pointer events on the drawing canvas in order, as the mouse
    /// sends them: moves (canvas pixels, or a world point projected through
    /// the editing camera), button presses and releases, and wheel steps.
    /// Each step reports where the cursor landed in the world, so a drag
    /// shows the points the interaction actually received.
    pub(super) fn control_pointer(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        let events = req["events"]
            .as_array()
            .filter(|events| !events.is_empty())
            .ok_or_else(|| failure("invalid_request", "Supply a non-empty events array"))?;
        let modifiers = req["modifiers"].as_str().unwrap_or("").to_ascii_lowercase();
        let coords = |value: &Value, n: usize| -> Option<Vec<f64>> {
            let values: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
            (values.len() == n && values.iter().all(|v| v.is_finite())).then_some(values)
        };
        let saved = (self.ctrl_down, self.shift_down);
        self.ctrl_down = modifiers.contains("ctrl");
        self.shift_down = modifiers.contains("shift");
        let mut tasks = Vec::new();
        let mut steps = Vec::new();
        let mut outcome = Ok(());
        for event in events {
            let message = if let Some(xy) = event.get("move").and_then(|v| coords(v, 2)) {
                Message::ViewportMove(iced::Point::new(xy[0] as f32, xy[1] as f32))
            } else if let Some(at) = event.get("move_world").and_then(|v| coords(v, 3)) {
                match self.control_world_to_canvas(glam::DVec3::new(at[0], at[1], at[2])) {
                    Ok(cursor) => Message::ViewportMove(cursor),
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
                }
            } else if let Some(button) = event.get("press").and_then(Value::as_str) {
                match button {
                    "left" => Message::ViewportLeftPress,
                    "right" => Message::ViewportRightPress,
                    "middle" => Message::ViewportMiddlePress,
                    _ => {
                        outcome = Err(failure("invalid_request", "press is left, right or middle"));
                        break;
                    }
                }
            } else if let Some(button) = event.get("release").and_then(Value::as_str) {
                match button {
                    "left" => Message::ViewportLeftRelease,
                    "right" => Message::ViewportRightRelease,
                    "middle" => Message::ViewportMiddleRelease,
                    _ => {
                        outcome = Err(failure(
                            "invalid_request",
                            "release is left, right or middle",
                        ));
                        break;
                    }
                }
            } else if let Some(lines) = event.get("wheel").and_then(Value::as_f64) {
                Message::ViewportScroll(iced::mouse::ScrollDelta::Lines {
                    x: 0.0,
                    y: lines as f32,
                })
            } else if event.get("leave").and_then(Value::as_bool) == Some(true) {
                Message::ViewportExit
            } else {
                outcome = Err(failure(
                    "invalid_request",
                    "Each event is move, move_world, press, release, wheel or leave",
                ));
                break;
            };
            tasks.push(self.update(message));
            let tab = &self.tabs[i];
            let world = tab.last_cursor_world;
            steps.push(json!({
                "screen":[tab.last_cursor_screen.x, tab.last_cursor_screen.y],
                "world":[world.x, world.y, world.z],
                "dragging_grip":tab.active_grip.is_some(),
            }));
        }
        (self.ctrl_down, self.shift_down) = saved;
        outcome?;
        self.set_control_result(json!({"steps":steps}));
        Ok(Task::batch(tasks))
    }

    pub(super) fn control_set_property(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        self.refresh_properties();
        let field = string(req, "field")?;
        let p = self.tabs[self.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .find(|p| row_id(p) == field)
            .cloned()
            .ok_or_else(|| {
                failure(
                    "unknown_property",
                    "Read properties for the current selection",
                )
            })?;
        self.set_property_value(p, &req["value"])
    }

    /// Write `raw` (protocol JSON, see [`property_json`]) into one Properties
    /// row through the panel's own messages, on the current property targets.
    pub(crate) fn set_property_value(
        &mut self,
        p: crate::scene::model::object::Property,
        raw: &Value,
    ) -> Result<Task<Message>, Value> {
        let value = raw
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| raw.to_string());
        Ok(match p.value {
            PropValue::EditText(_)
            | PropValue::PlainText(_)
            | PropValue::Hyperlink(_)
            | PropValue::EditChoice { .. } => {
                let input = self.update(Message::PropGeomInput {
                    field: p.field,
                    value,
                });
                let commit = self.update(Message::PropGeomCommit(p.field));
                Task::batch([input, commit])
            }
            PropValue::Choice { options, .. } | PropValue::ChoiceWithTooltip { options, .. } => {
                if !options.contains(&value) {
                    return Err(failure(
                        "invalid_choice",
                        "Value is not an available option",
                    ));
                }
                self.update(Message::PropGeomChoiceChanged {
                    field: p.field,
                    value,
                })
            }
            PropValue::LayerChoice(_) => self.update(Message::PropLayerChanged(value)),
            PropValue::LinetypeChoice(_) => self.update(Message::PropLinetypeChanged(value)),
            PropValue::HatchPatternChoice(_) => {
                self.update(Message::PropHatchPatternChanged(value))
            }
            PropValue::BoolToggle { field, value: old } => {
                let v = raw
                    .as_bool()
                    .ok_or_else(|| failure("invalid_value", "Expected boolean"))?;
                if old != v {
                    self.update(Message::PropBoolToggle(field))
                } else {
                    Task::none()
                }
            }
            PropValue::AttrText { tag, .. } => {
                let input = self.update(Message::PropAttrInput {
                    tag: tag.clone(),
                    value,
                });
                let commit = self.update(Message::PropAttrCommit(tag));
                Task::batch([input, commit])
            }
            PropValue::ColorChoice(_)
            | PropValue::NamedColorChoice { .. }
            | PropValue::ColorVaries => {
                let color = control_color(raw)?;
                if p.field == "background_color" {
                    self.update(Message::PropBgColorChanged(color))
                } else if matches!(
                    p.field,
                    "gradient_color_1"
                        | "gradient_color_2"
                        | "dim_line_color"
                        | "dim_ext_line_color"
                        | "dim_text_color"
                        | "dim_text_fill_color"
                        | "line_color"
                        | "text_color"
                        | "block_content_color"
                        | "background_fill_color"
                        | "indicator_fill_color"
                ) {
                    self.update(Message::PropColorFieldChanged {
                        field: p.field.into(),
                        color,
                    })
                } else {
                    self.update(Message::PropColorChanged(color))
                }
            }
            PropValue::LwChoice(_) | PropValue::LwVaries => {
                self.update(Message::PropLwChanged(control_lineweight(raw)?))
            }
            PropValue::FieldLwChoice { field, .. } | PropValue::FieldLwVaries { field } => self
                .update(Message::PropFieldLwChanged {
                    field,
                    value: control_lineweight(raw)?,
                }),
            PropValue::BlockNumber { key, .. } => {
                let input = self.update(Message::PropBlockInput { key, value });
                let commit = self.update(Message::PropBlockCommit(key));
                Task::batch([input, commit])
            }
            PropValue::BlockChoice { key, options, .. } => {
                let index = options
                    .iter()
                    .position(|option| option == &value)
                    .or_else(|| {
                        let index = value.trim().parse::<usize>().ok()?;
                        (index < options.len()).then_some(index)
                    })
                    .ok_or_else(|| {
                        failure("invalid_choice", "Value is not an available option")
                    })?;
                self.update(Message::PropBlockChoice { key, index })
            }
            PropValue::ReadOnly(_) | PropValue::ReadOnlyWithTooltip { .. } => {
                return Err(failure("readonly_property", "Property cannot be edited"))
            }
            _ => {
                return Err(failure(
                    "unsupported_property",
                    "Use the corresponding command for this property type",
                ))
            }
        })
    }
    pub(super) fn control_ui_action(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?;
        // Without a prompt there is no failed open to retry.
        if name == "recovery_accept"
            && self.active_modal != Some(crate::app::ModalKind::RecoveryPrompt)
        {
            return Ok(Task::none());
        }
        let msg = match name {
            "close_modal" => Message::CloseModal,
            // The recovery prompt's Recover button; `close_modal` declines.
            "recovery_accept" => Message::RecoveryAttempt,
            // The conversion-loss warning's choices: save with the losses it
            // lists, or drop the held-back save.
            "conversion_loss_save_anyway" => Message::ConversionLossProceed,
            "conversion_loss_back" => Message::ConversionLossBack,
            // Show a layout (or "Model"), as clicking its tab does.
            "layout_switch" => Message::LayoutSwitch(string(req, "value")?.into()),
            // The "changed outside" modal's suggested actions (D6): reload
            // the disk copy, keep the in-memory one and overwrite, save as,
            // or dismiss. Native only — the web build has no such dialog.
            #[cfg(not(target_arch = "wasm32"))]
            "external_change_reload" => Message::ExternalChangeReload,
            #[cfg(not(target_arch = "wasm32"))]
            "external_change_overwrite" => Message::ExternalChangeOverwrite,
            #[cfg(not(target_arch = "wasm32"))]
            "external_change_save_as" => Message::ExternalChangeSaveAs,
            #[cfg(not(target_arch = "wasm32"))]
            "external_change_cancel" => Message::ExternalChangeCancel,
            // The open PDF dialog's OK button.
            // Underlay Layers: switch a layer of the shown underlay.
            "pdf_layer_toggle" => {
                Message::PdfDialog(crate::ui::window::pdf_dialogs::PdfDialogMsg::LayersToggle(
                    string(req, "value")?.into(),
                ))
            }
            // Point Cloud Color Map: one edit, "tab=elevation", "count=3",
            // "even", "reverse", "scheme=Earth", "gradient=0", "max=90",
            // "min=10", "interval=0.1", "extents=0", "range=2", "current=0".
            "pc_colormap" => {
                use crate::ui::window::pdf_dialogs::{OutOfRange, PdfDialogMsg as M};
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let on = arg != "0";
                Message::PdfDialog(match key {
                    "tab" => M::MapTab(arg == "elevation"),
                    "scheme" => M::MapScheme(arg.into()),
                    "count" => {
                        M::MapCount(arg.parse().map_err(|_| failure("invalid_value", "count"))?)
                    }
                    "even" => M::MapEven,
                    "reverse" => M::MapReverse,
                    "gradient" => M::MapGradient(on),
                    "max" => M::MapMax(arg.into()),
                    "min" => M::MapMin(arg.into()),
                    "interval" => M::MapInterval(arg.into()),
                    "extents" => M::MapExtents(on),
                    "range" => M::MapOutOfRange(OutOfRange(arg.parse().unwrap_or(1))),
                    "current" => M::MapCurrent(on),
                    "new" => M::MapNew,
                    "rename" => M::MapRename,
                    "name" => M::MapNameInput(arg.into()),
                    "name_ok" => M::MapNameOk,
                    "delete" => M::MapDelete,
                    "apply" => M::MapApply,
                    _ => return Err(failure("invalid_value", "Unknown color map edit")),
                })
            }
            // Section extraction dialog: "min=0.01", "connect=0.02", "angle=5",
            // "points=18000", "lines=1", "perimeter=1", "preview=0", "width=0".
            "pc_section" => {
                use crate::ui::window::pdf_dialogs::PdfDialogMsg as M;
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                Message::PdfDialog(match key {
                    "min" => M::SecMinLength(arg.into()),
                    "connect" => M::SecConnect(arg.into()),
                    "angle" => M::SecAngle(arg.into()),
                    "points" => M::SecMaxPoints(arg.into()),
                    "width" => M::SecWidth(arg.into()),
                    "lines" => M::SecPolylines(arg == "0"),
                    "perimeter" => M::SecPerimeter(arg != "0"),
                    "preview" => M::SecPreview(arg != "0"),
                    _ => return Err(failure("invalid_value", "Unknown section setting")),
                })
            }
            // Point Cloud Manager: flip a row's switch, "<handle>:<row>" with
            // row cloud, unassigned, scans or scan:<name>.
            "pc_manager_toggle" => {
                use crate::ui::window::pc_manager::{PcManagerMsg, Row};
                let value = string(req, "value")?;
                let parsed = value.split_once(':').and_then(|(handle, row)| {
                    Some((
                        codec::Handle::new(u64::from_str_radix(handle, 16).ok()?),
                        Row::parse(row)?,
                    ))
                });
                let Some((handle, row)) = parsed else {
                    return Err(failure("bad_value", "Expected <handle>:<row>"));
                };
                Message::PcManager(PcManagerMsg::Toggle(handle, row))
            }
            // Count palette and count mode: "search=<text>", "sort=name|count",
            // "open=NAME", "child=NAME:<n>" (an expanded row), "menu=<review|field|
            // layer|scale|mirror>:NAME", "create_table", "check=NAME:1",
            // "all=1", "table_cancel", "table_insert", "back", "match=<0|1|2>:1",
            // "details", "errors", "error=<n>", "area", "prev", "next", "select",
            // "field", "close", "invalid_undo", "invalid_continue", "invalid_always=1".
            "count_palette" => {
                use crate::ui::window::count_palette::{CountMsg as M, RowAction};
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let flag = |s: &str| s == "1" || s.eq_ignore_ascii_case("true");
                let bad = || failure("invalid_value", "Unknown count palette edit");
                Message::Count(match key {
                    "search" => M::Search(arg.into()),
                    "sort" => M::Sort(arg == "count"),
                    "open" => M::Open(arg.into(), codec::count::CountKey::default()),
                    "child" => {
                        let (name, n) = arg.rsplit_once(':').ok_or_else(bad)?;
                        let n: usize = n.parse().map_err(|_| bad())?;
                        let [l, s, m] = self.count_palette.expansion(name).ok_or_else(bad)?;
                        let i = self.active_tab;
                        let area = self.tabs[i].count.as_ref().and_then(|c| c.area.clone());
                        let instances = codec::count::block_instances(
                            &self.tabs[i].scene.document,
                            area.as_deref(),
                        );
                        let (k, _) = codec::count::expanded_counts(&instances, name, l, s, m)
                            .into_iter()
                            .nth(n)
                            .ok_or_else(bad)?;
                        M::Open(name.into(), k)
                    }
                    "menu" => {
                        let (action, name) = arg.split_once(':').ok_or_else(bad)?;
                        let action = match action {
                            "review" => RowAction::Review,
                            "field" => RowAction::Field,
                            "layer" => RowAction::Expand(0),
                            "scale" => RowAction::Expand(1),
                            "mirror" => RowAction::Expand(2),
                            _ => return Err(bad()),
                        };
                        M::Menu(name.into(), codec::count::CountKey::default(), action)
                    }
                    "create_table" => M::CreateTable,
                    "check" => {
                        let (name, on) = arg.rsplit_once(':').unwrap_or((arg, "1"));
                        M::TableCheck(name.into(), flag(on))
                    }
                    "all" => M::TableAll(flag(arg)),
                    "table_cancel" => M::TableCancel,
                    "table_insert" => M::TableInsert,
                    "back" => M::Back,
                    "match" => {
                        let (k, on) = arg.split_once(':').unwrap_or((arg, "1"));
                        M::Match(
                            k.parse::<usize>().ok().filter(|k| *k < 3).ok_or_else(bad)?,
                            flag(on),
                        )
                    }
                    "details" => M::ToggleDetails,
                    "errors" => M::ToggleErrors,
                    "error" => {
                        let i = self.active_tab;
                        let n: usize = arg.parse().map_err(|_| bad())?;
                        let mode = self.tabs[i].count.as_ref().ok_or_else(bad)?;
                        M::ShowError(
                            *mode
                                .result(&self.tabs[i].scene.document)
                                .errors
                                .get(n)
                                .ok_or_else(bad)?,
                        )
                    }
                    "area" => M::Area,
                    "prev" => M::Prev,
                    "next" => M::Next,
                    "select" => M::Select,
                    "field" => M::Field,
                    "close" => M::Close,
                    // The Invalid Area dialog.
                    "invalid_undo" => M::InvalidUndo,
                    "invalid_continue" => M::InvalidContinue,
                    "invalid_always" => M::InvalidAlways(arg == "1"),
                    _ => return Err(bad()),
                })
            }
            // Point Cloud Manager tree: "search=<text>", "collapse", "expand",
            // "toggle_node=<key>" or "select=<key>".
            "pc_manager" => {
                use crate::ui::window::pc_manager::PcManagerMsg as M;
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                Message::PcManager(match key {
                    "search" => M::Search(arg.into()),
                    "collapse" => M::CollapseAll,
                    "expand" => M::ExpandAll,
                    "toggle_node" => M::Expand(arg.into()),
                    "select" => M::Select(arg.into()),
                    _ => return Err(failure("invalid_value", "Unknown point cloud manager edit")),
                })
            }
            // Blocks palette: "tab=recent", "search=D*", "view=2",
            // "option=x:2", "options", "library=<dir>", "insert=<item>",
            // "drop=<item>@x,y", "menu=<action>:<item>". An item is
            // "current:NAME", "recent:N", "fav:N", "lib:N" or "file:<path>".
            "blocks_palette" => {
                use crate::ui::window::block_palette::{
                    BlockPaletteMsg as M, Item, MenuAction, OptionMsg, Tab, ViewMode,
                };
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let item = |app: &Self, spec: &str| -> Result<Item, Value> {
                    let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
                    let index = || {
                        rest.parse::<usize>()
                            .map_err(|_| failure("invalid_value", "Bad item index"))
                    };
                    let missing = || failure("invalid_value", "No such palette item");
                    Ok(match kind {
                        "current" => Item::Current(rest.to_string()),
                        "recent" => Item::Ref(
                            app.block_palette
                                .recent
                                .get(index()?)
                                .cloned()
                                .ok_or_else(missing)?,
                        ),
                        "fav" => Item::Ref(
                            app.block_palette
                                .favorites
                                .get(index()?)
                                .cloned()
                                .ok_or_else(missing)?,
                        ),
                        "lib" => {
                            let e = app
                                .block_palette
                                .library_entries
                                .get(index()?)
                                .ok_or_else(missing)?;
                            if e.folder {
                                Item::Folder(e.path.clone())
                            } else {
                                Item::File(e.path.clone())
                            }
                        }
                        "file" => Item::File(std::path::PathBuf::from(rest)),
                        _ => return Err(missing()),
                    })
                };
                match key {
                    "tab" => Message::BlockPalette(M::Tab(match arg {
                        "recent" => Tab::Recent,
                        "favorites" => Tab::Favorites,
                        "libraries" => Tab::Libraries,
                        _ => Tab::Current,
                    })),
                    "search" => Message::BlockPalette(M::Search(arg.into())),
                    "view" => {
                        Message::BlockPalette(M::View(ViewMode::from_u8(arg.parse().unwrap_or(1))))
                    }
                    "options" => Message::BlockPalette(M::ToggleOptions),
                    "library" => Message::BlockPalette(M::LibraryPicked(Ok(arg.into()))),
                    "up" => Message::BlockPalette(M::LibraryUp),
                    "option" => {
                        let (name, v) = arg.split_once(':').unwrap_or((arg, "1"));
                        let on = v == "1";
                        Message::BlockPalette(M::Option(match name {
                            "insertion_point" => OptionMsg::InsertionPoint(on),
                            "scale" => OptionMsg::Scale(on),
                            "uniform" => OptionMsg::Uniform(on),
                            "rotation" => OptionMsg::Rotation(on),
                            "auto" => OptionMsg::AutoPlacement(on),
                            "repeat" => OptionMsg::Repeat(on),
                            "explode" => OptionMsg::Explode(on),
                            "x" => OptionMsg::X(v.into()),
                            "y" => OptionMsg::Y(v.into()),
                            "z" => OptionMsg::Z(v.into()),
                            "angle" => OptionMsg::Angle(v.into()),
                            _ => return Err(failure("invalid_value", "Unknown palette option")),
                        }))
                    }
                    "insert" => {
                        let it = item(self, arg)?;
                        self.block_palette.pressed = Some(it.clone());
                        Message::BlockPalette(M::Release(it))
                    }
                    "drop" => {
                        let (spec, at) = arg
                            .rsplit_once('@')
                            .ok_or_else(|| failure("invalid_value", "drop needs @x,y"))?;
                        let xy: Vec<f64> = at
                            .split(',')
                            .filter_map(|v| v.trim().parse().ok())
                            .collect();
                        if xy.len() < 2 {
                            return Err(failure("invalid_value", "drop needs @x,y"));
                        }
                        let it = item(self, spec)?;
                        let i = self.active_tab;
                        self.tabs[i].last_cursor_world =
                            glam::DVec3::new(xy[0], xy[1], xy.get(2).copied().unwrap_or(0.0));
                        self.block_palette.pressed = Some(it);
                        Message::ViewportLeftRelease
                    }
                    "menu" => {
                        let (action, spec) = arg
                            .split_once(':')
                            .ok_or_else(|| failure("invalid_value", "menu needs action:item"))?;
                        let action = match action {
                            "insert" => MenuAction::Insert,
                            "redefine" => MenuAction::Redefine,
                            "favorite" => MenuAction::Favorite,
                            "unfavorite" => MenuAction::Unfavorite,
                            "edit" => MenuAction::Edit,
                            "remove" => MenuAction::Remove,
                            _ => return Err(failure("invalid_value", "Unknown menu action")),
                        };
                        Message::BlockPalette(M::Menu(item(self, spec)?, action))
                    }
                    _ => return Err(failure("invalid_value", "Unknown blocks palette edit")),
                }
            }
            // Attach dialog: choose pages by index ("0,2").
            "pdf_page_select" => {
                let pages: Vec<usize> = string(req, "value")?
                    .split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect();
                let Some(state) = self.pdf_attach.as_mut() else {
                    return Err(failure("no_dialog", "The Attach dialog is not open"));
                };
                state.selected = pages;
                return Ok(Task::none());
            }
            // Bring a ribbon tab forward by module id.
            "ribbon_tab" => {
                let id = string(req, "value")?;
                if !self.ribbon.select_by_id(id) {
                    return Err(failure("no_tab", "No such ribbon tab"));
                }
                return Ok(Task::none());
            }
            // The open Insert Table dialog: `style=…`, `columns=…`, `rows=…`,
            // `width=…`, `lines=…`, `window=0|1`, `columns_fixed=0|1`,
            // `rows_fixed=0|1`, `preview=0|1`, `styles` (the Table Style
            // button), `ok`.
            "table_insert_dialog" => {
                use crate::ui::window::annotation_data::{TableInsertField as F, TableInsertion};
                if self.active_modal != Some(crate::app::ModalKind::InsertTable) {
                    return Err(failure("no_dialog", "The Insert Table dialog is not open"));
                }
                let value = string(req, "value")?;
                let (key, v) = value.split_once('=').unwrap_or((value, ""));
                let on = v == "1";
                match key {
                    "style" => Message::TableInsertStyle(v.to_string()),
                    "columns" => Message::TableInsertField(F::Columns(v.to_string())),
                    "rows" => Message::TableInsertField(F::DataRows(v.to_string())),
                    "width" => Message::TableInsertField(F::ColumnWidth(v.to_string())),
                    "lines" => Message::TableInsertField(F::RowHeight(v.to_string())),
                    "window" => Message::TableInsertField(F::Insertion(if on {
                        TableInsertion::Window
                    } else {
                        TableInsertion::Point
                    })),
                    "columns_fixed" => Message::TableInsertField(F::ColumnsFixed(on)),
                    "rows_fixed" => Message::TableInsertField(F::RowsFixed(on)),
                    "preview" => Message::TableInsertField(F::Preview(on)),
                    "styles" => Message::TableInsertManageStyles,
                    "ok" => Message::TableInsertApply,
                    _ => return Err(failure("invalid_value", "Unknown Insert Table field")),
                }
            }
            // The open Block Definition dialog: `name=…`, `open_in_editor=0|1`,
            // `base_on_screen=0|1`, `objects_on_screen=0|1`, `create_empty`,
            // `select_objects`.
            "block_def_dialog" => {
                if self.block_definition.is_none() {
                    return Err(failure("no_dialog", "The Block Definition dialog is not open"));
                }
                let value = string(req, "value")?;
                let (key, v) = value.split_once('=').unwrap_or((value, ""));
                let on = v == "1";
                match key {
                    "name" => Message::BlockDefNameSelect(v.to_string()),
                    "open_in_editor" => Message::BlockDefOpenInEditor(on),
                    "base_on_screen" => Message::BlockDefBaseOnScreen(on),
                    "objects_on_screen" => Message::BlockDefObjectsOnScreen(on),
                    "create_empty" => Message::BlockDefCreateEmpty,
                    "select_objects" => Message::BlockDefSelectObjects,
                    _ => return Err(failure("invalid_value", "Unknown block definition field")),
                }
            }
            // One field of the open attribute definition dialog: `tag=…`,
            // `prompt=…`, `default=…`, `justify=MC`, `style=…`, `height=…`,
            // `rotation=…`, `width=…`, `on_screen=0|1`, `x=…` / `y=…` / `z=…`,
            // `mode=<bit>:0|1`, `annotative=0|1`, `align_below=0|1`, `edit_value`, `dismiss`.
            "attdef_dialog" => {
                use crate::ui::window::attdef_dialog::{AttdefDialogMsg as M, JustifyChoice};
                let value = string(req, "value")?;
                let (key, v) = value.split_once('=').unwrap_or((value, ""));
                let on = v == "1";
                let editing = self.active_modal == Some(crate::app::ModalKind::AttDefEdit);
                Message::AttdefDialog(match key {
                    "tag" if editing => M::EditTag(v.into()),
                    "prompt" if editing => M::EditPrompt(v.into()),
                    "default" if editing => M::EditDefault(v.into()),
                    "tag" => M::Tag(v.into()),
                    "prompt" => M::Prompt(v.into()),
                    "default" => M::Default(v.into()),
                    "style" => M::Style(v.into()),
                    "height" => M::Height(v.into()),
                    "rotation" => M::Rotation(v.into()),
                    "width" => M::Width(v.into()),
                    "on_screen" => M::OnScreen(on),
                    "annotative" => M::Annotative(on),
                    "align_below" => M::AlignBelow(on),
                    "edit_value" => M::EditValue,
                    "dismiss" => M::DismissError,
                    "insert_field" if editing => M::EditInsertField,
                    "insert_field" => M::InsertField,
                    "x" => M::Coord(0, v.into()),
                    "y" => M::Coord(1, v.into()),
                    "z" => M::Coord(2, v.into()),
                    "justify" => M::Justify(JustifyChoice(
                        crate::modules::draw::draw::attdef::Justify::from_keyword(v)
                            .ok_or_else(|| failure("invalid_value", "Unknown justification"))?,
                    )),
                    "mode" => {
                        let (bit, state) = v.split_once(':').unwrap_or((v, "1"));
                        let bit = bit
                            .parse::<u8>()
                            .map_err(|_| failure("invalid_value", "mode=<bit>:0|1"))?;
                        M::Mode(bit, state == "1")
                    }
                    _ => return Err(failure("invalid_value", "Unknown attdef dialog field")),
                })
            }
            // The open Block Attribute Manager: `block=<name>`, `select=<row>`,
            // `up`, `down`, `remove`, `edit`, `tab=attribute|text|properties`,
            // `tag=…`, `prompt=…`, `default=…`, `mode=<invisible|verify|preset|multiline>:0|1`,
            // `style=…`, `justify=MC`, `height=…`, `rotation=…`, `width=…`, `oblique=…`,
            // `boundary=…`, `annotative=0|1`, `backwards=0|1`, `upside_down=0|1`,
            // `layer=…`, `linetype=…`, `color=…`, `lineweight=…`, `column=<name>:0|1`,
            // `more_columns`, `apply_existing=0|1`, `emphasize=0|1`, `live=0|1`,
            // `pick`, `sync`, `apply`, `ok`, `cancel`.
            "attman_dialog" => {
                use crate::ui::window::attman_dialog::{AttManMsg as A, Column, EditTab};
                let Some(state) = self.attman.as_ref() else {
                    return Err(failure("no_dialog", "The attribute manager is not open"));
                };
                let value = string(req, "value")?;
                let (key, v) = value.split_once('=').unwrap_or((value, ""));
                let on = v == "1";
                Message::AttMan(match key {
                    "block" => A::Block(
                        state
                            .blocks
                            .iter()
                            .position(|b| b.name.eq_ignore_ascii_case(v))
                            .ok_or_else(|| failure("invalid_value", "No such attributed block"))?,
                    ),
                    "select" => A::Select(
                        v.parse()
                            .ok()
                            .filter(|r: &usize| *r < state.rows.len())
                            .ok_or_else(|| failure("invalid_value", "No such row"))?,
                    ),
                    "up" => A::MoveUp,
                    "down" => A::MoveDown,
                    "remove" => A::Remove,
                    "edit" => A::ToggleEdit,
                    "tab" => A::Tab(match v {
                        "text" => EditTab::Text,
                        "properties" => EditTab::Properties,
                        _ => EditTab::Attribute,
                    }),
                    "tag" => A::Tag(v.into()),
                    "prompt" => A::Prompt(v.into()),
                    "default" => A::Default(v.into()),
                    "mode" => {
                        let (mode, state) = v.split_once(':').unwrap_or((v, "1"));
                        let mode = match mode {
                            "invisible" => "invisible",
                            "verify" => "verify",
                            "preset" => "preset",
                            "multiline" => "multiline",
                            _ => return Err(failure("invalid_value", "Unknown mode")),
                        };
                        A::Mode(mode, state == "1")
                    }
                    "style" => A::Style(v.into()),
                    "justify" => A::Justify(
                        crate::modules::draw::draw::attdef::Justify::from_keyword(v)
                            .ok_or_else(|| failure("invalid_value", "Unknown justification"))?,
                    ),
                    "height" => A::Height(v.into()),
                    "rotation" => A::Rotation(v.into()),
                    "width" => A::Width(v.into()),
                    "oblique" => A::Oblique(v.into()),
                    "boundary" => A::Boundary(v.into()),
                    "annotative" => A::Annotative(on),
                    "backwards" => A::Backwards(on),
                    "upside_down" => A::UpsideDown(on),
                    "layer" => A::Layer(v.into()),
                    "linetype" => A::Linetype(v.into()),
                    "color" => A::Color(v.into()),
                    "lineweight" => A::Lineweight(v.into()),
                    "column" => {
                        let (name, state) = v.split_once(':').unwrap_or((v, "1"));
                        A::Column(
                            Column::from_name(name)
                                .ok_or_else(|| failure("invalid_value", "Unknown column"))?,
                            state == "1",
                        )
                    }
                    "more_columns" => A::MoreColumns,
                    "apply_existing" => A::ApplyExisting(on),
                    "emphasize" => A::Emphasize(on),
                    "live" => A::Live(on),
                    "pick" => A::PickBlock,
                    "sync" => A::Sync,
                    "apply" => A::Apply,
                    "ok" => A::Ok,
                    "cancel" => A::Cancel,
                    _ => return Err(failure("invalid_value", "Unknown attribute manager field")),
                })
            }
            // One field of the open Field dialog: `category=<index>`, `name=Date`,
            // `date=yyyy-MM-dd`, `case=0..4`, `file=1|2|3`, `ext=0|1`, `size=0..2`,
            // `sysvar=dimscale`, `diesel=…`, `named_type=<index>`, `named=<index>`,
            // `prop=Area`, `select_object`, `formula=(1+2)*3`, `formula_format=<index>`,
            // `precision=<index>`, `link_text=…`, `link_url=…`, `plot_scale=<index>`.
            "field_dialog" => {
                use crate::ui::window::field_dialog::{self as fd, FieldDialogMsg as F};
                let value = string(req, "value")?;
                let (key, v) = value.split_once('=').unwrap_or((value, ""));
                // The dialog indexes its lists with these, so one out of range
                // would panic while it draws.
                let index = |len: usize| {
                    v.parse::<usize>()
                        .ok()
                        .filter(|i| *i < len)
                        .ok_or_else(|| failure("invalid_value", "index out of range"))
                };
                let named_len = self
                    .field_dialog
                    .as_ref()
                    .map_or(0, |d| d.named_names.len());
                let known = |list: &[&'static str]| {
                    list.iter()
                        .copied()
                        .find(|n| n.eq_ignore_ascii_case(v))
                        .ok_or_else(|| failure("invalid_value", "unknown name"))
                };
                Message::FieldDialog(match key {
                    "category" => F::Category(index(fd::CATEGORIES.len())?),
                    "name" => F::Name(known(&fd::fields_of(0))?),
                    "date" => F::DateFormat(v.into()),
                    "case" => F::TextCase(index(fd::TEXT_CASES.len())?),
                    "file" => F::FileParts(
                        v.parse::<u8>()
                            .map_err(|_| failure("invalid_value", "1|2|3"))?,
                    ),
                    "ext" => F::FileExtension(v == "1"),
                    "size" => F::SizeUnit(index(fd::SIZE_UNITS.len())?),
                    "sysvar" => F::SysVar(v.into()),
                    "diesel" => F::Diesel(v.into()),
                    "named_type" => F::NamedType(index(fd::NAMED_TYPES.len())?),
                    "named" => F::Named(index(named_len)?),
                    "prop" => F::ObjectProp(known(&[
                        "Area",
                        "Center",
                        "Circumference",
                        "Diameter",
                        "EndPoint",
                        "Length",
                        "Radius",
                        "StartPoint",
                    ])?),
                    "select_object" => F::SelectObject,
                    "formula" => F::Formula(v.into()),
                    "formula_format" => {
                        F::FormulaFormat(index(codec::fields::FORMULA_FORMATS.len())?)
                    }
                    "precision" => F::FormulaPrecision(index(fd::FORMULA_PRECISIONS.len())?),
                    "link_text" => F::HyperlinkText(v.into()),
                    "link_url" => F::HyperlinkUrl(v.into()),
                    "plot_scale" => F::PlotScale(index(codec::fields::PLOT_SCALE_FORMATS.len())?),
                    "count" => F::CountExpression(v.into()),
                    "show_instances" => F::ShowCountInstances,
                    "ss_custom" => F::SsCustom(v.into()),
                    "ss_set" => F::SsSet(v.parse().map_err(|_| failure("invalid_value", "index"))?),
                    "ss_dst" => F::SsPicked(Some(std::path::PathBuf::from(v))),
                    "ss_node" => {
                        let state = self
                            .field_dialog
                            .as_ref()
                            .ok_or_else(|| failure("invalid_state", "No Field dialog"))?;
                        let db = state
                            .ss_sets
                            .get(state.ss_set)
                            .ok_or_else(|| failure("invalid_state", "No sheet set"))?;
                        let views = state.name == "SheetView";
                        let id = fd::nav_nodes(db, views)
                            .into_iter()
                            .find(|(_, id, label, _)| label == v || id == v)
                            .map(|(_, id, _, _)| id)
                            .ok_or_else(|| failure("invalid_value", "No such node"))?;
                        F::SsNode(id)
                    }
                    "ss_prop" => F::SsProp(v.into()),
                    "ss_href" => F::SsHref(v == "1"),
                    "ss_placeholder" => F::SsPlaceholder(index(fd::PLACEHOLDER_TYPES.len())?),
                    "ss_scale" => F::SsScaleFormat(index(codec::fields::PLOT_SCALE_FORMATS.len())?),
                    "placeholder" => F::PlaceholderProperty(index(
                        codec::fields::BLOCK_PLACEHOLDER_PROPERTIES.len(),
                    )?),
                    "table_function" => F::TableFunction(
                        ["Average", "Sum", "Count", "Cell"]
                            .into_iter()
                            .find(|f| f.eq_ignore_ascii_case(v))
                            .ok_or_else(|| failure("invalid_value", "Average|Sum|Count|Cell"))?,
                    ),
                    _ => return Err(failure("invalid_value", "Unknown field dialog key")),
                })
            }
            // Sheet Set Manager and its dialogs: "open=<dst>", "tab=list|views|model",
            // "select=<target>", "activate=<target>", "menu=<action>:<target>"
            // (open, new_sheet, new_subset, import, rename, remove, properties,
            // close; a target is a sheet number or title, a subset name or
            // "set"), "import_file=<dwg>", "new" (wizard), "existing=0|1",
            // "step=<n>", "name=", "description=", "folder=", "hierarchy=0|1",
            // "add_folder=<dir>", "layout=<i>:0|1", "wizard_props",
            // "row=<i>:<value>", "prompt=0|1:<row>", "template=<file>",
            // "custom=<i>:<value>", "add_custom", "custom_name=", "custom_default=",
            // "custom_owner=0|1", "add_custom_ok", "number=", "title=",
            // "file_name=", "pick_layout=<i>", "ok", "cancel".
            "sheet_set" => {
                use crate::ui::window::sheet_set::{
                    FieldId as F, MenuAction as A, SheetSetMsg as M, SsmTab, WizardMsg as W,
                };
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let target = |app: &Self, spec: &str| {
                    app.sheet_set_target(spec)
                        .ok_or_else(|| failure("invalid_value", "No such sheet set component"))
                };
                let index_value = |spec: &str| -> Result<(usize, String), Value> {
                    let (i, v) = spec.split_once(':').unwrap_or((spec, ""));
                    Ok((
                        i.parse()
                            .map_err(|_| failure("invalid_value", "Expected <index>:<value>"))?,
                        v.to_string(),
                    ))
                };
                Message::SheetSet(match key {
                    "open" => M::OpenPicked(Some(std::path::PathBuf::from(arg))),
                    "import_file" => M::ImportPicked(Some(std::path::PathBuf::from(arg))),
                    "tab" => M::Tab(match arg {
                        "views" => SsmTab::SheetViews,
                        "model" => SsmTab::ModelViews,
                        _ => SsmTab::SheetList,
                    }),
                    "select" => M::Select(target(self, arg)?),
                    "activate" => M::Activate(target(self, arg)?),
                    "menu" => {
                        let (action, spec) = arg.split_once(':').unwrap_or((arg, "set"));
                        let action = match action {
                            "open" => A::Open,
                            "new_sheet" => A::NewSheet,
                            "new_subset" => A::NewSubset,
                            "import" => A::ImportLayout,
                            "rename" => A::Rename,
                            "remove" => A::Remove,
                            "properties" => A::Properties,
                            "close" => A::Close,
                            _ => {
                                return Err(failure(
                                    "invalid_value",
                                    "Unknown sheet set menu action",
                                ))
                            }
                        };
                        if action == A::ImportLayout {
                            self.sheet_set_import_parent = Some(target(self, spec)?);
                            return Ok(Task::none());
                        }
                        M::Menu(target(self, spec)?, action)
                    }
                    "new" => return Ok(self.dispatch_command("NEWSHEETSET")),
                    "existing" => M::Wizard(W::Existing(arg == "1")),
                    "step" => M::Wizard(W::Step(
                        arg.parse().map_err(|_| failure("invalid_value", "step"))?,
                    )),
                    "layout" => {
                        let (i, v) = index_value(arg)?;
                        M::Wizard(W::LayoutOn(i, v == "1"))
                    }
                    "name" => M::Input(F::Name, arg.into()),
                    "description" => M::Input(F::Description, arg.into()),
                    "folder" => M::Input(F::Folder, arg.into()),
                    "hierarchy" => M::Toggle(F::Hierarchy, arg == "1"),
                    "add_folder" => {
                        M::FolderPicked(F::AddFolder, Some(std::path::PathBuf::from(arg)))
                    }
                    "wizard_props" => M::WizardProperties,
                    "row" => {
                        let (i, v) = index_value(arg)?;
                        M::Input(F::Row(i), v)
                    }
                    "prompt" => {
                        let (i, v) = index_value(arg)?;
                        M::Choose(F::Row(i), usize::from(v != "1"))
                    }
                    "template" => M::TemplatePicked(Some(std::path::PathBuf::from(arg))),
                    "custom" => {
                        let (i, v) = index_value(arg)?;
                        M::CustomValue(i, v)
                    }
                    "add_custom" => M::AddCustom,
                    "custom_name" => M::Input(F::CustomName, arg.into()),
                    "custom_default" => M::Input(F::CustomDefault, arg.into()),
                    "custom_owner" => M::Choose(F::CustomOwner, usize::from(arg == "1")),
                    "add_custom_ok" => M::AddCustomOk,
                    "number" => M::Input(F::Number, arg.into()),
                    "title" => M::Input(F::Title, arg.into()),
                    "file_name" => M::Input(F::FileName, arg.into()),
                    "pick_layout" => M::Choose(
                        F::Layout,
                        arg.parse().map_err(|_| failure("invalid_value", "index"))?,
                    ),
                    "refresh" => M::Refresh,
                    "open_after" => M::Toggle(F::OpenAfter, arg == "1"),
                    "row_toggle" => {
                        let (i, v) = index_value(arg)?;
                        M::Toggle(F::Row(i), v == "1")
                    }
                    "publish" => M::Toggle(F::Publish, arg == "1"),
                    "layout_name" => M::Input(F::LayoutName, arg.into()),
                    "rename_opt" => {
                        let (i, v) = index_value(arg)?;
                        M::Toggle(F::RenameOption(i.min(3)), v == "1")
                    }
                    "import_add" => {
                        M::ImportAdd(arg.split('|').map(std::path::PathBuf::from).collect())
                    }
                    "import_row" => {
                        let (i, v) = index_value(arg)?;
                        M::Toggle(F::ImportRow(i), v == "1")
                    }
                    "prefix" => M::Toggle(F::ImportPrefix, arg == "1"),
                    "previous" => M::Previous,
                    "next" => M::Next,
                    "views_by_category" => M::ViewsByCategory(arg == "1"),
                    "new_category" => M::NewCategory,
                    "category" => {
                        let id = self
                            .sheet_set
                            .db()
                            .and_then(|db| {
                                db.view_categories()
                                    .iter()
                                    .find(|c| c.prop("Name") == Some(arg))
                                    .map(|c| c.id().to_string())
                            })
                            .ok_or_else(|| failure("invalid_value", "No such view category"))?;
                        M::CategoryProperties(id)
                    }
                    "remove_category" => {
                        let id = self
                            .sheet_set
                            .db()
                            .and_then(|db| {
                                db.view_categories()
                                    .iter()
                                    .find(|c| c.prop("Name") == Some(arg))
                                    .map(|c| c.id().to_string())
                            })
                            .ok_or_else(|| failure("invalid_value", "No such view category"))?;
                        M::CategoryRemove(id)
                    }
                    "category_block" => {
                        let (i, v) = index_value(arg)?;
                        M::Toggle(F::CategoryBlock(i), v == "1")
                    }
                    "add_location" => M::LocationPicked(Some(std::path::PathBuf::from(arg))),
                    "remove_location" => {
                        let id = self
                            .sheet_set
                            .locations
                            .iter()
                            .find(|(_, folder, _)| {
                                codec::sheet_set::path_key(folder)
                                    == codec::sheet_set::path_key(arg)
                            })
                            .map(|(id, _, _)| id.clone())
                            .ok_or_else(|| failure("invalid_value", "No such location"))?;
                        M::RemoveLocation(id)
                    }
                    "open_drawing" => M::OpenDrawing(arg.into()),
                    "toggle_drawing" => M::ToggleDrawing(arg.into()),
                    "see_views" => M::SeeViews(arg.into()),
                    // place=<drawing>[|<view>]
                    "place" => {
                        let (drawing, view) = arg
                            .split_once('|')
                            .map_or((arg, None), |(d, v)| (d, Some(v.to_string())));
                        M::PlaceOnSheet(drawing.into(), view)
                    }
                    // view=<view title or number - title>:<display|rename|label|category=<name>|callout=<block>>
                    "view" => {
                        use crate::ui::window::sheet_set::ViewAction as VA;
                        let (spec, action) = arg
                            .rsplit_once(':')
                            .ok_or_else(|| failure("invalid_value", "view=<view>:<action>"))?;
                        let db = self
                            .sheet_set
                            .db()
                            .ok_or_else(|| failure("invalid_state", "No sheet set"))?;
                        let view = db
                            .sheets()
                            .into_iter()
                            .flat_map(|s| db.sheet_views(s))
                            .find(|v| {
                                codec::sheet_set::number_and_title(v) == spec
                                    || v.prop("Title") == Some(spec)
                            })
                            .map(|v| v.id().to_string())
                            .ok_or_else(|| failure("invalid_value", "No such sheet view"))?;
                        let (verb, name) = action.split_once('=').unwrap_or((action, ""));
                        let a = match verb {
                            "display" => VA::Display,
                            "rename" => VA::Rename,
                            "label" => VA::Label,
                            "category" => VA::Category(
                                db.view_categories()
                                    .iter()
                                    .find(|c| c.prop("Name") == Some(name))
                                    .map(|c| c.id().to_string())
                                    .ok_or_else(|| {
                                        failure("invalid_value", "No such view category")
                                    })?,
                            ),
                            "callout" => VA::Callout(
                                db.callout_blocks()
                                    .iter()
                                    .find(|b| b.prop("Name") == Some(name))
                                    .map(|b| b.id().to_string())
                                    .ok_or_else(|| {
                                        failure("invalid_value", "No such callout block")
                                    })?,
                            ),
                            _ => {
                                return Err(failure(
                                    "invalid_value",
                                    "display|rename|label|category=|callout=",
                                ))
                            }
                        };
                        M::ViewMenu(view, a)
                    }
                    "add_blocks" => M::AddBlocks,
                    "block_list_select" => M::BlockListSelect(
                        arg.parse().map_err(|_| failure("invalid_value", "index"))?,
                    ),
                    "block_list_add" => M::BlockListAdd,
                    "block_list_delete" => M::BlockListDelete,
                    "select_block_file" => {
                        M::SelectBlockPicked(Some(std::path::PathBuf::from(arg)))
                    }
                    "select_block_whole" => M::SelectBlockWhole(arg == "1"),
                    "select_block_check" => {
                        let (i, v) = index_value(arg)?;
                        M::SelectBlockCheck(i, v == "1")
                    }
                    "ok" => M::Ok,
                    "cancel" => M::Cancel,
                    _ => return Err(failure("invalid_value", "Unknown sheet set action")),
                })
            }
            "ribbon_dropdown" => Message::ToggleRibbonDropdown(string(req, "value")?.into()),
            // The Password Protected PDF window, one field per call:
            // "password=<text>", "show=0|1", "remember=0|1", "ok", "cancel" or
            // "help".
            "pdf_password_dialog" => {
                use crate::ui::window::pdf_password_dialog::PdfPasswordMsg as P;
                if self.active_modal != Some(crate::app::ModalKind::PdfPassword) {
                    return Err(failure(
                        "no_dialog",
                        "The Password Protected PDF window is not open",
                    ));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let flag = || match value {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(failure("invalid_value", spec)),
                };
                if key == "cancel" {
                    Message::CloseModal
                } else {
                    Message::PdfPassword(match key {
                        "password" => P::Input(value.to_string()),
                        "show" => P::Show(flag()?),
                        "remember" => P::Remember(flag()?),
                        "ok" => P::Ok,
                        "help" => P::Help,
                        _ => {
                            return Err(failure(
                                "invalid_value",
                                "Unknown Password Protected PDF field",
                            ))
                        }
                    })
                }
            }
            // The Check Spelling window, one step per call:
            // "scope=drawing|space|selection", "start", "replacement=<text>",
            // "change", "change_all", "ignore", "ignore_all", "add", "undo",
            // "language=<name>", "settings", "option=<n>,0|1" (0 dimension
            // text … 7 punctuation) or "close".
            "spell_dialog" => {
                use crate::ui::window::spell_dialog::{SpellMsg as S, SpellScope, SpellSetting};
                if self.active_modal != Some(crate::app::ModalKind::Spell) {
                    return Err(failure("no_dialog", "The Check Spelling window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let message = match key {
                    "close" => S::Help,
                    "scope" => S::Scope(match value {
                        "space" => SpellScope::Space,
                        "selection" => SpellScope::Selection,
                        _ => SpellScope::Drawing,
                    }),
                    "start" => S::Start,
                    "replacement" => S::Replacement(value.to_string()),
                    "change" => S::Change,
                    "change_all" => S::ChangeAll,
                    "ignore" => S::Ignore,
                    "ignore_all" => S::IgnoreAll,
                    "add" => S::AddWord,
                    "undo" => S::Undo,
                    "language" => S::Language(value.to_string()),
                    "settings" => S::ToggleSettings,
                    "option" => {
                        let (n, on) = value.split_once(',').unwrap_or((value, "1"));
                        let setting = match n {
                            "0" => SpellSetting::Dimensions,
                            "1" => SpellSetting::Attributes,
                            "2" => SpellSetting::Xrefs,
                            "3" => SpellSetting::IgnoreCapitalized,
                            "4" => SpellSetting::IgnoreMixed,
                            "5" => SpellSetting::IgnoreUpper,
                            "6" => SpellSetting::IgnoreNumbers,
                            _ => SpellSetting::IgnorePunctuation,
                        };
                        S::Setting(setting, on != "0")
                    }
                    _ => return Err(failure("invalid_value", spec)),
                };
                if key == "close" {
                    Message::CloseModal
                } else {
                    Message::Spell(message)
                }
            }
            // The Inspection Dimension window, one field per call:
            // "shape=round|angular|none", "label_on=0|1", "label=<text>",
            // "rate_on=0|1", "rate=<text>", "select", "remove", "ok" or
            // "cancel".
            "dim_inspect_dialog" => {
                use crate::entities::dim_inspect::Shape;
                use crate::ui::window::dim_inspect_dialog::DimInspectMsg as D;
                if self.active_modal != Some(crate::app::ModalKind::DimInspect) {
                    return Err(failure(
                        "no_dialog",
                        "The Inspection Dimension window is not open",
                    ));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let flag = || match value {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(failure("invalid_value", spec)),
                };
                if key == "cancel" {
                    Message::CloseModal
                } else {
                    Message::DimInspect(match key {
                        "shape" => D::Shape(match value {
                            "angular" => Shape::Angular,
                            "none" => Shape::None,
                            "round" => Shape::Round,
                            _ => return Err(failure("invalid_value", spec)),
                        }),
                        "label_on" => D::LabelOn(flag()?),
                        "label" => D::Label(value.to_string()),
                        "rate_on" => D::RateOn(flag()?),
                        "rate" => D::Rate(value.to_string()),
                        "select" => D::Select,
                        "remove" => D::Remove,
                        "ok" => D::Ok,
                        _ => {
                            return Err(failure(
                                "invalid_value",
                                "Unknown Inspection Dimension field",
                            ))
                        }
                    })
                }
            }
            // The Insert / Edit Hyperlink window, one field per call:
            // "address=<file, web address or file#location>", "text=<text>",
            // "target=file|view", "view=<name>", "relative=0|1", "dwf=0|1",
            // "browse", "ok", "remove" or "cancel".
            "hyperlink_dialog" => {
                use crate::ui::window::hyperlink_dialog::HyperlinkMsg as H;
                if self.active_modal != Some(crate::app::ModalKind::Hyperlink) {
                    return Err(failure("no_dialog", "The Hyperlink window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let flag = || match value {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(failure("invalid_value", spec)),
                };
                match key {
                    "address" => Message::HyperlinkUrlChanged(value.to_string()),
                    "text" => Message::HyperlinkDescriptionChanged(value.to_string()),
                    "target" => Message::HyperlinkDialog(H::ViewTarget(value == "view")),
                    "view" => Message::HyperlinkDialog(H::View(value.to_string())),
                    "relative" => Message::HyperlinkDialog(H::Relative(flag()?)),
                    "dwf" => Message::HyperlinkDialog(H::Dwf(flag()?)),
                    "browse" => Message::HyperlinkDialog(H::Browse),
                    "ok" => Message::HyperlinkApply,
                    "remove" => Message::HyperlinkRemove,
                    "cancel" => Message::HyperlinkCancel,
                    _ => {
                        return Err(failure("invalid_value", "Unknown Hyperlink window field"))
                    }
                }
            }
            // The Edit Reference dialog, one field per call: "level=<n>" (0 =
            // the picked reference), "nested=0|1" (pick the working set in the
            // drawing), "attributes=0|1", "lock=0|1", "unique=0|1", "ok" or
            // "cancel".
            "refedit_dialog" => {
                use crate::ui::window::refedit_dialog::RefEditDialogMsg as R;
                if self.active_modal != Some(crate::app::ModalKind::RefEdit) {
                    return Err(failure(
                        "no_dialog",
                        "The Reference Edit dialog is not open",
                    ));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let flag = || match value {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(failure("invalid_value", spec)),
                };
                if key == "cancel" {
                    Message::CloseModal
                } else {
                    Message::RefEditDialog(match key {
                        "ok" => R::Ok,
                        "level" => {
                            R::Level(value.parse().map_err(|_| failure("invalid_value", spec))?)
                        }
                        "nested" => R::Nested(flag()?),
                        "attributes" => R::Attributes(flag()?),
                        "lock" => R::Lock(flag()?),
                        "unique" => R::Unique(flag()?),
                        _ => return Err(failure("invalid_value", "Unknown Reference Edit field")),
                    })
                }
            }
            // Ribbon key tips: `show`, `alt` (a lone Alt), `type:<codes>`, `enter`,
            // `esc` or `hide`. Read them back with the `keytips` read operation.
            "keytips" => {
                use crate::ui::ribbon::KeyTipInput as K;
                let value = string(req, "value")?;
                let inputs: Vec<K> = match value {
                    "show" => vec![K::Open { refocus: false }],
                    "alt" => vec![K::AltTap],
                    "esc" => vec![K::Escape],
                    "hide" => vec![K::Hide],
                    "enter" => vec![K::Enter],
                    _ => match value.strip_prefix("type:") {
                        Some(codes) if !codes.is_empty() => codes
                            .chars()
                            .map(|ch| K::Char(ch.to_uppercase().collect()))
                            .collect(),
                        _ => {
                            return Err(failure(
                                "invalid_value",
                                "Use show, alt, type:<codes>, enter, esc or hide",
                            ))
                        }
                    },
                };
                let tasks: Vec<_> = inputs
                    .into_iter()
                    .map(|input| self.on_keytip(input))
                    .collect();
                return Ok(Task::batch(tasks));
            }
            // The -VIEW Background dialog, one field per call: "type=gradient",
            // "color|top|middle|bottom=r,g,b", "three=1", "rotation=30",
            // "image=<path>", "position=center|stretch|tile", "aspect=1", "offset=x,y" (pixels), "scale=x,y",
            // "step=offset_x|offset_y|scale_x|scale_y,+1|-1", "reset",
            // "apply" or "cancel".
            "view_background" => {
                use crate::ui::window::pdf_dialogs::{BgKind, BgPosition, PdfDialogMsg as P};
                if self.active_modal != Some(crate::app::ModalKind::ViewBackground) {
                    return Err(failure("no_dialog", "The Background dialog is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let rgb = || -> Result<[u8; 3], Value> {
                    let parts: Vec<u8> = value
                        .split(',')
                        .map(|part| part.trim().parse::<u8>())
                        .collect::<Result<_, _>>()
                        .map_err(|_| bad())?;
                    <[u8; 3]>::try_from(parts).map_err(|_| bad())
                };
                match key {
                    "cancel" => Message::CloseModal,
                    "apply" => Message::PdfDialog(P::BgApply),
                    "type" => Message::PdfDialog(P::BgKind(match value {
                        "none" => BgKind::None,
                        "solid" => BgKind::Solid,
                        "gradient" => BgKind::Gradient,
                        "image" => BgKind::Image,
                        _ => return Err(bad()),
                    })),
                    "color" => Message::PdfDialog(P::BgColor(0, rgb()?)),
                    "top" => Message::PdfDialog(P::BgColor(1, rgb()?)),
                    "middle" => Message::PdfDialog(P::BgColor(2, rgb()?)),
                    "bottom" => Message::PdfDialog(P::BgColor(3, rgb()?)),
                    "three" => Message::PdfDialog(P::BgThree(value == "1")),
                    "rotation" => Message::PdfDialog(P::BgRotation(value.to_string())),
                    "image" => Message::PdfDialog(P::BgImage(value.to_string())),
                    "position" => Message::PdfDialog(P::BgPosition(match value {
                        "center" => BgPosition::Center,
                        "stretch" => BgPosition::Stretch,
                        "tile" => BgPosition::Tile,
                        _ => return Err(bad()),
                    })),
                    "aspect" => Message::PdfDialog(P::BgAspect(value == "1")),
                    "offset" | "scale" => {
                        let pair: Vec<String> = value
                            .split(',')
                            .map(|part| part.trim().to_string())
                            .collect();
                        let pair = <[String; 2]>::try_from(pair).map_err(|_| bad())?;
                        if pair
                            .iter()
                            .any(|v| v.parse::<f64>().map_or(true, |v| !v.is_finite()))
                        {
                            return Err(bad());
                        }
                        if key == "offset" {
                            Message::PdfDialog(P::BgOffset(pair))
                        } else {
                            Message::PdfDialog(P::BgScale(pair))
                        }
                    }
                    "step" => {
                        // step=offset_x|offset_y|scale_x|scale_y,+1|-1
                        let (field, dir) = value.split_once(',').ok_or_else(bad)?;
                        let field = match field {
                            "offset_x" => 0,
                            "offset_y" => 1,
                            "scale_x" => 2,
                            "scale_y" => 3,
                            _ => return Err(bad()),
                        };
                        let dir: i8 = dir.trim().parse().map_err(|_| bad())?;
                        Message::PdfDialog(P::BgStep(field, dir))
                    }
                    "reset" => Message::PdfDialog(P::BgReset),
                    _ => return Err(bad()),
                }
            }
            // The file the next PDF import picker returns, without showing
            // it (value: the path).
            "pdf_import_pick" => {
                crate::sys::set_pdf_import_pick(string(req, "value")?.into());
                return Ok(Task::none());
            }
            // The Import PDF window, one field per call: "page=<n>" (1-based),
            // "on_screen=0|1" (insertion point asked after OK), "scale=<value>",
            // "rotation=0|90|180|270", "ok" or "cancel".
            "pdf_import_dialog" => {
                use crate::ui::window::pdf_dialogs::{PdfDialogMsg as P, RotationChoice};
                if self.active_modal != Some(crate::app::ModalKind::PdfImportFile) {
                    return Err(failure("no_dialog", "The Import PDF window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                if key == "cancel" {
                    return Ok(Task::done(Message::CloseModal));
                }
                Message::PdfDialog(match key {
                    "ok" => P::ImportOk,
                    "page" => P::ImportPageText(value.to_string()),
                    "on_screen" => P::ImportInsertOnScreen(value == "1"),
                    "scale" => P::ImportScale(value.to_string()),
                    "rotation" => P::ImportRotation(
                        RotationChoice::ALL
                            .into_iter()
                            .find(|r| value.trim().parse() == Ok(r.0))
                            .ok_or_else(bad)?,
                    ),
                    _ => return Err(bad()),
                })
            }
            // The Import File window, one field per call: "path=<file>",
            // "format=<index or name>", "search=<text>", "filter=all|supported",
            // "recent=<n>", "ok" or "cancel".
            "import_file_dialog" => {
                use crate::ui::window::import_dialogs::{ImportDialogMsg as I, FORMATS};
                if self.active_modal != Some(crate::app::ModalKind::ImportFile) {
                    return Err(failure("no_dialog", "The Import File window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                if key == "cancel" {
                    return Ok(Task::done(Message::CloseModal));
                }
                Message::ImportDialog(match key {
                    "ok" => I::ImportOk,
                    "path" => I::Path(value.to_string()),
                    "search" => I::Search(value.to_string()),
                    "filter" => I::OnlySupported(match value {
                        "all" => false,
                        "supported" => true,
                        _ => return Err(bad()),
                    }),
                    "recent" => I::Recent(value.trim().parse().map_err(|_| bad())?),
                    "format" => I::Format(match value.trim().parse::<usize>() {
                        Ok(k) if k < FORMATS.len() => k,
                        _ => FORMATS
                            .iter()
                            .position(|f| f.name.eq_ignore_ascii_case(value.trim()))
                            .ok_or_else(bad)?,
                    }),
                    _ => return Err(bad()),
                })
            }
            // The 3D Studio Import window, one field per call: "check=<name>"
            // and "uncheck=<name>" (the object's name, or "Global Settings"),
            // "all", "clear", "filter=all|mesh|light|camera|general",
            // "search=<text>", "layers=object|material|color|single",
            // "multi=ask|split|first|none", "path=<file>" (as Change… picks
            // it), "ok" or "cancel".
            "tds_import_dialog" => {
                use crate::ui::window::import_dialogs::ImportDialogMsg as I;
                if self.active_modal != Some(crate::app::ModalKind::TdsImport) {
                    return Err(failure("no_dialog", "The 3D Studio Import window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                if key == "cancel" {
                    return Ok(Task::done(Message::CloseModal));
                }
                let pick = |options: &[&str]| options.iter().position(|o| *o == value).ok_or_else(bad);
                Message::ImportDialog(match key {
                    "ok" => I::TdsOk,
                    "all" => I::TdsAll,
                    "clear" => I::TdsClear,
                    "check" | "uncheck" => {
                        let k = self
                            .import_dialogs
                            .tds
                            .as_ref()
                            .and_then(|s| s.objects.iter().position(|(name, _, _)| name.eq_ignore_ascii_case(value)))
                            .ok_or_else(bad)?;
                        I::TdsCheck(k, key == "check")
                    }
                    "filter" => I::TdsFilter(pick(&["all", "mesh", "light", "camera", "general"])?),
                    "search" => I::TdsSearch(value.to_string()),
                    "layers" => I::TdsLayers(pick(&["object", "material", "color", "single"])?),
                    "multi" => I::TdsMulti(pick(&["ask", "split", "first", "none"])?),
                    "path" => I::TdsPicked(Some(value.to_string())),
                    _ => return Err(bad()),
                })
            }
            // The multiple-material mesh window of a 3D Studio import, one
            // field per call: "split", "first", "select",
            // "material=<name>" ("material=" for no material; selects the
            // choice too), "remember=on|off", "apply" or "cancel".
            "tds_material_dialog" => {
                use crate::ui::window::import_dialogs::ImportDialogMsg as I;
                if self.active_modal != Some(crate::app::ModalKind::TdsMaterial) {
                    return Err(failure("no_dialog", "The multiple-material window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                Message::ImportDialog(match key {
                    "split" => I::TdsMatChoice(0),
                    "first" => I::TdsMatChoice(1),
                    "select" => I::TdsMatChoice(2),
                    "material" => {
                        let k = if value.is_empty() {
                            0
                        } else {
                            self.import_dialogs
                                .tds_material
                                .as_ref()
                                .and_then(|s| s.queue.first())
                                .and_then(|(_, m)| m.iter().position(|(name, _)| name.eq_ignore_ascii_case(value)))
                                .map(|k| k + 1)
                                .ok_or_else(bad)?
                        };
                        I::TdsMatPick(k)
                    }
                    "remember" => I::TdsMatRemember(match value {
                        "on" | "1" | "true" => true,
                        "off" | "0" | "false" => false,
                        _ => return Err(bad()),
                    }),
                    "apply" | "ok" => I::TdsMatApply,
                    "cancel" => return Ok(Task::done(Message::CloseModal)),
                    _ => return Err(bad()),
                })
            }
            // The Data Extraction wizard, one step per call: "next", "back",
            // "finish", "settings=<file.dxex>", "begin=new|template|edit",
            // "source=drawings|selection", "object=<name>:on|off",
            // "property=<key>:on|off", "category=<name>:on|off",
            // "combine|count|name=on|off", "table=on|off", "file=on|off",
            // "path=<file>", "style=<table style>", "title=<text>",
            // "headers=on|off".
            "data_extraction" => {
                use crate::ui::window::annotation_data::{
                    DataExtractionField as F, ExtractionBegin, ExtractionSource,
                };
                if self.active_modal != Some(crate::app::ModalKind::DataExtraction) {
                    return Err(failure("no_dialog", "The Data Extraction wizard is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let on = |v: &str| match v {
                    "on" | "1" | "true" => Ok(true),
                    "off" | "0" | "false" => Ok(false),
                    _ => Err(bad()),
                };
                let named = value.rsplit_once(':');
                match key {
                    "next" => Message::DataExtractionNext,
                    "back" => Message::DataExtractionBack,
                    "finish" => Message::DataExtractionFinish,
                    "settings" => Message::DataExtractionField(F::SettingsPath(value.to_string())),
                    "begin" => Message::DataExtractionField(F::Begin(match value {
                        "new" => ExtractionBegin::New,
                        "template" => ExtractionBegin::Template,
                        "edit" => ExtractionBegin::Edit,
                        _ => return Err(bad()),
                    })),
                    "source" => Message::DataExtractionField(F::Source(match value {
                        "drawings" => ExtractionSource::DrawingsAndFolders,
                        "selection" => ExtractionSource::CurrentSelection,
                        _ => return Err(bad()),
                    })),
                    "object" => {
                        let (name, state) = named.ok_or_else(bad)?;
                        let index = self
                            .data_extraction
                            .objects
                            .iter()
                            .position(|item| item.name.eq_ignore_ascii_case(name))
                            .ok_or_else(bad)?;
                        Message::DataExtractionField(F::Object(index, on(state)?))
                    }
                    "property" => {
                        let (name, state) = named.ok_or_else(bad)?;
                        let index = self
                            .data_extraction
                            .properties
                            .iter()
                            .position(|item| item.key == name || item.name.eq_ignore_ascii_case(name))
                            .ok_or_else(bad)?;
                        Message::DataExtractionField(F::Property(index, on(state)?))
                    }
                    "category" => {
                        let (name, state) = named.ok_or_else(bad)?;
                        Message::DataExtractionField(F::Category(name.to_string(), on(state)?))
                    }
                    "combine" => Message::DataExtractionField(F::CombineIdentical(on(value)?)),
                    "count" => Message::DataExtractionField(F::ShowCount(on(value)?)),
                    "name" => Message::DataExtractionField(F::ShowName(on(value)?)),
                    "table" => Message::DataExtractionField(F::OutputTable(on(value)?)),
                    "file" => Message::DataExtractionField(F::OutputFile(on(value)?)),
                    "path" => Message::DataExtractionField(F::OutputPath(value.to_string())),
                    "style" => Message::DataExtractionField(F::TableStyle(value.to_string())),
                    "title" => Message::DataExtractionField(F::TableTitle(value.to_string())),
                    "headers" => Message::DataExtractionField(F::PropertyHeaders(on(value)?)),
                    _ => return Err(bad()),
                }
            }
            // The Data Link Manager, one step per call: "new", "name=<text>",
            // "path=<file>" (as the file picker sets it), "sheet=<name>",
            // "range=entire|named|cell", "cells=<A1:C4 or name>",
            // "write=on|off", "insert_after=on|off", "save", "cancel_edit",
            // "select=<link name>", "edit", "delete", "insert" or "close".
            "data_link_manager" => {
                use crate::ui::window::annotation_data::{DataLinkField as F, LinkRange};
                if self.active_modal != Some(crate::app::ModalKind::DataLinkManager) {
                    return Err(failure("no_dialog", "The Data Link Manager is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let on = |v: &str| match v {
                    "on" | "1" | "true" => Ok(true),
                    "off" | "0" | "false" => Ok(false),
                    _ => Err(bad()),
                };
                match key {
                    "new" => Message::DataLinkNew,
                    "name" => Message::DataLinkField(F::Name(value.to_string())),
                    "path" => Message::DataLinkBrowseResult(Some(value.into())),
                    "sheet" => Message::DataLinkField(F::Sheet(value.to_string())),
                    "range" => Message::DataLinkField(F::RangeKind(match value {
                        "entire" => LinkRange::EntireSheet,
                        "named" => LinkRange::NamedRange,
                        "cell" => LinkRange::CellRange,
                        _ => return Err(bad()),
                    })),
                    "cells" => Message::DataLinkField(F::Range(value.to_string())),
                    "write" => {
                        self.data_link_manager.more_options = true;
                        Message::DataLinkField(F::AllowWrite(on(value)?))
                    }
                    "insert_after" => Message::DataLinkField(F::InsertTable(on(value)?)),
                    "save" => Message::DataLinkSave,
                    "cancel_edit" => Message::DataLinkEditCancel,
                    "select" => Message::DataLinkSelect(
                        self.data_link_manager
                            .links
                            .iter()
                            .find(|link| link.name.eq_ignore_ascii_case(value))
                            .map(|link| link.handle)
                            .ok_or_else(bad)?,
                    ),
                    "edit" => Message::DataLinkEdit,
                    "delete" => Message::DataLinkDelete,
                    "insert" => Message::DataLinkInsert,
                    "close" => Message::DataLinkClose,
                    _ => return Err(bad()),
                }
            }
            // The missing texture map window of a 3D Studio import: "ok"
            // (or "close") dismisses the shown map; the next one follows.
            // The outdated extraction table window: "update", "update_all",
            // "skip", "skip_all" or "close".
            "extraction_outdated" => {
                use crate::ui::window::extraction_outdated::OutdatedChoice as C;
                if self.active_modal != Some(crate::app::ModalKind::ExtractionOutdated) {
                    return Err(failure(
                        "no_dialog",
                        "The outdated extraction table window is not open",
                    ));
                }
                let spec = string(req, "value")?;
                match spec {
                    "update" => Message::ExtractionOutdated(C::Update),
                    "update_all" => Message::ExtractionOutdated(C::UpdateAll),
                    "skip" => Message::ExtractionOutdated(C::Skip),
                    "skip_all" => Message::ExtractionOutdated(C::SkipAll),
                    "close" => Message::CloseModal,
                    _ => return Err(failure("invalid_value", spec)),
                }
            }
            // The Print All (PUBLISH) window: "open", "pdf=<path>" (the PDF
            // button with its file already picked), "print" or "close". Both
            // outputs run the DXEVAL publish check first.
            "print_all" => {
                let spec = string(req, "value")?;
                let open = self.active_modal == Some(crate::app::ModalKind::PrintAll);
                match spec.split_once('=') {
                    _ if spec == "open" => Message::PrintAllOpen,
                    _ if !open => {
                        return Err(failure("no_dialog", "The Print All window is not open"));
                    }
                    Some(("pdf", path)) if !path.is_empty() => {
                        Message::PrintAllPdfTo(std::path::PathBuf::from(path))
                    }
                    _ if spec == "print" => Message::PrintAllPrint,
                    _ if spec == "close" => Message::CloseModal,
                    _ => return Err(failure("invalid_value", spec)),
                }
            }
            "tds_map_dialog" => {
                if self.active_modal != Some(crate::app::ModalKind::TdsMissingMap) {
                    return Err(failure("no_dialog", "The missing texture map window is not open"));
                }
                let spec = string(req, "value")?;
                match spec {
                    "ok" | "close" => Message::CloseModal,
                    _ => return Err(failure("invalid_value", spec)),
                }
            }
            // The DGN Import window, one field per call: "path=<file>" (as
            // Change… picks it), "model=<n>", "target=new|current",
            // "conflict=prefix|ignore", "units=master|sub", "explode=on|off",
            // "references=translate|ignore|underlay", "existing=prompt|overwrite|keep",
            // "mapping=layer|linetype|lineweight|color", "ok" or "cancel".
            "dgn_import_dialog" => {
                use crate::ui::window::import_dialogs::ImportDialogMsg as I;
                if self.active_modal != Some(crate::app::ModalKind::DgnImport) {
                    return Err(failure("no_dialog", "The DGN Import window is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let on = |v: &str| match v {
                    "on" | "1" | "true" => Ok(true),
                    "off" | "0" | "false" => Ok(false),
                    _ => Err(bad()),
                };
                if key == "cancel" {
                    return Ok(Task::done(Message::CloseModal));
                }
                Message::ImportDialog(match key {
                    "ok" => I::DgnOk,
                    "path" => I::DgnPicked(Some(value.to_string())),
                    "model" => I::DgnModel(value.trim().parse().map_err(|_| bad())?),
                    "target" => I::DgnCurrent(match value {
                        "new" => false,
                        "current" => true,
                        _ => return Err(bad()),
                    }),
                    "conflict" => I::DgnPrefix(match value {
                        "prefix" => true,
                        "ignore" => false,
                        _ => return Err(bad()),
                    }),
                    "units" => I::DgnSub(match value {
                        "master" => false,
                        "sub" => true,
                        _ => return Err(bad()),
                    }),
                    "explode" => I::DgnExplode(on(value)?),
                    "references" => I::DgnReferences(match value {
                        "translate" => crate::modules::insert::dgn_import::ReferenceMode::Translate,
                        "ignore" => crate::modules::insert::dgn_import::ReferenceMode::Ignore,
                        "underlay" => crate::modules::insert::dgn_import::ReferenceMode::Underlay,
                        _ => return Err(bad()),
                    }),
                    "existing" => I::DgnExisting(match value {
                        "prompt" => crate::modules::insert::dgn_import::ExistingReference::Prompt,
                        "overwrite" => crate::modules::insert::dgn_import::ExistingReference::Overwrite,
                        "keep" => crate::modules::insert::dgn_import::ExistingReference::Keep,
                        _ => return Err(bad()),
                    }),
                    "mapping" => I::DgnMappingTab(match value {
                        "layer" => 0,
                        "linetype" => 1,
                        "lineweight" => 2,
                        "color" => 3,
                        _ => return Err(bad()),
                    }),
                    _ => return Err(bad()),
                })
            }
            // The Image Adjust dialog, one field per call: "brightness=70",
            // "contrast=40", "fade=20" (the bar), "type=fade,abc" (typed into
            // the box), "step=brightness,+1|-1", "reset", "apply" or "cancel".
            "image_adjust" => {
                use crate::ui::window::pdf_dialogs::PdfDialogMsg as P;
                if self.active_modal != Some(crate::app::ModalKind::ImageAdjust) {
                    return Err(failure("no_dialog", "The Image Adjust dialog is not open"));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let row = |name: &str| match name.trim() {
                    "brightness" => Ok(0u8),
                    "contrast" => Ok(1),
                    "fade" => Ok(2),
                    _ => Err(bad()),
                };
                match key {
                    "cancel" => Message::CloseModal,
                    "apply" => Message::PdfDialog(P::IaApply),
                    "reset" => Message::PdfDialog(P::IaReset),
                    "brightness" | "contrast" | "fade" => {
                        let v: u8 = value
                            .trim()
                            .parse()
                            .ok()
                            .filter(|v| *v <= 100)
                            .ok_or_else(bad)?;
                        Message::PdfDialog(P::IaValue(row(key)?, v))
                    }
                    "type" => {
                        let (name, typed) = value.split_once(',').ok_or_else(bad)?;
                        Message::PdfDialog(P::IaText(row(name)?, typed.to_string()))
                    }
                    "step" => {
                        let (name, dir) = value.split_once(',').ok_or_else(bad)?;
                        let dir: i8 = dir.trim().parse().map_err(|_| bad())?;
                        Message::PdfDialog(P::IaStep(row(name)?, dir))
                    }
                    _ => return Err(bad()),
                }
            }
            // Text to MText Settings (TXT2MTXT SEttings): "combine=0",
            // "order=select" / "order=top", "wrap=0", "uniform=1", "ok",
            // "cancel".
            "txt2mtxt_settings" => {
                use crate::ui::window::pdf_dialogs::PdfDialogMsg as M;
                if self.active_modal != Some(crate::app::ModalKind::Txt2MtxtSettings) {
                    return Err(failure("no_dialog", "Text to MText Settings is not open"));
                }
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let on = arg != "0";
                match key {
                    "combine" => Message::PdfDialog(M::T2mCombine(on)),
                    "order" => Message::PdfDialog(M::T2mSelectOrder(arg == "select")),
                    "wrap" => Message::PdfDialog(M::T2mWrap(on)),
                    "uniform" => Message::PdfDialog(M::T2mUniform(on)),
                    "ok" => Message::PdfDialog(M::T2mOk),
                    "cancel" => Message::CloseModal,
                    _ => return Err(failure("invalid_value", "Unknown setting")),
                }
            }
            // The PDF Text Recognition Settings dialog, one edit per call:
            // "select=<n>", "toggle=<n>,0|1", "up", "down", "remove",
            // "add=<path to .shx>", "move=<from>,<to>" (dragging a row),
            // "threshold=<1-100>", "threshold_text=<typed>", "step=+1|-1",
            // "layer=current|geometry", "best=0|1", "sample=<text>",
            // "defaults", "apply" or "cancel" (rows count from 0).
            "pdf_shx_settings" => {
                use crate::ui::window::pdf_shx_dialog::ShxDialogMsg as S;
                if self.active_modal != Some(crate::app::ModalKind::PdfShxSettings) {
                    return Err(failure(
                        "no_dialog",
                        "The PDF Text Recognition Settings dialog is not open",
                    ));
                }
                let spec = string(req, "value")?;
                let (key, value) = spec.split_once('=').unwrap_or((spec, ""));
                let bad = || failure("invalid_value", spec);
                let index = |v: &str| v.trim().parse::<usize>().map_err(|_| bad());
                let flag = |v: &str| match v.trim() {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(bad()),
                };
                match key {
                    "cancel" => Message::CloseModal,
                    "move" => {
                        let (from, to) = value.split_once(',').ok_or_else(bad)?;
                        let (from, to) = (index(from)?, index(to)?);
                        return Ok(Task::batch([
                            self.update(Message::PdfShxDialog(S::DragStart(from))),
                            self.update(Message::PdfShxDialog(S::DragDrop(to))),
                        ]));
                    }
                    _ => Message::PdfShxDialog(match key {
                        "apply" => S::Apply,
                        "defaults" => S::Defaults,
                        "up" => S::Up,
                        "down" => S::Down,
                        "remove" => S::Remove,
                        "select" => S::Select(index(value)?),
                        "toggle" => {
                            let (row, on) = value.split_once(',').ok_or_else(bad)?;
                            S::Toggle(index(row)?, flag(on)?)
                        }
                        "add" => S::Added(Some(std::path::PathBuf::from(value.trim()))),
                        "threshold" => S::Threshold(
                            value
                                .trim()
                                .parse::<u8>()
                                .ok()
                                .filter(|v| (1..=100).contains(v))
                                .ok_or_else(bad)?,
                        ),
                        "threshold_text" => S::ThresholdText(value.to_string()),
                        "step" => S::Step(value.trim().parse::<i8>().map_err(|_| bad())?),
                        "layer" => match value.trim() {
                            "current" => S::CurrentLayer(true),
                            "geometry" => S::CurrentLayer(false),
                            _ => return Err(bad()),
                        },
                        "best" => S::BestFont(flag(value)?),
                        "sample" => S::Sample(value.to_string()),
                        _ => return Err(bad()),
                    }),
                }
            }
            "pdf_dialog_ok" => {
                use crate::ui::window::pdf_dialogs::PdfDialogMsg;
                Message::PdfDialog(match self.active_modal {
                    Some(crate::app::ModalKind::PdfAttach) => PdfDialogMsg::AttachOk,
                    Some(crate::app::ModalKind::PointCloudAttach) => PdfDialogMsg::CloudOk,
                    Some(crate::app::ModalKind::PointCloudColorMap) => PdfDialogMsg::MapOk,
                    Some(crate::app::ModalKind::PcSection) => PdfDialogMsg::SecCreate,
                    Some(crate::app::ModalKind::UnderlayLayers) => PdfDialogMsg::LayersOk,
                    Some(crate::app::ModalKind::PdfImportSettings) => PdfDialogMsg::SettingsOk,
                    Some(crate::app::ModalKind::PdfImportFile) => PdfDialogMsg::ImportOk,
                    _ => return Err(failure("no_dialog", "No PDF dialog is open")),
                })
            }
            // The open dialog's OK button.
            "dialog_ok" => match self.active_modal {
                Some(crate::app::ModalKind::Unsaved) => Message::UnsavedDialogSave,
                Some(crate::app::ModalKind::XrefAttach) => {
                    Message::XrefAttach(crate::ui::window::xref_attach::XrefAttachMsg::Apply)
                }
                Some(crate::app::ModalKind::BlockDefinition) => Message::BlockDefApply,
                Some(crate::app::ModalKind::AttDef) => {
                    Message::AttdefDialog(crate::ui::window::attdef_dialog::AttdefDialogMsg::Ok)
                }
                Some(crate::app::ModalKind::AttDefEdit) => {
                    Message::AttdefDialog(crate::ui::window::attdef_dialog::AttdefDialogMsg::EditOk)
                }
                Some(crate::app::ModalKind::SheetSet) => {
                    Message::SheetSet(crate::ui::window::sheet_set::SheetSetMsg::Ok)
                }
                Some(crate::app::ModalKind::Field) => {
                    Message::FieldDialog(crate::ui::window::field_dialog::FieldDialogMsg::Ok)
                }
                Some(crate::app::ModalKind::AttMan) => {
                    Message::AttMan(crate::ui::window::attman_dialog::AttManMsg::Ok)
                }
                _ => return Err(failure("no_dialog", "No dialog with an OK button is open")),
            },
            "dialog_discard" | "dialog_cancel" => match self.active_modal {
                Some(crate::app::ModalKind::Unsaved) => {
                    if name == "dialog_discard" {
                        Message::UnsavedDialogDiscard
                    } else {
                        Message::UnsavedDialogCancel
                    }
                }
                _ => return Err(failure("no_dialog", "No dialog with that button is open")),
            },
            "close_document" => Message::TabClose(self.tabs[self.active_tab].id),
            "toggle_properties" => Message::ToggleProperties,
            "toggle_layers" => Message::ToggleLayers,
            "toggle_grid" => Message::ToggleGrid,
            "toggle_snap" => Message::ToggleSnapEnabled,
            "toggle_ortho" => Message::ToggleOrtho,
            // Choose an entry of the open multi-functional grip menu by its
            // position (0 = first), as a click on it does.
            "grip_menu_pick" => {
                let entries = self
                    .grip_popup
                    .as_ref()
                    .map_or(0, |popup| popup.items.len());
                if entries == 0 {
                    return Err(failure("menu_closed", "No grip menu is open"));
                }
                let index = string(req, "value")?
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < entries)
                    .ok_or_else(|| failure("invalid_value", "value is a menu entry index"))?;
                Message::GripMenuPick(index)
            }
            "mtext_insert" => {
                if self.mtext_editor.is_none() {
                    return Err(failure("editor_closed", "MText editor is closed"));
                }
                Message::MTextInsert(string(req, "value")?.into())
            }
            "mtext_commit" => Message::MTextOk,
            "mtext_cancel" => Message::MTextCancel,
            "text_input" => Message::TextInlineInput(string(req, "value")?.into()),
            "text_commit" => Message::TextInlineOk,
            "pointer_move"
            | "pointer_press"
            | "pointer_release"
            | "pointer_right_press"
            | "pointer_right_release" => {
                let x = req["x"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| failure("invalid_point", "Missing finite x"))?
                    as f32;
                let y = req["y"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| failure("invalid_point", "Missing finite y"))?
                    as f32;
                let size = self.tabs[self.active_tab]
                    .scene
                    .selection
                    .borrow()
                    .view
                    .vp_size;
                if x < 0. || y < 0. || x > size.0 || y > size.1 {
                    return Err(failure(
                        "outside_viewport",
                        "Pointer coordinates are outside viewport_size",
                    ));
                }
                let move_task = self.update(Message::ViewportMove(iced::Point::new(x, y)));
                let event = match name {
                    "pointer_press" => self.update(Message::ViewportLeftPress),
                    "pointer_release" => self.update(Message::ViewportLeftRelease),
                    // Right button: the context menu / Enter behaviour chosen in
                    // Options (see `Message::ViewportRightRelease`).
                    "pointer_right_press" => self.update(Message::ViewportRightPress),
                    "pointer_right_release" => self.update(Message::ViewportRightRelease),
                    _ => Task::none(),
                };
                return Ok(Task::batch([move_task, event]));
            }
            "view_home" => Message::ViewCubeHome,
            "zoom_extents" => return Ok(self.dispatch_command("ZOOM EXTENTS")),
            "undo" => Message::Undo,
            "redo" => Message::Redo,
            "layer_visible" | "layer_locked" | "layer_frozen" | "layer_current" => {
                let layer = string(req, "layer")?.to_string();
                // The Layer* messages carry the layer name and resolve it to
                // the current panel row at dispatch time (#22) — no index
                // lookup here, so panel sort order cannot mistarget the CLI.
                if !self.tabs[self.active_tab]
                    .scene
                    .document
                    .layers
                    .contains(&layer)
                {
                    return Err(failure("unknown_layer", "Layer does not exist"));
                }
                match name {
                    "layer_visible" => Message::LayerToggleVisible(layer),
                    "layer_locked" => Message::LayerToggleLock(layer),
                    "layer_frozen" => Message::LayerToggleFreeze(layer),
                    _ => {
                        let select = self.update(Message::LayerSelect(layer));
                        let current = self.update(Message::LayerSetCurrent);
                        return Ok(Task::batch([select, current]));
                    }
                }
            }
            _ => {
                return Err(failure(
                    "unknown_action",
                    "Read commands.actions for supported action IDs",
                ))
            }
        };
        Ok(self.update(msg))
    }
}
