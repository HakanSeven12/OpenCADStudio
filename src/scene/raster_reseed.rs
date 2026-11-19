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
