use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, CoincidentPick, SymmetricConstraintSelection,
};
use crate::scene::parametric_constraints::{is_parametric_point_near, ParametricRef};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectFamily {
    Line,
    Circular,
    Ellipse,
}

#[derive(Clone, Copy)]
struct ObjectPick {
    reference: ParametricRef,
    family: ObjectFamily,
}

#[derive(Clone, Copy)]
enum Step {
    ObjectOrTwoPoints,
    SecondObject(ObjectPick),
    FirstPoint,
    SecondPoint(CoincidentPick),
    ObjectAxis(ObjectPick, ObjectPick),
    PointAxis(CoincidentPick, CoincidentPick),
}

pub struct SymmetricConstraintCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl SymmetricConstraintCommand {
    pub fn new() -> Self {
        Self {
            step: Step::ObjectOrTwoPoints,
            picked_entity: None,
        }
    }

    fn point_pick(handle: Option<Handle>, point: DVec3) -> CoincidentPick {
        CoincidentPick {
            handle,
            point,
            whole_curve: false,
        }
    }

    fn segment_count(entity: &EntityType) -> Option<usize> {
        match entity {
            EntityType::LwPolyline(polyline) => Some(if polyline.is_closed {
                polyline.vertices.len()
            } else {
                polyline.vertices.len().saturating_sub(1)
            }),
            EntityType::Polyline2D(polyline) => Some(if polyline.is_closed() {
                polyline.vertices.len()
            } else {
                polyline.vertices.len().saturating_sub(1)
            }),
            _ => None,
        }
    }

    fn segment_family(entity: &EntityType, index: usize) -> Option<ObjectFamily> {
        let bulge = match entity {
            EntityType::LwPolyline(polyline) => polyline.vertices.get(index)?.bulge,
            EntityType::Polyline2D(polyline) => polyline.vertices.get(index)?.bulge,
            _ => return None,
        };
        Some(if bulge.abs() <= 1.0e-12 {
            ObjectFamily::Line
        } else {
            ObjectFamily::Circular
        })
    }

    fn picked_segment(entity: &EntityType, point: DVec3) -> Option<(usize, ObjectFamily)> {
        let planar = crate::entities::curve::entity_curve(entity)?;
        let local = planar.plane.project(point.to_array())?;
        let segments = planar.curve.segments();
        let (index, _) = cadkernel::geom2d::nearest_of(segments.iter(), local)?;
        Some((index, Self::segment_family(entity, index)?))
    }

    fn picked_object(entity: &EntityType, handle: Handle, point: DVec3) -> Option<ObjectPick> {
        let (reference, family) = match entity {
            EntityType::Line(_) => (ParametricRef::whole(handle), ObjectFamily::Line),
            EntityType::Circle(_) | EntityType::Arc(_) => {
                (ParametricRef::whole(handle), ObjectFamily::Circular)
            }
            EntityType::Ellipse(_) => (ParametricRef::whole(handle), ObjectFamily::Ellipse),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (index, family) = Self::picked_segment(entity, point)?;
                (ParametricRef::segment(handle, index), family)
            }
            _ => return None,
        };
        Some(ObjectPick { reference, family })
    }

    fn preselected_object(entity: &EntityType, handle: Handle) -> Option<ObjectPick> {
        match entity {
            EntityType::Line(_) => Some(ObjectPick {
                reference: ParametricRef::whole(handle),
                family: ObjectFamily::Line,
            }),
            EntityType::Circle(_) | EntityType::Arc(_) => Some(ObjectPick {
                reference: ParametricRef::whole(handle),
                family: ObjectFamily::Circular,
            }),
            EntityType::Ellipse(_) => Some(ObjectPick {
                reference: ParametricRef::whole(handle),
                family: ObjectFamily::Ellipse,
            }),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_)
                if Self::segment_count(entity) == Some(1) =>
            {
                Some(ObjectPick {
                    reference: ParametricRef::segment(handle, 0),
                    family: Self::segment_family(entity, 0)?,
                })
            }
            _ => None,
        }
    }

    fn picked_axis(entity: &EntityType, handle: Handle, point: DVec3) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_) => Some(ParametricRef::whole(handle)),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (index, family) = Self::picked_segment(entity, point)?;
                (family == ObjectFamily::Line).then_some(ParametricRef::segment(handle, index))
            }
            _ => None,
        }
    }

    fn preselected_axis(entity: &EntityType, handle: Handle) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_) => Some(ParametricRef::whole(handle)),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_)
                if Self::segment_count(entity) == Some(1)
                    && Self::segment_family(entity, 0) == Some(ObjectFamily::Line) =>
            {
                Some(ParametricRef::segment(handle, 0))
            }
            _ => None,
        }
    }

    pub fn preselected_refs(
        first: (&EntityType, Handle),
        second: (&EntityType, Handle),
        axis: (&EntityType, Handle),
    ) -> Option<[ParametricRef; 3]> {
        let first = Self::preselected_object(first.0, first.1)?;
        let second = Self::preselected_object(second.0, second.1)?;
        let axis = Self::preselected_axis(axis.0, axis.1)?;
        (first.family == second.family
            && first.reference != second.reference
            && axis.entity != first.reference.entity
            && axis.entity != second.reference.entity)
            .then_some([first.reference, second.reference, axis])
    }

    fn invalid_object() -> CmdResult {
        CmdResult::ReportError(
            "Symmetric: select two compatible lines, circular curves, polyline segments, or ellipses."
                .to_string(),
        )
    }

    fn invalid_point() -> CmdResult {
        CmdResult::ReportError(
            "Symmetric: select an endpoint, center, midpoint, or polyline vertex.".to_string(),
        )
    }

    fn invalid_axis() -> CmdResult {
        CmdResult::ReportError("Symmetric: select a line as the symmetry axis.".to_string())
    }
}

impl CadCommand for SymmetricConstraintCommand {
    fn name(&self) -> &'static str {
        "SYCONSTRAINT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::ObjectOrTwoPoints => {
                "Select first object or [2Points] <2Points>:".to_string()
            }
            Step::SecondObject(_) => "Select second object:".to_string(),
            Step::FirstPoint => "Select first point:".to_string(),
            Step::SecondPoint(_) => "Select second point:".to_string(),
            Step::ObjectAxis(_, _) | Step::PointAxis(_, _) => {
                "Select symmetry line:".to_string()
            }
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
                let Some(first) = Self::picked_object(&entity, handle, point) else {
                    return Self::invalid_object();
                };
                self.step = Step::SecondObject(first);
                CmdResult::NeedPoint
            }
            Step::SecondObject(first) => {
                let Some(second) = Self::picked_object(&entity, handle, point) else {
                    return Self::invalid_object();
                };
                if first.family != second.family || first.reference == second.reference {
                    return Self::invalid_object();
                }
                self.step = Step::ObjectAxis(first, second);
                CmdResult::NeedPoint
            }
            Step::FirstPoint | Step::SecondPoint(_) => {
                let world = acadrust::types::Vector3::new(point.x, point.y, point.z);
                if !is_parametric_point_near(&entity, world) {
                    return Self::invalid_point();
                }
                let picked = Self::point_pick(Some(handle), point);
                match self.step {
                    Step::FirstPoint => {
                        self.step = Step::SecondPoint(picked);
                        CmdResult::NeedPoint
                    }
                    Step::SecondPoint(first) => {
                        self.step = Step::PointAxis(first, picked);
                        CmdResult::NeedPoint
                    }
                    _ => unreachable!(),
                }
            }
            Step::ObjectAxis(first, second) => {
                let Some(axis) = Self::picked_axis(&entity, handle, point) else {
                    return Self::invalid_axis();
                };
                if axis.entity == first.reference.entity || axis.entity == second.reference.entity {
                    return Self::invalid_axis();
                }
                CmdResult::AddSymmetricConstraint {
                    selection: SymmetricConstraintSelection::Objects(
                        first.reference,
                        second.reference,
                    ),
                    axis,
                    label: "Symmetric constraint",
                }
            }
            Step::PointAxis(first, second) => {
                let Some(axis) = Self::picked_axis(&entity, handle, point) else {
                    return Self::invalid_axis();
                };
                if first.handle == Some(axis.entity) || second.handle == Some(axis.entity) {
                    return Self::invalid_axis();
                }
                CmdResult::AddSymmetricConstraint {
                    selection: SymmetricConstraintSelection::Points(first, second),
                    axis,
                    label: "Symmetric constraint",
                }
            }
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let picked = Self::point_pick(None, point);
        match self.step {
            Step::ObjectOrTwoPoints | Step::FirstPoint => {
                self.step = Step::SecondPoint(picked);
                CmdResult::NeedPoint
            }
            Step::SecondPoint(first) => {
                self.step = Step::PointAxis(first, picked);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
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

inventory::submit!(crate::command::CommandRegistration {
    names: &["SYCONSTRAINT"]
});
