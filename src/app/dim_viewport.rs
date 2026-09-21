//! Paper-space dimension acquisition and measurement. Acquiring model geometry
//! supplies a viewport scale; sheet-only dimensions retain paper units.

use super::OpenCADStudio;
use crate::command::{DimensionAssociationSource, DimensionPreview};
use crate::entities::dim_override;
use crate::scene::viewport_ref::{AcceptedSnap, MeasurementScale, SnapSourceRef, ViewportFrame};
use acadrust::entities::Dimension;
use acadrust::types::Handle;
use acadrust::EntityType;
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DimensionMeasureSpace {
    Direct,
    Viewport(ViewportFrame),
    ConflictingViewports,
}

impl OpenCADStudio {
    pub(crate) fn finish_command_click(&mut self, i: usize) {
        let mut selection = self.tabs[i].scene.selection.borrow_mut();
        selection.left_down = false;
        selection.left_press_pos = None;
        selection.left_press_time = None;
        selection.left_dragging = false;
    }

    pub(crate) fn dimension_measure_space(&self, i: usize) -> DimensionMeasureSpace {
        self.dimension_measure_space_with_candidate(i, None)
    }

    fn dimension_measure_space_with_candidate(
        &self,
        i: usize,
        candidate: Option<ViewportFrame>,
    ) -> DimensionMeasureSpace {
        let scene = &self.tabs[i].scene;
        if scene.current_layout == "Model" || scene.active_viewport.is_some() {
            return DimensionMeasureSpace::Direct;
        }
        let mut frame: Option<ViewportFrame> = None;
        for f in self
            .accepted_snaps()
            .iter()
            .filter_map(|snap| snap.frame)
            .chain(candidate)
        {
            match frame {
                None => frame = Some(f),
                Some(previous) if previous.viewport == f.viewport => {}
                Some(_) => return DimensionMeasureSpace::ConflictingViewports,
            }
        }
        frame.map_or(
            DimensionMeasureSpace::Direct,
            DimensionMeasureSpace::Viewport,
        )
    }

    /// Reject an incompatible measuring input before it advances the command.
    pub(crate) fn dimension_acquisition_allowed(
        &mut self,
        i: usize,
        viewport: Option<Handle>,
    ) -> bool {
        if !self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.measures_through_viewports())
        {
            return true;
        }
        // Sheet geometry and free sheet points may accompany one viewport.
        // A second viewport would make the measurement scale ambiguous.
        if viewport.is_some_and(|viewport| {
            self.accepted_snaps()
                .iter()
                .filter_map(|s| s.viewport)
                .any(|previous| previous != viewport)
        }) {
            self.command_line.push_error(crate::t!("Model geometry must come from one viewport. Paper-space points can be included.").as_ref());
            return false;
        }
        true
    }

    /// A command can reject a degenerate point without advancing its step.
    pub(crate) fn sync_dimension_snaps(&mut self, i: usize) {
        if let Some(cmd) = self.tabs[i]
            .active_cmd
            .as_ref()
            .filter(|c| c.measures_through_viewports())
        {
            self.accepted_snaps
                .truncate(cmd.dimension_acquired_points().len());
        }
    }

    /// Record each definition point actually acquired by an object pick.
    /// Arcs may supply three angular slots, lines two, and radial picks one.
    pub(crate) fn record_dimension_entity_points(
        &mut self,
        i: usize,
        frame: Option<ViewportFrame>,
        entity: Handle,
        block_path: Vec<Handle>,
    ) {
        let Some(cmd) = self.tabs[i]
            .active_cmd
            .as_ref()
            .filter(|c| c.measures_through_viewports())
        else {
            return;
        };
        let points = cmd.dimension_acquired_points();
        for paper_point in points.into_iter().skip(self.accepted_snaps.len()) {
            self.push_accepted_snap(AcceptedSnap {
                paper_point,
                model_point: frame.map_or(paper_point, |f| f.paper_to_model(paper_point)),
                viewport: frame.map(|f| f.viewport),
                frame,
                source: Some(SnapSourceRef {
                    source: DimensionAssociationSource::inferred(entity),
                    block_path: block_path.clone(),
                    snap_type: crate::snap::SnapType::ObjectPick,
                    intersection: None,
                }),
            });
        }
    }

    pub(crate) fn apply_viewport_dimension_measurement(
        &mut self,
        i: usize,
        entity: &mut EntityType,
    ) -> bool {
        if !matches!(entity, EntityType::Dimension(_)) {
            return true;
        }
        let angular = is_angular_dimension(entity);
        let frame = match self.dimension_measure_space(i) {
            DimensionMeasureSpace::Direct => return true,
            DimensionMeasureSpace::ConflictingViewports => {
                self.command_line.push_error(crate::t!("Model geometry must come from one viewport. Paper-space points can be included.").as_ref());
                return false;
            }
            DimensionMeasureSpace::Viewport(f) => f,
        };
        if angular {
            return true;
        }
        let Some(scale) = self.viewport_dimension_scale(i, entity, frame) else {
            return false;
        };
        // Owner is needed even by DIMASSOC=0's explode path, before insertion.
        entity.common_mut().owner_handle = self.tabs[i].scene.current_layout_block_handle_pub();
        scale.write_to_entity(entity);
        true
    }

    fn viewport_dimension_scale(
        &self,
        i: usize,
        entity: &EntityType,
        frame: ViewportFrame,
    ) -> Option<MeasurementScale> {
        let dimlfac = self.pending_dimension_style_dimlfac(i, entity);
        let scale = MeasurementScale {
            user_lfac: MeasurementScale::user_lfac_for_space(dimlfac, true),
            viewport_compensation: frame.paper_to_model_length_factor(),
        };
        (scale.paper_factor().is_finite() && scale.paper_factor() > 0.0).then_some(scale)
    }

    /// Styled dimensions for the placement preview at `cursor`, carrying the
    /// same style and viewport measurement the commit path would apply.
    /// `None` when the active command has no committable dimension at this
    /// stage; an empty list when the candidate geometry or scale is invalid.
    pub(crate) fn dimension_preview_entities(
        &self,
        i: usize,
        cursor: DVec3,
    ) -> Option<Vec<EntityType>> {
        let command = self.tabs[i].active_cmd.as_ref()?;
        let previews = command.dimension_preview(cursor)?;
        let scene = &self.tabs[i].scene;
        let measure_space = if command.measures_through_viewports() {
            // The hovered model snap supplies a provisional viewport only while
            // the second definition point is still being acquired.
            let candidate = if command.dimension_placement_pending() {
                None
            } else {
                self.vp_snap_frame.filter(|frame| {
                    self.tabs[i]
                        .snap_result
                        .is_some_and(|hit| hit.viewport == Some(frame.viewport))
                })
            };
            self.dimension_measure_space_with_candidate(i, candidate)
        } else {
            DimensionMeasureSpace::Direct
        };
        if matches!(measure_space, DimensionMeasureSpace::ConflictingViewports) {
            return Some(Vec::new());
        }
        let mut entities = Vec::with_capacity(previews.len());
        for DimensionPreview {
            mut entity,
            preserve_base_style,
        } in previews
        {
            if !preserve_base_style {
                crate::scene::creation_style::apply_current_creation_styles(
                    &scene.document,
                    &mut entity,
                );
            }
            entity.common_mut().owner_handle = scene.current_layout_block_handle_pub();
            if let DimensionMeasureSpace::Viewport(frame) = measure_space {
                if !is_angular_dimension(&entity) {
                    let Some(scale) = self.viewport_dimension_scale(i, &entity, frame) else {
                        return Some(Vec::new());
                    };
                    scale.write_to_entity(&mut entity);
                }
            }
            entities.push(entity);
        }
        Some(entities)
    }

    pub(crate) fn dimension_preview_wires(
        &self,
        i: usize,
        cursor: DVec3,
    ) -> Option<Vec<crate::scene::WireModel>> {
        let entities = self.dimension_preview_entities(i, cursor)?;
        // Degenerate geometry, an invalid scale, or conflicting viewports
        // produce no styled dimension. Returning None lets the command's
        // plain rubber-band preview run, which shows no value to mistrust.
        if entities.is_empty() {
            return None;
        }
        let mut wires = self.tabs[i].scene.wires_for_entities(&entities);
        for wire in &mut wires {
            wire.color = crate::scene::WireModel::CYAN;
            for vertex in &mut wire.text_verts {
                vertex.color = crate::scene::WireModel::CYAN;
            }
        }
        Some(wires)
    }

    pub(crate) fn try_dimension_viewport_entity_pick(
        &mut self,
        i: usize,
        paper: DVec3,
        aperture_paper: f64,
    ) -> Option<crate::command::CmdResult> {
        let pick = self.tabs[i]
            .scene
            .dimension_pick_through_viewport(paper, aperture_paper)?;
        if !self.dimension_acquisition_allowed(i, Some(pick.frame.viewport)) {
            return Some(crate::command::CmdResult::NeedPoint);
        }
        let command = self.tabs[i].active_cmd.as_mut()?;
        command.inject_picked_entity(pick.paper_entity);
        let result = command.on_entity_pick(pick.entity_handle, pick.paper_point);
        self.record_dimension_entity_points(
            i,
            Some(pick.frame),
            pick.entity_handle,
            pick.block_path,
        );
        Some(result)
    }

    fn pending_dimension_style_dimlfac(&self, i: usize, entity: &EntityType) -> f64 {
        let EntityType::Dimension(dimension) = entity else {
            return 1.0;
        };
        dim_override::real(
            &dimension.base().common.extended_data,
            dim_override::DIMLFAC,
        )
        .or_else(|| {
            self.tabs[i]
                .scene
                .document
                .dim_styles
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(&dimension.base().style_name))
                .map(|s| s.dimlfac)
        })
        .unwrap_or(1.0)
    }
}

impl OpenCADStudio {
    pub(crate) fn viewport_dimension_snaps(
        &self,
        i: usize,
        dimension: Handle,
    ) -> Vec<Option<AcceptedSnap>> {
        let slots = self.tabs[i]
            .scene
            .dimension_association_slot_points(dimension);
        if slots.is_empty() {
            return Vec::new();
        }
        let measuring = self.accepted_snaps();
        slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let snap = measuring.get(index)?.clone();
                let frame = snap.frame;
                let slot = DVec3::new(slot.x, slot.y, slot.z);
                if snap
                    .paper_point
                    .truncate()
                    .distance_squared(slot.truncate())
                    <= 1e-12
                {
                    return Some(snap);
                }
                Some(AcceptedSnap {
                    paper_point: slot,
                    model_point: frame.map_or(slot, |frame| frame.paper_to_model(slot)),
                    ..snap
                })
            })
            .collect()
    }

    /// Record the viewport association for a dimension that has just been
    /// committed while measuring through a layout viewport.
    ///
    /// The stored reference chains establish dependencies. Only the new
    /// dimension needs a render update; its sources have not changed.
    pub(crate) fn attach_viewport_dimension_association(&mut self, i: usize, dimension: Handle) {
        let snaps = self.viewport_dimension_snaps(i, dimension);
        if snaps.iter().flatten().all(|snap| snap.source.is_none()) {
            return;
        }
        self.tabs[i]
            .scene
            .attach_viewport_dimension_association(dimension, &snaps);
        self.tabs[i]
            .scene
            .bump_entities(&[(dimension, crate::scene::ChangeKind::Modified)]);
    }
}

fn is_angular_dimension(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Dimension(Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_))
    )
}
