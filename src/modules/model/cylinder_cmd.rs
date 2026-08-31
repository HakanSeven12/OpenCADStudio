use acadrust::entities::Solid3D;
use acadrust::objects::SolidHistoryOperation;
use acadrust::EntityType;
use cadkernel::brep::Body;
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec,
    TangentObject, WorkingPlane,
};
use crate::scene::model::solid_model;
use crate::scene::model::wire_model::WireModel;
use crate::t;

#[derive(Clone, Copy)]
enum Step {
    BaseCenter,
    BaseRadius,
    BaseDiameter,
    ThreePointFirst,
    ThreePointSecond(DVec3),
    ThreePointThird(DVec3, DVec3),
    TwoPointFirst,
    TwoPointSecond(DVec3),
    EllipseFirst,
    EllipseCenter,
    EllipseCenterFirstAxis(DVec3),
    EllipseCenterSecondAxis(DVec3, DVec3),
    EllipseSecond(DVec3),
    EllipseThird(DVec3, DVec3),
    TtrFirst,
    TtrSecond { object: TangentObject, hit: DVec3 },
    TtrRadius {
        first: TangentObject,
        second: TangentObject,
        first_hit: DVec3,
        second_hit: DVec3,
    },
    Height,
    HeightFirstPoint,
    HeightSecondPoint(DVec3),
    AxisEndpoint,
}

#[derive(Clone, Copy)]
struct Defaults {
    major_radius: f64,
    minor_radius: f64,
    height: f64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            major_radius: 1.0,
            minor_radius: 1.0,
            height: 1.0,
        }
    }
}

fn defaults() -> &'static std::sync::Mutex<Defaults> {
    static DEFAULTS: std::sync::OnceLock<std::sync::Mutex<Defaults>> =
        std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| std::sync::Mutex::new(Defaults::default()))
}

pub struct CylinderCommand {
    step: Step,
    frame: Option<WorkingPlane>,
    major_radius: f64,
    minor_radius: f64,
    remembered: Defaults,
    plane: WorkingPlane,
}

impl CylinderCommand {
    pub fn new() -> Self {
        let remembered = defaults()
            .lock()
            .map(|value| *value)
            .unwrap_or_default();
        Self {
            step: Step::BaseCenter,
            frame: None,
            major_radius: remembered.major_radius,
            minor_radius: remembered.minor_radius,
            remembered,
            plane: WorkingPlane::default(),
        }
    }

    fn frame_at(&self, center: DVec3) -> WorkingPlane {
        WorkingPlane {
            origin: center,
            x: self.plane.x,
            y: self.plane.y,
            z: self.plane.z,
        }
    }

    fn set_base(&mut self, mut frame: WorkingPlane, mut x_radius: f64, mut y_radius: f64) -> bool {
        if !frame.origin.is_finite()
            || !frame.x.is_finite()
            || !frame.y.is_finite()
            || !frame.z.is_finite()
            || !x_radius.is_finite()
            || !y_radius.is_finite()
            || x_radius < 1e-6
            || y_radius < 1e-6
        {
            return false;
        }
        if y_radius > x_radius {
            std::mem::swap(&mut x_radius, &mut y_radius);
            frame = WorkingPlane {
                origin: frame.origin,
                x: frame.y,
                y: -frame.x,
                z: frame.z,
            };
        }
        self.frame = Some(frame);
        self.major_radius = x_radius;
        self.minor_radius = y_radius;
        self.step = Step::Height;
        true
    }

    fn three_point_frame(a: DVec3, b: DVec3, c: DVec3) -> Option<(WorkingPlane, f64)> {
        let normal = (b - a).cross(c - a).try_normalize()?;
        let first_axis = (b - a).try_normalize()?;
        let second_axis = normal.cross(first_axis).try_normalize()?;
        let plane = WorkingPlane::new(a, first_axis, second_axis);
        let local_b = plane.to_local(b);
        let local_c = plane.to_local(c);
        let circle = cadkernel::geom2d::arc_through_points(
            [0.0, 0.0],
            [local_b.x, local_b.y],
            [local_c.x, local_c.y],
        )?;
        let center = plane.to_world(DVec3::new(circle.centre[0], circle.centre[1], 0.0));
        let x = (a - center).try_normalize()?;
        let y = normal.cross(x).try_normalize()?;
        Some((WorkingPlane::new(center, x, y), circle.radius))
    }

    fn axis_frame(&self, axis: DVec3) -> Option<(WorkingPlane, f64)> {
        let current = self.frame?;
        let height = axis.length();
        if !height.is_finite() || height < 1e-6 {
            return None;
        }
        let z = axis / height;
        let mut x = current.x - z * current.x.dot(z);
        if x.length_squared() < 1e-12 {
            x = current.y - z * current.y.dot(z);
        }
        let x = x.try_normalize()?;
        let y = z.cross(x).try_normalize()?;
        Some((WorkingPlane::new(current.origin, x, y), height))
    }

    fn history_transform(frame: WorkingPlane) -> [f64; 16] {
        glam::DMat4::from_cols(
            frame.x.extend(0.0),
            frame.y.extend(0.0),
            frame.z.extend(0.0),
            frame.origin.extend(1.0),
        )
        .to_cols_array()
    }

    fn build(&self, axis: DVec3) -> Option<(Body, SolidHistoryOperation, f64)> {
        let (frame, height) = self.axis_frame(axis)?;
        let local = solid_model::elliptical_cylinder_solid(
            [0.0; 3],
            self.major_radius,
            self.minor_radius,
            height,
        )?;
        let placed = solid_model::placed(
            &local,
            frame.x.to_array(),
            frame.y.to_array(),
            frame.z.to_array(),
            frame.origin.to_array(),
        )?;
        Some((
            placed,
            crate::scene::model::solid_history::elliptical_cylinder_op(
                Self::history_transform(frame),
                self.major_radius,
                self.minor_radius,
                height,
            ),
            height,
        ))
    }

    fn commit_axis(&self, axis: DVec3) -> CmdResult {
        let Some((solid, history, height)) = self.build(axis) else {
            return CmdResult::NeedPoint;
        };
        if let Ok(mut remembered) = defaults().lock() {
            *remembered = Defaults {
                major_radius: self.major_radius,
                minor_radius: self.minor_radius,
                height,
            };
        }
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&solid) else {
            return CmdResult::Cancel;
        };
        let mut entity = Solid3D::new();
        entity.set_sat_document(&document);
        entity.wires = solid_model::edge_wires(&solid);
        CmdResult::CommitSolid {
            entity: EntityType::Solid3D(entity),
            solid: Box::new(solid),
            history,
        }
    }

    fn commit_height(&self, height: f64) -> CmdResult {
        let Some(frame) = self.frame else {
            return CmdResult::NeedPoint;
        };
        if !height.is_finite() || height.abs() < 1e-6 {
            return CmdResult::NeedPoint;
        }
        self.commit_axis(frame.z * height)
    }

    fn height_at(&self, point: DVec3) -> Option<f64> {
        let frame = self.frame?;
        Some((point - frame.origin).dot(frame.z))
    }

    fn ttr_center(&self, radius: f64) -> Option<DVec3> {
        let Step::TtrRadius {
            first,
            second,
            first_hit,
            second_hit,
        } = self.step
        else {
            return None;
        };
        let candidates = crate::modules::draw::draw::circle::ttr_candidates(first, second, radius);
        let local = crate::modules::draw::draw::circle::best_of(
            &candidates,
            (first_hit + second_hit) * 0.5,
        )?;
        Some(self.plane.to_world(local))
    }

    fn base_preview(&self, frame: WorkingPlane, x_radius: f64, y_radius: f64) -> WireModel {
        let mut points = Vec::new();
        push_ellipse(
            &mut points,
            frame.origin,
            frame.x,
            frame.y,
            x_radius.max(1e-9),
            y_radius.max(1e-9),
        );
        wire("cylinder_base_preview", points)
    }

    fn solid_preview(&self, axis: DVec3) -> Option<WireModel> {
        let (frame, height) = self.axis_frame(axis)?;
        let top = frame.origin + frame.z * height;
        let mut points = Vec::new();
        push_ellipse(
            &mut points,
            frame.origin,
            frame.x,
            frame.y,
            self.major_radius,
            self.minor_radius,
        );
        push_ellipse(
            &mut points,
            top,
            frame.x,
            frame.y,
            self.major_radius,
            self.minor_radius,
        );
        for index in 0..4 {
            let angle = index as f64 * std::f64::consts::FRAC_PI_2;
            let base = frame.origin
                + frame.x * (self.major_radius * angle.cos())
                + frame.y * (self.minor_radius * angle.sin());
            push_segment(&mut points, base, base + frame.z * height);
        }
        Some(wire("cylinder_height_preview", points))
    }

    fn on_tangent(&mut self, object: TangentObject, hit: DVec3) -> CmdResult {
        let object = crate::modules::draw::draw::circle::tangent_object_local(object, self.plane);
        let hit = self.plane.to_local(hit);
        match self.step {
            Step::TtrFirst => self.step = Step::TtrSecond { object, hit },
            Step::TtrSecond {
                object: first,
                hit: first_hit,
            } => {
                self.step = Step::TtrRadius {
                    first,
                    second: object,
                    first_hit,
                    second_hit: hit,
                };
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn on_text(&mut self, raw: &str) -> Option<CmdResult> {
        let token = raw.trim();
        let upper = token.to_uppercase();
        match self.step {
            Step::BaseCenter => {
                self.step = match upper.as_str() {
                    "3P" => Step::ThreePointFirst,
                    "2P" => Step::TwoPointFirst,
                    "T" | "TTR" => Step::TtrFirst,
                    "E" | "ELLIPTICAL" => Step::EllipseFirst,
                    _ => return None,
                };
                return Some(CmdResult::NeedPoint);
            }
            Step::BaseRadius if matches!(upper.as_str(), "D" | "DIAMETER") => {
                self.step = Step::BaseDiameter;
                return Some(CmdResult::NeedPoint);
            }
            Step::EllipseFirst if matches!(upper.as_str(), "C" | "CENTER") => {
                self.step = Step::EllipseCenter;
                return Some(CmdResult::NeedPoint);
            }
            Step::Height => {
                self.step = match upper.as_str() {
                    "2P" | "2POINT" => Step::HeightFirstPoint,
                    "A" | "AXIS" | "AXIS ENDPOINT" => Step::AxisEndpoint,
                    _ => {
                        let height = crate::entities::common::parse_typed_length(token)?;
                        return Some(self.commit_height(height));
                    }
                };
                return Some(CmdResult::NeedPoint);
            }
            _ => {}
        }
        let number = crate::entities::common::parse_typed_length(token)?;
        match self.step {
            Step::BaseRadius => {
                let frame = self.frame?;
                (number > 0.0).then(|| {
                    self.set_base(frame, number, number);
                    CmdResult::NeedPoint
                })
            }
            Step::BaseDiameter => {
                let frame = self.frame?;
                (number > 0.0).then(|| {
                    self.set_base(frame, number * 0.5, number * 0.5);
                    CmdResult::NeedPoint
                })
            }
            Step::EllipseCenterFirstAxis(center) => (number > 0.0).then(|| {
                self.step = Step::EllipseCenterSecondAxis(
                    center,
                    center + self.plane.x * number,
                );
                CmdResult::NeedPoint
            }),
            Step::TtrRadius { .. } => {
                let center = self.ttr_center(number)?;
                (number > 0.0).then(|| {
                    self.set_base(self.frame_at(center), number, number);
                    CmdResult::NeedPoint
                })
            }
            _ => None,
        }
    }

    fn mouse_preview(&self, point: DVec3) -> Option<WireModel> {
        match self.step {
            Step::BaseRadius | Step::BaseDiameter => {
                let frame = self.frame?;
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                Some(self.base_preview(frame, radius, radius))
            }
            Step::ThreePointSecond(first) | Step::TwoPointSecond(first) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(point);
                let center = self.plane.to_world((a + b) * 0.5);
                let radius = (b - a).truncate().length() * 0.5;
                Some(self.base_preview(self.frame_at(center), radius, radius))
            }
            Step::ThreePointThird(first, second) => {
                let (frame, radius) = Self::three_point_frame(first, second, point)?;
                Some(self.base_preview(frame, radius, radius))
            }
            Step::EllipseThird(first, second) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(second);
                let center_local = (a + b) * 0.5;
                let axis = (b - a).truncate();
                let x2 = axis.try_normalize()?;
                let x = self.plane.vector_to_world(DVec3::new(x2.x, x2.y, 0.0));
                let y = self.plane.z.cross(x).normalize_or_zero();
                let frame = WorkingPlane::new(self.plane.to_world(center_local), x, y);
                let local = frame.to_local(point);
                Some(self.base_preview(frame, axis.length() * 0.5, local.y.abs()))
            }
            Step::EllipseCenterSecondAxis(center, first_axis_end) => {
                let axis = first_axis_end - center;
                let x = axis.try_normalize()?;
                let y = self.plane.z.cross(x).normalize_or_zero();
                let frame = WorkingPlane::new(center, x, y);
                let local = frame.to_local(point);
                Some(self.base_preview(frame, axis.length(), local.y.abs()))
            }
            Step::TtrRadius { second_hit, .. } => {
                let local = self.plane.to_local(point);
                let radius = (local - second_hit).truncate().length();
                let center = self.ttr_center(radius)?;
                Some(self.base_preview(self.frame_at(center), radius, radius))
            }
            Step::Height => {
                let frame = self.frame?;
                self.solid_preview(frame.z * self.height_at(point)?)
            }
            Step::HeightSecondPoint(first) => {
                let frame = self.frame?;
                self.solid_preview(frame.z * first.distance(point))
            }
            Step::AxisEndpoint => {
                let frame = self.frame?;
                self.solid_preview(point - frame.origin)
            }
            _ => None,
        }
    }
}

impl CadCommand for CylinderCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        matches!(self.step, Step::Height).then(|| {
            let frame = self.frame.unwrap_or(self.plane);
            (frame.origin, frame.z.normalize_or_zero())
        })
    }

    fn name(&self) -> &'static str {
        "CYLINDER"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::BaseCenter =>
                t!("CYLINDER  Specify center point of base or [3P/2P/Ttr/Elliptical]:").into_owned(),
            Step::BaseRadius => crate::tf!(
                "CYLINDER  Specify base radius or [Diameter] <{:.4}>:",
                self.remembered.major_radius
            ).into_owned(),
            Step::BaseDiameter => crate::tf!(
                "CYLINDER  Specify base diameter <{:.4}>:",
                self.remembered.major_radius * 2.0
            ).into_owned(),
            Step::ThreePointFirst => t!("CYLINDER  Specify first point on base:").into_owned(),
            Step::ThreePointSecond(_) => t!("CYLINDER  Specify second point on base:").into_owned(),
            Step::ThreePointThird(_, _) => t!("CYLINDER  Specify third point on base:").into_owned(),
            Step::TwoPointFirst => t!("CYLINDER  Specify first endpoint of base diameter:").into_owned(),
            Step::TwoPointSecond(_) => t!("CYLINDER  Specify second endpoint of base diameter:").into_owned(),
            Step::EllipseFirst =>
                t!("CYLINDER  Specify endpoint of first axis or [Center]:").into_owned(),
            Step::EllipseCenter => t!("CYLINDER  Specify center point:").into_owned(),
            Step::EllipseCenterFirstAxis(_) => crate::tf!(
                "CYLINDER  Specify distance to first axis <{:.4}>:",
                self.remembered.major_radius
            ).into_owned(),
            Step::EllipseCenterSecondAxis(_, _) =>
                t!("CYLINDER  Specify endpoint of second axis:").into_owned(),
            Step::EllipseSecond(_) =>
                t!("CYLINDER  Specify other endpoint of first axis:").into_owned(),
            Step::EllipseThird(_, _) =>
                t!("CYLINDER  Specify endpoint of second axis:").into_owned(),
            Step::TtrFirst => t!("CYLINDER  Select first tangent object:").into_owned(),
            Step::TtrSecond { .. } => t!("CYLINDER  Select second tangent object:").into_owned(),
            Step::TtrRadius { .. } => crate::tf!(
                "CYLINDER  Specify base radius <{:.4}>:",
                self.remembered.major_radius
            ).into_owned(),
            Step::Height => crate::tf!(
                "CYLINDER  Specify height or [2Point/Axis endpoint] <{:.4}>:",
                self.remembered.height
            ).into_owned(),
            Step::HeightFirstPoint =>
                t!("CYLINDER  Specify first point for height:").into_owned(),
            Step::HeightSecondPoint(_) =>
                t!("CYLINDER  Specify second point for height:").into_owned(),
            Step::AxisEndpoint => t!("CYLINDER  Specify axis endpoint:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::BaseCenter => vec![
                CmdOption::new("3P", "3P"),
                CmdOption::new("2P", "2P"),
                CmdOption::new("Ttr", "TTR"),
                CmdOption::new(t!("Elliptical").as_ref(), "E"),
            ],
            Step::BaseRadius => vec![CmdOption::new(t!("Diameter").as_ref(), "D")],
            Step::EllipseFirst => vec![CmdOption::new(t!("Center").as_ref(), "C")],
            Step::Height => vec![
                CmdOption::new("2Point", "2P"),
                CmdOption::new(t!("Axis endpoint").as_ref(), "A"),
            ],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::BaseCenter => {
                self.frame = Some(self.frame_at(point));
                self.step = Step::BaseRadius;
                CmdResult::NeedPoint
            }
            Step::BaseRadius | Step::BaseDiameter => {
                let Some(frame) = self.frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                self.set_base(frame, radius, radius);
                CmdResult::NeedPoint
            }
            Step::ThreePointFirst => {
                self.step = Step::ThreePointSecond(point);
                CmdResult::NeedPoint
            }
            Step::ThreePointSecond(first) => {
                if point.distance_squared(first) > 1e-12 {
                    self.step = Step::ThreePointThird(first, point);
                }
                CmdResult::NeedPoint
            }
            Step::ThreePointThird(first, second) => {
                if let Some((frame, radius)) = Self::three_point_frame(first, second, point) {
                    self.set_base(frame, radius, radius);
                }
                CmdResult::NeedPoint
            }
            Step::TwoPointFirst => {
                self.step = Step::TwoPointSecond(point);
                CmdResult::NeedPoint
            }
            Step::TwoPointSecond(first) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(point);
                let radius = (b - a).truncate().length() * 0.5;
                let center = self.plane.to_world((a + b) * 0.5);
                self.set_base(self.frame_at(center), radius, radius);
                CmdResult::NeedPoint
            }
            Step::EllipseFirst => {
                self.step = Step::EllipseSecond(point);
                CmdResult::NeedPoint
            }
            Step::EllipseCenter => {
                self.step = Step::EllipseCenterFirstAxis(point);
                CmdResult::NeedPoint
            }
            Step::EllipseCenterFirstAxis(center) => {
                if point.distance_squared(center) > 1e-12 {
                    self.step = Step::EllipseCenterSecondAxis(center, point);
                }
                CmdResult::NeedPoint
            }
            Step::EllipseCenterSecondAxis(center, first_axis_end) => {
                let axis = first_axis_end - center;
                if let Some(x) = axis.try_normalize() {
                    let y = self.plane.z.cross(x).normalize_or_zero();
                    let frame = WorkingPlane::new(center, x, y);
                    let local = frame.to_local(point);
                    self.set_base(frame, axis.length(), local.y.abs());
                }
                CmdResult::NeedPoint
            }
            Step::EllipseSecond(first) => {
                if point.distance_squared(first) > 1e-12 {
                    self.step = Step::EllipseThird(first, point);
                }
                CmdResult::NeedPoint
            }
            Step::EllipseThird(first, second) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(second);
                let center_local = (a + b) * 0.5;
                let axis = (b - a).truncate();
                let x_radius = axis.length() * 0.5;
                if let Some(x2) = axis.try_normalize() {
                    let x = self.plane.vector_to_world(DVec3::new(x2.x, x2.y, 0.0));
                    let y = self.plane.z.cross(x).normalize_or_zero();
                    let frame = WorkingPlane::new(self.plane.to_world(center_local), x, y);
                    let local = frame.to_local(point);
                    self.set_base(frame, x_radius, local.y.abs());
                }
                CmdResult::NeedPoint
            }
            Step::TtrRadius { second_hit, .. } => {
                let local = self.plane.to_local(point);
                let radius = (local - second_hit).truncate().length();
                if let Some(center) = self.ttr_center(radius) {
                    self.set_base(self.frame_at(center), radius, radius);
                }
                CmdResult::NeedPoint
            }
            Step::Height => self
                .height_at(point)
                .map(|height| self.commit_height(height))
                .unwrap_or(CmdResult::NeedPoint),
            Step::HeightFirstPoint => {
                self.step = Step::HeightSecondPoint(point);
                CmdResult::NeedPoint
            }
            Step::HeightSecondPoint(first) => self.commit_height(first.distance(point)),
            Step::AxisEndpoint => {
                let Some(frame) = self.frame else {
                    return CmdResult::NeedPoint;
                };
                self.commit_axis(point - frame.origin)
            }
            Step::TtrFirst | Step::TtrSecond { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::BaseRadius | Step::BaseDiameter => {
                let Some(frame) = self.frame else {
                    return CmdResult::Cancel;
                };
                self.set_base(
                    frame,
                    self.remembered.major_radius,
                    self.remembered.major_radius,
                );
                CmdResult::NeedPoint
            }
            Step::TtrRadius { .. } => {
                let radius = self.remembered.major_radius;
                let Some(center) = self.ttr_center(radius) else {
                    return CmdResult::NeedPoint;
                };
                self.set_base(self.frame_at(center), radius, radius);
                CmdResult::NeedPoint
            }
            Step::EllipseCenterFirstAxis(center) => {
                self.step = Step::EllipseCenterSecondAxis(
                    center,
                    center + self.plane.x * self.remembered.major_radius,
                );
                CmdResult::NeedPoint
            }
            Step::Height => self.commit_height(self.remembered.height),
            _ => CmdResult::Cancel,
        }
    }

    fn needs_tangent_pick(&self) -> bool {
        matches!(self.step, Step::TtrFirst | Step::TtrSecond { .. })
    }

    fn on_tangent_point(&mut self, object: TangentObject, hit: DVec3) -> CmdResult {
        self.on_tangent(object, hit)
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            Step::BaseCenter
                | Step::BaseRadius
                | Step::BaseDiameter
                | Step::EllipseFirst
                | Step::EllipseCenterFirstAxis(_)
                | Step::TtrRadius { .. }
                | Step::Height
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            Step::BaseCenter | Step::BaseRadius | Step::EllipseFirst | Step::Height
        )
    }

    fn on_text_input(&mut self, raw: &str) -> Option<CmdResult> {
        self.on_text(raw)
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        self.mouse_preview(point)
    }

    fn dyn_spec(&self) -> Option<DynSpec> {
        let frame = self.frame.unwrap_or(self.plane);
        let role = match self.step {
            Step::BaseRadius | Step::TtrRadius { .. } => DynRole::Radius,
            Step::BaseDiameter => DynRole::Diameter,
            Step::EllipseCenterFirstAxis(_) => DynRole::Distance,
            Step::Height | Step::HeightSecondPoint(_) | Step::AxisEndpoint => DynRole::Height,
            _ => return None,
        };
        Some(DynSpec {
            anchor: DynAnchor::Point(frame.origin),
            fields: vec![DynFieldSpec::new(role)],
            guide: if matches!(role, DynRole::Radius | DynRole::Diameter) {
                DynGuide::Radius
            } else {
                DynGuide::None
            },
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        match self.step {
            Step::BaseRadius => {
                let local = self.frame?.to_local(cursor);
                Some(local.x.hypot(local.y))
            }
            Step::BaseDiameter => {
                let local = self.frame?.to_local(cursor);
                Some(local.x.hypot(local.y) * 2.0)
            }
            Step::TtrRadius { second_hit, .. } => {
                let local = self.plane.to_local(cursor);
                Some((local - second_hit).truncate().length())
            }
            Step::EllipseCenterFirstAxis(center) => Some(center.distance(cursor)),
            Step::Height => self.height_at(cursor).map(f64::abs),
            Step::HeightSecondPoint(first) => Some(first.distance(cursor)),
            Step::AxisEndpoint => self.frame.map(|frame| frame.origin.distance(cursor)),
            _ => None,
        }
    }
}

fn push_break(points: &mut Vec<[f32; 3]>) {
    if !points.is_empty() {
        points.push([f32::NAN; 3]);
    }
}

fn push_segment(points: &mut Vec<[f32; 3]>, first: DVec3, second: DVec3) {
    push_break(points);
    points.extend([first.as_vec3().to_array(), second.as_vec3().to_array()]);
}

fn push_ellipse(
    points: &mut Vec<[f32; 3]>,
    center: DVec3,
    x_axis: DVec3,
    y_axis: DVec3,
    x_radius: f64,
    y_radius: f64,
) {
    push_break(points);
    const SEGMENTS: usize = 64;
    for index in 0..=SEGMENTS {
        let angle = index as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
        let point = center
            + x_axis * (x_radius * angle.cos())
            + y_axis * (y_radius * angle.sin());
        points.push(point.as_vec3().to_array());
    }
}

fn wire(name: &str, points: Vec<[f32; 3]>) -> WireModel {
    WireModel::solid(name.to_string(), points, WireModel::CYAN, false)
}
