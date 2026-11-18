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

/// An import that the UI thread started: what its worker reads, and what
/// the placement needs when the conversion ends.
#[derive(Clone, Debug)]
pub struct PdfImportJob {
    kind: JobKind,
    path: String,
    page: String,
    settings: pdf_import::PdfImportSettings,
    /// The current layer when the import started.
    current_layer: String,
}

#[derive(Clone, Debug)]
enum JobKind {
    /// A placed underlay, as it was when the import started.
    Underlay {
        request: PdfImportRequest,
        underlay: Box<document::entities::Underlay>,
    },
    /// A file: the worker reads its placement from the page.
    File(pdf_import::PdfFileImport),
}

/// What the worker of an import gives back.
#[derive(Clone, Debug)]
pub struct PdfConverted {
    /// Where the page goes.
    underlay: document::entities::Underlay,
    /// The notice of a password supplied earlier (a file import only).
    notice: Option<String>,
    /// The objects; `None` when the page could not be read.
    result: Option<pdf_import::ImportResult>,
}

/// The worker of an import: the page's placement and content read from the
/// file, and the content converted.
fn read_and_convert(job: &PdfImportJob, naming: &pdf_import::LayerNaming) -> PdfConverted {
    let (underlay, area, notice) = match &job.kind {
        JobKind::Underlay { request, underlay } => {
            ((**underlay).clone(), request.area.clone(), None)
        }
        JobKind::File(import) => (
            import.placement(),
            ImportArea::All,
            crate::scene::model::pdf_password::reopened_notice(&job.path),
        ),
    };
    // The underlay's own layer overrides leave those layers out.
    let hidden = crate::scene::model::pdf_layers::hidden_layers(&underlay);
    let result =
        crate::scene::model::pdf_vector::page_content_by_layer(&job.path, &job.page, &hidden)
            .map(|content| pdf_import::convert(&content, &underlay, &area, naming));
    PdfConverted {
        underlay,
        notice,
        result,
    }
}

impl OpenCADStudio {
    /// PDFIMPORT: the page read and converted off the UI thread;
    /// [`Self::finish_pdf_import`] places the result when it arrives. A
    /// panic in the reader or the conversion becomes an error on the
    /// command line.
    pub(crate) fn run_pdf_import(&mut self, i: usize, source: PdfImportSource) -> Task<Message> {
        self.run_pdf_import_with(i, source, read_and_convert)
    }

    /// [`Self::run_pdf_import`] with the worker given, so a test can give
    /// one that panics.
    fn run_pdf_import_with(
        &mut self,
        i: usize,
        source: PdfImportSource,
        worker: fn(&PdfImportJob, &pdf_import::LayerNaming) -> PdfConverted,
    ) -> Task<Message> {
        use crate::ui::window::pdf_dialogs::PdfDialogMsg;
        let document = &self.tabs[i].scene.document;
        let (kind, path, page) = match source {
            PdfImportSource::Underlay(request) => {
                let Some(document::EntityType::Underlay(underlay)) =
                    document.get_entity(request.underlay)
                else {
                    return Task::none();
                };
                let Some(def) = crate::entities::underlay::definition(underlay, document) else {
                    return Task::none();
                };
                if def.unloaded {
                    self.command_line
                        .push_error("Cannot bind a PDF underlay that is unloaded.");
                    return Task::none();
                }
                let path = def.file_path.clone();
                let page = crate::entities::underlay::page_of(def).to_string();
                let underlay = Box::new(underlay.clone());
                (JobKind::Underlay { request, underlay }, path, page)
            }
            PdfImportSource::File(import) => {
                let (path, page) = (import.path.clone(), import.page.clone());
                (JobKind::File(import), path, page)
            }
        };
        let shown = crate::entities::underlay::display_path(&path);
        match &kind {
            JobKind::Underlay { .. } => self
                .command_line
                .push_output(&format!("Binding PDF file {shown}, page {page} ...")),
            JobKind::File(_) => self
                .command_line
                .push_output(&format!("Importing page {page} of PDF file: {shown}...")),
        }
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
        let job = PdfImportJob {
            kind,
            path,
            page,
            settings,
            current_layer,
        };
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let worker_job = job.clone();
        crate::app::update::background_task(
            "PDFIMPORT",
            move || worker(&worker_job, &naming),
            move |converted| {
                Message::PdfDialog(PdfDialogMsg::ImportConverted(
                    tab_id,
                    generation,
                    Box::new(job),
                    Box::new(converted),
                ))
            },
        )
    }

    /// Places a finished PDF import on the UI thread, one undo step: the
    /// objects with the layers and text styles they use, then what happens
    /// to the underlay (Keep / Detach / Unload). A result for a drawing that
    /// was closed, replaced or cleared is dropped, and so is one for an
    /// underlay that was erased during the read.
    pub(in crate::app) fn finish_pdf_import(
        &mut self,
        tab_id: u64,
        generation: u64,
        job: PdfImportJob,
        converted: PdfConverted,
    ) {
        use document::objects::ObjectType;
        let Some(i) = self.import_target(tab_id, generation) else {
            return;
        };
        let PdfConverted {
            underlay,
            notice,
            result,
        } = converted;
        if let JobKind::Underlay { request, .. } = &job.kind {
            let present = matches!(
                self.tabs[i].scene.document.get_entity(request.underlay),
                Some(document::EntityType::Underlay(_))
            );
            if !present {
                self.command_line
                    .push_info("PDFIMPORT: the underlay was erased while the file was read.");
                return;
            }
        }
        if let Some(notice) = notice {
            self.command_line.push_output(&notice);
        }
        let Some(mut result) = result else {
            let shown = crate::entities::underlay::display_path(&job.path);
            self.command_line.push_error(&format!("{shown} not found."));
            return;
        };
        let (path, page) = (&job.path, &job.page);
        let settings = job.settings;
        let current_layer = job.current_layer.clone();
        let images = std::mem::take(&mut result.images);
        let image_dir = pdf_import::image_dir(path);

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
            let file = dir.join(format!("{stem}{:08x}.png", image_file_id(path, page, n)));
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
        if let JobKind::Underlay { request, .. } = &job.kind {
            let handle = request.underlay;
            match request.mode {
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
    /// definitions dictionary) and underlay in one undo step. When the
    /// pages can be placed by their geospatial data (`auto_place` and a
    /// coordinate system), the PDF is read off the UI thread first, and the
    /// pages are placed when the read ends ([`Self::finish_attach_pdf_pages`]).
    pub(crate) fn attach_pdf_pages(
        &mut self,
        i: usize,
        label: String,
        kind: document::entities::UnderlayType,
        path: &str,
        pages: Vec<(String, document::EntityType)>,
        auto_place: bool,
    ) -> iced::Task<crate::app::Message> {
        self.attach_pdf_pages_with(i, label, kind, path, pages, auto_place, placements_in)
    }

    /// [`Self::attach_pdf_pages`] with the placement read given, so a test
    /// can give one that panics.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn attach_pdf_pages_with(
        &mut self,
        i: usize,
        label: String,
        kind: document::entities::UnderlayType,
        path: &str,
        pages: Vec<(String, document::EntityType)>,
        auto_place: bool,
        placer: GeoPdfPlacer,
    ) -> iced::Task<crate::app::Message> {
        let Some(context) = geo_context(&self.tabs[i].scene, kind, auto_place) else {
            let placements = vec![None; pages.len()];
            self.place_pdf_pages(i, label, kind, path, pages, &placements);
            return iced::Task::none();
        };
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let path = path.to_string();
        crate::app::update::background_task(
            "PDFATTACH",
            move || {
                let placements = placer(Some(&context), &path, &pages);
                Box::new(crate::ui::window::pdf_dialogs::PlacedPdfPages {
                    label,
                    kind,
                    path,
                    pages,
                    placements,
                })
            },
            move |placed| {
                crate::app::Message::PdfDialog(
                    crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachPlaced(
                        tab_id, generation, placed,
                    ),
                )
            },
        )
    }

    /// The end of the placement read: the pages are placed in the drawing
    /// the attach was started for. A result for a drawing that was closed,
    /// replaced or cleared is dropped, and so is one for a drawing that is
    /// not the current one now (`commit_entity_handle` writes to the
    /// current drawing).
    pub(in crate::app) fn finish_attach_pdf_pages(
        &mut self,
        tab_id: u64,
        generation: u64,
        placed: crate::ui::window::pdf_dialogs::PlacedPdfPages,
    ) {
        let Some(i) = self.import_target(tab_id, generation) else {
            return;
        };
        if i != self.active_tab {
            self.command_line.push_error(
                "Attach PDF cancelled: a different drawing became current while the file was read.",
            );
            return;
        }
        let crate::ui::window::pdf_dialogs::PlacedPdfPages {
            label,
            kind,
            path,
            pages,
            placements,
        } = placed;
        self.place_pdf_pages(i, label, kind, &path, pages, &placements);
    }

    /// The pages in one undo step, each placed by its GeoPDF placement when
    /// it has one.
    fn place_pdf_pages(
        &mut self,
        i: usize,
        label: String,
        kind: document::entities::UnderlayType,
        path: &str,
        pages: Vec<(String, document::EntityType)>,
        placements: &[Option<crate::io::geopdf::GeoPlacement>],
    ) {
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
#[cfg(test)]
pub(super) fn fully_georeferenced_pdf_pages(
    scene: &crate::scene::Scene,
    kind: document::entities::UnderlayType,
    path: &str,
    page_names: &[String],
    plane: crate::command::WorkingPlane,
) -> Option<Vec<(String, document::EntityType)>> {
    georeferenced_pages_in(
        geo_context(scene, kind, true).as_ref(),
        kind,
        path,
        page_names,
        plane,
    )
}

/// [`fully_georeferenced_pdf_pages`] with the drawing's part given
/// ([`geo_context`]): it reads only the PDF, so a worker can run it.
pub(super) fn georeferenced_pages_in(
    context: Option<&GeoContext>,
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
    let placements = placements_in(context, path, &pages);
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

/// The drawing's coordinate system and its millimetres per drawing unit.
pub(super) type GeoContext = (crate::io::crs::CrsDefinition, f64);

/// A GeoPDF check without the drawing: [`georeferenced_pages_in`], or a
/// test's check.
pub(super) type GeoPdfCheck = fn(
    Option<&GeoContext>,
    document::entities::UnderlayType,
    &str,
    &[String],
    crate::command::WorkingPlane,
) -> Option<Vec<(String, document::EntityType)>>;

/// The GeoPDF placement of each page without the drawing: [`placements_in`],
/// or a test's.
pub(in crate::app) type GeoPdfPlacer = fn(
    Option<&GeoContext>,
    &str,
    &[(String, document::EntityType)],
) -> Vec<Option<crate::io::geopdf::GeoPlacement>>;

/// The part of a GeoPDF placement that comes from the drawing: `None` when
/// PDF pages cannot be placed by their geospatial data here.
pub(super) fn geo_context(
    scene: &crate::scene::Scene,
    kind: document::entities::UnderlayType,
    enabled: bool,
) -> Option<GeoContext> {
    (enabled
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
    .flatten()
}

#[cfg(test)]
fn auto_pdf_placements(
    scene: &crate::scene::Scene,
    kind: document::entities::UnderlayType,
    path: &str,
    pages: &[(String, document::EntityType)],
    enabled: bool,
) -> Vec<Option<crate::io::geopdf::GeoPlacement>> {
    placements_in(geo_context(scene, kind, enabled).as_ref(), path, pages)
}

/// The GeoPDF placement of each page for a drawing context; it reads only
/// the PDF.
fn placements_in(
    context: Option<&GeoContext>,
    path: &str,
    pages: &[(String, document::EntityType)],
) -> Vec<Option<crate::io::geopdf::GeoPlacement>> {
    pages
        .iter()
        .map(|(page, entity)| {
            let (crs, unit_mm) = context?;
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
        let placed = app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::new(10.0, 20.0, 0.0)),
            true,
        );
        app.drive_headless_task(placed).expect("the placement read");
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
        let read = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(read).expect("the read task");
        let state = app.pdf_attach.as_mut().expect("attach dialog");
        state.path_type =
            crate::ui::window::pdf_dialogs::PathTypeChoice(crate::io::xref_model::Pathtype::None);
        state.insert_on_screen = true;
        state.scale_on_screen = true;
        state.rotation_on_screen = true;
        assert!(state.geopdf_available);
        assert!(state.geopdf_auto);

        let ok = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachOk,
        ));
        app.drive_headless_task(ok).expect("OK's GeoPDF read");

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
        let read = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(read).expect("the read task");
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
        let read = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(read).expect("the read task");
        app.ctrl_down = true;
        let check = app.update(crate::app::Message::PdfDialog(
            crate::ui::window::pdf_dialogs::PdfDialogMsg::AttachPage(1),
        ));
        app.ctrl_down = false;
        app.drive_headless_task(check).expect("the GeoPDF check");
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

    /// A GeoPDF check that panics gives an error, and the window stays
    /// without GeoPDF placement.
    #[test]
    fn a_geopdf_check_that_panics_is_an_error() {
        let mut app = app();
        set_crs(&mut app);
        let path = "dialog-geopdf-panic.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let read = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(read).expect("the read task");
        assert!(app.pdf_attach.as_ref().unwrap().geopdf_available);
        app.pdf_attach.as_mut().unwrap().selected = vec![1];
        let check = app.check_pdf_attach_geopdf_with(|_, _, _, _, _| panic!("a reader bug"));
        app.drive_headless_task(check).expect("the GeoPDF check");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFATTACH failed: a reader bug")
        );
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::PdfAttach));
        assert!(
            app.pdf_attach.as_ref().unwrap().geopdf_available,
            "no answer, no change"
        );
    }

    /// The answer of a check is taken only for the file and the pages that
    /// the open window shows.
    #[test]
    fn a_geopdf_answer_for_other_pages_or_a_closed_window_changes_nothing() {
        use crate::ui::window::pdf_dialogs::PdfDialogMsg;
        let mut app = app();
        let path = "dialog-geopdf-answers.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let read = app.open_pdf_attach_dialog(path);
        app.drive_headless_task(read).expect("the read task");
        let available = |app: &OpenCADStudio| app.pdf_attach.as_ref().unwrap().geopdf_available;
        assert!(!available(&app), "no coordinate system");
        let answer = |path: &str, page: &str| {
            crate::app::Message::PdfDialog(PdfDialogMsg::AttachGeoChecked(
                path.to_string(),
                vec![page.to_string()],
                true,
            ))
        };
        let _ = app.update(answer(path, "2"));
        assert!(!available(&app), "other pages");
        let _ = app.update(answer("other.pdf", "1"));
        assert!(!available(&app), "another file");
        app.active_modal = None;
        let _ = app.update(answer(path, "1"));
        assert!(!available(&app), "a closed window");
        app.active_modal = Some(crate::app::ModalKind::PdfAttach);
        let _ = app.update(answer(path, "1"));
        let state = app.pdf_attach.as_ref().unwrap();
        assert!(state.geopdf_available, "the pages shown");
        assert!(state.geopdf_auto);
    }

    /// A name change checks the new file: a file with no page shows no
    /// GeoPDF placement, also after a GeoPDF file.
    #[test]
    fn a_name_change_checks_the_new_file_for_geopdf() {
        use crate::ui::window::pdf_dialogs::PdfDialogMsg;
        let mut app = app();
        set_crs(&mut app);
        let (geo, empty) = ("dialog-geopdf-name.pdf", "dialog-geopdf-name-empty.pdf");
        crate::scene::model::pdf_raster::register_source(geo, Arc::new(two_page_pdf()));
        crate::scene::model::pdf_raster::register_source(empty, Arc::new(b"not a PDF".to_vec()));
        let read = app.open_pdf_attach_dialog(geo);
        app.drive_headless_task(read).expect("the read task");
        let state = app.pdf_attach.as_mut().unwrap();
        assert!(state.geopdf_available);
        state.existing = vec![("empty".to_string(), empty.to_string())];
        let read = app.update(crate::app::Message::PdfDialog(PdfDialogMsg::AttachName(
            "empty".to_string(),
        )));
        app.drive_headless_task(read).expect("the name read");
        let state = app.pdf_attach.as_ref().unwrap();
        assert_eq!(state.path, empty);
        assert!(!state.geopdf_available);
        assert!(!state.geopdf_auto);
    }

    /// The underlays in drawing `i`.
    fn underlays_in(app: &OpenCADStudio, i: usize) -> usize {
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Underlay(_)))
            .count()
    }

    /// A PDFATTACH with automatic placement reads the placements on a
    /// worker: nothing is placed until the read ends.
    #[test]
    fn geopdf_pages_are_placed_only_when_the_read_ends() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "placed-on-a-worker.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let placed = app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::ZERO),
            true,
        );
        assert_eq!(
            underlays_in(&app, i),
            0,
            "nothing is placed during the read"
        );
        app.drive_headless_task(placed).expect("the placement read");
        assert_eq!(underlays_in(&app, i), 2);
    }

    /// The command's end (`CmdResult::AttachPdfPages` with automatic
    /// placement) returns the placement read, and the pages are placed
    /// when it ends.
    #[test]
    fn the_command_result_returns_the_placement_read() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "placed-by-the-command.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let placed = app.apply_cmd_result(crate::command::CmdResult::AttachPdfPages {
            kind: UnderlayType::Pdf,
            path: path.to_string(),
            pages: pages(DVec3::ZERO),
            auto_place: true,
        });
        assert_eq!(
            underlays_in(&app, i),
            0,
            "nothing is placed during the read"
        );
        app.drive_headless_task(placed).expect("the placement read");
        assert_eq!(underlays_in(&app, i), 2);
    }

    /// Without automatic placement nothing is read: the pages are placed at
    /// once.
    #[test]
    fn pages_without_automatic_placement_are_placed_at_once() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "placed-at-once.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let _ = app.attach_pdf_pages_with(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::ZERO),
            false,
            |_, _, _| panic!("no read"),
        );
        assert_eq!(underlays_in(&app, i), 2);
    }

    /// A placement read that panics gives an error, and nothing is placed.
    #[test]
    fn a_placement_read_that_panics_is_an_error() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let before = app.tabs[i].history.undo_stack.len();
        let placed = app.attach_pdf_pages_with(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            "placement-panics.pdf",
            pages(DVec3::ZERO),
            true,
            |_, _, _| panic!("a reader bug"),
        );
        app.drive_headless_task(placed).expect("the placement read");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFATTACH failed: a reader bug")
        );
        assert_eq!(underlays_in(&app, i), 0);
        app.finish_all_pending_history();
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
    }

    /// A result for a drawing that was replaced is dropped.
    #[test]
    fn a_placement_for_a_replaced_drawing_is_dropped() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "placement-replaced.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let placed = app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::ZERO),
            true,
        );
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(placed).expect("the placement read");
        assert_eq!(underlays_in(&app, app.active_tab), 0);
        let history = app
            .command_line
            .history
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(history.contains("was replaced"), "{history}");
    }

    /// A result for a drawing that is no longer the current one is dropped
    /// with a message: the commit writes to the current drawing.
    #[test]
    fn a_placement_for_a_drawing_that_is_not_current_is_dropped() {
        let mut app = app();
        let i = app.active_tab;
        set_crs(&mut app);
        let path = "placement-not-current.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(two_page_pdf()));
        let placed = app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            path,
            pages(DVec3::ZERO),
            true,
        );
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        app.drive_headless_task(placed).expect("the placement read");
        assert_eq!(underlays_in(&app, i), 0);
        assert_eq!(underlays_in(&app, app.active_tab), 0);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(
                "Attach PDF cancelled: a different drawing became current while the file was read."
            )
        );
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
        let _ = app.attach_pdf_pages(
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

#[cfg(test)]
mod pdfimport_tests {
    use super::*;
    use crate::command::StepInput;
    use crate::modules::insert::pdf_import::PdfFileImport;
    use document::entities::{Underlay, UnderlayType};
    use document::EntityType;
    use glam::DVec3;
    use std::sync::Arc;

    /// The Inkscape fixture of the PDF reader's tests, registered as a
    /// file picked this session.
    fn inkscape() -> String {
        let path = format!(
            "{}/tests/fixtures/pdf/inkscape.pdf",
            env!("CARGO_MANIFEST_DIR")
        );
        crate::scene::model::pdf_raster::register_source(
            &path,
            Arc::new(std::fs::read(&path).expect("the fixture")),
        );
        path
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// The fixture's page 1 attached at the origin, full size: the
    /// underlay's handle.
    fn attached(app: &mut OpenCADStudio) -> document::Handle {
        let i = app.active_tab;
        let path = inkscape();
        let pages = crate::modules::insert::pdf_attach::underlays_for_pages(
            UnderlayType::Pdf,
            &["1".to_string()],
            DVec3::ZERO,
            1.0,
            0.0,
        );
        let _ = app.attach_pdf_pages(
            i,
            "PDFATTACH".to_string(),
            UnderlayType::Pdf,
            &path,
            pages,
            false,
        );
        app.finish_all_pending_history();
        underlays(app)[0].0
    }

    fn underlays(app: &OpenCADStudio) -> Vec<(document::Handle, Underlay)> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Underlay(u) => Some((u.common.handle, u.clone())),
                _ => None,
            })
            .collect()
    }

    /// The texts of the drawing, and its other objects by kind (underlays
    /// left out).
    fn content(app: &OpenCADStudio) -> (Vec<String>, Vec<String>) {
        let mut texts = Vec::new();
        let mut others = Vec::new();
        for e in app.tabs[app.active_tab].scene.document.entities() {
            match e {
                EntityType::MText(m) => texts.push(m.value.clone()),
                EntityType::Underlay(_) => {}
                other => others.push(
                    format!("{other:?}")
                        .split('(')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                ),
            }
        }
        (texts, others)
    }

    fn import(app: &mut OpenCADStudio, source: PdfImportSource) -> usize {
        let i = app.active_tab;
        let before = app.tabs[i].history.undo_stack.len();
        let task = app.run_pdf_import(i, source);
        app.drive_headless_task(task).expect("the import task");
        app.tabs[i].history.undo_stack.len() - before
    }

    fn whole(underlay: document::Handle, mode: UnderlayMode) -> PdfImportSource {
        PdfImportSource::Underlay(PdfImportRequest {
            underlay,
            area: ImportArea::All,
            mode,
        })
    }

    fn history(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text()
    }

    /// The layers whose names follow the import's `PDF<n>_` prefix (the
    /// number counts the session's imports), without it.
    fn import_layers(app: &OpenCADStudio) -> Vec<String> {
        let mut names: Vec<String> = app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .iter()
            .filter_map(|l| {
                let rest = l.name.strip_prefix("PDF")?;
                let (number, kind) = rest.split_once('_')?;
                number
                    .chars()
                    .all(|c| c.is_ascii_digit())
                    .then(|| kind.to_string())
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn an_underlay_is_imported_in_one_undo_step_with_its_layers_and_styles() {
        let mut app = new_app();
        let handle = attached(&mut app);
        assert_eq!(import(&mut app, whole(handle, UnderlayMode::Keep)), 1);
        assert!(history(&app).contains("Binding PDF file "));
        assert!(history(&app).contains(", page 1 ..."));
        let (mut texts, others) = content(&app);
        texts.sort();
        assert_eq!(texts, ["Hello PDF", "Tilted"]);
        // The polyline, the circle, the rectangle filled and stroked.
        assert!(others.contains(&"Circle".to_string()), "{others:?}");
        assert!(others.contains(&"LwPolyline".to_string()), "{others:?}");
        assert!(others.contains(&"Solid".to_string()), "{others:?}");
        assert_eq!(import_layers(&app), ["Geometry", "Solid Fills", "Text"]);
        // Each layer in the colour most of its objects have, the first on a
        // tie: the polyline's, among the polyline, circle and rectangle.
        let geometry = app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .iter()
            .find(|l| l.name.ends_with("_Geometry"))
            .map(|l| l.color);
        assert_eq!(
            geometry,
            Some(document::types::Color::Rgb { r: 204, g: 0, b: 0 })
        );
        let styles: Vec<String> = app.tabs[app.active_tab]
            .scene
            .document
            .text_styles
            .iter()
            .map(|s| s.name.clone())
            .filter(|n| n.starts_with("PDF "))
            .collect();
        assert_eq!(styles, ["PDF ArialMT", "PDF TimesNewRomanPSMT"]);
        // Keep leaves the underlay as it was.
        assert_eq!(underlays(&app).len(), 1);
        let document = &app.tabs[app.active_tab].scene.document;
        let definition = crate::entities::underlay::definition(&underlays(&app)[0].1, document);
        assert!(definition.is_some_and(|d| !d.unloaded));
        // UNDO takes it all back.
        app.undo_steps(1);
        assert_eq!(content(&app), (Vec::new(), Vec::new()));
    }

    #[test]
    fn unload_and_detach_do_what_they_say() {
        // Unload: the underlay stays, unloaded; it cannot be imported again.
        let mut app = new_app();
        let handle = attached(&mut app);
        assert_eq!(import(&mut app, whole(handle, UnderlayMode::Unload)), 1);
        let (_, underlay) = underlays(&app).remove(0);
        let document = &app.tabs[app.active_tab].scene.document;
        assert!(
            crate::entities::underlay::definition(&underlay, document).is_some_and(|d| d.unloaded)
        );
        let objects = content(&app);
        assert_eq!(import(&mut app, whole(handle, UnderlayMode::Keep)), 0);
        assert!(history(&app).contains("Cannot bind a PDF underlay that is unloaded."));
        assert_eq!(content(&app), objects);

        // Detach: the underlay is erased, and its definition with it.
        let mut app = new_app();
        let handle = attached(&mut app);
        let definition = underlays(&app)[0].1.definition_handle;
        assert_eq!(import(&mut app, whole(handle, UnderlayMode::Detach)), 1);
        assert!(underlays(&app).is_empty());
        assert!(!app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .contains_key(&definition));
        assert_eq!(content(&app).0.len(), 2);
    }

    #[test]
    fn a_file_is_imported_at_its_insertion_point_scale_and_rotation() {
        let mut app = new_app();
        let import_file = PdfFileImport {
            path: inkscape(),
            page: "1".to_string(),
            scale: 2.0,
            rotation: 0.0,
            insertion: DVec3::new(10.0, 20.0, 0.0),
        };
        assert_eq!(import(&mut app, PdfImportSource::File(import_file)), 1);
        assert!(history(&app).contains("Importing page 1 of PDF file: "));
        // The circle of radius 0.5 round (5, 4.5) in, twice the size.
        let circle = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                EntityType::Circle(c) => Some(c.clone()),
                _ => None,
            })
            .expect("a circle");
        assert!((circle.center.x - 20.0).abs() < 1e-6, "{:?}", circle.center);
        assert!((circle.center.y - 29.0).abs() < 1e-6, "{:?}", circle.center);
        assert!((circle.radius - 1.0).abs() < 1e-6);
        // No underlay is attached for it.
        assert!(underlays(&app).is_empty());

        // A file that is not there.
        let mut app = new_app();
        let missing = PdfFileImport {
            path: "pdfimport-test-missing.pdf".to_string(),
            page: "1".to_string(),
            scale: 1.0,
            rotation: 0.0,
            insertion: DVec3::ZERO,
        };
        assert_eq!(import(&mut app, PdfImportSource::File(missing)), 0);
        assert!(history(&app).contains("pdfimport-test-missing.pdf not found."));
        assert_eq!(content(&app), (Vec::new(), Vec::new()));
    }

    /// -PDFIMPORT answered step by step (the point as the command line
    /// hands it on, already a point).
    #[test]
    fn pdfimport_typed_on_the_command_line_imports_the_file() {
        let mut app = new_app();
        let path = inkscape();
        let i = app.active_tab;
        let _ = app.update(Message::Command("-PDFIMPORT".to_string()));
        for input in [
            StepInput::Text("F".to_string()),
            StepInput::Text(path),
            StepInput::Enter,
            StepInput::Point(DVec3::new(5.0, 5.0, 0.0)),
            StepInput::Text("3".to_string()),
            StepInput::Enter,
        ] {
            let task = app.feed_command(input);
            app.drive_headless_task(task).expect("the command's task");
        }
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
        let (mut texts, _) = content(&app);
        texts.sort();
        assert_eq!(texts, ["Hello PDF", "Tilted"]);
    }

    /// A script line with all the answers waits at the read, as at a PAUSE,
    /// and takes the rest of the line when the read ends.
    #[test]
    fn a_one_line_script_takes_its_answers_after_the_read() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = inkscape();
        let response = app.automation_op(&format!(
            r#"{{"op":"run","cmd":"-PDFIMPORT F \"{path}\" 1 5,5 3 0"}}"#
        ));
        assert!(app.tabs[i].active_cmd.is_none(), "{response}");
        assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
        let (mut texts, _) = content(&app);
        texts.sort();
        assert_eq!(texts, ["Hello PDF", "Tilted"]);
    }

    /// A script line whose file needs a password keeps its answers while
    /// the password window is open; cancelling the window drops them, so
    /// they do not go to the next file name.
    #[test]
    fn a_cancelled_password_drops_the_rest_of_the_line() {
        let mut app = new_app();
        let i = app.active_tab;
        let path =
            std::env::temp_dir().join(format!("pdfimport-locked-{}.pdf", std::process::id()));
        let file = "%PDF-1.4\ntrailer\n<< /Encrypt << /Filter /Standard /V 1 /R 2 \
            /O <95fe1f4219662a774442fb072e3d9f19e9d130ec09a4d0061e78fe920f7ab62f> \
            /U <0102030405060708090a0b0c0d0e0f100102030405060708090a0b0c0d0e0f10> /P -44 >> >>\n%%EOF\n";
        std::fs::write(&path, file).expect("write the locked file");
        let _ = app.automation_op(&format!(
            r#"{{"op":"run","cmd":"-PDFIMPORT F \"{}\" 1 5,5 3 0"}}"#,
            path.display()
        ));
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::PdfPassword));
        assert!(
            app.tabs[i].pending_pause_tokens.is_some(),
            "the answers wait"
        );
        let _ = app.cancel_pdf_password();
        assert_eq!(app.tabs[i].pending_pause_tokens, None);
        let _ = std::fs::remove_file(path);
    }

    /// -PDFIMPORT at its file name prompt, the File option taken.
    fn at_file_name(app: &mut OpenCADStudio) {
        let _ = app.update(Message::Command("-PDFIMPORT".to_string()));
        let task = app.feed_command(StepInput::Text("F".to_string()));
        app.drive_headless_task(task).expect("the command's task");
    }

    fn prompt(app: &OpenCADStudio) -> String {
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
            .unwrap_or_default()
    }

    /// The file name prompt reads the file on a worker: the command waits,
    /// after a `Reading` line, and asks for the page when the read ends.
    #[test]
    fn the_file_name_prompt_waits_for_the_read() {
        let mut app = new_app();
        let path = inkscape();
        at_file_name(&mut app);
        let read = app.feed_command(StepInput::Text(path.clone()));
        assert_eq!(prompt(&app), "Reading the file…");
        // Input waits until the read ends.
        let _ = app.feed_command(StepInput::Text("other.pdf".to_string()));
        assert_eq!(prompt(&app), "Reading the file…");
        assert!(
            history(&app).contains(&format!(
                "Reading {}…",
                crate::entities::underlay::display_path(&path)
            )),
            "{}",
            history(&app)
        );
        app.drive_headless_task(read).expect("the read task");
        assert_eq!(prompt(&app), "Enter page number or [?] <1>:");
    }

    /// A reader that panics gives an error, and the command asks for the
    /// file name again; it does not wait for ever.
    #[test]
    fn a_reader_panic_at_the_prompt_asks_for_the_file_again() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = inkscape();
        at_file_name(&mut app);
        let _ = app.feed_command(StepInput::Text(path.clone()));
        let read = app.read_pdf_for_import_prompt_with(i, path, |_| panic!("a reader bug"));
        app.drive_headless_task(read).expect("the read task");
        assert!(
            history(&app).contains("PDFIMPORT failed: a reader bug"),
            "{}",
            history(&app)
        );
        assert_eq!(prompt(&app), "Enter filename for PDF import:");
    }

    /// A read for a command that ended changes nothing.
    #[test]
    fn a_prompt_read_after_the_command_ended_changes_nothing() {
        let mut app = new_app();
        let i = app.active_tab;
        at_file_name(&mut app);
        let read = app.feed_command(StepInput::Text(inkscape()));
        let _ = app.update(Message::Command("LINE".to_string()));
        let line = prompt(&app);
        app.drive_headless_task(read).expect("the read task");
        assert_eq!(prompt(&app), line);
        assert!(app.tabs[i].active_cmd.is_some());
    }

    /// The read of a file named before the command started again changes
    /// nothing; the read of the file named now goes on.
    #[test]
    fn the_read_of_an_older_file_name_changes_nothing() {
        let mut app = new_app();
        at_file_name(&mut app);
        let older = app.feed_command(StepInput::Text("pdfimport-older-name.pdf".to_string()));
        let _ = app.feed_command(StepInput::Escape);
        at_file_name(&mut app);
        let newer = app.feed_command(StepInput::Text(inkscape()));
        app.drive_headless_task(older).expect("the older read");
        assert_eq!(prompt(&app), "Reading the file…");
        assert!(!history(&app).contains("pdfimport-older-name.pdf does not exist."));
        app.drive_headless_task(newer).expect("the newer read");
        assert_eq!(prompt(&app), "Enter page number or [?] <1>:");
    }

    /// The read for a command in a drawing that is not the current one
    /// moves that command on to the page prompt, without messages here.
    #[test]
    fn a_prompt_read_in_another_drawing_moves_its_command_on() {
        let mut app = new_app();
        let i = app.active_tab;
        at_file_name(&mut app);
        let read = app.feed_command(StepInput::Text(inkscape()));
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        app.drive_headless_task(read).expect("the read task");
        let waiting = app.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        assert_eq!(waiting.as_deref(), Some("Enter page number or [?] <1>:"));
    }

    /// A read in another drawing that ends with an error sends that
    /// drawing's command back to the file name prompt, and shows nothing
    /// in the current drawing.
    #[test]
    fn a_failed_prompt_read_in_another_drawing_shows_nothing_here() {
        let mut app = new_app();
        let i = app.active_tab;
        at_file_name(&mut app);
        let read = app.feed_command(StepInput::Text(
            "pdfimport-elsewhere-missing.pdf".to_string(),
        ));
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        app.drive_headless_task(read).expect("the read task");
        assert!(
            !history(&app).contains("does not exist"),
            "{}",
            history(&app)
        );
        let waiting = app.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        assert_eq!(waiting.as_deref(), Some("Enter filename for PDF import:"));
    }

    /// The import reads and converts on a worker: nothing changes in the
    /// drawing until the worker ends, then the result is one undo step.
    #[test]
    fn the_import_is_placed_only_when_the_worker_ends() {
        let mut app = new_app();
        let handle = attached(&mut app);
        let i = app.active_tab;
        let before = app.tabs[i].history.undo_stack.len();
        let task = app.run_pdf_import(i, whole(handle, UnderlayMode::Keep));
        assert_eq!(content(&app), (Vec::new(), Vec::new()));
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
        assert!(!content(&app).0.is_empty());
    }

    /// A worker that panics gives an error, the drawing does not change,
    /// and the next import works.
    #[test]
    fn a_worker_panic_is_an_error_and_the_drawing_does_not_change() {
        let mut app = new_app();
        let handle = attached(&mut app);
        let i = app.active_tab;
        let before = app.tabs[i].history.undo_stack.len();
        let task = app.run_pdf_import_with(i, whole(handle, UnderlayMode::Detach), |_, _| {
            panic!("a reader bug")
        });
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFIMPORT failed: a reader bug")
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        assert_eq!(content(&app), (Vec::new(), Vec::new()));
        assert_eq!(underlays(&app).len(), 1, "Detach did not run");
        assert_eq!(import(&mut app, whole(handle, UnderlayMode::Keep)), 1);
    }

    /// A page that cannot be read is an error when the worker ends, and the
    /// drawing does not change.
    #[test]
    fn a_page_that_cannot_be_read_is_an_error() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = "pdfimport-not-a-pdf.pdf";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(b"not a PDF".to_vec()));
        let source = PdfImportSource::File(PdfFileImport {
            path: path.to_string(),
            page: "1".to_string(),
            scale: 1.0,
            rotation: 0.0,
            insertion: DVec3::ZERO,
        });
        assert_eq!(import(&mut app, source), 0);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(format!("{path} not found.").as_str())
        );
        assert_eq!(app.tabs[i].scene.document.entities().count(), 0);
    }

    /// A result for a drawing that was replaced during the read is
    /// dropped; the new drawing gets nothing.
    #[test]
    fn a_result_for_a_replaced_drawing_is_dropped() {
        let mut app = new_app();
        let handle = attached(&mut app);
        let i = app.active_tab;
        let task = app.run_pdf_import(i, whole(handle, UnderlayMode::Keep));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(task).expect("the import task");
        let now = app.active_tab;
        assert_eq!(app.tabs[now].scene.document.entities().count(), 0);
        assert!(app.command_line.last_error.is_none());
        assert!(history(&app).contains("was replaced"), "{}", history(&app));
    }

    /// A result for an underlay that was erased during the read is
    /// dropped: nothing is added, and Detach does not run.
    #[test]
    fn a_result_for_an_erased_underlay_is_dropped() {
        let mut app = new_app();
        let handle = attached(&mut app);
        let i = app.active_tab;
        let task = app.run_pdf_import(i, whole(handle, UnderlayMode::Detach));
        app.tabs[i].scene.erase_entities(&[handle]);
        let before = app.tabs[i].history.undo_stack.len();
        app.drive_headless_task(task).expect("the import task");
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        assert_eq!(content(&app), (Vec::new(), Vec::new()));
        assert!(
            history(&app).contains("PDFIMPORT: the underlay was erased while the file was read."),
            "{}",
            history(&app)
        );
    }
}
