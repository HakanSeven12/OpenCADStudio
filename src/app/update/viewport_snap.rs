//! Snap to displayed model geometry while a paper-space command is active.
//! Viewport queries use the renderer's resident wires and interaction index,
//! then project the accepted model point onto the sheet. Ordinary selection
//! continues to use paper-space hit testing.

use iced::Point;

use acadrust::types::Handle;

use crate::app::OpenCADStudio;
use crate::scene::viewport_ref::{AcceptedSnap, ViewportFrame};
use crate::snap::{SnapResult, SnapType};

/// Bound on the retained accepted-snap history. A command's point steps are a
/// handful at most; the cap only stops a long polyline from growing forever.
const MAX_ACCEPTED_SNAPS: usize = 64;

impl OpenCADStudio {
    /// Snaps accepted by the active command's point steps, oldest first, in
    /// step order.
    pub(crate) fn accepted_snaps(&self) -> &[AcceptedSnap] {
        &self.accepted_snaps
    }

    /// Infer direct sources only when the measurement uses the active space.
    /// Projected model points must retain their acquired source identities;
    /// proximity on the sheet could bind them to unrelated paper geometry.
    pub(crate) fn infer_dimension_sources_guarded(
        &self,
        tab: usize,
        dimension: Handle,
    ) -> Vec<Option<Handle>> {
        if !self.dimension_association_allowed(tab) {
            return Vec::new();
        }
        self.tabs[tab].scene.infer_dimension_sources(dimension)
    }

    /// Whether the dimension can use ordinary active-space associations.
    /// Placement clicks are excluded by the measurement classification.
    pub(crate) fn dimension_association_allowed(&self, tab: usize) -> bool {
        matches!(
            self.dimension_measure_space(tab),
            crate::app::dim_viewport::DimensionMeasureSpace::Direct
        )
    }

    /// Forget the accepted snaps of a finished / abandoned command.
    pub(crate) fn clear_accepted_snaps(&mut self) {
        self.accepted_snaps.clear();
    }

    /// Record the snap a point step just accepted. `snap` is the displayed snap
    /// result (already in the current space's coordinates) or `None` for a free
    /// / typed point; `committed` is the point the command actually received
    /// after ortho / polar / axis-lock / dynamic-input resolution.
    pub(crate) fn record_accepted_snap(
        &mut self,
        tab: usize,
        snap: Option<SnapResult>,
        frame: Option<ViewportFrame>,
        committed: glam::DVec3,
    ) -> bool {
        let mut accepted = match snap {
            // A viewport snap keeps its frame only when the hit really came
            // through it, so a paper-sheet snap never claims a model point.
            Some(hit) => {
                let frame = hit.viewport.and(frame);
                AcceptedSnap::from_snap(&hit, frame).with_paper_point(committed)
            }
            None => AcceptedSnap::free(committed),
        };
        if self.tabs[tab]
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.measures_through_viewports())
        {
            if self.tabs[tab]
                .active_cmd
                .as_ref()
                .is_some_and(|cmd| cmd.dimension_placement_pending())
            {
                return true;
            }
            if !self.dimension_acquisition_allowed(tab, accepted.viewport) {
                return false;
            }
        }
        self.resolve_source_block_path(tab, &mut accepted);
        self.push_accepted_snap(accepted);
        true
    }

    /// Resolve the instance path once, when a feature is accepted. Failure
    /// drops source identity so a failed descent cannot attach to the insert's
    /// origin. Ambiguous intersections within one batched block remain unlinked.
    fn resolve_source_block_path(&self, tab: usize, accepted: &mut AcceptedSnap) {
        let resolve = |source: &mut crate::scene::viewport_ref::SnapSourceRef| {
            if !source.block_path.is_empty() {
                return true;
            }
            if !matches!(
                self.tabs[tab]
                    .scene
                    .document
                    .get_entity(source.source.handle),
                Some(acadrust::EntityType::Insert(_))
            ) {
                return true;
            }
            if source.snap_type == SnapType::Insertion {
                let Some(acadrust::EntityType::Insert(insert)) = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(source.source.handle)
                else {
                    return false;
                };
                let origin = glam::DVec3::new(
                    insert.insert_point.x,
                    insert.insert_point.y,
                    insert.insert_point.z,
                );
                return origin.distance(accepted.model_point) < 1e-5;
            }
            let Some((entity, path)) = self.tabs[tab].scene.resolve_measurable_feature(
                source.source.handle,
                accepted.model_point,
                accepted.viewport,
                Some(source.snap_type),
            ) else {
                return false;
            };
            let distance = crate::scene::viewport_dimension_pick::feature_pick_distance(
                &entity,
                accepted.model_point,
                Some(source.snap_type),
            );
            if !distance.is_some_and(|distance| distance < 1e-5) {
                return false;
            }
            source.source =
                crate::command::DimensionAssociationSource::inferred(entity.common().handle);
            source.block_path = path;
            true
        };
        let Some(source) = accepted.source.as_mut() else {
            return;
        };
        if !resolve(source) {
            accepted.source = None;
            return;
        }
        if let Some(other) = source.intersection.as_mut() {
            if !resolve(other)
                || (other.source.handle == source.source.handle
                    && other.block_path == source.block_path)
            {
                source.intersection = None;
            }
        }
    }

    /// Retain a bounded history for point and object picks.
    pub(crate) fn push_accepted_snap(&mut self, accepted: AcceptedSnap) {
        if self.accepted_snaps.len() >= MAX_ACCEPTED_SNAPS {
            self.accepted_snaps.remove(0);
        }
        self.accepted_snaps.push(accepted);
    }
    /// Query visible viewport geometry with a canvas-pixel cursor and sheet point.
    /// Returns paper coordinates and canvas pixels, plus the source viewport frame.
    pub(in crate::app) fn paper_viewport_snap(
        &mut self,
        i: usize,
        cursor_canvas: Point,
        canvas: (f32, f32),
        cursor_paper: glam::DVec3,
    ) -> Option<(SnapResult, ViewportFrame)> {
        if !self.snapper.snap_enabled {
            return None;
        }
        {
            let scene = &self.tabs[i].scene;
            if scene.current_layout == "Model" || scene.active_viewport.is_some() {
                return None;
            }
        }
        if canvas.0 < 1.0 || canvas.1 < 1.0 {
            return None;
        }

        for frame in self.tabs[i]
            .scene
            .viewport_frames_at_paper_point(cursor_paper)
        {
            let handle = frame.viewport;
            let Some((cam, rect)) = self.tabs[i].scene.viewport_edit_frame_for(handle, canvas)
            else {
                continue;
            };
            if rect.width < 1.0 || rect.height < 1.0 {
                continue;
            }
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: rect.width,
                height: rect.height,
            };
            let local = Point::new(cursor_canvas.x - rect.x, cursor_canvas.y - rect.y);
            let view_rot = cam.view_proj_rte(bounds);
            let eye = cam.eye();
            // The cursor in MODEL space, through the frame — exactly the point
            // the viewport camera projects back onto `local`.
            let model_cursor = frame.paper_to_model(cursor_paper);

            let wires = self.tabs[i]
                .scene
                .model_wires_for_viewport_arc(handle, bounds.height);
            let candidates = self.tabs[i].scene.interaction_candidates_near(
                wires,
                model_cursor,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );

            // The grid belongs to the sheet. Translate the construction
            // anchor for perpendicular/tangent snaps into this viewport's
            // model coordinates, then restore the shared snapper state.
            let saved_grid = self.snapper.grid_snap_on;
            let saved_from = self.snapper.from_point;
            self.snapper.grid_snap_on = false;
            self.snapper.from_point =
                saved_from.map(|p| frame.paper_to_model(p.as_dvec3()).as_vec3());
            let hit = self.snapper.snap(
                model_cursor,
                local,
                &candidates,
                view_rot,
                eye,
                bounds,
                glam::Vec3::ZERO,
                (glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z),
                None,
            );
            self.snapper.grid_snap_on = saved_grid;
            self.snapper.from_point = saved_from;

            let Some(hit) = hit.filter(|h| h.snap_type != SnapType::Grid) else {
                continue;
            };
            // The snap aperture can reach outside the visible clip boundary.
            let paper = frame.model_to_paper(hit.world);
            if !self.tabs[i]
                .scene
                .viewport_displays_paper_point(handle, paper.truncate())
            {
                continue;
            }
            return Some((project_hit_to_paper(hit, &frame, rect, handle), frame));
        }
        None
    }
}

/// Re-express a snap taken in a viewport's model space as a paper-space result:
/// the point projects onto the sheet through `frame`, and the pane-local pixel
/// positions shift by the viewport's screen origin so they share the canvas
/// frame with the ordinary paper snap.
fn project_hit_to_paper(
    hit: SnapResult,
    frame: &ViewportFrame,
    rect: iced::Rectangle,
    viewport: Handle,
) -> SnapResult {
    let to_canvas = |p: Point| Point::new(p.x + rect.x, p.y + rect.y);
    SnapResult {
        world: frame.model_to_paper(hit.world),
        screen: to_canvas(hit.screen),
        snap_type: hit.snap_type,
        // Tangent geometry is model-space and would be consumed as paper-space
        // by TTR/TTT; drop it rather than hand over a wrong circle.
        tangent_obj: None,
        extension_base: hit.extension_base.map(to_canvas),
        extension_base2: hit.extension_base2.map(to_canvas),
        extension_origin: hit.extension_origin.map(|p| frame.model_to_paper(p)),
        extension_dir: hit.extension_dir.map(|d| {
            let mapped = frame.model_to_paper_dir(d.truncate());
            glam::DVec3::new(mapped.x, mapped.y, d.z)
        }),
        viewport: Some(viewport),
        source: hit.source,
        secondary_source: hit.secondary_source,
        model_point: Some(hit.world),
    }
}
