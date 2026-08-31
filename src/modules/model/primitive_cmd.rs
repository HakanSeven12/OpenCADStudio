// Interactive kernel primitives stored as ACIS Solid3D entities.

use acadrust::entities::Solid3D;
use acadrust::objects::SolidHistoryOperation;
use acadrust::EntityType;
use cadkernel::brep::Body;
use cadkernel::geom2d::{
    fillets_between, Circle as KernelCircle, Curve as KernelCurve, Line as KernelLine,
    Tolerance,
};
use glam::DVec3;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::command::{CadCommand, CmdOption, CmdResult, TangentObject, WorkingPlane};
use crate::scene::model::solid_model;
use crate::scene::model::wire_model::WireModel;
use crate::t;

static LAST_WEDGE_HEIGHT: AtomicU64 = AtomicU64::new(0);

/// Which primitive a `PrimitiveCommand` builds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Box,
    Wedge,
    Cylinder,
    Cone,
    Sphere,
    Pyramid,
    Torus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxStep {
    FirstCorner,
    CenterPoint,
    OppositeCorner,
    CubeSize,
    Length,
    Width,
    Height,
    HeightFirstPoint,
    HeightSecondPoint,
}

#[derive(Clone, Copy)]
enum SphereStep {
    Center,
    Radius(DVec3),
    Diameter(DVec3),
    TwoPointFirst,
    TwoPointSecond(DVec3),
    ThreePointFirst,
    ThreePointSecond(DVec3),
    ThreePointThird(DVec3, DVec3),
    TtrFirst,
    TtrSecond {
        object: TangentObject,
        hit: DVec3,
    },
    TtrRadius {
        first: TangentObject,
        second: TangentObject,
        first_hit: DVec3,
        second_hit: DVec3,
    },
}

#[derive(Clone, Copy)]
enum ConeStep {
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
    HeightAfterTopRadius,
    HeightFirstPoint,
    HeightSecondPoint(DVec3),
    AxisEndpoint,
    TopRadius,
}

#[derive(Clone, Copy)]
struct ConeDefaults {
    base_x_radius: f64,
    base_y_radius: f64,
    top_radius: f64,
    height: f64,
}

impl Default for ConeDefaults {
    fn default() -> Self {
        Self {
            base_x_radius: 1.0,
            base_y_radius: 1.0,
            top_radius: 0.0,
            height: 1.0,
        }
    }
}

fn sphere_radius_store() -> &'static Mutex<f64> {
    static VALUE: OnceLock<Mutex<f64>> = OnceLock::new();
    VALUE.get_or_init(|| Mutex::new(1.0))
}

fn sphere_radius_default() -> f64 {
    sphere_radius_store()
        .lock()
        .map(|value| *value)
        .unwrap_or(1.0)
}

fn remember_sphere_radius(radius: f64) {
    if radius.is_finite() && radius > 1e-6 {
        if let Ok(mut value) = sphere_radius_store().lock() {
            *value = radius;
        }
    }
}

fn sphere_tangent_local(object: TangentObject, plane: WorkingPlane) -> TangentObject {
    match object {
        TangentObject::Line { p1, p2 } => TangentObject::Line {
            p1: plane.to_local(p1),
            p2: plane.to_local(p2),
        },
        TangentObject::Circle { center, radius } => TangentObject::Circle {
            center: plane.to_local(center),
            radius,
        },
    }
}

fn sphere_tangent_curve(object: TangentObject) -> KernelCurve {
    match object {
        TangentObject::Line { p1, p2 } => KernelCurve::Line(KernelLine {
            start: [p1.x, p1.y],
            end: [p2.x, p2.y],
        }),
        TangentObject::Circle { center, radius } => KernelCurve::Circle(KernelCircle {
            centre: [center.x, center.y],
            radius,
        }),
    }
}

fn sphere_ttr_centers(
    first: TangentObject,
    second: TangentObject,
    radius: f64,
) -> Vec<DVec3> {
    fillets_between(
        &sphere_tangent_curve(first),
        &sphere_tangent_curve(second),
        radius,
        Tolerance::default(),
    )
    .into_iter()
    .map(|fillet| DVec3::new(fillet.centre[0], fillet.centre[1], 0.0))
    .collect()
}

fn closest_sphere_center(candidates: &[DVec3], hint: DVec3) -> Option<DVec3> {
    candidates.iter().copied().min_by(|first, second| {
        first
            .distance(hint)
            .partial_cmp(&second.distance(hint))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn sphere_through_three_points(
    first: DVec3,
    second: DVec3,
    third: DVec3,
) -> Option<(DVec3, f64)> {
    let circle = cadkernel::geom2d::arc_through_points(
        [first.x, first.y],
        [second.x, second.y],
        [third.x, third.y],
    )?;
    Some((
        DVec3::new(circle.centre[0], circle.centre[1], first.z),
        circle.radius,
    ))
}

fn cone_defaults() -> &'static std::sync::Mutex<ConeDefaults> {
    static DEFAULTS: std::sync::OnceLock<std::sync::Mutex<ConeDefaults>> =
        std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| std::sync::Mutex::new(ConeDefaults::default()))
}

impl Shape {
    fn from_id(id: &str) -> Option<Shape> {
        Some(match id {
            "BOX" => Shape::Box,
            "WEDGE" => Shape::Wedge,
            "CYLINDER" => Shape::Cylinder,
            "CONE" => Shape::Cone,
            "SPHERE" => Shape::Sphere,
            "PYRAMID" | "PYR" => Shape::Pyramid,
            "TORUS" => Shape::Torus,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Shape::Box => "BOX",
            Shape::Wedge => "WEDGE",
            Shape::Cylinder => "CYLINDER",
            Shape::Cone => "CONE",
            Shape::Sphere => "SPHERE",
            Shape::Pyramid => "PYRAMID",
            Shape::Torus => "TORUS",
        }
    }
    /// True for footprints picked as a centre + radius (round shapes); false
    /// for corner-to-corner footprints (box/wedge).
    fn radial(self) -> bool {
        !self.rectangular()
    }
    fn rectangular(self) -> bool {
        matches!(self, Shape::Box | Shape::Wedge)
    }
    /// Whether a height value is collected after the footprint.
    fn needs_height(self) -> bool {
        !matches!(self, Shape::Sphere | Shape::Torus)
    }
}

pub struct PrimitiveCommand {
    shape: Shape,
    /// Footprint points collected so far (local/world XY, z = 0).
    pts: Vec<DVec3>,
    /// True once the footprint is set and we are collecting the height.
    height_step: bool,
    box_step: BoxStep,
    box_centered: bool,
    box_origin: Option<DVec3>,
    box_length: Option<f64>,
    box_width: Option<f64>,
    box_angle: f64,
    box_width_sign: f64,
    box_height_anchor: Option<DVec3>,
    sphere_step: SphereStep,
    sphere_default_radius: f64,
    cone_step: ConeStep,
    cone_frame: Option<WorkingPlane>,
    cone_base_x_radius: f64,
    cone_base_y_radius: f64,
    cone_top_radius: f64,
    cone_defaults: ConeDefaults,
    plane: WorkingPlane,
}

impl PrimitiveCommand {
    pub fn new(id: &str) -> Self {
        let remembered = cone_defaults()
            .lock()
            .map(|value| *value)
            .unwrap_or_default();
        Self {
            shape: Shape::from_id(id).unwrap_or(Shape::Box),
            pts: Vec::new(),
            height_step: false,
            box_step: BoxStep::FirstCorner,
            box_centered: false,
            box_origin: None,
            box_length: None,
            box_width: None,
            box_angle: 0.0,
            box_width_sign: 1.0,
            box_height_anchor: None,
            sphere_step: SphereStep::Center,
            sphere_default_radius: sphere_radius_default(),
            cone_step: ConeStep::BaseCenter,
            cone_frame: None,
            cone_base_x_radius: remembered.base_x_radius,
            cone_base_y_radius: remembered.base_y_radius,
            cone_top_radius: remembered.top_radius,
            cone_defaults: remembered,
            plane: WorkingPlane::default(),
        }
    }

    fn commit_sphere(&mut self, center: DVec3, radius: f64) -> CmdResult {
        if !center.is_finite() || !radius.is_finite() || radius <= 1e-6 {
            return CmdResult::NeedPoint;
        }
        self.pts = vec![center, center + DVec3::X * radius];
        self.sphere_default_radius = radius;
        remember_sphere_radius(radius);
        self.commit(0.0)
    }

    fn sphere_ttr_result(&mut self, radius: f64) -> CmdResult {
        let SphereStep::TtrRadius {
            first,
            second,
            first_hit,
            second_hit,
        } = self.sphere_step
        else {
            return CmdResult::NeedPoint;
        };
        let hint = (first_hit + second_hit) * 0.5;
        let Some(center) = closest_sphere_center(
            &sphere_ttr_centers(first, second, radius),
            hint,
        ) else {
            return CmdResult::NeedPoint;
        };
        self.commit_sphere(center, radius)
    }

    fn sphere_preview(&self, center: DVec3, radius: f64) -> Option<WireModel> {
        if !center.is_finite() || !radius.is_finite() || radius <= 1e-6 {
            return None;
        }
        Some(self.place_preview(footprint_wire(
            Shape::Sphere,
            &[center, center + DVec3::X * radius],
        )))
    }

    fn cone_frame_at(&self, center: DVec3) -> WorkingPlane {
        WorkingPlane {
            origin: center,
            x: self.plane.x,
            y: self.plane.y,
            z: self.plane.z,
        }
    }

    fn set_cone_base(
        &mut self,
        frame: WorkingPlane,
        base_x_radius: f64,
        base_y_radius: f64,
    ) -> bool {
        if !frame.origin.is_finite()
            || !frame.x.is_finite()
            || !frame.y.is_finite()
            || !frame.z.is_finite()
            || !base_x_radius.is_finite()
            || !base_y_radius.is_finite()
            || base_x_radius < 1e-6
            || base_y_radius < 1e-6
        {
            return false;
        }
        self.cone_frame = Some(frame);
        self.cone_base_x_radius = base_x_radius;
        self.cone_base_y_radius = base_y_radius;
        self.cone_step = ConeStep::Height;
        true
    }

    fn cone_three_point_frame(a: DVec3, b: DVec3, c: DVec3) -> Option<(WorkingPlane, f64)> {
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

    fn cone_axis_frame(&self, axis: DVec3) -> Option<(WorkingPlane, f64)> {
        let current = self.cone_frame?;
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

    fn cone_history_transform(frame: WorkingPlane) -> [f64; 16] {
        glam::DMat4::from_cols(
            frame.x.extend(0.0),
            frame.y.extend(0.0),
            frame.z.extend(0.0),
            frame.origin.extend(1.0),
        )
        .to_cols_array()
    }

    fn build_cone(&self, axis: DVec3) -> Option<(Body, SolidHistoryOperation, f64)> {
        let (frame, height) = self.cone_axis_frame(axis)?;
        let local = solid_model::cone_frustum_solid(
            [0.0; 3],
            self.cone_base_x_radius,
            self.cone_base_y_radius,
            self.cone_top_radius,
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
            crate::scene::model::solid_history::cone_op(
                Self::cone_history_transform(frame),
                self.cone_base_x_radius,
                self.cone_base_y_radius,
                self.cone_top_radius,
                height,
            ),
            height,
        ))
    }

    fn commit_cone_axis(&self, axis: DVec3) -> CmdResult {
        let Some((solid, history, height)) = self.build_cone(axis) else {
            return CmdResult::NeedPoint;
        };
        if let Ok(mut defaults) = cone_defaults().lock() {
            *defaults = ConeDefaults {
                base_x_radius: self.cone_base_x_radius,
                base_y_radius: self.cone_base_y_radius,
                top_radius: self.cone_top_radius,
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

    fn commit_cone_height(&self, height: f64) -> CmdResult {
        let Some(frame) = self.cone_frame else {
            return CmdResult::NeedPoint;
        };
        if !height.is_finite() || height.abs() < 1e-6 {
            return CmdResult::NeedPoint;
        }
        self.commit_cone_axis(frame.z * height)
    }

    fn cone_height_at(&self, point: DVec3) -> Option<f64> {
        let frame = self.cone_frame?;
        Some((point - frame.origin).dot(frame.z))
    }

    fn cone_ttr_center(&self, radius: f64) -> Option<DVec3> {
        let ConeStep::TtrRadius {
            first,
            second,
            first_hit,
            second_hit,
        } = self.cone_step
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

    fn cone_preview(&self, axis: DVec3) -> Option<WireModel> {
        let (frame, height) = self.cone_axis_frame(axis)?;
        let top = frame.origin + frame.z * height;
        let top_y = if self.cone_top_radius > 0.0 {
            self.cone_top_radius * self.cone_base_y_radius / self.cone_base_x_radius
        } else {
            0.0
        };
        let mut points = Vec::new();
        push_ellipse_world(
            &mut points,
            frame.origin,
            frame.x,
            frame.y,
            self.cone_base_x_radius,
            self.cone_base_y_radius,
        );
        if self.cone_top_radius > 1e-9 {
            push_ellipse_world(
                &mut points,
                top,
                frame.x,
                frame.y,
                self.cone_top_radius,
                top_y,
            );
        }
        for index in 0..4 {
            let angle = index as f64 * std::f64::consts::FRAC_PI_2;
            let base = frame.origin
                + frame.x * (self.cone_base_x_radius * angle.cos())
                + frame.y * (self.cone_base_y_radius * angle.sin());
            let upper = if self.cone_top_radius > 1e-9 {
                top + frame.x * (self.cone_top_radius * angle.cos())
                    + frame.y * (top_y * angle.sin())
            } else {
                top
            };
            push_segment(&mut points, base, upper);
        }
        Some(wire("primitive_height_preview", points))
    }

    fn cone_base_preview(&self, frame: WorkingPlane, x_radius: f64, y_radius: f64) -> WireModel {
        let mut points = Vec::new();
        push_ellipse_world(
            &mut points,
            frame.origin,
            frame.x,
            frame.y,
            x_radius.max(1e-9),
            y_radius.max(1e-9),
        );
        wire("primitive_preview", points)
    }

    fn cone_prompt(&self) -> String {
        match self.cone_step {
            ConeStep::BaseCenter =>
                t!("CONE  Specify center point of base or [3P/2P/Ttr/Elliptical]:").into_owned(),
            ConeStep::BaseRadius => crate::tf!(
                "CONE  Specify base radius or [Diameter] <{:.4}>:",
                self.cone_defaults.base_x_radius
            ).into_owned(),
            ConeStep::BaseDiameter => crate::tf!(
                "CONE  Specify base diameter <{:.4}>:",
                self.cone_defaults.base_x_radius * 2.0
            ).into_owned(),
            ConeStep::ThreePointFirst => t!("CONE  Specify first point on base:").into_owned(),
            ConeStep::ThreePointSecond(_) => t!("CONE  Specify second point on base:").into_owned(),
            ConeStep::ThreePointThird(_, _) => t!("CONE  Specify third point on base:").into_owned(),
            ConeStep::TwoPointFirst => t!("CONE  Specify first endpoint of base diameter:").into_owned(),
            ConeStep::TwoPointSecond(_) => t!("CONE  Specify second endpoint of base diameter:").into_owned(),
            ConeStep::EllipseFirst =>
                t!("CONE  Specify endpoint of first axis or [Center]:").into_owned(),
            ConeStep::EllipseCenter => t!("CONE  Specify center point:").into_owned(),
            ConeStep::EllipseCenterFirstAxis(_) => crate::tf!(
                "CONE  Specify distance to first axis <{:.4}>:",
                self.cone_defaults.base_x_radius
            )
            .into_owned(),
            ConeStep::EllipseCenterSecondAxis(_, _) =>
                t!("CONE  Specify endpoint of second axis:").into_owned(),
            ConeStep::EllipseSecond(_) => t!("CONE  Specify second endpoint of ellipse axis:").into_owned(),
            ConeStep::EllipseThird(_, _) => t!("CONE  Specify distance to other ellipse axis:").into_owned(),
            ConeStep::TtrFirst => t!("CONE  Select first tangent object:").into_owned(),
            ConeStep::TtrSecond { .. } => t!("CONE  Select second tangent object:").into_owned(),
            ConeStep::TtrRadius { .. } => crate::tf!(
                "CONE  Specify base radius <{:.4}>:",
                self.cone_defaults.base_x_radius
            ).into_owned(),
            ConeStep::Height => crate::tf!(
                "CONE  Specify height or [2Point/Axis endpoint/Top radius] <{:.4}>:",
                self.cone_defaults.height
            ).into_owned(),
            ConeStep::HeightAfterTopRadius => crate::tf!(
                "CONE  Specify height or [2Point/Axis endpoint] <{:.4}>:",
                self.cone_defaults.height
            )
            .into_owned(),
            ConeStep::HeightFirstPoint => t!("CONE  Specify first point for height:").into_owned(),
            ConeStep::HeightSecondPoint(_) => t!("CONE  Specify second point for height:").into_owned(),
            ConeStep::AxisEndpoint => t!("CONE  Specify axis endpoint:").into_owned(),
            ConeStep::TopRadius => crate::tf!(
                "CONE  Specify top radius <{:.4}>:",
                self.cone_defaults.top_radius
            ).into_owned(),
        }
    }

    fn cone_options(&self) -> Vec<CmdOption> {
        match self.cone_step {
            ConeStep::BaseCenter => vec![
                CmdOption::new("3P", "3P"),
                CmdOption::new("2P", "2P"),
                CmdOption::new("Ttr", "TTR"),
                CmdOption::new(t!("Elliptical").as_ref(), "E"),
            ],
            ConeStep::BaseRadius => vec![CmdOption::new(t!("Diameter").as_ref(), "D")],
            ConeStep::EllipseFirst => vec![CmdOption::new(t!("Center").as_ref(), "C")],
            ConeStep::Height => vec![
                CmdOption::new("2Point", "2P"),
                CmdOption::new(t!("Axis endpoint").as_ref(), "A"),
                CmdOption::new(t!("Top radius").as_ref(), "T"),
            ],
            ConeStep::HeightAfterTopRadius => vec![
                CmdOption::new("2Point", "2P"),
                CmdOption::new(t!("Axis endpoint").as_ref(), "A"),
            ],
            _ => Vec::new(),
        }
    }

    fn on_cone_point(&mut self, point: DVec3) -> CmdResult {
        match self.cone_step {
            ConeStep::BaseCenter => {
                self.cone_frame = Some(self.cone_frame_at(point));
                self.cone_step = ConeStep::BaseRadius;
                CmdResult::NeedPoint
            }
            ConeStep::BaseRadius => {
                let Some(frame) = self.cone_frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                self.set_cone_base(frame, radius, radius);
                CmdResult::NeedPoint
            }
            ConeStep::BaseDiameter => {
                let Some(frame) = self.cone_frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y) * 0.5;
                self.set_cone_base(frame, radius, radius);
                CmdResult::NeedPoint
            }
            ConeStep::ThreePointFirst => {
                self.cone_step = ConeStep::ThreePointSecond(point);
                CmdResult::NeedPoint
            }
            ConeStep::ThreePointSecond(first) => {
                if point.distance_squared(first) > 1e-12 {
                    self.cone_step = ConeStep::ThreePointThird(first, point);
                }
                CmdResult::NeedPoint
            }
            ConeStep::ThreePointThird(first, second) => {
                if let Some((frame, radius)) = Self::cone_three_point_frame(first, second, point) {
                    self.set_cone_base(frame, radius, radius);
                }
                CmdResult::NeedPoint
            }
            ConeStep::TwoPointFirst => {
                self.cone_step = ConeStep::TwoPointSecond(point);
                CmdResult::NeedPoint
            }
            ConeStep::TwoPointSecond(first) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(point);
                let radius = (b - a).truncate().length() * 0.5;
                let center = self.plane.to_world((a + b) * 0.5);
                self.set_cone_base(self.cone_frame_at(center), radius, radius);
                CmdResult::NeedPoint
            }
            ConeStep::EllipseFirst => {
                self.cone_step = ConeStep::EllipseSecond(point);
                CmdResult::NeedPoint
            }
            ConeStep::EllipseCenter => {
                self.cone_step = ConeStep::EllipseCenterFirstAxis(point);
                CmdResult::NeedPoint
            }
            ConeStep::EllipseCenterFirstAxis(center) => {
                if point.distance_squared(center) > 1e-12 {
                    self.cone_step = ConeStep::EllipseCenterSecondAxis(center, point);
                }
                CmdResult::NeedPoint
            }
            ConeStep::EllipseCenterSecondAxis(center, first_axis_end) => {
                let axis = first_axis_end - center;
                if let Some(x) = axis.try_normalize() {
                    let y = self.plane.z.cross(x).normalize_or_zero();
                    let frame = WorkingPlane::new(center, x, y);
                    let local = frame.to_local(point);
                    self.set_cone_base(frame, axis.length(), local.y.abs());
                }
                CmdResult::NeedPoint
            }
            ConeStep::EllipseSecond(first) => {
                if point.distance_squared(first) > 1e-12 {
                    self.cone_step = ConeStep::EllipseThird(first, point);
                }
                CmdResult::NeedPoint
            }
            ConeStep::EllipseThird(first, second) => {
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
                    self.set_cone_base(frame, x_radius, local.y.abs());
                }
                CmdResult::NeedPoint
            }
            ConeStep::TtrRadius { second_hit, .. } => {
                let local = self.plane.to_local(point);
                let radius = (local - second_hit).truncate().length();
                if let Some(center) = self.cone_ttr_center(radius) {
                    self.set_cone_base(self.cone_frame_at(center), radius, radius);
                }
                CmdResult::NeedPoint
            }
            ConeStep::Height | ConeStep::HeightAfterTopRadius => self
                .cone_height_at(point)
                .map(|height| self.commit_cone_height(height))
                .unwrap_or(CmdResult::NeedPoint),
            ConeStep::HeightFirstPoint => {
                self.cone_step = ConeStep::HeightSecondPoint(point);
                CmdResult::NeedPoint
            }
            ConeStep::HeightSecondPoint(first) => self.commit_cone_height(first.distance(point)),
            ConeStep::AxisEndpoint => {
                let Some(frame) = self.cone_frame else {
                    return CmdResult::NeedPoint;
                };
                self.commit_cone_axis(point - frame.origin)
            }
            ConeStep::TopRadius => {
                let Some(frame) = self.cone_frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                if radius.is_finite() {
                    self.cone_top_radius = radius;
                    self.cone_step = ConeStep::HeightAfterTopRadius;
                }
                CmdResult::NeedPoint
            }
            ConeStep::TtrFirst | ConeStep::TtrSecond { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_cone_tangent(&mut self, object: TangentObject, hit: DVec3) -> CmdResult {
        let object = crate::modules::draw::draw::circle::tangent_object_local(object, self.plane);
        let hit = self.plane.to_local(hit);
        match self.cone_step {
            ConeStep::TtrFirst => {
                self.cone_step = ConeStep::TtrSecond { object, hit };
            }
            ConeStep::TtrSecond {
                object: first,
                hit: first_hit,
            } => {
                self.cone_step = ConeStep::TtrRadius {
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

    fn on_cone_text(&mut self, raw: &str) -> Option<CmdResult> {
        let token = raw.trim();
        let upper = token.to_uppercase();
        match self.cone_step {
            ConeStep::BaseCenter => {
                self.cone_step = match upper.as_str() {
                    "3P" => ConeStep::ThreePointFirst,
                    "2P" => ConeStep::TwoPointFirst,
                    "T" | "TTR" => ConeStep::TtrFirst,
                    "E" | "ELLIPTICAL" => ConeStep::EllipseFirst,
                    _ => return None,
                };
                return Some(CmdResult::NeedPoint);
            }
            ConeStep::BaseRadius if matches!(upper.as_str(), "D" | "DIAMETER") => {
                self.cone_step = ConeStep::BaseDiameter;
                return Some(CmdResult::NeedPoint);
            }
            ConeStep::EllipseFirst if matches!(upper.as_str(), "C" | "CENTER") => {
                self.cone_step = ConeStep::EllipseCenter;
                return Some(CmdResult::NeedPoint);
            }
            ConeStep::Height => {
                self.cone_step = match upper.as_str() {
                    "2P" | "2POINT" => ConeStep::HeightFirstPoint,
                    "A" | "AXIS" | "AXIS ENDPOINT" => ConeStep::AxisEndpoint,
                    "T" | "TOP" | "TOP RADIUS" => ConeStep::TopRadius,
                    _ => {
                        let height = crate::entities::common::parse_typed_length(token)?;
                        return Some(self.commit_cone_height(height));
                    }
                };
                return Some(CmdResult::NeedPoint);
            }
            ConeStep::HeightAfterTopRadius => {
                self.cone_step = match upper.as_str() {
                    "2P" | "2POINT" => ConeStep::HeightFirstPoint,
                    "A" | "AXIS" | "AXIS ENDPOINT" => ConeStep::AxisEndpoint,
                    _ => {
                        let height = crate::entities::common::parse_typed_length(token)?;
                        return Some(self.commit_cone_height(height));
                    }
                };
                return Some(CmdResult::NeedPoint);
            }
            _ => {}
        }
        let number = crate::entities::common::parse_typed_length(token)?;
        match self.cone_step {
            ConeStep::BaseRadius => {
                let frame = self.cone_frame?;
                (number > 0.0).then(|| {
                    self.set_cone_base(frame, number, number);
                    CmdResult::NeedPoint
                })
            }
            ConeStep::BaseDiameter => {
                let frame = self.cone_frame?;
                (number > 0.0).then(|| {
                    self.set_cone_base(frame, number * 0.5, number * 0.5);
                    CmdResult::NeedPoint
                })
            }
            ConeStep::EllipseCenterFirstAxis(center) => (number > 0.0).then(|| {
                self.cone_step = ConeStep::EllipseCenterSecondAxis(
                    center,
                    center + self.plane.x * number,
                );
                CmdResult::NeedPoint
            }),
            ConeStep::TtrRadius { .. } => {
                let center = self.cone_ttr_center(number)?;
                (number > 0.0).then(|| {
                    self.set_cone_base(self.cone_frame_at(center), number, number);
                    CmdResult::NeedPoint
                })
            }
            ConeStep::TopRadius if number >= 0.0 => {
                self.cone_top_radius = number;
                self.cone_step = ConeStep::HeightAfterTopRadius;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn cone_mouse_move(&self, point: DVec3) -> Option<WireModel> {
        match self.cone_step {
            ConeStep::BaseRadius => {
                let frame = self.cone_frame?;
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                Some(self.cone_base_preview(frame, radius, radius))
            }
            ConeStep::BaseDiameter => {
                let frame = self.cone_frame?;
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y) * 0.5;
                Some(self.cone_base_preview(frame, radius, radius))
            }
            ConeStep::ThreePointSecond(first) | ConeStep::TwoPointSecond(first) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(point);
                let center = self.plane.to_world((a + b) * 0.5);
                let radius = (b - a).truncate().length() * 0.5;
                Some(self.cone_base_preview(self.cone_frame_at(center), radius, radius))
            }
            ConeStep::ThreePointThird(first, second) => {
                let (frame, radius) = Self::cone_three_point_frame(first, second, point)?;
                Some(self.cone_base_preview(frame, radius, radius))
            }
            ConeStep::EllipseThird(first, second) => {
                let a = self.plane.to_local(first);
                let b = self.plane.to_local(second);
                let center_local = (a + b) * 0.5;
                let axis = (b - a).truncate();
                let x2 = axis.try_normalize()?;
                let x = self.plane.vector_to_world(DVec3::new(x2.x, x2.y, 0.0));
                let y = self.plane.z.cross(x).normalize_or_zero();
                let frame = WorkingPlane::new(self.plane.to_world(center_local), x, y);
                let local = frame.to_local(point);
                Some(self.cone_base_preview(frame, axis.length() * 0.5, local.y.abs()))
            }
            ConeStep::EllipseCenterSecondAxis(center, first_axis_end) => {
                let axis = first_axis_end - center;
                let x = axis.try_normalize()?;
                let y = self.plane.z.cross(x).normalize_or_zero();
                let frame = WorkingPlane::new(center, x, y);
                let local = frame.to_local(point);
                Some(self.cone_base_preview(frame, axis.length(), local.y.abs()))
            }
            ConeStep::TtrRadius { second_hit, .. } => {
                let local = self.plane.to_local(point);
                let radius = (local - second_hit).truncate().length();
                let center = self.cone_ttr_center(radius)?;
                Some(self.cone_base_preview(self.cone_frame_at(center), radius, radius))
            }
            ConeStep::Height | ConeStep::HeightAfterTopRadius => {
                let frame = self.cone_frame?;
                self.cone_preview(frame.z * self.cone_height_at(point)?)
            }
            ConeStep::HeightSecondPoint(first) => {
                let frame = self.cone_frame?;
                self.cone_preview(frame.z * first.distance(point))
            }
            ConeStep::AxisEndpoint => {
                let frame = self.cone_frame?;
                self.cone_preview(point - frame.origin)
            }
            _ => None,
        }
    }

    /// Number of footprint points the shape needs before the height step.
    fn footprint_pts(&self) -> usize {
        match self.shape {
            Shape::Torus => 3, // centre, major-radius, minor-radius
            _ => 2,            // corner/corner  or  centre/radius
        }
    }

    /// A reasonable default height when the user just presses Enter.
    fn default_height(&self) -> f64 {
        if self.shape == Shape::Wedge {
            let bits = LAST_WEDGE_HEIGHT.load(Ordering::Relaxed);
            if bits != 0 {
                let height = f64::from_bits(bits);
                if height.is_finite() && height >= 1e-6 {
                    return height;
                }
            }
        }
        let height = match self.shape {
            Shape::Box | Shape::Wedge => match (self.box_length, self.box_width) {
                (Some(length), Some(width)) => length.max(width),
                _ if self.pts.len() >= 2 => {
                    let d = self.pts[1] - self.pts[0];
                    d.x.abs().max(d.y.abs())
                }
                _ => 1.0,
            },
            _ if self.pts.len() >= 2 => (self.pts[1] - self.pts[0]).length(),
            _ => 1.0,
        };
        height.max(1.0)
    }

    fn cursor_height(&self, point: DVec3) -> f64 {
        let height = self.plane.to_local(point).z - self.pts[0].z;
        if self.shape.rectangular() {
            height
        } else {
            height.max(1e-6)
        }
    }

    fn place_preview(&self, mut preview: WireModel) -> WireModel {
        for point in &mut preview.points {
            if !point[0].is_nan() {
                *point = self
                    .plane
                    .to_world(glam::Vec3::from_array(*point).as_dvec3())
                    .as_vec3()
                    .to_array();
            }
        }
        preview
    }

    fn history_transform(&self, origin: DVec3) -> [f64; 16] {
        glam::DMat4::from_cols(
            self.plane.x.extend(0.0),
            self.plane.y.extend(0.0),
            self.plane.z.extend(0.0),
            self.plane.to_world(origin).extend(1.0),
        )
        .to_cols_array()
    }

    fn history_transform_axes(
        &self,
        origin: DVec3,
        x_axis: DVec3,
        y_axis: DVec3,
    ) -> [f64; 16] {
        let to_world_vector = |value: DVec3| {
            self.plane.x * value.x + self.plane.y * value.y + self.plane.z * value.z
        };
        glam::DMat4::from_cols(
            to_world_vector(x_axis).extend(0.0),
            to_world_vector(y_axis).extend(0.0),
            self.plane.z.extend(0.0),
            self.plane.to_world(origin).extend(1.0),
        )
        .to_cols_array()
    }

    fn box_spec(&self, height: f64) -> Option<(DVec3, DVec3, DVec3, f64, f64, f64)> {
        let (mut origin, x_axis, y_axis, length, width) =
            if let (Some(origin), Some(length), Some(width)) =
                (self.box_origin, self.box_length, self.box_width)
            {
                let (sin, cos) = self.box_angle.sin_cos();
                (
                    origin,
                    DVec3::new(cos, sin, 0.0),
                    DVec3::new(-sin, cos, 0.0) * self.box_width_sign,
                    length,
                    width,
                )
            } else {
                let (a, b) = (*self.pts.first()?, *self.pts.get(1)?);
                (
                    DVec3::new(a.x.min(b.x), a.y.min(b.y), a.z),
                    DVec3::X,
                    DVec3::Y,
                    (b.x - a.x).abs(),
                    (b.y - a.y).abs(),
                )
            };
        let height_abs = height.abs();
        if !origin.is_finite()
            || !x_axis.is_finite()
            || !y_axis.is_finite()
            || !length.is_finite()
            || !width.is_finite()
            || !height_abs.is_finite()
            || length < 1e-6
            || width < 1e-6
            || height_abs < 1e-6
        {
            return None;
        }
        if height < 0.0 {
            origin.z += height;
        }
        if !origin.is_finite() {
            return None;
        }
        Some((origin, x_axis, y_axis, length, width, height_abs))
    }

    fn build_box(&self, height: f64) -> Option<(Body, SolidHistoryOperation)> {
        use crate::scene::model::solid_history;

        let (origin, x_axis, y_axis, length, width, height) = self.box_spec(height)?;
        let base = solid_model::box_solid(
            [length * 0.5, width * 0.5, height * 0.5],
            length,
            width,
            height,
        )?;
        let placed = solid_model::placed(
            &base,
            x_axis.to_array(),
            y_axis.to_array(),
            DVec3::Z.to_array(),
            origin.to_array(),
        )?;
        Some((
            placed,
            solid_history::box_op(
                self.history_transform_axes(origin, x_axis, y_axis),
                length,
                width,
                height,
            ),
        ))
    }

    fn set_box_corner_footprint(&mut self, point: DVec3) -> bool {
        let first = self.pts[0];
        let delta = point - first;
        if delta.x.abs() < 1e-6 || delta.y.abs() < 1e-6 {
            return false;
        }
        if self.box_centered {
            self.pts = vec![
                DVec3::new(first.x - delta.x.abs(), first.y - delta.y.abs(), first.z),
                DVec3::new(first.x + delta.x.abs(), first.y + delta.y.abs(), first.z),
            ];
        } else {
            self.pts.push(point);
        }
        self.box_step = BoxStep::Height;
        self.height_step = true;
        true
    }

    fn set_box_cube(&mut self, size: f64, angle: f64) -> Option<CmdResult> {
        let size = size.abs();
        if !size.is_finite() || !angle.is_finite() || size < 1e-6 {
            return None;
        }
        let (sin, cos) = angle.sin_cos();
        let x_axis = DVec3::new(cos, sin, 0.0);
        let y_axis = DVec3::new(-sin, cos, 0.0);
        let origin = if self.box_centered {
            self.pts[0] - x_axis * size * 0.5 - y_axis * size * 0.5
        } else {
            self.pts[0]
        };
        self.box_origin = Some(origin);
        self.box_length = Some(size);
        self.box_width = Some(size);
        self.box_angle = angle;
        self.box_width_sign = 1.0;
        Some(self.commit(size))
    }

    fn set_box_length(&mut self, length: f64, angle: f64) -> bool {
        let length = length.abs();
        if !length.is_finite() || !angle.is_finite() || length < 1e-6 {
            return false;
        }
        self.box_origin = self.pts.first().copied();
        self.box_length = Some(length);
        self.box_angle = angle;
        self.box_step = BoxStep::Width;
        true
    }

    fn set_box_width(&mut self, width: f64, sign: f64) -> bool {
        let width = width.abs();
        if !width.is_finite() || !sign.is_finite() || width < 1e-6 {
            return false;
        }
        self.box_width = Some(width);
        self.box_width_sign = if sign < 0.0 { -1.0 } else { 1.0 };
        if self.box_centered {
            let center = self.pts[0];
            let (sin, cos) = self.box_angle.sin_cos();
            let x_axis = DVec3::new(cos, sin, 0.0);
            let y_axis = DVec3::new(-sin, cos, 0.0) * self.box_width_sign;
            self.box_origin = Some(
                center
                    - x_axis * self.box_length.unwrap_or_default() * 0.5
                    - y_axis * width * 0.5,
            );
        }
        self.box_step = BoxStep::Height;
        self.height_step = true;
        true
    }

    fn rectangular_preview(&self, height: f64) -> Option<WireModel> {
        let (origin, x_axis, y_axis, length, width, height) = self.box_spec(height)?;
        let mut points = Vec::new();
        if self.shape == Shape::Wedge {
            let near = [
                origin,
                origin + x_axis * length,
                origin + DVec3::Z * height,
            ];
            let far = near.map(|point| point + y_axis * width);
            push_loop(&mut points, &near);
            push_loop(&mut points, &far);
            for index in 0..3 {
                push_segment(&mut points, near[index], far[index]);
            }
        } else {
            let base = [
                origin,
                origin + x_axis * length,
                origin + x_axis * length + y_axis * width,
                origin + y_axis * width,
            ];
            let top = base.map(|point| point + DVec3::Z * height);
            push_loop(&mut points, &base);
            push_loop(&mut points, &top);
            for index in 0..4 {
                push_segment(&mut points, base[index], top[index]);
            }
        }
        Some(self.place_preview(wire("primitive_height_preview", points)))
    }

    fn build_wedge(&self, height: f64) -> Option<(Body, SolidHistoryOperation)> {
        use crate::scene::model::solid_history;

        let (origin, x_axis, y_axis, length, width, height) = self.box_spec(height)?;
        let base = solid_model::wedge_solid([0.0; 3], length, width, height)?;
        let placed = solid_model::placed(
            &base,
            x_axis.to_array(),
            y_axis.to_array(),
            DVec3::Z.to_array(),
            origin.to_array(),
        )?;
        Some((
            placed,
            solid_history::wedge_op(
                self.history_transform_axes(origin, x_axis, y_axis),
                length,
                width,
                height,
            ),
        ))
    }

    /// Build the persistent kernel body and its history node.
    fn build(&self, height: f64) -> Option<(Body, SolidHistoryOperation)> {
        use crate::scene::model::solid_history;

        let (solid, history) = match self.shape {
            Shape::Box => return self.build_box(height),
            Shape::Wedge => return self.build_wedge(height),
            Shape::Cylinder => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 || height < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                (
                    solid_model::cylinder_solid(center, r, height),
                    solid_history::cylinder_op(self.history_transform(c), r, height),
                )
            }
            Shape::Cone => return None,
            Shape::Sphere => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                (
                    solid_model::sphere_solid(center, r),
                    solid_history::sphere_op(self.history_transform(c), r),
                )
            }
            Shape::Pyramid => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 || height < 1e-6 {
                    return None;
                }
                (
                    solid_model::pyramid_solid([c.x, c.y, c.z], r, height, 4),
                    solid_history::pyramid_op(self.history_transform(c), r, height, 4),
                )
            }
            Shape::Torus => {
                let c = self.pts[0];
                let first = (self.pts[1] - c).length();
                let second = (self.pts[2] - c).length();
                let outer = first.max(second);
                let inner = first.min(second);
                if inner < 1e-6 || outer - inner < 1e-6 {
                    return None;
                }
                let major = (outer + inner) * 0.5;
                let minor = (outer - inner) * 0.5;
                let center = [c.x, c.y, c.z];
                (
                    solid_model::torus_solid(center, major, minor),
                    solid_history::torus_op(
                        self.history_transform(c),
                        major,
                        minor,
                    ),
                )
            }
        };
        let solid = solid?;
        Some((solid, history))
    }

    fn commit(&self, height: f64) -> CmdResult {
        match self.build(height) {
            Some((solid, history)) => {
                if self.shape == Shape::Wedge {
                    LAST_WEDGE_HEIGHT.store(height.abs().to_bits(), Ordering::Relaxed);
                }
                // Place the local body on the working plane.
                let placed = solid_model::placed(
                    &solid,
                    [self.plane.x.x, self.plane.x.y, self.plane.x.z],
                    [self.plane.y.x, self.plane.y.y, self.plane.y.z],
                    [self.plane.z.x, self.plane.z.y, self.plane.z.z],
                    [
                        self.plane.origin.x,
                        self.plane.origin.y,
                        self.plane.origin.z,
                    ],
                );
                match placed {
                    Some(placed) => {
                        let Some(document) =
                            crate::scene::convert::acis_export::solid_to_sat(&placed)
                        else {
                            return CmdResult::Cancel;
                        };
                        let mut entity = Solid3D::new();
                        entity.set_sat_document(&document);
                        entity.wires = solid_model::edge_wires(&placed);
                        CmdResult::CommitSolid {
                            entity: EntityType::Solid3D(entity),
                            solid: Box::new(placed),
                            history,
                        }
                    }
                    None => CmdResult::Cancel,
                }
            }
            None => CmdResult::Cancel,
        }
    }
}

impl CadCommand for PrimitiveCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn cursor_axis(&self) -> Option<(DVec3, DVec3)> {
        if self.shape == Shape::Cone {
            return matches!(self.cone_step, ConeStep::Height | ConeStep::HeightAfterTopRadius)
                .then(|| {
                let frame = self.cone_frame.unwrap_or(self.plane);
                (frame.origin, frame.z.normalize_or_zero())
            });
        }
        let constrained = if self.shape.rectangular() {
            self.box_step == BoxStep::Height
        } else {
            self.height_step
        };
        constrained.then(|| {
            (
                self.plane.to_world(self.pts[0]),
                self.plane.z.normalize_or_zero(),
            )
        })
    }

    fn name(&self) -> &'static str {
        self.shape.name()
    }

    fn needs_tangent_pick(&self) -> bool {
        (self.shape == Shape::Sphere
            && matches!(
                self.sphere_step,
                SphereStep::TtrFirst | SphereStep::TtrSecond { .. }
            ))
            || (self.shape == Shape::Cone
                && matches!(
                    self.cone_step,
                    ConeStep::TtrFirst | ConeStep::TtrSecond { .. }
                ))
    }

    fn on_tangent_point(&mut self, object: TangentObject, hit: DVec3) -> CmdResult {
        if self.shape == Shape::Cone {
            return self.on_cone_tangent(object, hit);
        }
        if self.shape != Shape::Sphere {
            return self.on_point(hit);
        }
        let object = sphere_tangent_local(object, self.plane);
        let hit = self.plane.to_local(hit);
        match self.sphere_step {
            SphereStep::TtrFirst => {
                self.sphere_step = SphereStep::TtrSecond { object, hit };
                CmdResult::NeedPoint
            }
            SphereStep::TtrSecond {
                object: first,
                hit: first_hit,
            } => {
                self.sphere_step = SphereStep::TtrRadius {
                    first,
                    second: object,
                    first_hit,
                    second_hit: hit,
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn prompt(&self) -> String {
        let n = self.shape.name();
        if self.shape == Shape::Cone {
            return self.cone_prompt();
        }
        if self.shape.rectangular() {
            return match self.box_step {
                BoxStep::FirstCorner => t!("%{n}  Specify first corner or [Center]:", n = n),
                BoxStep::CenterPoint => t!("%{n}  Specify center point:", n = n),
                BoxStep::OppositeCorner => {
                    t!("%{n}  Specify other corner or [Cube/Length]:", n = n)
                }
                BoxStep::CubeSize => t!("%{n}  Specify cube length:", n = n),
                BoxStep::Length => t!("%{n}  Specify length:", n = n),
                BoxStep::Width => t!("%{n}  Specify width:", n = n),
                BoxStep::Height if self.shape == Shape::Wedge => t!(
                    "%{n}  Specify height or [2Point] <%{height}>:",
                    n = n,
                    height = format!("{:.4}", self.default_height())
                ),
                BoxStep::Height => t!("%{n}  Specify height or [2Point]:", n = n),
                BoxStep::HeightFirstPoint => {
                    t!("%{n}  Specify first point for height:", n = n)
                }
                BoxStep::HeightSecondPoint => {
                    t!("%{n}  Specify second point for height:", n = n)
                }
            }
            .into_owned();
        }
        if self.shape == Shape::Sphere {
            return match self.sphere_step {
                SphereStep::Center => {
                    t!("SPHERE  Specify center point or [3P/2P/Ttr]:").into_owned()
                }
                SphereStep::Radius(_) => crate::tf!(
                    "SPHERE  Specify radius or [Diameter] <{:.4}>:",
                    self.sphere_default_radius
                )
                .into_owned(),
                SphereStep::Diameter(_) => crate::tf!(
                    "SPHERE  Specify diameter <{:.4}>:",
                    self.sphere_default_radius * 2.0
                )
                .into_owned(),
                SphereStep::TwoPointFirst => {
                    t!("SPHERE  Specify first end point of diameter:").into_owned()
                }
                SphereStep::TwoPointSecond(_) => {
                    t!("SPHERE  Specify second end point of diameter:").into_owned()
                }
                SphereStep::ThreePointFirst => {
                    t!("SPHERE  Specify first point on sphere:").into_owned()
                }
                SphereStep::ThreePointSecond(_) => {
                    t!("SPHERE  Specify second point on sphere:").into_owned()
                }
                SphereStep::ThreePointThird(_, _) => {
                    t!("SPHERE  Specify third point on sphere:").into_owned()
                }
                SphereStep::TtrFirst => {
                    t!("SPHERE  Select first tangent object:").into_owned()
                }
                SphereStep::TtrSecond { .. } => {
                    t!("SPHERE  Select second tangent object:").into_owned()
                }
                SphereStep::TtrRadius { .. } => crate::tf!(
                    "SPHERE  Specify radius <{:.4}>:",
                    self.sphere_default_radius
                )
                .into_owned(),
            };
        }
        if self.height_step {
            return t!("%{n}  Specify height <Enter for default>:", n = n).into_owned();
        }
        match (self.shape, self.pts.len()) {
            (Shape::Torus, 0) => t!("%{n}  Specify center point:", n = n).into_owned(),
            (Shape::Torus, 1) => t!("%{n}  Specify outer radius:", n = n).into_owned(),
            (Shape::Torus, _) => t!("%{n}  Specify inner radius:", n = n).into_owned(),
            (shape, 0) if shape.radial() => {
                t!("%{n}  Specify center point:", n = n).into_owned()
            }
            (shape, _) if shape.radial() => {
                t!("%{n}  Specify radius:", n = n).into_owned()
            }
            (_, 0) => t!("%{n}  Specify first corner:", n = n).into_owned(),
            (_, _) => t!("%{n}  Specify opposite corner:", n = n).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.shape == Shape::Sphere {
            return match self.sphere_step {
                SphereStep::Center => vec![
                    CmdOption::new("3P", "3P"),
                    CmdOption::new("2P", "2P"),
                    CmdOption::new("Ttr", "T"),
                ],
                SphereStep::Radius(_) => vec![CmdOption::new(t!("Diameter").as_ref(), "D")],
                _ => Vec::new(),
            };
        }
        if self.shape == Shape::Cone {
            return self.cone_options();
        }
        if !self.shape.rectangular() {
            return Vec::new();
        }
        match self.box_step {
            BoxStep::FirstCorner => vec![CmdOption::new(t!("Center").as_ref(), "C")],
            BoxStep::OppositeCorner => vec![
                CmdOption::new(t!("Cube").as_ref(), "C"),
                CmdOption::new(t!("Length").as_ref(), "L"),
            ],
            BoxStep::Height => vec![CmdOption::new(t!("2Point").as_ref(), "2P")],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.shape == Shape::Sphere {
            let local = self.plane.to_local(pt);
            if !local.is_finite() {
                return CmdResult::NeedPoint;
            }
            return match self.sphere_step {
                SphereStep::Center => {
                    self.sphere_step = SphereStep::Radius(local);
                    CmdResult::NeedPoint
                }
                SphereStep::Radius(center) => self.commit_sphere(center, center.distance(local)),
                SphereStep::Diameter(center) => {
                    self.commit_sphere(center, center.distance(local) * 0.5)
                }
                SphereStep::TwoPointFirst => {
                    self.sphere_step = SphereStep::TwoPointSecond(local);
                    CmdResult::NeedPoint
                }
                SphereStep::TwoPointSecond(first) => {
                    self.commit_sphere((first + local) * 0.5, first.distance(local) * 0.5)
                }
                SphereStep::ThreePointFirst => {
                    self.sphere_step = SphereStep::ThreePointSecond(local);
                    CmdResult::NeedPoint
                }
                SphereStep::ThreePointSecond(first) => {
                    self.sphere_step = SphereStep::ThreePointThird(first, local);
                    CmdResult::NeedPoint
                }
                SphereStep::ThreePointThird(first, second) => {
                    let Some((center, radius)) =
                        sphere_through_three_points(first, second, local)
                    else {
                        return CmdResult::NeedPoint;
                    };
                    self.commit_sphere(center, radius)
                }
                SphereStep::TtrRadius { second_hit, .. } => {
                    self.sphere_ttr_result(second_hit.distance(local))
                }
                SphereStep::TtrFirst | SphereStep::TtrSecond { .. } => CmdResult::NeedPoint,
            };
        }
        if self.shape == Shape::Cone {
            return self.on_cone_point(pt);
        }
        if self.shape.rectangular() {
            let local = self.plane.to_local(pt);
            if !local.is_finite() {
                return CmdResult::NeedPoint;
            }
            return match self.box_step {
                BoxStep::FirstCorner | BoxStep::CenterPoint => {
                    self.pts = vec![local];
                    self.box_step = BoxStep::OppositeCorner;
                    CmdResult::NeedPoint
                }
                BoxStep::OppositeCorner => {
                    let height = local.z - self.pts[0].z;
                    if !self.set_box_corner_footprint(local) {
                        CmdResult::NeedPoint
                    } else if self.shape == Shape::Wedge && height.abs() >= 1e-6 {
                        self.commit(height)
                    } else {
                        CmdResult::NeedPoint
                    }
                }
                BoxStep::CubeSize => {
                    let delta = local - self.pts[0];
                    let (size, angle) = if self.shape == Shape::Wedge {
                        (delta.x.hypot(delta.y), delta.y.atan2(delta.x))
                    } else {
                        (delta.length(), 0.0)
                    };
                    self.set_box_cube(size, angle)
                    .unwrap_or(CmdResult::NeedPoint)
                }
                BoxStep::Length => {
                    let delta = local - self.pts[0];
                    if !self.set_box_length(delta.x.hypot(delta.y), delta.y.atan2(delta.x)) {
                        return CmdResult::NeedPoint;
                    }
                    CmdResult::NeedPoint
                }
                BoxStep::Width => {
                    let delta = local - self.pts[0];
                    let normal = DVec3::new(-self.box_angle.sin(), self.box_angle.cos(), 0.0);
                    let signed = delta.dot(normal);
                    if !self.set_box_width(signed, signed) {
                        return CmdResult::NeedPoint;
                    }
                    CmdResult::NeedPoint
                }
                BoxStep::Height => {
                    let height = self.cursor_height(pt);
                    if height.abs() < 1e-6 {
                        CmdResult::NeedPoint
                    } else {
                        self.commit(height)
                    }
                }
                BoxStep::HeightFirstPoint => {
                    self.box_height_anchor = Some(local);
                    self.box_step = BoxStep::HeightSecondPoint;
                    CmdResult::NeedPoint
                }
                BoxStep::HeightSecondPoint => {
                    let Some(first) = self.box_height_anchor else {
                        return CmdResult::NeedPoint;
                    };
                    let height = (local - first).length();
                    if height < 1e-6 {
                        CmdResult::NeedPoint
                    } else {
                        self.commit(height)
                    }
                }
            };
        }
        if self.height_step {
            return self.commit(self.cursor_height(pt));
        }
        self.pts.push(self.plane.to_local(pt));
        if self.pts.len() < self.footprint_pts() {
            return CmdResult::NeedPoint;
        }
        // Footprint complete.
        if self.shape.needs_height() {
            self.height_step = true;
            CmdResult::NeedPoint
        } else {
            self.commit(0.0)
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.shape == Shape::Sphere {
            return match self.sphere_step {
                SphereStep::Radius(center) => {
                    self.commit_sphere(center, self.sphere_default_radius)
                }
                SphereStep::Diameter(center) => {
                    self.commit_sphere(center, self.sphere_default_radius)
                }
                SphereStep::TtrRadius { .. } => {
                    self.sphere_ttr_result(self.sphere_default_radius)
                }
                _ => CmdResult::Cancel,
            };
        }
        if self.shape == Shape::Cone {
            return match self.cone_step {
                ConeStep::BaseRadius | ConeStep::BaseDiameter => {
                    let Some(frame) = self.cone_frame else {
                        return CmdResult::Cancel;
                    };
                    self.set_cone_base(
                        frame,
                        self.cone_defaults.base_x_radius,
                        self.cone_defaults.base_y_radius,
                    );
                    CmdResult::NeedPoint
                }
                ConeStep::TtrRadius { .. } => {
                    let radius = self.cone_defaults.base_x_radius;
                    let Some(center) = self.cone_ttr_center(radius) else {
                        return CmdResult::NeedPoint;
                    };
                    self.set_cone_base(self.cone_frame_at(center), radius, radius);
                    CmdResult::NeedPoint
                }
                ConeStep::EllipseCenterFirstAxis(center) => {
                    self.cone_step = ConeStep::EllipseCenterSecondAxis(
                        center,
                        center + self.plane.x * self.cone_defaults.base_x_radius,
                    );
                    CmdResult::NeedPoint
                }
                ConeStep::Height | ConeStep::HeightAfterTopRadius => {
                    self.commit_cone_height(self.cone_defaults.height)
                }
                ConeStep::TopRadius => {
                    self.cone_top_radius = self.cone_defaults.top_radius;
                    self.cone_step = ConeStep::HeightAfterTopRadius;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::Cancel,
            };
        }
        if self.shape.rectangular() {
            return if self.shape == Shape::Wedge && self.box_step == BoxStep::Height {
                self.commit(self.default_height())
            } else {
                CmdResult::Cancel
            };
        }
        if self.height_step {
            let h = self.default_height();
            return self.commit(h);
        }
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        if self.shape == Shape::Sphere {
            return matches!(
                self.sphere_step,
                SphereStep::Center
                    | SphereStep::Radius(_)
                    | SphereStep::Diameter(_)
                    | SphereStep::TtrRadius { .. }
            );
        }
        if self.shape == Shape::Cone {
            return matches!(
                self.cone_step,
                ConeStep::BaseCenter
                    | ConeStep::BaseRadius
                    | ConeStep::BaseDiameter
                    | ConeStep::EllipseFirst
                    | ConeStep::EllipseCenterFirstAxis(_)
                    | ConeStep::TtrRadius { .. }
                    | ConeStep::Height
                    | ConeStep::HeightAfterTopRadius
                    | ConeStep::TopRadius
            );
        }
        if self.shape.rectangular() {
            matches!(
                self.box_step,
                BoxStep::FirstCorner
                    | BoxStep::OppositeCorner
                    | BoxStep::CubeSize
                    | BoxStep::Length
                    | BoxStep::Width
                    | BoxStep::Height
            )
        } else {
            self.height_step
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        if self.shape == Shape::Sphere {
            return matches!(self.sphere_step, SphereStep::Center | SphereStep::Radius(_));
        }
        if self.shape == Shape::Cone {
            return matches!(
                self.cone_step,
                ConeStep::BaseCenter
                    | ConeStep::BaseRadius
                    | ConeStep::EllipseFirst
                    | ConeStep::Height
                    | ConeStep::HeightAfterTopRadius
            );
        }
        self.shape.rectangular()
            && matches!(
                self.box_step,
                BoxStep::FirstCorner | BoxStep::OppositeCorner | BoxStep::Height
            )
    }

    fn on_text_input(&mut self, raw: &str) -> Option<CmdResult> {
        if self.shape == Shape::Sphere {
            let token = raw.trim();
            let upper = token.to_uppercase();
            return match self.sphere_step {
                SphereStep::Center if upper == "3P" => {
                    self.sphere_step = SphereStep::ThreePointFirst;
                    Some(CmdResult::NeedPoint)
                }
                SphereStep::Center if upper == "2P" => {
                    self.sphere_step = SphereStep::TwoPointFirst;
                    Some(CmdResult::NeedPoint)
                }
                SphereStep::Center if matches!(upper.as_str(), "T" | "TTR") => {
                    self.sphere_step = SphereStep::TtrFirst;
                    Some(CmdResult::NeedPoint)
                }
                SphereStep::Radius(_) if matches!(upper.as_str(), "D" | "DIAMETER") => {
                    let SphereStep::Radius(center) = self.sphere_step else {
                        unreachable!();
                    };
                    self.sphere_step = SphereStep::Diameter(center);
                    Some(CmdResult::NeedPoint)
                }
                SphereStep::Radius(center) => {
                    let radius = crate::entities::common::parse_typed_length(token)?;
                    Some(self.commit_sphere(center, radius))
                }
                SphereStep::Diameter(center) => {
                    let diameter = crate::entities::common::parse_typed_length(token)?;
                    Some(self.commit_sphere(center, diameter * 0.5))
                }
                SphereStep::TtrRadius { .. } => {
                    let radius = crate::entities::common::parse_typed_length(token)?;
                    Some(self.sphere_ttr_result(radius))
                }
                _ => None,
            };
        }
        if self.shape == Shape::Cone {
            return self.on_cone_text(raw);
        }
        if self.shape.rectangular() {
            let token = raw.trim();
            let upper = token.to_uppercase();
            return match self.box_step {
                BoxStep::FirstCorner if matches!(upper.as_str(), "C" | "CENTER") => {
                    self.box_centered = true;
                    self.box_step = BoxStep::CenterPoint;
                    Some(CmdResult::NeedPoint)
                }
                BoxStep::OppositeCorner if matches!(upper.as_str(), "C" | "CUBE") => {
                    self.box_step = BoxStep::CubeSize;
                    Some(CmdResult::NeedPoint)
                }
                BoxStep::OppositeCorner if matches!(upper.as_str(), "L" | "LENGTH") => {
                    self.box_step = BoxStep::Length;
                    Some(CmdResult::NeedPoint)
                }
                BoxStep::Height if matches!(upper.as_str(), "2P" | "2POINT") => {
                    self.box_step = BoxStep::HeightFirstPoint;
                    Some(CmdResult::NeedPoint)
                }
                BoxStep::CubeSize => crate::entities::common::parse_typed_length(token)
                    .and_then(|value| self.set_box_cube(value, 0.0)),
                BoxStep::Length => {
                    let value = crate::entities::common::parse_typed_length(token)?;
                    self.set_box_length(value, 0.0)
                        .then_some(CmdResult::NeedPoint)
                }
                BoxStep::Width => {
                    let value = crate::entities::common::parse_typed_length(token)?;
                    self.set_box_width(value, value)
                        .then_some(CmdResult::NeedPoint)
                }
                BoxStep::Height => {
                    let value = crate::entities::common::parse_typed_length(token)?;
                    (value.is_finite() && value.abs() >= 1e-6).then(|| self.commit(value))
                }
                _ => None,
            };
        }
        if !self.height_step {
            return None;
        }
        let h: f64 = raw.trim().parse().ok().filter(|v| *v > 0.0)?;
        Some(self.commit(h))
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.shape == Shape::Sphere {
            let local = self.plane.to_local(pt);
            if !local.is_finite() {
                return None;
            }
            return match self.sphere_step {
                SphereStep::Radius(center) => self.sphere_preview(center, center.distance(local)),
                SphereStep::Diameter(center) => {
                    self.sphere_preview(center, center.distance(local) * 0.5)
                }
                SphereStep::TwoPointSecond(first) => {
                    self.sphere_preview((first + local) * 0.5, first.distance(local) * 0.5)
                }
                SphereStep::ThreePointSecond(first) => {
                    self.sphere_preview((first + local) * 0.5, first.distance(local) * 0.5)
                }
                SphereStep::ThreePointThird(first, second) => {
                    let (center, radius) = sphere_through_three_points(first, second, local)?;
                    self.sphere_preview(center, radius)
                }
                SphereStep::TtrRadius {
                    first,
                    second,
                    first_hit,
                    second_hit,
                } => {
                    let radius = second_hit.distance(local);
                    let hint = (first_hit + second_hit) * 0.5;
                    let center = closest_sphere_center(
                        &sphere_ttr_centers(first, second, radius),
                        hint,
                    )?;
                    self.sphere_preview(center, radius)
                }
                _ => None,
            };
        }
        if self.shape == Shape::Cone {
            return self.cone_mouse_move(pt);
        }
        if self.pts.is_empty() {
            return None;
        }
        if self.shape.rectangular() {
            let local = self.plane.to_local(pt);
            if !local.is_finite() {
                return None;
            }
            return match self.box_step {
                BoxStep::OppositeCorner => {
                    let first = self.pts[0];
                    let points = if self.box_centered {
                        let delta = local - first;
                        vec![
                            DVec3::new(
                                first.x - delta.x.abs(),
                                first.y - delta.y.abs(),
                                first.z,
                            ),
                            DVec3::new(
                                first.x + delta.x.abs(),
                                first.y + delta.y.abs(),
                                first.z,
                            ),
                        ]
                    } else {
                        vec![first, local]
                    };
                    Some(self.place_preview(footprint_wire(Shape::Box, &points)))
                }
                BoxStep::CubeSize => {
                    let delta = local - self.pts[0];
                    let size = if self.shape == Shape::Wedge {
                        delta.x.hypot(delta.y)
                    } else {
                        delta.length()
                    }
                    .max(1e-6);
                    let mut preview = PrimitiveCommand::new(self.shape.name());
                    preview.plane = self.plane;
                    preview.pts = self.pts.clone();
                    preview.box_length = Some(size);
                    preview.box_width = Some(size);
                    preview.box_width_sign = 1.0;
                    if self.shape == Shape::Wedge {
                        preview.box_origin = Some(self.pts[0]);
                        preview.box_angle = delta.y.atan2(delta.x);
                    } else {
                        preview.box_origin = Some(if self.box_centered {
                            self.pts[0] - DVec3::new(size * 0.5, size * 0.5, 0.0)
                        } else {
                            self.pts[0]
                        });
                    }
                    if self.shape == Shape::Wedge && self.box_centered {
                        let (sin, cos) = preview.box_angle.sin_cos();
                        let x_axis = DVec3::new(cos, sin, 0.0);
                        let y_axis = DVec3::new(-sin, cos, 0.0);
                        preview.box_origin = Some(
                            self.pts[0] - x_axis * size * 0.5 - y_axis * size * 0.5,
                        );
                    }
                    preview.rectangular_preview(size)
                }
                BoxStep::Length => Some(self.place_preview(wire(
                    "primitive_preview",
                    vec![self.pts[0].as_vec3().to_array(), local.as_vec3().to_array()],
                ))),
                BoxStep::Width => {
                    let delta = local - self.pts[0];
                    let normal = DVec3::new(-self.box_angle.sin(), self.box_angle.cos(), 0.0);
                    let signed = delta.dot(normal);
                    let mut preview = PrimitiveCommand::new(self.shape.name());
                    preview.plane = self.plane;
                    preview.pts = self.pts.clone();
                    preview.box_origin = self.box_origin;
                    preview.box_length = self.box_length;
                    preview.box_width = Some(signed.abs().max(1e-6));
                    preview.box_angle = self.box_angle;
                    preview.box_width_sign = if signed < 0.0 { -1.0 } else { 1.0 };
                    preview.rectangular_preview(1e-6)
                }
                BoxStep::Height => self.rectangular_preview(self.cursor_height(pt)),
                BoxStep::HeightSecondPoint => self.box_height_anchor.map(|first| {
                    self.place_preview(wire(
                        "primitive_preview",
                        vec![first.as_vec3().to_array(), local.as_vec3().to_array()],
                    ))
                }),
                _ => None,
            };
        }
        if self.height_step {
            return Some(self.place_preview(height_wire(
                self.shape,
                &self.pts,
                self.cursor_height(pt),
            )));
        }
        let mut foot = self.pts.clone();
        foot.push(self.plane.to_local(pt));
        Some(self.place_preview(footprint_wire(self.shape, &foot)))
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        use crate::command::{DynAnchor, DynFieldSpec, DynGuide, DynRole, DynSpec};

        if self.shape == Shape::Sphere {
            let (anchor, role) = match self.sphere_step {
                SphereStep::Radius(center) => (center, DynRole::Radius),
                SphereStep::Diameter(center) => (center, DynRole::Diameter),
                SphereStep::TtrRadius { second_hit, .. } => {
                    (second_hit, DynRole::Radius)
                }
                _ => return None,
            };
            return Some(DynSpec {
                anchor: DynAnchor::Point(self.plane.to_world(anchor)),
                fields: vec![DynFieldSpec::new(role)],
                guide: DynGuide::Radius,
                ref_point: None,
            });
        }

        if self.shape == Shape::Cone {
            let frame = self.cone_frame.unwrap_or(self.plane);
            let (anchor, role) = match self.cone_step {
                ConeStep::BaseRadius | ConeStep::TopRadius => {
                    (frame.origin, DynRole::Radius)
                }
                ConeStep::BaseDiameter => (frame.origin, DynRole::Diameter),
                ConeStep::TtrRadius { second_hit, .. } => {
                    (self.plane.to_world(second_hit), DynRole::Radius)
                }
                ConeStep::EllipseCenterFirstAxis(center) => (center, DynRole::Distance),
                ConeStep::Height | ConeStep::HeightAfterTopRadius | ConeStep::AxisEndpoint => {
                    (frame.origin, DynRole::Height)
                }
                ConeStep::HeightSecondPoint(first) => (first, DynRole::Height),
                _ => return None,
            };
            return Some(DynSpec {
                anchor: DynAnchor::Point(anchor),
                fields: vec![DynFieldSpec::new(role)],
                guide: if matches!(role, DynRole::Radius | DynRole::Diameter) {
                    DynGuide::Radius
                } else {
                    DynGuide::None
                },
                ref_point: None,
            });
        }

        let role = if self.shape.rectangular() {
            match self.box_step {
                BoxStep::CubeSize | BoxStep::Length => DynRole::Distance,
                BoxStep::Width => DynRole::Width,
                BoxStep::Height => DynRole::Height,
                _ => return None,
            }
        } else if self.height_step {
            DynRole::Height
        } else {
            return None;
        };
        Some(DynSpec {
            anchor: DynAnchor::Point(self.plane.to_world(self.pts[0])),
            fields: vec![DynFieldSpec::new(role)],
            guide: DynGuide::None,
            ref_point: None,
        })
    }

    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        if self.shape == Shape::Sphere {
            let local = self.plane.to_local(cursor);
            return match self.sphere_step {
                SphereStep::Radius(center) => Some(center.distance(local)),
                SphereStep::Diameter(center) => Some(center.distance(local)),
                SphereStep::TtrRadius { second_hit, .. } => {
                    Some(second_hit.distance(local))
                }
                _ => None,
            };
        }
        if self.shape == Shape::Cone {
            return match self.cone_step {
                ConeStep::BaseRadius | ConeStep::TopRadius => {
                    let frame = self.cone_frame?;
                    let local = frame.to_local(cursor);
                    Some(local.x.hypot(local.y))
                }
                ConeStep::BaseDiameter => {
                    let frame = self.cone_frame?;
                    let local = frame.to_local(cursor);
                    Some(local.x.hypot(local.y))
                }
                ConeStep::TtrRadius { second_hit, .. } => {
                    let local = self.plane.to_local(cursor);
                    Some((local - second_hit).truncate().length())
                }
                ConeStep::EllipseCenterFirstAxis(center) => Some(center.distance(cursor)),
                ConeStep::Height | ConeStep::HeightAfterTopRadius => {
                    self.cone_height_at(cursor).map(f64::abs)
                }
                ConeStep::HeightSecondPoint(first) => Some(first.distance(cursor)),
                ConeStep::AxisEndpoint => self.cone_frame.map(|frame| frame.origin.distance(cursor)),
                _ => None,
            };
        }
        if self.shape.rectangular() {
            let local = self.plane.to_local(cursor);
            if !local.is_finite() {
                return None;
            }
            return match self.box_step {
                BoxStep::CubeSize | BoxStep::Length => {
                    let delta = local - self.pts[0];
                    Some(if self.shape == Shape::Wedge {
                        delta.x.hypot(delta.y)
                    } else {
                        delta.length()
                    })
                }
                BoxStep::Width => {
                    let normal = DVec3::new(-self.box_angle.sin(), self.box_angle.cos(), 0.0);
                    Some((local - self.pts[0]).dot(normal).abs())
                }
                BoxStep::Height => Some(self.cursor_height(cursor)),
                _ => None,
            };
        }
        self.height_step.then(|| self.cursor_height(cursor))
    }
}

// ── Footprint preview ───────────────────────────────────────────────────────

fn footprint_wire(shape: Shape, pts: &[DVec3]) -> WireModel {
    let mut points: Vec<[f32; 3]> = Vec::new();
    if shape == Shape::Pyramid {
        let center = pts[0];
        let radius = (pts[1] - center).length();
        let corners: [DVec3; 4] = std::array::from_fn(|index| {
            let angle = index as f64 * std::f64::consts::FRAC_PI_2;
            center + DVec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
        });
        push_loop(&mut points, &corners);
    } else if shape.radial() {
        let c = pts[0];
        let r = (pts[1] - c).length();
        circle_points(&mut points, c, r);
        if shape == Shape::Torus && pts.len() >= 3 {
            let inner = (pts[2] - c).length();
            points.push([f32::NAN; 3]);
            circle_points(&mut points, c, inner);
        }
    } else {
        let (a, b) = (pts[0], pts[1]);
        points.extend_from_slice(&[
            [a.x as f32, a.y as f32, a.z as f32],
            [b.x as f32, a.y as f32, a.z as f32],
            [b.x as f32, b.y as f32, a.z as f32],
            [a.x as f32, b.y as f32, a.z as f32],
            [a.x as f32, a.y as f32, a.z as f32],
        ]);
    }
    wire("primitive_preview", points)
}

fn height_wire(shape: Shape, pts: &[DVec3], height: f64) -> WireModel {
    let mut points = Vec::new();
    match shape {
        Shape::Box => {
            let (a, b) = (pts[0], pts[1]);
            let base = [
                DVec3::new(a.x, a.y, a.z),
                DVec3::new(b.x, a.y, a.z),
                DVec3::new(b.x, b.y, a.z),
                DVec3::new(a.x, b.y, a.z),
            ];
            let top = base.map(|point| point + DVec3::Z * height);
            push_loop(&mut points, &base);
            push_loop(&mut points, &top);
            for i in 0..4 {
                push_segment(&mut points, base[i], top[i]);
            }
        }
        Shape::Wedge => {
            let (a, b) = (pts[0], pts[1]);
            let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
            let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
            let low = [
                DVec3::new(x0, y0, a.z),
                DVec3::new(x1, y0, a.z),
                DVec3::new(x0, y0, a.z + height),
            ];
            let high = low.map(|point| DVec3::new(point.x, y1, point.z));
            push_loop(&mut points, &low);
            push_loop(&mut points, &high);
            for i in 0..3 {
                push_segment(&mut points, low[i], high[i]);
            }
        }
        Shape::Cylinder | Shape::Cone | Shape::Pyramid => {
            let center = pts[0];
            let radius = (pts[1] - center).length();
            if shape == Shape::Pyramid {
                let base: [DVec3; 4] = std::array::from_fn(|index| {
                    let angle = index as f64 * std::f64::consts::FRAC_PI_2;
                    center + DVec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
                });
                push_loop(&mut points, &base);
                let apex = center + DVec3::Z * height;
                for corner in base {
                    push_segment(&mut points, corner, apex);
                }
                return wire("primitive_height_preview", points);
            }
            push_circle(&mut points, center, radius);
            if shape == Shape::Cylinder {
                push_circle(&mut points, center + DVec3::Z * height, radius);
                for i in 0..4 {
                    let angle = i as f64 * std::f64::consts::FRAC_PI_2;
                    let base = center + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0);
                    push_segment(&mut points, base, base + DVec3::Z * height);
                }
            } else {
                let apex = center + DVec3::Z * height;
                for i in 0..4 {
                    let angle = i as f64 * std::f64::consts::FRAC_PI_2;
                    let base = center + DVec3::new(angle.cos() * radius, angle.sin() * radius, 0.0);
                    push_segment(&mut points, base, apex);
                }
            }
        }
        Shape::Sphere | Shape::Torus => {}
    }
    wire("primitive_height_preview", points)
}

fn push_break(points: &mut Vec<[f32; 3]>) {
    if !points.is_empty() {
        points.push([f32::NAN; 3]);
    }
}

fn push_loop<const N: usize>(points: &mut Vec<[f32; 3]>, path: &[DVec3; N]) {
    push_break(points);
    points.extend(path.iter().chain(path.first()).map(|point| point.as_vec3().to_array()));
}

fn push_segment(points: &mut Vec<[f32; 3]>, a: DVec3, b: DVec3) {
    push_break(points);
    points.extend([a.as_vec3().to_array(), b.as_vec3().to_array()]);
}

fn push_circle(points: &mut Vec<[f32; 3]>, center: DVec3, radius: f64) {
    push_break(points);
    circle_points(points, center, radius);
}

fn push_ellipse_world(
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

fn circle_points(out: &mut Vec<[f32; 3]>, c: DVec3, r: f64) {
    const SEG: usize = 48;
    for i in 0..=SEG {
        let t = i as f64 / SEG as f64 * std::f64::consts::TAU;
        out.push([
            (c.x + r * t.cos()) as f32,
            (c.y + r * t.sin()) as f32,
            c.z as f32,
        ]);
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["BOX", "WEDGE", "CYLINDER", "CONE", "SPHERE", "PYRAMID", "PYR", "TORUS"]
});

fn wire(name: &str, points: Vec<[f32; 3]>) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name: name.into(),
        points,
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        aci: 0,
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}
