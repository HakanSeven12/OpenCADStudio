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
        use codec::objects::ObjectType;
        let document = &self.tabs[i].scene.document;
        let (underlay, path, page, area, mode, handle) = match &source {
            PdfImportSource::Underlay(request) => {
                let Some(codec::EntityType::Underlay(underlay)) =
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
            PdfImportSource::File(_) => self
                .command_line
                .push_output(&format!("Importing page {page} of PDF file: {shown}...")),
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
        };
        let mut result = pdf_import::convert(&content, &underlay, &area, &naming);
        let images = std::mem::take(&mut result.images);
        let image_dir = pdf_import::image_dir(&path);

        self.push_undo_snapshot(i, "PDFIMPORT");
        let scene = &mut self.tabs[i].scene;
        if result.dashed && !scene.document.line_types.contains(pdf_import::DASH_LINETYPE) {
            let mut lt = codec::tables::LineType::new(pdf_import::DASH_LINETYPE);
            lt.handle = scene.document.allocate_handle();
            lt.add_element(codec::tables::LineTypeElement::dash(0.8));
            lt.add_element(codec::tables::LineTypeElement::space(0.2));
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
            if image::save_buffer(&file, &image.rgba, image.width, image.height, image::ColorType::Rgba8)
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
            let mut raster = codec::entities::RasterImage::with_size(
                &stored,
                image.insertion,
                image.width as f64,
                image.height as f64,
                1.0,
                1.0,
            );
            raster.u_vector = image.u;
            raster.v_vector = image.v;
            raster.flags = codec::entities::ImageDisplayFlags::SHOW_IMAGE
                | codec::entities::ImageDisplayFlags::SHOW_NOT_ALIGNED
                | codec::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY
                | codec::entities::ImageDisplayFlags::TRANSPARENCY_ON;
            raster.common.layer = image.layer.clone();
            result.entities.push(codec::EntityType::RasterImage(raster));
        }
        for (name, color) in &result.layers {
            if !scene.document.layers.contains(name) {
                let mut layer = codec::tables::Layer::new(name.as_str());
                layer.handle = scene.document.allocate_handle();
                layer.color = *color;
                let _ = scene.document.layers.add(layer);
            }
        }
        for (name, font) in &result.text_styles {
            if !scene.document.text_styles.contains(name) {
                let mut style = codec::tables::TextStyle::new(name.as_str());
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
                let mut insert = codec::entities::Insert::new(
                    name,
                    codec::types::Vector3::new(0.0, 0.0, 0.0),
                );
                insert.common.layer = current_layer;
                scene.add_entity(codec::EntityType::Insert(insert));
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
                        matches!(entity, codec::EntityType::Underlay(other)
                            if other.definition_handle == definition)
                    });
                    if !referenced {
                        scene.document.objects.remove(&definition);
                        for object in scene.document.objects.values_mut() {
                            if let ObjectType::Dictionary(dictionary) = object {
                                dictionary.entries.retain(|(_, target)| *target != definition);
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
        path: &str,
        pages: Vec<(String, codec::EntityType)>,
    ) {
        let pending = self.begin_undo(i, label, pages.len(), false);

        for (page, mut entity) in pages {
            let definition = crate::modules::insert::pdf_attach::ensure_pdf_definition(
                &mut self.tabs[i].scene.document,
                path,
                &page,
            );
            if let codec::EntityType::Underlay(underlay) = &mut entity {
                underlay.definition_handle = definition;
            }
            self.commit_entity_handle(entity);
        }
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }
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
