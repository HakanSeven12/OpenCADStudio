use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, CoincidentPick, HorizontalConstraintSelection, WorkingPlane,
};
use crate::scene::parametric_constraints::{
    directional_axis_endpoints, is_parametric_point_near, ConstraintKind, ParametricRef,
};

#[derive(Clone, Copy)]
enum Step {
    ObjectOrTwoPoints,
    FirstPoint,
    SecondPoint(CoincidentPick),
}

/// Interactive front end for the Horizontal and Vertical geometric
/// constraints — one flow, mirrored across the working plane's X or Y axis.
pub struct HorizontalConstraintCommand {
    kind: ConstraintKind,
    step: Step,
    picked_entity: Option<EntityType>,
    plane: WorkingPlane,
}

impl HorizontalConstraintCommand {
    pub fn new() -> Self {
        Self::for_kind(ConstraintKind::Horizontal)
    }

    pub fn vertical() -> Self {
        Self::for_kind(ConstraintKind::Vertical)
    }

    fn for_kind(kind: ConstraintKind) -> Self {
        Self {
            kind,
            step: Step::ObjectOrTwoPoints,
            picked_entity: None,
            plane: WorkingPlane::default(),
        }
    }

    /// Re-enter the 2Points flow after the host rejected a pick: back at
    /// the first point, or at the second with `first` kept.
    pub fn resume(kind: ConstraintKind, first: Option<CoincidentPick>) -> Self {
        let mut command = Self::for_kind(kind);
        command.step = match first {
            Some(first) => Step::SecondPoint(first),
            None => Step::FirstPoint,
        };
        command
    }

    pub fn kind(&self) -> ConstraintKind {
        self.kind
    }

    fn axis(&self) -> &'static str {
        match self.kind {
            ConstraintKind::Vertical => "Vertical",
            _ => "Horizontal",
        }
    }

    fn label(&self) -> &'static str {
        match self.kind {
            ConstraintKind::Vertical => "Vertical constraint",
            _ => "Horizontal constraint",
        }
    }

    fn point_pick(handle: Option<Handle>, point: DVec3) -> CoincidentPick {
        CoincidentPick {
            handle,
            point,
            whole_curve: false,
        }
    }

    fn direction(&self) -> acadrust::types::Vector3 {
        let direction = match self.kind {
            ConstraintKind::Vertical => self.plane.y.normalize_or(DVec3::Y),
            _ => self.plane.x.normalize_or(DVec3::X),
        };
        acadrust::types::Vector3::new(direction.x, direction.y, direction.z)
    }

    fn invalid_object(&self) -> CmdResult {
        CmdResult::ReportError(format!(
            "Invalid selection for {}. Select a line segment, polyline segment, text, MText, major or minor axis of ellipse or elliptical arc.",
            self.axis()
        ))
    }

    fn invalid_point() -> CmdResult {
        CmdResult::ReportError("No valid constraint point found.".to_string())
    }

    pub(crate) fn segment_is_straight(entity: &EntityType, index: usize) -> bool {
        match entity {
            EntityType::LwPolyline(polyline) => polyline
                .vertices
                .get(index)
                .is_some_and(|vertex| vertex.bulge.abs() <= 1.0e-12),
            EntityType::Polyline2D(polyline) => polyline
                .vertices
                .get(index)
                .is_some_and(|vertex| vertex.bulge.abs() <= 1.0e-12),
            _ => false,
        }
    }

    fn distance_to_axis(point: DVec3, endpoints: [acadrust::types::Vector3; 2]) -> f64 {
        cadkernel::space::Vec3::from(point.to_array())
            .distance_to_line(
                cadkernel::space::Vec3::new(endpoints[0].x, endpoints[0].y, endpoints[0].z),
                cadkernel::space::Vec3::new(endpoints[1].x, endpoints[1].y, endpoints[1].z),
            )
            .unwrap_or(f64::INFINITY)
    }

    fn ellipse_reference(
        entity: &EntityType,
        handle: Handle,
        point: DVec3,
    ) -> Option<ParametricRef> {
        let major = ParametricRef::ellipse_major_axis(handle);
        let minor = ParametricRef::ellipse_minor_axis(handle);
        let major_distance =
            Self::distance_to_axis(point, directional_axis_endpoints(entity, major)?);
        let minor_distance =
            Self::distance_to_axis(point, directional_axis_endpoints(entity, minor)?);
        Some(if minor_distance < major_distance {
            minor
        } else {
            major
        })
    }

    fn picked_reference(
        entity: &EntityType,
        handle: Handle,
        point: DVec3,
    ) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_) | EntityType::Ray(_) | EntityType::XLine(_) => {
                Some(ParametricRef::whole(handle))
            }
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (source, _, _) =
                    crate::scene::centerline::picked_source(entity, handle, point)?;
                let index = usize::try_from(source.segment_index).ok()?;
                Self::segment_is_straight(entity, index)
                    .then_some(ParametricRef::segment(handle, index))
            }
            EntityType::Text(_) | EntityType::MText(_) => {
                Some(ParametricRef::text_baseline(handle))
            }
            EntityType::Ellipse(_) => Self::ellipse_reference(entity, handle, point),
            _ => None,
        }
    }

    pub fn preselected_reference(entity: &EntityType, handle: Handle) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_) | EntityType::Ray(_) | EntityType::XLine(_) => {
                Some(ParametricRef::whole(handle))
            }
            EntityType::Text(_) | EntityType::MText(_) => {
                Some(ParametricRef::text_baseline(handle))
            }
            EntityType::Ellipse(_) => Some(ParametricRef::ellipse_major_axis(handle)),
            EntityType::LwPolyline(polyline) => {
                let count = if polyline.is_closed {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (count == 1 && Self::segment_is_straight(entity, 0))
                    .then_some(ParametricRef::segment(handle, 0))
            }
            EntityType::Polyline2D(polyline) => {
                let count = if polyline.is_closed() {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (count == 1 && Self::segment_is_straight(entity, 0))
                    .then_some(ParametricRef::segment(handle, 0))
            }
            _ => None,
        }
    }

    fn finish_points(&mut self, first: CoincidentPick, second: CoincidentPick) -> CmdResult {
        // The reference refuses the same pick twice and asks again for the
        // second point (the host repeats this check on the resolved refs).
        if first.handle == second.handle && (first.point - second.point).length() <= 1.0e-9 {
            self.step = Step::SecondPoint(first);
            return CmdResult::ReportError(
                "The object or point is already selected. Select a different object or constraint point."
                    .to_string(),
            );
        }
        CmdResult::AddHorizontalConstraint {
            kind: self.kind,
            selection: HorizontalConstraintSelection::Points(first, second),
            direction: self.direction(),
            label: self.label(),
        }
    }
}

impl CadCommand for HorizontalConstraintCommand {
    fn name(&self) -> &'static str {
        match self.kind {
            ConstraintKind::Vertical => "GCVERTICAL",
            _ => "GCHORIZONTAL",
        }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::ObjectOrTwoPoints => {
                format!("{}  Select an object or [2Points] <2Points>:", self.name())
            }
            Step::FirstPoint => format!("{}  Select first point:", self.name()),
            Step::SecondPoint(_) => format!("{}  Select second point:", self.name()),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        matches!(self.step, Step::ObjectOrTwoPoints)
            .then(|| vec![CmdOption::new("2Points", "2P")])
            .unwrap_or_default()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::ObjectOrTwoPoints)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if matches!(self.step, Step::ObjectOrTwoPoints)
            && matches!(keyword.as_str(), "2" | "2P" | "2POINT" | "2POINTS")
        {
            self.step = Step::FirstPoint;
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_accepts_points(&self) -> bool {
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
            return self.on_point(point);
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        match self.step {
            Step::ObjectOrTwoPoints => {
                let Some(reference) = Self::picked_reference(&entity, handle, point) else {
                    return self.invalid_object();
                };
                CmdResult::AddHorizontalConstraint {
                    kind: self.kind,
                    selection: HorizontalConstraintSelection::Reference(reference),
                    direction: self.direction(),
                    label: self.label(),
                }
            }
            Step::FirstPoint | Step::SecondPoint(_) => {
                let world = acadrust::types::Vector3::new(point.x, point.y, point.z);
                if !is_parametric_point_near(&entity, world) {
                    return Self::invalid_point();
                }
                let pick = Self::point_pick(Some(handle), point);
                match self.step {
                    Step::FirstPoint => {
                        self.step = Step::SecondPoint(pick);
                        CmdResult::NeedPoint
                    }
                    Step::SecondPoint(first) => self.finish_points(first, pick),
                    Step::ObjectOrTwoPoints => unreachable!(),
                }
            }
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let pick = Self::point_pick(None, point);
        match self.step {
            // The host resolves the pick against the document: a hit resumes
            // at the second point, a miss re-asks for the first one at once.
            Step::ObjectOrTwoPoints | Step::FirstPoint => CmdResult::CheckHorizontalPoint {
                kind: self.kind,
                pick,
            },
            Step::SecondPoint(first) => self.finish_points(first, pick),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.step, Step::ObjectOrTwoPoints) {
            self.step = Step::FirstPoint;
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;

    #[test]
    fn object_pick_captures_the_working_x_direction() {
        let handle = Handle::new(7);
        let mut command = HorizontalConstraintCommand::new();
        command.set_working_plane(WorkingPlane::new(DVec3::ZERO, DVec3::Y, DVec3::NEG_X));
        command.inject_picked_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(2.0, 3.0, 0.0),
        )));

        let CmdResult::AddHorizontalConstraint {
            selection: HorizontalConstraintSelection::Reference(reference),
            direction,
            ..
        } = command.on_entity_pick(handle, DVec3::ZERO)
        else {
            panic!("object pick must create the constraint");
        };
        assert_eq!(reference, ParametricRef::whole(handle));
        assert!((direction - Vector3::UNIT_Y).length() < 1.0e-12);
    }

    #[test]
    fn enter_starts_the_two_point_flow() {
        let mut command = HorizontalConstraintCommand::new();
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        let CmdResult::CheckHorizontalPoint { kind, pick } =
            command.on_point(DVec3::new(1.0, 2.0, 0.0))
        else {
            panic!("the host must check the first point");
        };
        let mut command = HorizontalConstraintCommand::resume(kind, Some(pick));
        let CmdResult::AddHorizontalConstraint {
            selection: HorizontalConstraintSelection::Points(first, second),
            direction,
            ..
        } = command.on_point(DVec3::new(4.0, 5.0, 0.0))
        else {
            panic!("second point must create the constraint");
        };
        assert_eq!(first.point, DVec3::new(1.0, 2.0, 0.0));
        assert_eq!(second.point, DVec3::new(4.0, 5.0, 0.0));
        assert!((direction - Vector3::UNIT_X).length() < 1.0e-12);
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GCHORIZONTAL", "GCVERTICAL"]
});
