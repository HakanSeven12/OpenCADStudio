//! Underlay and OLE rasters drawn again off the UI thread when the view
//! rests on a new zoom step (#3292).
//!
//! At a step, [`Scene::refresh_underlay_resolution`] takes what each raster
//! needs ([`RasterInput`]: the object, its definition, the background, the
//! scale and the view) into a [`ReseedJob`] and draws nothing. The app gives
//! the job to a worker ([`ReseedJob::render`]); the old rasters stay on
//! screen meanwhile. When the worker ends, [`Scene::apply_reseed`] takes each
//! raster whose input is still the one that the scene would use now; a
//! result for an older step, or for an object that changed since, is
//! dropped. The web build has no threads: it draws at once, as before.

use super::*;

/// What drawing an underlay's or an OLE picture's raster needs, taken on
/// the UI thread so that a worker can draw it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RasterInput {
    Ole {
        ole: Box<document::entities::Ole2Frame>,
        world_per_pixel: Option<f64>,
        view: Option<[f64; 4]>,
    },
    Underlay {
        underlay: Box<document::entities::Underlay>,
        definition: Box<document::objects::UnderlayDefinition>,
        background: [f32; 4],
        world_per_pixel: Option<f64>,
        view: Option<[f64; 4]>,
    },
}

impl RasterInput {
    /// The raster, and whether it drew a zoomed-in region.
    pub(crate) fn render(&self) -> Option<(ImageModel, bool)> {
        #[cfg(test)]
        entity::IMAGE_SEEDS.with(|seeds| seeds.set(seeds.get() + 1));
        match self {
            Self::Ole {
                ole,
                world_per_pixel,
                view,
            } => ImageModel::from_ole2frame_in_view(ole, *world_per_pixel, *view),
            Self::Underlay {
                underlay,
                definition,
                background,
                world_per_pixel,
                view,
            } => ImageModel::from_underlay_in_view(
                underlay,
                definition,
                *background,
                *world_per_pixel,
                *view,
            ),
        }
    }
}

/// The rasters to draw again for a zoom step.
#[derive(Clone, Debug, Default)]
pub struct ReseedJob {
    items: Vec<(Handle, RasterInput)>,
}

/// One object of a finished job: its handle, the input it was drawn from,
/// and its raster with whether it drew a zoomed-in region.
type DrawnRaster = (Handle, RasterInput, Option<(ImageModel, bool)>);

/// A finished [`ReseedJob`]: each object's input and its raster.
#[derive(Clone, Debug)]
pub struct ReseedDone {
    items: Vec<DrawnRaster>,
}

impl ReseedJob {
    /// Draws the rasters; runs on a worker.
    pub fn render(self) -> ReseedDone {
        ReseedDone {
            items: self
                .items
                .into_iter()
                .map(|(handle, input)| {
                    let raster = input.render();
                    (handle, input, raster)
                })
                .collect(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

impl Scene {
    /// What drawing the raster of `entity` needs now, for an underlay or an
    /// OLE picture; `None` for other objects, and for an underlay without
    /// its definition.
    pub(crate) fn raster_input_for(&self, entity: &EntityType) -> Option<RasterInput> {
        match entity {
            EntityType::Ole2Frame(ole) => {
                // Pictures of the layout in view follow its scale; others
                // keep the size used when the view is unknown.
                let in_view = ole.common.owner_handle == self.current_layout_block_handle();
                let scale = &self.ole_scale;
                let (world_per_pixel, view) = if in_view {
                    (scale.world_per_pixel, scale.view)
                } else {
                    (None, None)
                };
                Some(RasterInput::Ole {
                    ole: ole.clone(),
                    world_per_pixel,
                    view,
                })
            }
            EntityType::Underlay(u) => match self.document.objects.get(&u.definition_handle) {
                Some(document::objects::ObjectType::UnderlayDefinition(def)) => {
                    // A paper-space underlay adjusts to the sheet, not the canvas.
                    let owner = u.common.owner_handle;
                    let background = if owner.is_null() || owner == self.model_space_block_handle()
                    {
                        self.bg_color
                    } else {
                        self.paper_bg_color
                    };
                    Some(RasterInput::Underlay {
                        underlay: Box::new(u.clone()),
                        definition: Box::new(def.clone()),
                        background,
                        world_per_pixel: self.underlay_scale.world_per_pixel,
                        view: self.underlay_scale.view,
                    })
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Draws `input` on this thread, as the raster of an object of this
    /// scene: a zoomed-in region marks its scale as using one.
    pub(crate) fn render_raster_input(&self, input: &RasterInput) -> Option<ImageModel> {
        let (model, sharp) = input.render()?;
        if sharp {
            self.mark_region_used(input);
        }
        Some(model)
    }

    fn mark_region_used(&self, input: &RasterInput) {
        match input {
            RasterInput::Ole { .. } => self.ole_scale.region_used.set(true),
            RasterInput::Underlay { .. } => self.underlay_scale.region_used.set(true),
        }
    }

    /// Adds the rasters of the objects `matches` picks to the job that waits
    /// for a worker; an object already in it takes its new input.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn queue_reseed(&mut self, matches: impl Fn(&EntityType) -> bool) {
        let items: Vec<(Handle, RasterInput)> = self
            .document
            .entities()
            .filter(|entity| matches(entity))
            .filter_map(|entity| Some((entity.common().handle, self.raster_input_for(entity)?)))
            .collect();
        let job = self.pending_reseed.get_or_insert_with(ReseedJob::default);
        for (handle, input) in items {
            job.items.retain(|(queued, _)| *queued != handle);
            job.items.push((handle, input));
        }
    }

    /// The job that waits for a worker, if a zoom step made one.
    pub fn take_reseed_job(&mut self) -> Option<ReseedJob> {
        self.pending_reseed.take().filter(|job| job.len() > 0)
    }

    /// Takes the rasters of a finished job: each one whose input is still
    /// the one this scene would use now replaces the object's raster, in
    /// one bump that does not draw them again. Returns how many it took.
    pub fn apply_reseed(&mut self, done: ReseedDone) -> usize {
        let mut changes = Vec::new();
        for (handle, input, raster) in done.items {
            let current = self
                .document
                .get_entity(handle)
                .and_then(|entity| self.raster_input_for(entity));
            if current.as_ref() != Some(&input) {
                continue;
            }
            self.images.remove(&handle);
            if let Some((model, sharp)) = raster {
                if sharp {
                    self.mark_region_used(&input);
                }
                self.images.insert(handle, model);
            }
            self.fresh_images.insert(handle);
            changes.push((handle, ChangeKind::Modified));
        }
        let taken = changes.len();
        if taken > 0 {
            self.bump_entities(&changes);
        }
        self.fresh_images.clear();
        taken
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use document::types::Vector3;
    use std::time::Duration;

    fn seeds() -> usize {
        entity::IMAGE_SEEDS.with(std::cell::Cell::get)
    }

    /// Four PDF underlays of a one-inch page, side by side in model space.
    fn model_with_underlays(name: &str) -> (Scene, Vec<Handle>) {
        use printpdf::{Mm, PdfDocument, PdfPage, PdfSaveOptions};
        let mut pdf = PdfDocument::new("reseed worker");
        pdf.pages.push(PdfPage::new(Mm(25.4), Mm(25.4), Vec::new()));
        let bytes = pdf.save(&PdfSaveOptions::default(), &mut Vec::new());
        let path = format!("memory://reseed-worker-{name}.pdf");
        crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
        let mut scene = Scene::new();
        let definition = scene.document.allocate_handle();
        scene.document.objects.insert(
            definition,
            document::objects::ObjectType::UnderlayDefinition(
                document::objects::UnderlayDefinition::pdf(&path, "1"),
            ),
        );
        let handles = (0..4)
            .map(|k| {
                let mut underlay = document::entities::Underlay::pdf();
                underlay.definition_handle = definition;
                underlay.insertion_point = Vector3::new(f64::from(k) * 2.0, 0.0, 0.0);
                scene.add_entity(EntityType::Underlay(underlay))
            })
            .collect();
        (scene, handles)
    }

    /// Zooms to `[x0, y0]`–`[x1, y1]` in a 1000-pixel square view and lets
    /// the view rest; the job is left for the caller.
    fn rest_at(scene: &mut Scene, x0: f32, y0: f32, x1: f32, y1: f32) {
        scene.zoom_to_window(glam::Vec3::new(x0, y0, 0.0), glam::Vec3::new(x1, y1, 0.0));
        scene.set_render_pixel_scale(1000.0, 1000.0);
        let start = iced::time::Instant::now();
        scene.refresh_underlay_resolution(start);
        scene.refresh_underlay_resolution(start + Duration::from_millis(200));
    }

    fn pixels(scene: &Scene, handles: &[Handle]) -> Vec<std::sync::Arc<Vec<u8>>> {
        handles
            .iter()
            .map(|handle| std::sync::Arc::clone(&scene.images[handle].pixels))
            .collect()
    }

    /// A zoom step draws nothing on the UI thread: the old rasters stay
    /// until the worker's result is taken, and taking it draws nothing.
    #[test]
    fn a_zoom_step_draws_nothing_on_the_ui_thread() {
        let (mut scene, handles) = model_with_underlays("ui-thread");
        let old = pixels(&scene, &handles);
        let before = seeds();
        rest_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
        assert_eq!(seeds(), before, "no render at the zoom step");
        assert!(
            !scene.underlay_resolution_stale(),
            "no more frames are asked for"
        );
        let shown = pixels(&scene, &handles);
        assert!(
            old.iter()
                .zip(&shown)
                .all(|(a, b)| std::sync::Arc::ptr_eq(a, b)),
            "the old rasters stay on screen"
        );
        let job = scene.take_reseed_job().expect("a job for the worker");
        assert!(scene.take_reseed_job().is_none(), "the job is taken once");
        let done = job.render();
        let drawn = seeds();
        assert_eq!(
            drawn - before,
            handles.len(),
            "the worker draws each one once"
        );
        assert_eq!(scene.apply_reseed(done), handles.len());
        assert_eq!(seeds(), drawn, "taking the result draws nothing again");
        let new = pixels(&scene, &handles);
        assert!(
            old.iter()
                .zip(&new)
                .all(|(a, b)| !std::sync::Arc::ptr_eq(a, b)),
            "the new rasters replace the old"
        );
    }

    /// A result for an older zoom step is dropped; the newer one is taken.
    #[test]
    fn a_result_for_an_older_step_is_dropped() {
        let (mut scene, handles) = model_with_underlays("older-step");
        rest_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
        let older = scene.take_reseed_job().expect("a job").render();
        rest_at(&mut scene, 0.0, 0.0, 2.0, 2.0);
        let newer = scene.take_reseed_job().expect("a job").render();
        assert_eq!(
            scene.apply_reseed(older),
            0,
            "the older step's rasters are dropped"
        );
        assert_eq!(scene.apply_reseed(newer), handles.len());
    }

    /// An object changed while the worker draws keeps the raster its edit
    /// drew; the others take theirs.
    #[test]
    fn a_changed_object_keeps_its_own_raster() {
        let (mut scene, handles) = model_with_underlays("changed");
        rest_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
        let done = scene.take_reseed_job().expect("a job").render();
        if let Some(EntityType::Underlay(u)) = scene.document.get_entity_mut(handles[0]) {
            u.fade = 50;
        }
        scene.bump_entities(&[(handles[0], ChangeKind::Modified)]);
        let edited = std::sync::Arc::clone(&scene.images[&handles[0]].pixels);
        assert_eq!(scene.apply_reseed(done), handles.len() - 1);
        assert!(
            std::sync::Arc::ptr_eq(&edited, &scene.images[&handles[0]].pixels),
            "the edited underlay keeps the raster of its edit"
        );
    }

    /// A raster of a zoomed-in region marks its scale, when the result is
    /// taken, so that leaving the region asks for frames again.
    #[test]
    fn a_zoomed_in_region_is_marked_when_taken() {
        let (mut scene, _) = model_with_underlays("region");
        rest_at(&mut scene, 0.4, 0.4, 0.6, 0.6);
        assert!(!scene.underlay_scale.region_used.get(), "nothing drawn yet");
        let done = scene.take_reseed_job().expect("a job").render();
        scene.apply_reseed(done);
        assert!(
            scene.underlay_scale.region_used.get(),
            "the region is marked"
        );
        scene.zoom_to_window(
            glam::Vec3::new(4.0, 4.0, 0.0),
            glam::Vec3::new(4.2, 4.2, 0.0),
        );
        assert!(
            scene.underlay_resolution_stale(),
            "leaving the region asks for frames"
        );
    }

    /// Two zoom steps before the worker starts make one job, with the
    /// newer input of each object.
    #[test]
    fn a_job_keeps_one_input_for_each_object() {
        let (mut scene, handles) = model_with_underlays("one-input");
        rest_at(&mut scene, 0.0, 0.0, 8.0, 8.0);
        rest_at(&mut scene, 0.0, 0.0, 2.0, 2.0);
        let job = scene.take_reseed_job().expect("a job");
        assert_eq!(job.len(), handles.len());
        assert_eq!(scene.apply_reseed(job.render()), handles.len());
    }
}
