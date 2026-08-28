use acadrust::entities::{Helix, HelixConstraint, Spline};
use acadrust::types::Vector3;
use acadrust::EntityType;
use cadkernel::space::{HelixCurve, HelixDirection, Vec3};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, DynField, WorkingPlane};
use crate::modules::draw::defaults;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

const EPSILON: f64 = 1.0e-9;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "HELIX",
        label: "Helix",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("HELIX".to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Center,
    BaseRadius,
    BaseDiameter,
    TopRadius,
    TopDiameter,
    Final,
    AxisEndpoint,
    Turns,
    TurnHeight,
    Twist,
}

pub struct HelixCommand {
    step: Step,
    center: DVec3,
    base_radius: f64,
    top_radius: f64,
    height: f64,
    turns: f64,
    requested_turn_height: Option<f64>,
    counter_clockwise: bool,
    constraint: HelixConstraint,
    plane: WorkingPlane,
}

impl HelixCommand {
    pub fn new() -> Self {
        let base_radius = defaults::get_helix_base_radius();
        Self {
            step: Step::Center,
            center: DVec3::ZERO,
            base_radius,
            top_radius: base_radius,
            height: defaults::get_helix_height(),
            turns: defaults::get_helix_turns(),
            requested_turn_height: None,
            counter_clockwise: defaults::get_helix_counter_clockwise(),
            constraint: HelixConstraint::Turns,
            plane: WorkingPlane::default(),
        }
    }

    fn parse_value(text: &str) -> Option<f64> {
        crate::entities::common::parse_typed_length(text)
    }

    fn parse_turns(text: &str) -> Option<f64> {
        text.trim().replace(',', ".").parse().ok().filter(|value: &f64| value.is_finite())
    }

    fn axis_start_direction(&self, axis: Vec3) -> Option<Vec3> {
        let project = |direction: DVec3| {
            let direction = Vec3::from(direction.to_array());
            (direction - axis * direction.dot(axis)).normalize()
        };
        project(self.plane.x).or_else(|| project(self.plane.y))
    }

    fn resolved_turns(&self, height: f64) -> f64 {
        match self.requested_turn_height {
            Some(turn_height) if turn_height > EPSILON => (height.abs() / turn_height).max(EPSILON),
            _ => self.turns,
        }
    }

    fn kernel_curve(&self, height: f64, axis: DVec3) -> Option<HelixCurve> {
        let axis = Vec3::from(axis.to_array())
            .normalize()
            .or_else(|| Vec3::from(self.plane.z.to_array()).normalize())?;
        Some(HelixCurve {
            base_center: self.center.to_array(),
            axis_direction: axis.to_array(),
            start_direction: self.axis_start_direction(axis)?.to_array(),
            base_radius: self.base_radius,
            top_radius: self.top_radius,
            height,
            turns: self.resolved_turns(height),
            direction: if self.counter_clockwise {
                HelixDirection::CounterClockwise
            } else {
                HelixDirection::Clockwise
            },
        })
    }

    fn build(&self, height: f64, axis: DVec3) -> Option<EntityType> {
        let curve = self.kernel_curve(height, axis)?;
        let nurbs = curve.nurbs()?;
        let axis = DVec3::from_array(curve.axis_direction).normalize_or(self.plane.z);
        let start_direction = DVec3::from_array(curve.start_direction).normalize_or(self.plane.x);
        let mut spline = Spline::new();
        spline.degree = nurbs.degree() as i32;
        spline.knots = nurbs.knots().to_vec();
        spline.control_points = nurbs
            .control_points()
            .iter()
            .map(|point| Vector3::new(point[0], point[1], point[2]))
            .collect();
        spline.flags.planar = false;
        spline.flags.rational = nurbs.is_rational();
        if spline.flags.rational {
            spline.weights = nurbs.weights().to_vec();
        }

        let mut helix = Helix::new();
        helix.spline = spline;
        helix.axis_base_point = Vector3::new(self.center.x, self.center.y, self.center.z);
        let start = self.center + start_direction * curve.base_radius;
        helix.start_point = Vector3::new(start.x, start.y, start.z);
        helix.axis_vector = Vector3::new(axis.x, axis.y, axis.z);
        helix.radius = curve.base_radius;
        helix.turns = curve.turns;
        helix.turn_height = height / curve.turns;
        helix.handedness = self.counter_clockwise;
        helix.constraint = self.constraint;
        Some(EntityType::Helix(helix))
    }

    fn commit(&mut self, height: f64, axis: DVec3) -> CmdResult {
        let Some(entity) = self.build(height, axis) else {
            return CmdResult::NeedPoint;
        };
        self.height = height;
        self.turns = self.resolved_turns(height);
        defaults::set_helix_base_radius(self.base_radius);
        defaults::set_helix_height(height);
        defaults::set_helix_turns(self.turns);
        defaults::set_helix_counter_clockwise(self.counter_clockwise);
        CmdResult::CommitAndExit(entity)
    }

    fn preview(&self, height: f64, axis: DVec3) -> Option<WireModel> {
        let points = self
            .kernel_curve(height, axis)?
            .nurbs()?
            .tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE);
        Some(WireModel::solid_f64(
            "helix_preview".to_string(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

impl CadCommand for HelixCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "HELIX"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Center => crate::tf!(
                "HELIX  Number of turns = {:.4}  Twist = {}\nSpecify center point of base:",
                self.turns,
                if self.counter_clockwise { "CCW" } else { "CW" }
            )
            .into_owned(),
            Step::BaseRadius => crate::tf!(
                "HELIX  Specify base radius or [Diameter] <{:.4}>:",
                self.base_radius
            )
            .into_owned(),
            Step::BaseDiameter => crate::tf!(
                "HELIX  Specify base diameter <{:.4}>:",
                self.base_radius * 2.0
            )
            .into_owned(),
            Step::TopRadius => crate::tf!(
                "HELIX  Specify top radius or [Diameter] <{:.4}>:",
                self.base_radius
            )
            .into_owned(),
            Step::TopDiameter => crate::tf!(
                "HELIX  Specify top diameter <{:.4}>:",
                self.base_radius * 2.0
            )
            .into_owned(),
            Step::Final => crate::tf!(
                "HELIX  Specify helix height or [Axis endpoint/Turns/turn Height/tWist] <{:.4}>:",
                self.height
            )
            .into_owned(),
            Step::AxisEndpoint => t!("HELIX  Specify axis endpoint:").into_owned(),
            Step::Turns => crate::tf!("HELIX  Enter number of turns <{:.4}>:", self.turns).into_owned(),
            Step::TurnHeight => crate::tf!(
                "HELIX  Specify distance between turns <{:.4}>:",
                self.height.abs() / self.turns.max(EPSILON)
            )
            .into_owned(),
            Step::Twist => crate::tf!(
                "HELIX  Enter twist direction [Clockwise/Counterclockwise] <{}>:",
                if self.counter_clockwise { "CCW" } else { "CW" }
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::BaseRadius | Step::TopRadius => vec![CmdOption::new(t!("Diameter").as_ref(), "D")],
            Step::Final => vec![
                CmdOption::new(t!("Axis endpoint").as_ref(), "A"),
                CmdOption::new(t!("Turns").as_ref(), "T"),
                CmdOption::new(t!("Turn Height").as_ref(), "H"),
                CmdOption::new(t!("Twist").as_ref(), "W"),
            ],
            Step::Twist => vec![
                CmdOption::new(t!("Clockwise").as_ref(), "CW"),
                CmdOption::new(t!("Counterclockwise").as_ref(), "CCW"),
            ],
            _ => Vec::new(),
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::BaseRadius | Step::TopRadius | Step::Final | Step::Twist)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Center => {
                self.center = point;
                self.step = Step::BaseRadius;
                CmdResult::NeedPoint
            }
            Step::BaseRadius | Step::BaseDiameter => {
                let local = self.plane.vector_to_local(point - self.center);
                let distance = local.x.hypot(local.y);
                let radius = if self.step == Step::BaseDiameter {
                    distance * 0.5
                } else {
                    distance
                };
                if radius > EPSILON {
                    self.base_radius = radius;
                    self.top_radius = radius;
                    self.step = Step::TopRadius;
                }
                CmdResult::NeedPoint
            }
            Step::TopRadius | Step::TopDiameter => {
                let local = self.plane.vector_to_local(point - self.center);
                let distance = local.x.hypot(local.y);
                self.top_radius = if self.step == Step::TopDiameter {
                    distance * 0.5
                } else {
                    distance
                };
                self.step = Step::Final;
                CmdResult::NeedPoint
            }
            Step::Final => {
                let height = (point - self.center).dot(self.plane.z);
                if height.abs() <= EPSILON {
                    CmdResult::NeedPoint
                } else {
                    self.commit(height, self.plane.z)
                }
            }
            Step::AxisEndpoint => {
                let vector = point - self.center;
                let height = vector.length();
                if height <= EPSILON {
                    CmdResult::NeedPoint
                } else {
                    self.commit(height, vector / height)
                }
            }
            Step::Turns | Step::TurnHeight | Step::Twist => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Center => CmdResult::Cancel,
            Step::BaseRadius | Step::BaseDiameter => {
                self.top_radius = self.base_radius;
                self.step = Step::TopRadius;
                CmdResult::NeedPoint
            }
            Step::TopRadius | Step::TopDiameter => {
                self.top_radius = self.base_radius;
                self.step = Step::Final;
                CmdResult::NeedPoint
            }
            Step::Final => self.commit(self.height, self.plane.z),
            Step::AxisEndpoint => CmdResult::NeedPoint,
            Step::Turns => {
                self.constraint = HelixConstraint::Turns;
                self.requested_turn_height = None;
                self.step = Step::Final;
                CmdResult::NeedPoint
            }
            Step::TurnHeight => {
                self.constraint = HelixConstraint::TurnHeight;
                self.requested_turn_height = Some(self.height.abs() / self.turns.max(EPSILON));
                self.step = Step::Final;
                CmdResult::NeedPoint
            }
            Step::Twist => {
                self.step = Step::Final;
                CmdResult::NeedPoint
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        self.step != Step::Center
    }

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::Center | Step::AxisEndpoint => DynField::Point,
            Step::BaseRadius | Step::BaseDiameter | Step::TopRadius | Step::TopDiameter | Step::Final | Step::TurnHeight => DynField::Distance,
            Step::Turns | Step::Twist => DynField::Scalar,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.to_ascii_uppercase();
        match self.step {
            Step::Center => None,
            Step::BaseRadius if matches!(keyword.as_str(), "D" | "DIAMETER") => {
                self.step = Step::BaseDiameter;
                Some(CmdResult::NeedPoint)
            }
            Step::TopRadius if matches!(keyword.as_str(), "D" | "DIAMETER") => {
                self.step = Step::TopDiameter;
                Some(CmdResult::NeedPoint)
            }
            Step::Final if matches!(keyword.as_str(), "A" | "AXIS" | "AXIS ENDPOINT") => {
                self.step = Step::AxisEndpoint;
                Some(CmdResult::NeedPoint)
            }
            Step::Final if matches!(keyword.as_str(), "T" | "TURNS") => {
                self.step = Step::Turns;
                Some(CmdResult::NeedPoint)
            }
            Step::Final if matches!(keyword.as_str(), "H" | "HEIGHT" | "TURN HEIGHT") => {
                self.step = Step::TurnHeight;
                Some(CmdResult::NeedPoint)
            }
            Step::Final if matches!(keyword.as_str(), "W" | "TWIST") => {
                self.step = Step::Twist;
                Some(CmdResult::NeedPoint)
            }
            Step::BaseRadius => {
                let radius = Self::parse_value(value)?;
                if radius > EPSILON {
                    self.base_radius = radius;
                    self.top_radius = radius;
                    self.step = Step::TopRadius;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::BaseDiameter => {
                let diameter = Self::parse_value(value)?;
                if diameter > EPSILON {
                    self.base_radius = diameter * 0.5;
                    self.top_radius = self.base_radius;
                    self.step = Step::TopRadius;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::TopRadius => {
                let radius = Self::parse_value(value)?;
                if radius >= 0.0 {
                    self.top_radius = radius;
                    self.step = Step::Final;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::TopDiameter => {
                let diameter = Self::parse_value(value)?;
                if diameter >= 0.0 {
                    self.top_radius = diameter * 0.5;
                    self.step = Step::Final;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::Final => {
                let height = Self::parse_value(value)?;
                (height.abs() > EPSILON).then(|| self.commit(height, self.plane.z))
            }
            Step::AxisEndpoint => None,
            Step::Turns => {
                let turns = Self::parse_turns(value)?;
                if turns > EPSILON {
                    self.turns = turns;
                    self.requested_turn_height = None;
                    self.constraint = HelixConstraint::Turns;
                    self.step = Step::Final;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::TurnHeight => {
                let turn_height = Self::parse_value(value)?;
                if turn_height > EPSILON {
                    self.requested_turn_height = Some(turn_height);
                    self.constraint = HelixConstraint::TurnHeight;
                    self.step = Step::Final;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::Twist => {
                match keyword.as_str() {
                    "C" | "CW" | "CLOCKWISE" => self.counter_clockwise = false,
                    "CC" | "CCW" | "COUNTERCLOCKWISE" => self.counter_clockwise = true,
                    _ => return Some(CmdResult::NeedPoint),
                }
                self.step = Step::Final;
                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match self.step {
            Step::Center => None,
            Step::BaseRadius | Step::BaseDiameter => {
                let local = self.plane.vector_to_local(point - self.center);
                let old = (self.base_radius, self.top_radius);
                let distance = local.x.hypot(local.y);
                self.base_radius = if self.step == Step::BaseDiameter {
                    distance * 0.5
                } else {
                    distance
                }
                .max(EPSILON);
                self.top_radius = self.base_radius;
                let preview = self.preview(0.0, self.plane.z);
                (self.base_radius, self.top_radius) = old;
                preview
            }
            Step::TopRadius | Step::TopDiameter => {
                let local = self.plane.vector_to_local(point - self.center);
                let old = self.top_radius;
                let distance = local.x.hypot(local.y);
                self.top_radius = if self.step == Step::TopDiameter {
                    distance * 0.5
                } else {
                    distance
                };
                let preview = self.preview(0.0, self.plane.z);
                self.top_radius = old;
                preview
            }
            Step::Final => self.preview((point - self.center).dot(self.plane.z), self.plane.z),
            Step::AxisEndpoint => {
                let vector = point - self.center;
                let height = vector.length();
                (height > EPSILON).then(|| self.preview(height, vector / height)).flatten()
            }
            Step::Turns | Step::TurnHeight | Step::Twist => self.preview(self.height, self.plane.z),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["HELIX"] });
