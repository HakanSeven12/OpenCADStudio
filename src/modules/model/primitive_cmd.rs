// Interactive kernel primitives stored as ACIS Solid3D entities.

use acadrust::entities::Solid3D;
use acadrust::objects::SolidHistoryOperation;
use acadrust::EntityType;
use cadkernel::brep::Body;
use glam::DVec3;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
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
    plane: WorkingPlane,
}

impl PrimitiveCommand {
    pub fn new(id: &str) -> Self {
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
            plane: WorkingPlane::default(),
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
            Shape::Cylinder | Shape::Cone => {
                let c = self.pts[0];
                let r = (self.pts[1] - c).length();
                if r < 1e-6 || height < 1e-6 {
                    return None;
                }
                let center = [c.x, c.y, c.z];
                if self.shape == Shape::Cylinder {
                    (
                        solid_model::cylinder_solid(center, r, height),
                        solid_history::cylinder_op(
                            self.history_transform(c),
                            r,
                            height,
                        ),
                    )
                } else {
                    (
                        solid_model::cone_solid(center, r, height),
                        solid_history::cone_op(
                            self.history_transform(c),
                            r,
                            height,
                        ),
                    )
                }
            }
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

    fn prompt(&self) -> String {
        let n = self.shape.name();
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
                BoxStep::Height => t!(
                    "%{n}  Specify height or [2Point] <%{height}>:",
                    n = n,
                    height = format!("{:.4}", self.default_height())
                ),
                BoxStep::HeightFirstPoint => {
                    t!("%{n}  Specify first point for height:", n = n)
                }
                BoxStep::HeightSecondPoint => {
                    t!("%{n}  Specify second point for height:", n = n)
                }
            }
            .into_owned();
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
                    } else if height.abs() >= 1e-6 {
                        self.commit(height)
                    } else {
                        CmdResult::NeedPoint
                    }
                }
                BoxStep::CubeSize => {
                    let delta = local - self.pts[0];
                    self.set_box_cube(
                        delta.x.hypot(delta.y),
                        delta.y.atan2(delta.x),
                    )
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
        if self.shape.rectangular() {
            return if self.box_step == BoxStep::Height {
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
        self.shape.rectangular()
            && matches!(
                self.box_step,
                BoxStep::FirstCorner | BoxStep::OppositeCorner | BoxStep::Height
            )
    }

    fn on_text_input(&mut self, raw: &str) -> Option<CmdResult> {
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
                    let size = delta.x.hypot(delta.y).max(1e-6);
                    let mut preview = PrimitiveCommand::new(self.shape.name());
                    preview.plane = self.plane;
                    preview.pts = self.pts.clone();
                    preview.box_origin = Some(self.pts[0]);
                    preview.box_length = Some(size);
                    preview.box_width = Some(size);
                    preview.box_angle = delta.y.atan2(delta.x);
                    preview.box_width_sign = 1.0;
                    if self.box_centered {
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
        if self.shape.rectangular() {
            let local = self.plane.to_local(cursor);
            if !local.is_finite() {
                return None;
            }
            return match self.box_step {
                BoxStep::CubeSize | BoxStep::Length => {
                    let delta = local - self.pts[0];
                    Some(delta.x.hypot(delta.y))
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
