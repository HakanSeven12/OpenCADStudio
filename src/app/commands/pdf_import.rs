//! PDFIMPORT: adding the converted PDF content to the drawing as one undo
//! step, with the layers and text styles it uses, and what then happens to
//! the underlay (Keep / Detach / Unload).

use super::*;
use crate::modules::insert::pdf_import::{self, ImportArea, PdfImportRequest, UnderlayMode};

/// What an import reads: a placed underlay, or a file imported at the
/// origin at full size.
pub(crate) enum PdfImportSource {
    Underlay(PdfImportRequest),
    File(String),
}

impl OpenCADStudio {
    pub(crate) fn run_pdf_import(&mut self, i: usize, source: PdfImportSource) {
        use acadrust::objects::ObjectType;
        let document = &self.tabs[i].scene.document;
        let (underlay, path, page, area, mode, handle) = match &source {
            PdfImportSource::Underlay(request) => {
                let Some(acadrust::EntityType::Underlay(underlay)) =
                    document.get_entity(request.underlay)
                else {
                    return;
                };
                let Some(def) = crate::entities::underlay::definition(underlay, document) else {
                    return;
                };
                if self.tabs[i]
                    .scene
                    .unloaded_underlay_definitions
                    .contains(&underlay.definition_handle)
                {
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
            PdfImportSource::File(path) => (
                acadrust::entities::Underlay::pdf(),
                path.clone(),
                "1".to_string(),
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
        let Some(vectors) = crate::scene::model::pdf_vector::page_vectors(&path, &page) else {
            self.command_line.push_error(&format!("{shown} not found."));
            return;
        };
        let result = pdf_import::convert(&vectors, &underlay, &area);

        self.push_undo_snapshot(i, "PDFIMPORT");
        let scene = &mut self.tabs[i].scene;
        for (name, color) in &result.layers {
            if !scene.document.layers.contains(name) {
                let mut layer = acadrust::tables::Layer::new(name.as_str());
                layer.handle = scene.document.allocate_handle();
                layer.color = *color;
                let _ = scene.document.layers.add(layer);
            }
        }
        for (name, font) in &result.text_styles {
            if !scene.document.text_styles.contains(name) {
                let mut style = acadrust::tables::TextStyle::new(name.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file = font.clone();
                let _ = scene.document.text_styles.add(style);
            }
        }
        for entity in result.entities {
            scene.add_entity(entity);
        }
        if let (Some(mode), Some(handle)) = (mode, handle) {
            match mode {
                UnderlayMode::Keep => {}
                UnderlayMode::Unload => {
                    scene
                        .unloaded_underlay_definitions
                        .insert(underlay.definition_handle);
                    scene.reseed_underlays();
                }
                UnderlayMode::Detach => {
                    scene.erase_entities(&[handle]);
                    // Drop the definition once nothing references it.
                    let definition = underlay.definition_handle;
                    let referenced = scene.document.entities().any(|entity| {
                        matches!(entity, acadrust::EntityType::Underlay(other)
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
