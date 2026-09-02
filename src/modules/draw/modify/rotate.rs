// Rotate tool — ribbon definition + interactive command.
//
// Command:  ROTATE (RO)
//   Requires at least one entity selected before starting.
//   Step 1: pick rotation center
//   Step 2: specify a relative rotation angle, or choose Reference
//
//   Reference rotation: the reference angle may be typed or measured between
//   two points. The new absolute angle is then typed or picked from the center;
//   the applied rotation is new-angle - reference-angle.

use acadrust::Handle;
use cadkernel::geom2d::{self, Curve as KernelCurve};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, DynField, EntityTransform, WorkingPlane};
use crate::modules::draw::defaults;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

const TAU: f64 = std::f64::consts::TAU;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "ROTATE",
        label: "Rotate",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/rotate.svg")),
        event: ModuleEvent::Command("ROTATE".to_string()),
    }
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    Center,
    Angle { center: DVec3 },
    RefFirst { center: DVec3 },
    RefSecond { center: DVec3, first: DVec3 },
    RefNew { center: DVec3, ref_angle: f64 },
}

pub struct RotateCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: Step,
    default_angle: f64, // degrees
    plane: WorkingPlane,
}

impl RotateCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: Step::Center,
            default_angle: defaults::get_rotate_angle(),
            plane: WorkingPlane::default(),
        }
    }

    fn commit(&self, center: DVec3, angle_rad: f64) -> CmdResult {
        defaults::set_rotate_angle(angle_rad.to_degrees());
        CmdResult::TransformSelected(
            self.handles.clone(),
            EntityTransform::Rotate {
                center,
                axis: self.plane.z,
                angle_rad,
            },
        )
    }

    fn angle_arc(
        center: [f64; 2],
        radius: f64,
        angle: f64,
    ) -> Option<(geom2d::Arc, bool)> {
        let (start, end, reverse) = if angle > 0.0 {
            (0.0, angle, false)
        } else {
            (angle, 0.0, true)
        };
        geom2d::bounded_arc(center, radius, start, end).map(|arc| (arc, reverse))
    }

    fn angle_guide(&self, center: DVec3, radius: f64, angle: f64) -> Vec<WireModel> {
        if radius <= f64::EPSILON {
            return vec![];
        }

        let center_local = self.plane.to_local(center);
        let reference = self.plane.to_world(DVec3::new(
            center_local.x + radius,
            center_local.y,
            center_local.z,
        ));
        let mut guides = vec![WireModel::solid_f64(
            "rotate_reference".into(),
            vec![center.to_array(), reference.to_array()],
            WireModel::CYAN,
            false,
        )];

        if angle.abs() <= f64::EPSILON {
            return guides;
        }

        let Some((arc, reverse)) = Self::angle_arc(
            [center_local.x, center_local.y],
            radius,
            angle,
        ) else {
            return guides;
        };
        let mut points = KernelCurve::Arc(arc).tessellate_angle(TAU / 64.0);
        if reverse {
            points.reverse();
        }
        guides.push(WireModel::solid_f64(
            "rotate_angle".into(),
            points
                .into_iter()
                .map(|point| {
                    self.plane
                        .to_world(DVec3::new(point[0], point[1], center_local.z))
                        .to_array()
                })
                .collect(),
            WireModel::CYAN,
            false,
        ));
        guides
    }
}

impl CadCommand for RotateCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ROTATE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Center => t!(
                "ROTATE  Specify rotation center  [%{count} objects]:",
                count = self.handles.len()
            )
            .into_owned(),
            Step::Angle { .. } => {
                let a = format!("{:.4}", self.default_angle);
                t!("ROTATE  Specify rotation angle  <%{a}>:", a = a).into_owned()
            }
            Step::RefFirst { .. } => {
                t!("ROTATE  Specify first reference point or type reference angle:").into_owned()
            }
            Step::RefSecond { .. } => {
                t!("ROTATE  Specify second reference point:").into_owned()
            }
            Step::RefNew { ref_angle, .. } => {
                let a = format!("{:.1}°", ref_angle.to_degrees());
                t!("ROTATE  Specify new absolute angle  [ref=%{a}]:", a = a).into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            Step::Angle { .. } => vec![CmdOption::new(t!("Reference").as_ref(), "R")],
            _ => vec![],
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Center => {
                self.step = Step::Angle { center: pt };
                CmdResult::NeedPoint
            }
            Step::Angle { center } => {
                let center = *center;
                let Some(angle_rad) = self.plane.angle(center, pt) else {
                    return CmdResult::NeedPoint;
                };
                self.commit(center, angle_rad)
            }
            Step::RefFirst { center } => {
                let center = *center;
                self.step = Step::RefSecond { center, first: pt };
                CmdResult::NeedPoint
            }
            Step::RefSecond { center, first } => {
                let center = *center;
                let Some(ref_angle) = self.plane.angle(*first, pt) else {
                    return CmdResult::NeedPoint;
                };
                self.step = Step::RefNew { center, ref_angle };
                CmdResult::NeedPoint
            }
            Step::RefNew { center, ref_angle } => {
                let center = *center;
                let Some(new_angle) = self.plane.angle(center, pt) else {
                    return CmdResult::NeedPoint;
                };
                self.commit(center, new_angle - *ref_angle)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // At the normal angle step, Enter uses the stored default angle.
        if let Step::Angle { center } = &self.step {
            let center = *center;
            return self.commit(center, self.default_angle.to_radians());
        }
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            Step::Angle { center } => {
                let center = *center;
                let low = t.to_ascii_lowercase();
                if low == "r" || low == "reference" {
                    self.step = Step::RefFirst { center };
                    return Some(CmdResult::NeedPoint);
                }
                // The value already carries the correct sign when it comes
                // from dynamic input.
                let deg: f64 = t.replace(',', ".").parse().ok()?;
                Some(self.commit(center, deg.to_radians()))
            }
            Step::RefFirst { center } => {
                let center = *center;
                let ref_deg: f64 = t.replace(',', ".").parse().ok()?;
                self.step = Step::RefNew {
                    center,
                    ref_angle: ref_deg.to_radians(),
                };
                Some(CmdResult::NeedPoint)
            }
            Step::RefNew { center, ref_angle } => {
                let (center, ref_angle) = (*center, *ref_angle);
                let new_deg: f64 = t.replace(',', ".").parse().ok()?;
                Some(self.commit(center, new_deg.to_radians() - ref_angle))
            }
            Step::Center | Step::RefSecond { .. } => None,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let (center, angle_rad, guide_angle) = match &self.step {
            Step::Angle { center } => {
                let Some(angle) = self.plane.angle(*center, pt) else {
                    return vec![];
                };
                (*center, angle, angle)
            }
            Step::RefSecond { first, .. } => {
                return vec![WireModel::solid(
                    "rubber_band".into(),
                    vec![
                        [first.x as f32, first.y as f32, first.z as f32],
                        [pt.x as f32, pt.y as f32, pt.z as f32],
                    ],
                    WireModel::CYAN,
                    false,
                )];
            }
            Step::RefNew { center, ref_angle } => {
                let Some(angle) = self.plane.angle(*center, pt) else {
                    return vec![];
                };
                (*center, angle - *ref_angle, angle)
            }
            _ => return vec![],
        };
        let mut previews: Vec<_> = self
            .wire_models
            .iter()
            .map(|w| {
                w.rotated_about_axis(
                    center.as_vec3(),
                    self.plane.z.as_vec3(),
                    angle_rad as f32,
                )
            })
            .collect();
        let radius = self.plane.vector_to_local(pt - center).truncate().length();
        previews.extend(self.angle_guide(center, radius, guide_angle));
        previews
    }

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::Angle { .. } | Step::RefNew { .. } => DynField::Angle,
            Step::RefFirst { .. } => DynField::Scalar,
            _ => DynField::Point,
        }
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};
        // Both normal rotation and Reference's new angle are absolute cursor
        // directions from the center. Reference mode subtracts its stored
        // reference angle only when previewing or committing the transform.
        match self.step {
            Step::Angle { center } | Step::RefNew { center, .. } => Some(DynSpec {
                anchor: DynAnchor::Point(center),
                fields: vec![DynFieldSpec::new(DynRole::Angle)],
                guide: DynGuide::None,
                ref_point: None,
            }),
            _ => None,
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.step, Step::Angle { .. } | Step::RefFirst { .. })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        match self.step {
            Step::Angle { center } | Step::RefNew { center, .. } => self
                .plane
                .angle(center, cursor)
                .map(|angle| crate::command::dyn_display_angle_deg(angle as f32) as f64),
            _ => None,
        }
    }

    fn dyn_label_point(&self, cursor: DVec3) -> Option<DVec3> {
        let center = match self.step {
            Step::Angle { center } | Step::RefNew { center, .. } => center,
            _ => return None,
        };
        let center_local = self.plane.to_local(center);
        let direction = self.plane.vector_to_local(cursor - center);
        let radius = direction.truncate().length();
        if radius <= f64::EPSILON {
            return None;
        }
        let angle = direction.y.atan2(direction.x);
        let point = if angle.abs() <= f64::EPSILON {
            [center_local.x + radius, center_local.y]
        } else {
            let (arc, _) = Self::angle_arc(
                [center_local.x, center_local.y],
                radius,
                angle,
            )?;
            KernelCurve::Arc(arc).point_at(0.5)
        };
        Some(
            self.plane
                .to_world(DVec3::new(point[0], point[1], center_local.z)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_label_uses_the_working_plane_arc_midpoint() {
        let plane = WorkingPlane::new(DVec3::ZERO, DVec3::Y, DVec3::Z);
        let mut command = RotateCommand::new(vec![], vec![]);
        command.set_working_plane(plane);
        command.step = Step::Angle {
            center: DVec3::ZERO,
        };

        let label = command
            .dyn_label_point(plane.y * 10.0)
            .expect("label point");
        let expected = (plane.x + plane.y) * (50.0_f64).sqrt();

        assert!(label.abs_diff_eq(expected, 1e-9));
    }
}
