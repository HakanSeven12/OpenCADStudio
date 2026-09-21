use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
use crate::t;

#[derive(Clone, Copy)]
struct SmoothTarget {
    endpoint: ParametricRef,
    curve: ParametricRef,
}

pub struct SmoothConstraintCommand {
    source: Option<ParametricRef>,
    picked_entity: Option<EntityType>,
}

impl SmoothConstraintCommand {
    pub fn new() -> Self {
        Self {
            source: None,
            picked_entity: None,
        }
    }

    fn point(point: acadrust::types::Vector3) -> DVec3 {
        DVec3::new(point.x, point.y, point.z)
    }

    fn nearest_marker(points: &[acadrust::types::Vector3], point: DVec3) -> Option<i32> {
        points
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                (Self::point(**left) - point)
                    .length_squared()
                    .total_cmp(&(Self::point(**right) - point).length_squared())
            })
            .map(|(index, _)| index as i32)
    }

    fn source_reference(entity: &EntityType, handle: Handle, point: DVec3) -> Option<ParametricRef> {
        let EntityType::Spline(spline) = entity else {
            return None;
        };
        if spline.flags.closed || spline.flags.periodic {
            return None;
        }
        let points = crate::scene::dimension_assoc::source_points(entity);
        let endpoints = [*points.first()?, *points.last()?];
        let marker = Self::nearest_marker(&endpoints, point)?;
        Some(ParametricRef::point(handle, marker))
    }

    fn target_reference(entity: &EntityType, handle: Handle, point: DVec3) -> Option<SmoothTarget> {
        match entity {
            EntityType::Line(_) | EntityType::Arc(_) => {
                let points = crate::scene::dimension_assoc::source_points(entity);
                let endpoints = [*points.first()?, *points.last()?];
                let marker = Self::nearest_marker(&endpoints, point)?;
                Some(SmoothTarget {
                    endpoint: ParametricRef::point(handle, marker),
                    curve: ParametricRef::whole(handle),
                })
            }
            EntityType::Spline(spline) if !spline.flags.closed && !spline.flags.periodic => {
                let points = crate::scene::dimension_assoc::source_points(entity);
                let endpoints = [*points.first()?, *points.last()?];
                let marker = Self::nearest_marker(&endpoints, point)?;
                Some(SmoothTarget {
                    endpoint: ParametricRef::point(handle, marker),
                    curve: ParametricRef::whole(handle),
                })
            }
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let planar = crate::entities::curve::entity_curve(entity)?;
                let local = planar.plane.project(point.to_array())?;
                let segments = planar.curve.segments();
                let (segment, _) = cadkernel::geom2d::nearest_of(segments.iter(), local)?;
                let points = crate::scene::dimension_assoc::source_points(entity);
                let closed = match entity {
                    EntityType::LwPolyline(polyline) => polyline.is_closed,
                    EntityType::Polyline2D(polyline) => polyline.is_closed(),
                    _ => false,
                };
                let end = if segment + 1 < points.len() {
                    segment + 1
                } else if closed {
                    0
                } else {
                    return None;
                };
                let candidates = [points.get(segment).copied()?, points.get(end).copied()?];
                let side = Self::nearest_marker(&candidates, point)? as usize;
                let vertex = if side == 0 { segment } else { end };
                Some(SmoothTarget {
                    endpoint: ParametricRef::point(handle, vertex as i32),
                    curve: ParametricRef::segment(handle, segment),
                })
            }
            _ => None,
        }
    }

    pub fn preselected_refs(
        first: (&EntityType, Handle),
        second: (&EntityType, Handle),
    ) -> Option<Vec<ParametricRef>> {
        let source_points = crate::scene::dimension_assoc::source_points(first.0);
        let source_ends = [*source_points.first()?, *source_points.last()?];
        let source_point = source_ends
            .iter()
            .copied()
            .min_by(|left, right| {
                let target_points = crate::scene::dimension_assoc::source_points(second.0);
                let distance = |source: acadrust::types::Vector3| {
                    target_points
                        .iter()
                        .map(|target| (source - *target).length_squared())
                        .fold(f64::INFINITY, f64::min)
                };
                distance(*left).total_cmp(&distance(*right))
            })?;
        let source = Self::source_reference(first.0, first.1, Self::point(source_point))?;

        let target_points = crate::scene::dimension_assoc::source_points(second.0);
        let target_point = target_points
            .iter()
            .copied()
            .min_by(|left, right| {
                (Self::point(*left) - Self::point(source_point))
                    .length_squared()
                    .total_cmp(
                        &(Self::point(*right) - Self::point(source_point)).length_squared(),
                    )
            })?;
        let target = Self::target_reference(second.0, second.1, Self::point(target_point))?;
        let mut refs = vec![source, target.endpoint];
        if target.curve.marker.is_some() {
            refs.push(target.curve);
        }
        Some(refs)
    }

    fn invalid_source() -> CmdResult {
        CmdResult::ReportError(
            t!("Invalid first selection for Smooth. Select an endpoint of an open spline.")
                .into_owned(),
        )
    }

    fn invalid_target() -> CmdResult {
        CmdResult::ReportError(
            t!("Invalid second selection for Smooth. Select an endpoint of a line, arc, polyline segment or open spline.")
                .into_owned(),
        )
    }
}

impl CadCommand for SmoothConstraintCommand {
    fn name(&self) -> &'static str {
        "GCSMOOTH"
    }

    fn prompt(&self) -> String {
        if self.source.is_some() {
            t!("GCSMOOTH  Select second curve endpoint:").into_owned()
        } else {
            t!("GCSMOOTH  Select first spline curve endpoint:").into_owned()
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
        let Some(source) = self.source else {
            let Some(source) = Self::source_reference(&entity, handle, point) else {
                return Self::invalid_source();
            };
            self.source = Some(source);
            return CmdResult::NeedPoint;
        };
        let Some(target) = Self::target_reference(&entity, handle, point) else {
            return Self::invalid_target();
        };
        if source.entity == target.endpoint.entity {
            return Self::invalid_target();
        }
        let mut refs = vec![source, target.endpoint];
        if target.curve.marker.is_some() {
            refs.push(target.curve);
        }
        CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Smooth,
            refs,
            driving_param: None,
            label: "Smooth constraint",
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
    fn polyline_target_keeps_the_picked_arc_segment_and_endpoint() {
        let handle = Handle::new(7);
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.vertices = vec![
            acadrust::entities::LwVertex::from_coords(0.0, 0.0),
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(5.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
        ];

        let target = SmoothConstraintCommand::target_reference(
            &EntityType::LwPolyline(polyline),
            handle,
            DVec3::new(10.0, 0.0, 0.0),
        )
        .expect("arc endpoint");
        assert_eq!(target.curve, ParametricRef::segment(handle, 1));
        assert_eq!(target.endpoint, ParametricRef::point(handle, 2));
    }
}
