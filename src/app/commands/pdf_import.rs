//! PDFIMPORT: adding the converted PDF content to the drawing as one undo
//! step, with the layers and text styles it uses, and what then happens to
//! the underlay (Keep / Detach / Unload).

use super::*;
use crate::modules::insert::pdf_import::{self, ImportArea, PdfImportRequest, UnderlayMode};

/// What an import reads: a placed underlay, or a file imported at the
/// origin at full size.
pub(crate) enum PdfImportSource {
    Underlay(PdfImportRequest),
    File(pdf_import::PdfFileImport),
}

impl OpenCADStudio {
    pub(crate) fn run_pdf_import(&mut self, i: usize, source: PdfImportSource) {
        use document::objects::ObjectType;
        let document = &self.tabs[i].scene.document;
        let (underlay, path, page, area, mode, handle) = match &source {
            PdfImportSource::Underlay(request) => {
                let Some(document::EntityType::Underlay(underlay)) =
                    document.get_entity(request.underlay)
                else {
                    return;
                };
                let Some(def) = crate::entities::underlay::definition(underlay, document) else {
                    return;
                };
                if def.unloaded {
                    self.command_line
                        .push_error("Cannot bind a PDF underlay that is unloaded.");
                    return;
                }
                (
                    underlay.clone(),
                    def.file_path.clone(),
                    crate::entities::underlay::page_of(def).to_string(),
                    request.area.clone(),
                    Some(request.mode),
                    Some(request.underlay),
                )
            }
            PdfImportSource::File(import) => (
                import.placement(),
                import.path.clone(),
                import.page.clone(),
                ImportArea::All,
                None,
                None,
            ),
        };
        let shown = crate::entities::underlay::display_path(&path);
        match &source {
            PdfImportSource::Underlay(_) => self
                .command_line
                .push_output(&format!("Binding PDF file {shown}, page {page} ...")),
            PdfImportSource::File(_) => {
                self.command_line
                    .push_output(&format!("Importing page {page} of PDF file: {shown}..."));
                if let Some(notice) = crate::scene::model::pdf_password::reopened_notice(&path) {
                    self.command_line.push_output(&notice);
                }
            }
        }
        // The underlay's own layer overrides leave those layers out.
        let hidden = crate::scene::model::pdf_layers::hidden_layers(&underlay);
        let Some(content) =
            crate::scene::model::pdf_vector::page_content_by_layer(&path, &page, &hidden)
        else {
            self.command_line.push_error(&format!("{shown} not found."));
            return;
        };
        let settings = pdf_import::import_settings();
        let current_layer = {
            let header = &self.tabs[i].scene.document.header;
            if header.current_layer_name.is_empty() {
                self.tabs[i].active_layer.clone()
            } else {
                header.current_layer_name.clone()
            }
        };
        let naming = pdf_import::LayerNaming {
            settings,
            prefix: import_prefix(),
            current: current_layer.clone(),
            styles: self.tabs[i]
                .scene
                .document
                .text_styles
                .iter()
                .map(|s| (s.name.clone(), s.font_file.clone()))
                .collect(),
        };
        let mut result = pdf_import::convert(&content, &underlay, &area, &naming);
        let images = std::mem::take(&mut result.images);
        let image_dir = pdf_import::image_dir(&path);

        self.push_undo_snapshot(i, "PDFIMPORT");
        let scene = &mut self.tabs[i].scene;
        if result.dashed
            && !scene
                .document
                .line_types
                .contains(pdf_import::DASH_LINETYPE)
        {
            let mut lt = document::tables::LineType::new(pdf_import::DASH_LINETYPE);
            lt.handle = scene.document.allocate_handle();
            lt.add_element(document::tables::LineTypeElement::dash(0.8));
            lt.add_element(document::tables::LineTypeElement::space(0.2));
            lt.pattern_length = 1.0;
            let _ = scene.document.line_types.add(lt);
        }
        // Raster images: each written as a PNG in the PDFIMPORTIMAGEPATH
        // folder, named after the PDF with 8 hex digits, then referenced.
        let stem = std::path::Path::new(&path.replace('\\', "/"))
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PDF".to_string());
        for (n, image) in images.iter().enumerate() {
            let Some(dir) = &image_dir else { break };
            if std::fs::create_dir_all(dir).is_err() {
                break;
            }
            let file = dir.join(format!("{stem}{:08x}.png", image_file_id(&path, &page, n)));
            if image::save_buffer(
                &file,
                &image.rgba,
                image.width,
                image.height,
                image::ColorType::Rgba8,
            )
            .is_err()
            {
                continue;
            }
            // Rebuilt from its components: one separator style, the platform's.
            let stored = file
                .components()
                .collect::<std::path::PathBuf>()
                .to_string_lossy()
                .into_owned();
            let mut raster = document::entities::RasterImage::with_size(
                &stored,
                image.insertion,
                image.width as f64,
                image.height as f64,
                1.0,
                1.0,
            );
            raster.u_vector = image.u;
            raster.v_vector = image.v;
            raster.flags = document::entities::ImageDisplayFlags::SHOW_IMAGE
                | document::entities::ImageDisplayFlags::SHOW_NOT_ALIGNED
                | document::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY
                | document::entities::ImageDisplayFlags::TRANSPARENCY_ON;
            raster.common.layer = image.layer.clone();
            result
                .entities
                .push(document::EntityType::RasterImage(raster));
        }
        for (name, color) in &result.layers {
            if !scene.document.layers.contains(name) {
                let mut layer = document::tables::Layer::new(name.as_str());
                layer.handle = scene.document.allocate_handle();
                layer.color = *color;
                let _ = scene.document.layers.add(layer);
            }
        }
        for (name, font) in &result.text_styles {
            if !scene.document.text_styles.contains(name) {
                let mut style = document::tables::TextStyle::new(name.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file = font.clone();
                let _ = scene.document.text_styles.add(style);
            }
        }
        if settings.as_block && !result.entities.is_empty() {
            // One block named after the file, inserted at the origin on the
            // current layer.
            let mut name = stem.clone();
            let mut n = 1;
            while scene.document.block_records.get(&name).is_some() {
                name = format!("{stem}{n}");
                n += 1;
            }
            if scene
                .define_block_from_owned_entities(result.entities, &name, glam::DVec3::ZERO)
                .is_ok()
            {
                let mut insert = document::entities::Insert::new(
                    name,
                    document::types::Vector3::new(0.0, 0.0, 0.0),
                );
                insert.common.layer = current_layer;
                scene.add_entity(document::EntityType::Insert(insert));
            }
        } else {
            for entity in result.entities {
                scene.add_entity(entity);
            }
        }
        if let (Some(mode), Some(handle)) = (mode, handle) {
            match mode {
                UnderlayMode::Keep => {}
                UnderlayMode::Unload => {
                    if let Some(ObjectType::UnderlayDefinition(definition)) =
                        scene.document.objects.get_mut(&underlay.definition_handle)
                    {
                        definition.unloaded = true;
                    }
                    scene.reseed_underlays();
                }
                UnderlayMode::Detach => {
                    scene.erase_entities(&[handle]);
                    // Drop the definition once nothing references it.
                    let definition = underlay.definition_handle;
                    let referenced = scene.document.entities().any(|entity| {
                        matches!(entity, document::EntityType::Underlay(other)
                            if other.definition_handle == definition)
                    });
                    if !referenced {
                        scene.document.objects.remove(&definition);
                        for object in scene.document.objects.values_mut() {
                            if let ObjectType::Dictionary(dictionary) = object {
                                dictionary
                                    .entries
                                    .retain(|(_, target)| *target != definition);
                            }
                        }
                    }
                }
            }
        }
        self.tabs[i].dirty = true;
        self.refresh_layer_panel();
        self.refresh_properties();
    }
}

impl OpenCADStudio {
    /// PDFATTACH's end: every page's definition (created or reused, with the
    /// definitions dictionary) and underlay in one undo step.
    pub(crate) fn attach_pdf_pages(
        &mut self,
        i: usize,
        label: String,
        kind: document::entities::UnderlayType,
        path: &str,
        pages: Vec<(String, document::EntityType)>,
        auto_place: bool,
    ) {
        // Resolve every page before opening the existing single transaction.
        // A bad or unsupported page retains the values built by PDFATTACH;
        // it never prevents a sibling page from being placed geospatially.
        let placements = auto_pdf_placements(&self.tabs[i].scene, kind, path, &pages, auto_place);
        let pending = self.begin_undo(i, label, pages.len(), false);

        for (index, (page, mut entity)) in pages.into_iter().enumerate() {
            let definition = crate::modules::insert::pdf_attach::ensure_underlay_definition(
                &mut self.tabs[i].scene.document,
                kind,
                path,
                &page,
            );
            if let document::EntityType::Underlay(underlay) = &mut entity {
                underlay.definition_handle = definition;
                if let Some(Some(placement)) = placements.get(index) {
                    apply_geo_placement(underlay, *placement);
                }
            }
            self.commit_entity_handle(entity);
        }
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }
}

fn apply_geo_placement(
    underlay: &mut document::entities::Underlay,
    placement: crate::io::geopdf::GeoPlacement,
) {
    underlay.insertion_point.x = placement.insertion[0];
    underlay.insertion_point.y = placement.insertion[1];
    underlay.x_scale = placement.x_scale;
    underlay.y_scale = placement.y_scale;
    underlay.rotation = placement.rotation;
}

/// Build final underlays without manual placement prompts only when every
/// selected PDF page has an unambiguous transform in the current host context.
pub(super) fn fully_georeferenced_pdf_pages(
    scene: &crate::scene::Scene,
    kind: document::entities::UnderlayType,
    path: &str,
    page_names: &[String],
    plane: crate::command::WorkingPlane,
) -> Option<Vec<(String, document::EntityType)>> {
    if kind != document::entities::UnderlayType::Pdf || page_names.is_empty() {
        return None;
    }
    let mut pages = crate::modules::insert::pdf_attach::underlays_for_pages(
        kind,
        page_names,
        glam::DVec3::ZERO,
        1.0,
        0.0,
    )
    .into_iter()
    .map(|(page, entity)| (page, plane.place_entity(entity)))
    .collect::<Vec<_>>();
    let placements = auto_pdf_placements(scene, kind, path, &pages, true);
    if placements.iter().any(Option::is_none) {
        return None;
    }
    for ((_, entity), placement) in pages.iter_mut().zip(placements) {
        if let document::EntityType::Underlay(underlay) = entity {
            apply_geo_placement(underlay, placement.expect("checked above"));
        }
    }
    Some(pages)
}

fn auto_pdf_placements(
    scene: &crate::scene::Scene,
    kind: document::entities::UnderlayType,
    path: &str,
    pages: &[(String, document::EntityType)],
    enabled: bool,
) -> Vec<Option<crate::io::geopdf::GeoPlacement>> {
    let context = (enabled
        && kind == document::entities::UnderlayType::Pdf
        && scene.current_layout == "Model"
        && scene.block_edit_block.is_none())
    .then(|| {
        let crs = crate::io::crs::document_crs(&scene.document).ok()??;
        // Drawing XY for a geographic CRS are already longitude/latitude.
        // Only a projected CRS needs INSUNITS to scale source map metres.
        let unit_mm = if crs.is_geographic() {
            1.0
        } else {
            crate::app::properties::insunits_to_mm(scene.document.header.insertion_units)?
        };
        Some((crs, unit_mm))
    })
    .flatten();
    pages
        .iter()
        .map(|(page, entity)| {
            let (crs, unit_mm) = context.as_ref()?;
            let document::EntityType::Underlay(underlay) = entity else {
                return None;
            };
            let normal = glam::DVec3::new(underlay.normal.x, underlay.normal.y, underlay.normal.z);
            if normal.length_squared() <= f64::EPSILON
                || !normal.normalize().abs_diff_eq(glam::DVec3::Z, 1e-10)
            {
                return None;
            }
            crate::io::geopdf::placement_for_source(path, page, crs, *unit_mm).ok()
        })
        .collect()
}

/// The layer prefix of an import: "PDF_" for the first import of the
/// session, "PDF2_", "PDF3_" … for each later one, of the same file or not.
// ponytail: counted for the session only; a reopened drawing starts from
// "PDF_" again.
fn import_prefix() -> String {
    static IMPORTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    match IMPORTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) {
        0 => "PDF_".to_string(),
        n => format!("PDF{}_", n + 1),
    }
}

/// A file name suffix for an imported image, different per import.
fn image_file_id(path: &str, page: &str, n: usize) -> u32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, page, n, std::time::SystemTime::now()).hash(&mut h);
    h.finish() as u32
}

#[cfg(test)]
mod geo_attach_tests {
    use super::*;
    use document::entities::{Underlay, UnderlayType};
    use glam::DVec3;
    use std::sync::Arc;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab].dirty = false;
        app
    }

    fn two_page_pdf() -> Vec<u8> {
        let georeferenced = "/LGIDict << /Type /LGIDict /Projection << /EPSG 5514 >> /CTM [2 0 0 3 -742925.2713891672 -1043020.3101239335] >>";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] {georeferenced} >>"),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_string(),
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend(b"0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    fn set_crs(app: &mut OpenCADStudio) {
        crate::io::crs::set_document_crs(
            &mut app.tabs[app.active_tab].scene.document,
            &crate::io::crs::CrsDefinition::from_epsg(5514).unwrap(),
        )
        .unwrap();
        app.tabs[app.active_tab]
            .scene
            .document
            .header
            .insertion_units = 6;
    }

    fn pages(insertion: DVec3) -> Vec<(String, document::EntityType)> {
        crate::modules::insert::pdf_attach::underlays_for_pages(
            UnderlayType::Pdf,
            &["1".to_string(), "2".to_string()],
            insertion,
            4.0,
            0.25,
        )
    }

    fn attached(app: &OpenCADStudio) -> Vec<(String, Underlay)> {
        let document = &app.tabs[app.active_tab].scene.document;
        document
            .entities()
            .filter_map(|entity| {
                let document::EntityType::Underlay(underlay) = entity else {
                    return None;
                };
                let page = crate::entities::underlay::definition(underlay, document)
                    .map(|definition| definition.page_name.clone())?;
                Some((page, underlay.clone()))
            })
            .collect()
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn auto_places_each_eligible_page_inside_one_undo_step() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "interactive-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let before = app.tabs[i].history.undo_stack.len();
        app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::new(10.0, 20.0, 0.0)),
            true,
        );
        app.finish_all_pending_history();
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);

        let placed = attached(&app);
        assert_eq!(placed.len(), 2);
        close(placed[0].1.insertion_point.x, -742_925.271_389_167_2);
        close(placed[0].1.insertion_point.y, -1_043_020.310_123_933_5);
        close(placed[0].1.x_scale, 144.0);
        close(placed[0].1.y_scale, 216.0);
        close(placed[0].1.rotation, 0.0);
        assert_eq!(placed[1].1.insertion_point.x, 10.0);
        assert_eq!(placed[1].1.insertion_point.y, 20.0);
        assert_eq!(placed[1].1.x_scale, 4.0);
        assert_eq!(placed[1].1.y_scale, 4.0);
        assert_eq!(placed[1].1.rotation, 0.25);

        app.undo_steps(1);
        assert!(attached(&app).is_empty());
        assert!(crate::io::crs::document_crs(&app.tabs[i].scene.document)
            .unwrap()
            .is_some());
        app.redo_steps(1);
        let restored = attached(&app);
        assert_eq!(restored.len(), 2);
        close(restored[0].1.insertion_point.x, -742_925.271_389_167_2);
        assert_eq!(restored[1].1.insertion_point.x, 10.0);
    }

    #[test]
    fn dialog_geopdf_skips_placement_prompts_and_uses_georeferencing() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let selected = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(document::entities::Line::new()));
        app.tabs[i].scene.selected.insert(selected);
        let path = "/virtual/dialog-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let _ = app.open_pdf_attach_dialog(path);
        let state = app.pdf_attach.as_mut().expect("attach dialog");
        state.path_type =
            crate::ui::window::pdf_dialogs::PathTypeChoice(crate::io::xref_model::Pathtype::None);
        state.insert_on_screen = true;
        state.scale_on_screen = true;
        state.rotation_on_screen = true;
        assert!(state.geopdf_available);
        assert!(state.geopdf_auto);

        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachOk,
        ));

        assert!(app.tabs[i].active_cmd.is_none());
        assert!(app.pdf_attach.is_none());
        assert!(app.tabs[i].scene.selected.is_empty());
        let placed = attached(&app);
        assert_eq!(placed.len(), 1);
        close(placed[0].1.insertion_point.x, -742_925.271_389_167_2);
        close(placed[0].1.insertion_point.y, -1_043_020.310_123_933_5);
        close(placed[0].1.x_scale, 144.0);
        close(placed[0].1.y_scale, 216.0);
        close(placed[0].1.rotation, 0.0);
        let definition =
            crate::entities::underlay::definition(&placed[0].1, &app.tabs[i].scene.document)
                .expect("attached definition");
        assert_eq!(definition.file_path, "dialog-geopdf.pdf");
    }

    #[test]
    fn dialog_geopdf_can_be_unchecked_for_manual_placement() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "dialog-manual-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let _ = app.open_pdf_attach_dialog(path);
        assert!(app.pdf_attach.as_ref().unwrap().geopdf_auto);

        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachGeoPdfAuto(false),
        ));
        let state = app.pdf_attach.as_mut().expect("attach dialog");
        assert!(!state.geopdf_auto);
        state.path_type =
            crate::ui::window::pdf_dialogs::PathTypeChoice(crate::io::xref_model::Pathtype::Full);
        state.insert_on_screen = false;
        state.insert = ["10".into(), "20".into(), "30".into()];
        state.scale_on_screen = false;
        state.scale = "4".into();
        state.rotation_on_screen = false;
        state.rotation = "0".into();

        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachOk,
        ));

        assert!(app.tabs[i].active_cmd.is_none());
        let placed = attached(&app);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].1.insertion_point.x, 10.0);
        assert_eq!(placed[0].1.insertion_point.y, 20.0);
        assert_eq!(placed[0].1.insertion_point.z, 30.0);
        assert_eq!(placed[0].1.x_scale, 4.0);
        assert_eq!(placed[0].1.y_scale, 4.0);
        assert_eq!(placed[0].1.rotation, 0.0);
    }

    #[test]
    fn dialog_mixed_geopdf_selection_keeps_manual_fallback_prompt() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "dialog-mixed-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let _ = app.open_pdf_attach_dialog(path);
        app.ctrl_down = true;
        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachPage(1),
        ));
        app.ctrl_down = false;
        let state = app.pdf_attach.as_mut().expect("attach dialog");
        assert_eq!(state.selected, vec![0, 1]);
        assert!(!state.geopdf_available);
        assert!(!state.geopdf_auto);
        state.insert_on_screen = true;
        state.scale_on_screen = false;
        state.rotation_on_screen = false;
        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachGeoPdfAuto(true),
        ));
        assert!(!app.pdf_attach.as_ref().unwrap().geopdf_auto);

        let _ = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachOk,
        ));

        assert!(attached(&app).is_empty());
        let command = app.tabs[i].active_cmd.as_ref().expect("manual fallback");
        assert_eq!(command.prompt(), "Specify insertion point:");
    }

    fn unit_geographic_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /VP << /Type /Viewport /BBox [0 0 100 50] /Measure << /Type /Measure /Subtype /GEO /GCS << /Type /GEOGCS /EPSG 4326 >> /LPTS [0 0 1 0 0 1 1 1] /GPTS [10 20 10 21 11 20 11 21] >> >> >>".to_string(),
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend(b"0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    #[test]
    fn unitless_geographic_crs_still_auto_places() {
        let mut app = app();
        let i = app.active_tab;
        crate::io::crs::set_document_crs(
            &mut app.tabs[i].scene.document,
            &crate::io::crs::CrsDefinition::from_epsg(4326).unwrap(),
        )
        .unwrap();
        let path = "unitless-geographic-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(unit_geographic_pdf()));
        let sample = pages(DVec3::new(10.0, 20.0, 0.0))
            .into_iter()
            .take(1)
            .collect::<Vec<_>>();
        let placements =
            auto_pdf_placements(&app.tabs[i].scene, UnderlayType::Pdf, path, &sample, true);
        assert_eq!(placements.len(), sample.len());
        assert!(placements.iter().all(Option::is_some), "{placements:?}");
        let placement = placements[0].expect("geographic placement");
        assert!((placement.rotation - 0.0).abs() < 1e-8);
        assert!(placement.insertion[0].is_finite() && placement.insertion[1].is_finite());
        assert!(placement.x_scale > 0.0 && placement.y_scale > 0.0);
    }

    #[test]
    fn manual_mode_and_ineligible_hosts_keep_authored_values() {
        let mut app = app();
        let i = app.active_tab;
        let path = "manual-geopdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let sample = pages(DVec3::new(10.0, 20.0, 30.0));
        assert!(
            auto_pdf_placements(&app.tabs[i].scene, UnderlayType::Pdf, path, &sample, true,)
                .iter()
                .all(Option::is_none)
        );
        set_crs(&mut app);
        let authored = sample.clone();
        app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            authored.clone(),
            false,
        );
        for ((_, actual), (_, expected)) in attached(&app).into_iter().zip(authored.iter()) {
            let document::EntityType::Underlay(expected) = expected else {
                unreachable!()
            };
            assert_eq!(actual.insertion_point, expected.insertion_point);
            assert_eq!(actual.x_scale, expected.x_scale);
            assert_eq!(actual.y_scale, expected.y_scale);
            assert_eq!(actual.rotation, expected.rotation);
        }

        let scene = &mut app.tabs[i].scene;
        scene.document.header.insertion_units = 0;
        assert!(
            auto_pdf_placements(scene, UnderlayType::Pdf, path, &sample, true)
                .iter()
                .all(Option::is_none)
        );
        scene.document.header.insertion_units = 6;
        scene.current_layout = "Layout1".to_string();
        assert!(
            auto_pdf_placements(scene, UnderlayType::Pdf, path, &sample, true)
                .iter()
                .all(Option::is_none)
        );
        scene.current_layout = "Model".to_string();
        let block = scene.model_space_block_handle();
        scene.block_edit_block = Some(block);
        assert!(
            auto_pdf_placements(scene, UnderlayType::Pdf, path, &sample, true)
                .iter()
                .all(Option::is_none)
        );
        scene.block_edit_block = None;
        let mut tilted = sample;
        let document::EntityType::Underlay(underlay) = &mut tilted[0].1 else {
            unreachable!()
        };
        underlay.normal = document::types::Vector3::new(0.0, 1.0, 1.0);
        let placements = auto_pdf_placements(scene, UnderlayType::Pdf, path, &tilted, true);
        assert!(placements[0].is_none());
        assert!(placements[1].is_none());
        let tilted_plane =
            crate::command::WorkingPlane::new(DVec3::ZERO, DVec3::X, DVec3::new(0.0, 1.0, 1.0));
        assert!(fully_georeferenced_pdf_pages(
            scene,
            UnderlayType::Pdf,
            path,
            &["1".to_string()],
            tilted_plane,
        )
        .is_none());
    }
}
