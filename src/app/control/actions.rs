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
    if let Some((r, g, b)) = color.rgb() {
        json!({"rgb":[r,g,b]})
    } else {
        json!(color.index())
    }
}
/// One Properties-panel row as the control protocol's JSON (`id`, `label`,
/// `kind`, `value`, `options`). The node graph moves values in this form too,
/// so a port's value round-trips through [`OpenCADStudio::set_property_value`].
pub(crate) fn property_json(p: &crate::scene::model::object::Property) -> Value {
    let (kind,value,options)=match &p.value{
                PropValue::ReadOnly(v)|PropValue::ReadOnlyWithTooltip{value:v,..}=>("readonly",json!(v),Value::Null),
                PropValue::EditText(v)=>("number",json!(v),Value::Null),
                PropValue::PlainText(v)=>("text",json!(v),Value::Null),
                PropValue::Hyperlink(v)=>("hyperlink",json!(v),Value::Null),
                PropValue::Choice{selected,options}=>("choice",json!(selected),json!(options)),
                PropValue::EditChoice{value,options}=>("editable_choice",json!(value),json!(options)),
                PropValue::LayerChoice(v)=>("layer",json!(v),Value::Null),
                PropValue::LinetypeChoice(v)=>("linetype",json!(v),Value::Null),
                PropValue::BoolToggle{value,..}=>("bool",json!(value),Value::Null),
                PropValue::ColorChoice(value)|PropValue::NamedColorChoice{color:value,..}=>("color",color_value(*value),Value::Null),
                PropValue::ColorVaries=>("color",Value::Null,Value::Null),
                PropValue::LwChoice(value)|PropValue::FieldLwChoice{value,..}=>("lineweight",json!(value.value()),json!(crate::ui::properties::lw_options().iter().map(|v|v.0.value()).collect::<Vec<_>>())),
                PropValue::LwVaries|PropValue::FieldLwVaries{..}=>("lineweight",Value::Null,json!(crate::ui::properties::lw_options().iter().map(|v|v.0.value()).collect::<Vec<_>>())),
                PropValue::AttrText{tag,value}=>("attribute",json!({"tag":tag,"value":value}),Value::Null),
                other=>("specialized",json!(format!("{other:?}")),Value::Null),
    };json!({"id":p.field,"label":p.label,"kind":kind,"value":value,"options":options})
}
pub(super) const NAMES: &[&str] = &[
    "close_modal",
    "pdf_dialog_ok",
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
    "dialog_ok",
    "attdef_dialog",
    "field_dialog",
    "sheet_set",
    "close_document",
    "toggle_properties",
    "toggle_layers",
    "toggle_grid",
    "toggle_snap",
    "toggle_ortho",
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
            let width = req["width"].as_f64().filter(|w| *w > 0.0).unwrap_or(default_width);
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
                let definition_handle = document.allocate_handle();
                let mut definition = codec::objects::ImageDefinition::with_dimensions(
                    path,
                    image.pixel_width,
                    image.pixel_height,
                );
                definition.handle = definition_handle;
                document.objects.insert(
                    definition_handle,
                    codec::objects::ObjectType::ImageDefinition(definition),
                );
                let mut entity = codec::entities::RasterImage::with_size(
                    path,
                    final_at,
                    image.pixel_width as f64,
                    image.pixel_height as f64,
                    final_width,
                    height,
                );
                entity.definition_handle = Some(definition_handle);
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
                document
                    .add_entity(codec::EntityType::RasterImage(entity))
                    .map_err(|e| failure("embed_failed", e))?
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
                        document,
                        &handles?,
                        &mut base,
                    )?;
                    Ok(base)
                }
                None => crate::modules::insert::wblock::extract_entities_to_doc(document, &handles?),
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
    pub(super) fn control_plot(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        use crate::io::pdf_export::PdfPageInput;

        let path = string(req, "path")?;
        if !path.to_ascii_lowercase().ends_with(".pdf") {
            return Err(failure("invalid_path", "plot writes .pdf files"));
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
                    failure("layout_missing", format!("Layout '{requested}' does not exist"))
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
                let values = req["window"].as_array().filter(|v| v.len() == 4).ok_or_else(|| {
                    failure("window_required", "area \"window\" needs window:[x0,y0,x1,y1]")
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
        let original_psltscale = self.tabs[i].scene.document.header.paper_space_linetype_scaling;
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
                let page =
                    page.ok_or_else(|| format!("Layout '{name}' plot area is empty."))?;
                pages.push(page);
            }
            Ok(pages)
        };
        let built = build(self);
        self.tabs[i].scene.current_layout = original_layout;
        self.tabs[i].scene.active_viewport = original_viewport;
        self.tabs[i].scene.document.header.paper_space_linetype_scaling = original_psltscale;
        self.tabs[i].scene.document.header.paper_space_limit_check = original_plimcheck;
        self.plot_dialog = original_dialog;
        self.plot_window = original_window;
        self.active_plot_style = original_style;
        *self.tabs[i].scene.camera.borrow_mut() = original_camera;
        self.tabs[i].scene.camera_generation = original_camera_generation;
        let pages = built.map_err(|e| failure("plot_failed", e))?;
        if req["per_page"].as_bool().unwrap_or(false) {
            // One PDF per layout: <stem>-<Layout>.pdf beside the requested
            // path (sheet-by-sheet delivery).
            let stem = path.strip_suffix(".pdf").unwrap_or(path).to_owned();
            let mut files = Vec::with_capacity(pages.len());
            for (name, page) in targets.iter().zip(&pages) {
                let safe: String = name
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                    .collect();
                let file = format!("{stem}-{safe}.pdf");
                crate::io::pdf_export::export_pdf_pages(
                    std::slice::from_ref(page),
                    std::path::Path::new(&file),
                    loaded_style.as_ref(),
                )
                .map_err(|e| failure("plot_failed", e))?;
                files.push(json!({"layout": name, "path": file}));
            }
            self.set_control_result(json!({ "files": files, "pages": pages.len() }));
            return Ok(Task::none());
        }
        // Pages may drop the style when a stored page setup overwrites the
        // dialog's style fields, so the explicit request style rides along as
        // the export-level fallback.
        crate::io::pdf_export::export_pdf_pages(
            &pages,
            std::path::Path::new(path),
            loaded_style.as_ref(),
        )
        .map_err(|e| failure("plot_failed", e))?;
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
                })
            })
            .collect::<Vec<_>>();
        json!({"ok": true, "grips": grips})
    }

    /// Drag one grip of an entity to a world point through the viewport's
    /// own grip gesture: select the entity, engage the grip, move, place.
    /// Object snaps, grid, ortho, polar and tracking are off for the move so
    /// the grip lands on `point`.
    pub(super) fn control_grip_drag(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        let target = handle(req)?;
        let grip_id = req["grip_id"]
            .as_u64()
            .ok_or_else(|| failure("invalid_request", "Missing grip_id"))? as usize;
        let to = point(req)?;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(target, false);
        self.refresh_properties();
        let (world, moves_entity) = self.tabs[i]
            .selected_grip_handles
            .iter()
            .zip(&self.tabs[i].selected_grips)
            .find(|(owner, grip)| **owner == target && grip.id == grip_id)
            .map(|(_, grip)| (grip.world, grip.is_midpoint))
            .ok_or_else(|| failure("unknown_grip", "Read grips for the entity's grip ids"))?;
        let edit = self.grip_edit_for_hit(i, target, grip_id, moves_entity, world);
        self.tabs[i].active_grip = Some(edit);

        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let tile = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let local = iced::Rectangle {
            width: tile.width,
            height: tile.height,
            ..iced::Rectangle::default()
        };
        let Some(screen) = self.tabs[i].scene.camera.borrow().project(to, local) else {
            self.cancel_active_grip_edit();
            return Err(failure("invalid_point", "The point is not in view"));
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
    pub(super) fn control_click(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        let at = point(req)?;
        let modifiers = req["modifiers"].as_str().unwrap_or("").to_ascii_lowercase();
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let tile = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
        let local = iced::Rectangle {
            width: tile.width,
            height: tile.height,
            ..iced::Rectangle::default()
        };
        let screen = self.tabs[i]
            .scene
            .camera
            .borrow()
            .project(at, local)
            .ok_or_else(|| failure("invalid_point", "The point is not in view"))?;
        let saved = (self.ctrl_down, self.shift_down);
        self.ctrl_down = modifiers.contains("ctrl");
        self.shift_down = modifiers.contains("shift");
        let cursor = iced::Point::new(screen.x + tile.x, screen.y + tile.y);
        let tasks = [
            self.update(Message::ViewportMove(cursor)),
            self.update(Message::ViewportLeftPress),
            self.update(Message::ViewportLeftRelease),
        ];
        (self.ctrl_down, self.shift_down) = saved;
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
            .find(|p| p.field == field)
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
            PropValue::Choice { options, .. } => {
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
        let msg = match name {
            "close_modal" => Message::CloseModal,
            // The open PDF dialog's OK button.
            // Underlay Layers: switch a layer of the shown underlay.
            "pdf_layer_toggle" => Message::PdfDialog(
                crate::ui::window::pdf_dialogs::PdfDialogMsg::LayersToggle(string(req, "value")?.into()),
            ),
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
                    "count" => M::MapCount(arg.parse().map_err(|_| failure("invalid_value", "count"))?),
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
                    Some((codec::Handle::new(u64::from_str_radix(handle, 16).ok()?), Row::parse(row)?))
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
                        let instances = codec::count::block_instances(&self.tabs[i].scene.document, area.as_deref());
                        let (k, _) = codec::count::expanded_counts(&instances, name, l, s, m).into_iter().nth(n).ok_or_else(bad)?;
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
                        M::Match(k.parse::<usize>().ok().filter(|k| *k < 3).ok_or_else(bad)?, flag(on))
                    }
                    "details" => M::ToggleDetails,
                    "errors" => M::ToggleErrors,
                    "error" => {
                        let i = self.active_tab;
                        let n: usize = arg.parse().map_err(|_| bad())?;
                        let mode = self.tabs[i].count.as_ref().ok_or_else(bad)?;
                        M::ShowError(*mode.result(&self.tabs[i].scene.document).errors.get(n).ok_or_else(bad)?)
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
                use crate::ui::window::block_palette::{BlockPaletteMsg as M, Item, MenuAction, OptionMsg, Tab, ViewMode};
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let item = |app: &Self, spec: &str| -> Result<Item, Value> {
                    let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
                    let index = || rest.parse::<usize>().map_err(|_| failure("invalid_value", "Bad item index"));
                    let missing = || failure("invalid_value", "No such palette item");
                    Ok(match kind {
                        "current" => Item::Current(rest.to_string()),
                        "recent" => Item::Ref(app.block_palette.recent.get(index()?).cloned().ok_or_else(missing)?),
                        "fav" => Item::Ref(app.block_palette.favorites.get(index()?).cloned().ok_or_else(missing)?),
                        "lib" => {
                            let e = app.block_palette.library_entries.get(index()?).ok_or_else(missing)?;
                            if e.folder { Item::Folder(e.path.clone()) } else { Item::File(e.path.clone()) }
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
                    "view" => Message::BlockPalette(M::View(ViewMode::from_u8(arg.parse().unwrap_or(1)))),
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
                        let (spec, at) = arg.rsplit_once('@').ok_or_else(|| failure("invalid_value", "drop needs @x,y"))?;
                        let xy: Vec<f64> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                        if xy.len() < 2 {
                            return Err(failure("invalid_value", "drop needs @x,y"));
                        }
                        let it = item(self, spec)?;
                        let i = self.active_tab;
                        self.tabs[i].last_cursor_world = glam::DVec3::new(xy[0], xy[1], xy.get(2).copied().unwrap_or(0.0));
                        self.block_palette.pressed = Some(it);
                        Message::ViewportLeftRelease
                    }
                    "menu" => {
                        let (action, spec) = arg.split_once(':').ok_or_else(|| failure("invalid_value", "menu needs action:item"))?;
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
                let named_len = self.field_dialog.as_ref().map_or(0, |d| d.named_names.len());
                let known = |list: &[&'static str]| {
                    list.iter().copied().find(|n| n.eq_ignore_ascii_case(v)).ok_or_else(|| failure("invalid_value", "unknown name"))
                };
                Message::FieldDialog(match key {
                    "category" => F::Category(index(fd::CATEGORIES.len())?),
                    "name" => F::Name(known(&fd::fields_of(0))?),
                    "date" => F::DateFormat(v.into()),
                    "case" => F::TextCase(index(fd::TEXT_CASES.len())?),
                    "file" => F::FileParts(v.parse::<u8>().map_err(|_| failure("invalid_value", "1|2|3"))?),
                    "ext" => F::FileExtension(v == "1"),
                    "size" => F::SizeUnit(index(fd::SIZE_UNITS.len())?),
                    "sysvar" => F::SysVar(v.into()),
                    "diesel" => F::Diesel(v.into()),
                    "named_type" => F::NamedType(index(fd::NAMED_TYPES.len())?),
                    "named" => F::Named(index(named_len)?),
                    "prop" => F::ObjectProp(known(&["Area", "Center", "Circumference", "Diameter", "EndPoint", "Length", "Radius", "StartPoint"])?),
                    "select_object" => F::SelectObject,
                    "formula" => F::Formula(v.into()),
                    "formula_format" => F::FormulaFormat(index(codec::fields::FORMULA_FORMATS.len())?),
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
                        let state = self.field_dialog.as_ref().ok_or_else(|| failure("invalid_state", "No Field dialog"))?;
                        let db = state.ss_sets.get(state.ss_set).ok_or_else(|| failure("invalid_state", "No sheet set"))?;
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
                    "placeholder" => F::PlaceholderProperty(index(codec::fields::BLOCK_PLACEHOLDER_PROPERTIES.len())?),
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
                use crate::ui::window::sheet_set::{FieldId as F, MenuAction as A, SheetSetMsg as M, SsmTab, WizardMsg as W};
                let value = string(req, "value")?;
                let (key, arg) = value.split_once('=').unwrap_or((value, ""));
                let target = |app: &Self, spec: &str| {
                    app.sheet_set_target(spec).ok_or_else(|| failure("invalid_value", "No such sheet set component"))
                };
                let index_value = |spec: &str| -> Result<(usize, String), Value> {
                    let (i, v) = spec.split_once(':').unwrap_or((spec, ""));
                    Ok((i.parse().map_err(|_| failure("invalid_value", "Expected <index>:<value>"))?, v.to_string()))
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
                            _ => return Err(failure("invalid_value", "Unknown sheet set menu action")),
                        };
                        if action == A::ImportLayout {
                            self.sheet_set_import_parent = Some(target(self, spec)?);
                            return Ok(Task::none());
                        }
                        M::Menu(target(self, spec)?, action)
                    }
                    "new" => return Ok(self.dispatch_command("NEWSHEETSET")),
                    "existing" => M::Wizard(W::Existing(arg == "1")),
                    "step" => M::Wizard(W::Step(arg.parse().map_err(|_| failure("invalid_value", "step"))?)),
                    "layout" => {
                        let (i, v) = index_value(arg)?;
                        M::Wizard(W::LayoutOn(i, v == "1"))
                    }
                    "name" => M::Input(F::Name, arg.into()),
                    "description" => M::Input(F::Description, arg.into()),
                    "folder" => M::Input(F::Folder, arg.into()),
                    "hierarchy" => M::Toggle(F::Hierarchy, arg == "1"),
                    "add_folder" => M::FolderPicked(F::AddFolder, Some(std::path::PathBuf::from(arg))),
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
                    "pick_layout" => M::Choose(F::Layout, arg.parse().map_err(|_| failure("invalid_value", "index"))?),
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
                    "import_add" => M::ImportAdd(arg.split('|').map(std::path::PathBuf::from).collect()),
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
                            .and_then(|db| db.view_categories().iter().find(|c| c.prop("Name") == Some(arg)).map(|c| c.id().to_string()))
                            .ok_or_else(|| failure("invalid_value", "No such view category"))?;
                        M::CategoryProperties(id)
                    }
                    "remove_category" => {
                        let id = self
                            .sheet_set
                            .db()
                            .and_then(|db| db.view_categories().iter().find(|c| c.prop("Name") == Some(arg)).map(|c| c.id().to_string()))
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
                            .find(|(_, folder, _)| codec::sheet_set::path_key(folder) == codec::sheet_set::path_key(arg))
                            .map(|(id, _, _)| id.clone())
                            .ok_or_else(|| failure("invalid_value", "No such location"))?;
                        M::RemoveLocation(id)
                    }
                    "open_drawing" => M::OpenDrawing(arg.into()),
                    "toggle_drawing" => M::ToggleDrawing(arg.into()),
                    "see_views" => M::SeeViews(arg.into()),
                    // place=<drawing>[|<view>]
                    "place" => {
                        let (drawing, view) = arg.split_once('|').map_or((arg, None), |(d, v)| (d, Some(v.to_string())));
                        M::PlaceOnSheet(drawing.into(), view)
                    }
                    // view=<view title or number - title>:<display|rename|label|category=<name>|callout=<block>>
                    "view" => {
                        use crate::ui::window::sheet_set::ViewAction as VA;
                        let (spec, action) = arg.rsplit_once(':').ok_or_else(|| failure("invalid_value", "view=<view>:<action>"))?;
                        let db = self.sheet_set.db().ok_or_else(|| failure("invalid_state", "No sheet set"))?;
                        let view = db
                            .sheets()
                            .into_iter()
                            .flat_map(|s| db.sheet_views(s))
                            .find(|v| codec::sheet_set::number_and_title(v) == spec || v.prop("Title") == Some(spec))
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
                                    .ok_or_else(|| failure("invalid_value", "No such view category"))?,
                            ),
                            "callout" => VA::Callout(
                                db.callout_blocks()
                                    .iter()
                                    .find(|b| b.prop("Name") == Some(name))
                                    .map(|b| b.id().to_string())
                                    .ok_or_else(|| failure("invalid_value", "No such callout block"))?,
                            ),
                            _ => return Err(failure("invalid_value", "display|rename|label|category=|callout=")),
                        };
                        M::ViewMenu(view, a)
                    }
                    "add_blocks" => M::AddBlocks,
                    "block_list_select" => M::BlockListSelect(arg.parse().map_err(|_| failure("invalid_value", "index"))?),
                    "block_list_add" => M::BlockListAdd,
                    "block_list_delete" => M::BlockListDelete,
                    "select_block_file" => M::SelectBlockPicked(Some(std::path::PathBuf::from(arg))),
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
                Some(crate::app::ModalKind::XrefAttach) => Message::XrefAttach(
                    crate::ui::window::xref_attach::XrefAttachMsg::Apply,
                ),
                Some(crate::app::ModalKind::BlockDefinition) => Message::BlockDefApply,
                Some(crate::app::ModalKind::AttDef) => Message::AttdefDialog(
                    crate::ui::window::attdef_dialog::AttdefDialogMsg::Ok,
                ),
                Some(crate::app::ModalKind::AttDefEdit) => Message::AttdefDialog(
                    crate::ui::window::attdef_dialog::AttdefDialogMsg::EditOk,
                ),
                Some(crate::app::ModalKind::SheetSet) => Message::SheetSet(
                    crate::ui::window::sheet_set::SheetSetMsg::Ok,
                ),
                Some(crate::app::ModalKind::Field) => Message::FieldDialog(
                    crate::ui::window::field_dialog::FieldDialogMsg::Ok,
                ),
                _ => return Err(failure("no_dialog", "No dialog with an OK button is open")),
            },
            "close_document" => Message::TabClose(self.tabs[self.active_tab].id),
            "toggle_properties" => Message::ToggleProperties,
            "toggle_layers" => Message::ToggleLayers,
            "toggle_grid" => Message::ToggleGrid,
            "toggle_snap" => Message::ToggleSnapEnabled,
            "toggle_ortho" => Message::ToggleOrtho,
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
            "pointer_move" | "pointer_press" | "pointer_release" | "pointer_right_press"
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
                let size = self.tabs[self.active_tab].scene.selection.borrow().view.vp_size;
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
