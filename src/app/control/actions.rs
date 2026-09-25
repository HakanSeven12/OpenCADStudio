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
    "pdf_page_select",
    "ribbon_tab",
    "ribbon_dropdown",
    "dialog_ok",
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
impl OpenCADStudio {
    /// `embed_image` — pack the picture at `path` into an OLE2FRAME placed
    /// with its lower-left corner at `at` (default width ≈ pixel_width/100,
    /// like the interactive command's Enter answer) and commit it as one
    /// undo step. The drawing stays self-contained: no external file to lose.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_embed_image(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let i = self.active_tab;
        let path = string(req, "path")?;
        let image = crate::io::ole_embed::EmbeddedImage::from_file(std::path::Path::new(&path))
            .map_err(|e| failure("embed_failed", e))?;
        let at = embed_point(req)?;
        let default_width = (image.pixel_width as f64 / 100.0).max(1.0);
        let width = req["width"].as_f64().filter(|w| *w > 0.0).unwrap_or(default_width);
        if req["linked"].as_bool().unwrap_or(false) {
            // Path-linked RasterImage + ImageDefinition: the drawing stores
            // only the file path, so the picture file must travel with the
            // drawing (unlike the embedded OLE2FRAME default below).
            self.push_undo_snapshot(i, "IMAGEATTACH");
            let height = width * image.pixel_height as f64 / image.pixel_width as f64;
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
                    at,
                    image.pixel_width as f64,
                    image.pixel_height as f64,
                    width,
                    height,
                );
                entity.definition_handle = Some(definition_handle);
                document
                    .add_entity(codec::EntityType::RasterImage(entity))
                    .map_err(|e| failure("embed_failed", e))?
            };
            self.tabs[i].scene.populate_images_from_document();
            self.post_ref_op(i);
            self.set_control_result(json!({
                "handle": format!("{:X}", handle.value()),
                "kind": "RasterImage",
                "path": path,
                "width": width,
                "height": height,
                "linked": true,
            }));
            return Ok(Task::none());
        }
        self.push_undo_snapshot(i, "IMAGEEMBED");
        let handle = crate::io::ole_embed::add_embedded_image(
            &mut self.tabs[i].scene.document,
            &image,
            at,
            width,
        )
        .map_err(|e| failure("embed_failed", e))?;
        self.post_ref_op(i);
        self.set_control_result(json!({
            "handle": format!("{:X}", handle.value()),
            "kind": "Ole2Frame",
            "path": path,
            "width": width,
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
            // Bring a ribbon tab forward by module id ("pdf_underlay" for the
            // contextual underlay tab).
            "ribbon_tab" => {
                let id = string(req, "value")?;
                if !self.ribbon.select_by_id(id) {
                    return Err(failure("no_tab", "No such ribbon tab"));
                }
                return Ok(Task::none());
            }
            "ribbon_dropdown" => Message::ToggleRibbonDropdown(string(req, "value")?.into()),
            "pdf_dialog_ok" => {
                use crate::ui::window::pdf_dialogs::PdfDialogMsg;
                Message::PdfDialog(match self.active_modal {
                    Some(crate::app::ModalKind::PdfAttach) => PdfDialogMsg::AttachOk,
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
                _ => return Err(failure("no_dialog", "No dialog with an OK button is open")),
            },
            "close_document" => Message::TabClose(self.active_tab),
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
                let size = self.tabs[self.active_tab].scene.selection.borrow().vp_size;
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
                let layer = string(req, "layer")?;
                let index = self.tabs[self.active_tab]
                    .scene
                    .document
                    .layers
                    .iter()
                    .position(|l| l.name == layer)
                    .ok_or_else(|| failure("unknown_layer", "Layer does not exist"))?;
                match name {
                    "layer_visible" => Message::LayerToggleVisible(index),
                    "layer_locked" => Message::LayerToggleLock(index),
                    "layer_frozen" => Message::LayerToggleFreeze(index),
                    _ => {
                        let select = self.update(Message::LayerSelect(index));
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
