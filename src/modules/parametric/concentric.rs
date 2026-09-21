use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::parametric_constraints::ParametricRef;
use crate::t;

pub struct ConcentricConstraintCommand {
    first: Option<ParametricRef>,
    picked_entity: Option<EntityType>,
}

impl ConcentricConstraintCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            picked_entity: None,
        }
    }

    pub fn with_first(first: ParametricRef) -> Self {
        Self {
            first: Some(first),
            picked_entity: None,
        }
    }

    fn invalid_selection() -> CmdResult {
        CmdResult::ReportError(
            t!("Invalid selection for Concentric. Select a circle, arc, ellipse or polyline arc segment.")
                .into_owned(),
        )
    }

    fn polyline_segment_is_arc(entity: &EntityType, index: usize) -> bool {
        match entity {
            EntityType::LwPolyline(polyline) => polyline
                .vertices
                .get(index)
                .is_some_and(|vertex| vertex.bulge.abs() > 1.0e-12),
            EntityType::Polyline2D(polyline) => polyline
                .vertices
                .get(index)
                .is_some_and(|vertex| vertex.bulge.abs() > 1.0e-12),
            _ => false,
        }
    }

    fn picked_reference(entity: &EntityType, handle: Handle, point: DVec3) -> Option<ParametricRef> {
        match entity {
            EntityType::Circle(_) | EntityType::Arc(_) | EntityType::Ellipse(_) => {
                Some(ParametricRef::center(handle))
            }
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let planar = crate::entities::curve::entity_curve(entity)?;
                let local = planar.plane.project(point.to_array())?;
                let segments = planar.curve.segments();
                let (index, _) = cadkernel::geom2d::nearest_of(segments.iter(), local)?;
                matches!(segments.get(index), Some(cadkernel::geom2d::Curve::Arc(_)))
                    .then(|| ParametricRef::segment_center(handle, index))
            }
            _ => None,
        }
    }

    pub fn preselected_reference(entity: &EntityType, handle: Handle) -> Option<ParametricRef> {
        match entity {
            EntityType::Circle(_) | EntityType::Arc(_) | EntityType::Ellipse(_) => {
                Some(ParametricRef::center(handle))
            }
            EntityType::LwPolyline(polyline) => {
                let segment_count = if polyline.is_closed {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (segment_count == 1 && Self::polyline_segment_is_arc(entity, 0))
                    .then(|| ParametricRef::segment_center(handle, 0))
            }
            EntityType::Polyline2D(polyline) => {
                let segment_count = if polyline.is_closed() {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (segment_count == 1 && Self::polyline_segment_is_arc(entity, 0))
                    .then(|| ParametricRef::segment_center(handle, 0))
            }
            _ => None,
        }
    }
}

impl CadCommand for ConcentricConstraintCommand {
    fn name(&self) -> &'static str {
        "GCCONCENTRIC"
    }

    fn prompt(&self) -> String {
        if self.first.is_some() {
            t!("GCCONCENTRIC  Select second object:").into_owned()
        } else {
            t!("GCCONCENTRIC  Select first object:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(reference) = Self::picked_reference(&entity, handle, point) else {
            return Self::invalid_selection();
        };
        let Some(first) = self.first else {
            self.first = Some(reference);
            return CmdResult::NeedPoint;
        };
        if first == reference {
            return Self::invalid_selection();
        }
        CmdResult::AddConcentricConstraint {
            first,
            second: reference,
            label: "Concentric constraint",
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyline_pick_keeps_the_selected_arc_center() {
        let handle = Handle::new(13);
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.vertices = vec![
            acadrust::entities::LwVertex::from_coords(0.0, 0.0),
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(5.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
        ];

        let reference = ConcentricConstraintCommand::picked_reference(
            &EntityType::LwPolyline(polyline),
            handle,
            DVec3::new(8.0, 2.0, 0.0),
        )
        .expect("arc segment");
        assert_eq!(reference, ParametricRef::segment_center(handle, 1));
    }
}
