// MEASUREGEOM command (alias MEA) — multi-mode geometry measurement.
//
// First step prompts for a mode keyword:
//   Distance / Radius / Angle / ARea
// then collects the geometry the mode needs and prints a one-line readout
// via `CmdResult::Measurement`, ending the command.
//
//   DISTANCE — two points → distance, delta X/Y/Z, angle in the XY plane.
//   AREA     — points until Enter → area (f64 shoelace relative to the first
//              vertex, mirroring inquiry/area.rs for precision) + perimeter.
//   ANGLE    — three points (vertex + two ray endpoints) → angle in degrees.
//   RADIUS   — pick a Circle or Arc → radius + diameter.
//
// All arithmetic is kept in f64 (picked points stay full precision; downcasting
// to f32 loses several hundredths of a unit at survey-scale coordinates).

use crate::t;
use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "MEASUREGEOM",
        label: "Measure Geometry",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("MEASUREGEOM".to_string()),
    }
}

// ── Mode ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Awaiting the mode keyword.
    Choose,
    Distance,
    Area,
    Angle,
    Radius,
    AngleLines,
}

// ── Command implementation ──────────────────────────────────────────────────

pub const RADIUS_ICON: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/measure-radius.svg"
));
pub const ANGLE_ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../../assets/icons/measure-angle.svg"));

pub struct MeasureGeomCommand {
    name: &'static str,
    hovered: Option<(Handle, EntityType)>,
    first_line: Option<(Handle, DVec3)>,
    mode: Mode,
    /// Picked points for the active point-pick mode.
    points: Vec<DVec3>,
    /// The picked entity for RADIUS, injected before `on_entity_pick`.
    picked: Option<EntityType>,
    plane: WorkingPlane,
}

impl MeasureGeomCommand {
    pub fn new() -> Self {
        Self {
            name: "MEASUREGEOM",
            hovered: None,
            first_line: None,
            mode: Mode::Choose,
            points: vec![],
            picked: None,
            plane: WorkingPlane::default(),
        }
    }

    pub fn radius() -> Self {
        Self {
            name: "MEASURERADIUS",
            mode: Mode::Radius,
            ..Self::new()
        }
    }

    pub fn angle() -> Self {
        Self {
            name: "MEASUREANGLE",
            mode: Mode::Angle,
            ..Self::new()
        }
    }

    fn radius_msg(radius: f64) -> String {
        t!(
            "Radius = %{radius},  Diameter = %{diameter}",
            radius = crate::entities::common::format_length(radius),
            diameter = crate::entities::common::format_length(radius * 2.0)
        )
        .into_owned()
    }

    // Normalize only after rescaling, retaining f64 precision without an
    // arbitrary drawing-unit tolerance or overflowing on long vectors.
    fn direction(vector: DVec3) -> Option<DVec3> {
        if !vector.is_finite() {
            return None;
        }
        let scale = vector.abs().max_element();
        if scale == 0.0 {
            return None;
        }
        let vector = vector / scale;
        Some(vector / vector.length())
    }

    fn angle_degrees(a: DVec3, b: DVec3) -> Option<f64> {
        let a = Self::direction(a)?;
        let b = Self::direction(b)?;
        Some(a.cross(b).length().atan2(a.dot(b)).to_degrees())
    }

    fn line_direction(entity: &EntityType) -> Option<DVec3> {
        let EntityType::Line(line) = entity else {
            return None;
        };
        Self::direction(
            DVec3::new(line.end.x, line.end.y, line.end.z)
                - DVec3::new(line.start.x, line.start.y, line.start.z),
        )
    }

    // Unoriented lines: the smaller angle, independent of endpoint order.
    fn line_angle(a: DVec3, b: DVec3) -> Option<f64> {
        Self::angle_degrees(a, b).map(|angle| angle.min(180.0 - angle))
    }

    fn angle_value_msg(angle: f64) -> String {
        let aprec = crate::entities::common::unit_context().auprec.max(0) as usize;
        let angle = crate::entities::common::without_negative_zero(format!("{angle:.aprec$}"));
        t!("Angle = %{angle}°", angle = angle).into_owned()
    }

    /// Build a cyan preview wire connecting the picked points and the cursor.
    fn preview_wire(name: &str, pts: Vec<[f32; 3]>) -> WireModel {
        WireModel {
            bg_adapt: None,
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            snap_only: false,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: name.to_string(),
            points: pts,
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

            ..Default::default()
        }
    }

    /// Extract (radius) from a Circle or Arc; `None` for anything else.
    fn radius_of(entity: &EntityType) -> Option<f64> {
        let radius = match entity {
            EntityType::Circle(circle) => circle.radius,
            EntityType::Arc(arc) => arc.radius,
            _ => return None,
        };
        (radius.is_finite() && radius > 0.0 && (radius * 2.0).is_finite()).then_some(radius)
    }

    /// DISTANCE readout for the two collected points.
    fn distance_msg(plane: WorkingPlane, p1: DVec3, p2: DVec3) -> String {
        let p1 = plane.to_local(p1);
        let p2 = plane.to_local(p2);
        let delta = p2 - p1;
        let dist = delta.length();
        let dx = delta.x;
        let dy = delta.y;
        let dz = delta.z;
        let angle_xy = dy.atan2(dx).to_degrees();
        let aprec = crate::entities::common::unit_context().auprec.max(0) as usize;
        let dist_s = crate::entities::common::format_length(dist);
        let angle_xy_s =
            crate::entities::common::without_negative_zero(format!("{angle_xy:.aprec$}"));
        let dx_s = crate::entities::common::format_length(dx);
        let dy_s = crate::entities::common::format_length(dy);
        let dz_s = crate::entities::common::format_length(dz);
        t!(
            "Distance = %{dist},  Angle in XY Plane = %{angle_xy}°\n  Delta X = %{dx},  Delta Y = %{dy},  Delta Z = %{dz}",
            dist = dist_s,
            angle_xy = angle_xy_s,
            dx = dx_s,
            dy = dy_s,
            dz = dz_s,
        )
        .into_owned()
    }

    /// AREA readout: shoelace area (f64, relative to first vertex) + perimeter.
    fn area_msg(plane: WorkingPlane, points: &[DVec3]) -> String {
        let points = points
            .iter()
            .map(|point| plane.to_local(*point))
            .collect::<Vec<_>>();
        let n = points.len();
        let origin = points[0];
        let mut area_sum = 0.0f64;
        let mut perimeter = 0.0f64;
        for idx in 0..n {
            let a = points[idx] - origin;
            let b = points[(idx + 1) % n] - origin;
            area_sum += a.x * b.y - b.x * a.y;
            perimeter += (points[(idx + 1) % n] - points[idx]).length();
        }
        let area = (area_sum * 0.5).abs();
        let area_s = crate::entities::common::format_area(area);
        let perimeter_s = crate::entities::common::format_length(perimeter);
        t!(
            "Area = %{area},  Perimeter = %{perimeter}",
            area = area_s,
            perimeter = perimeter_s
        )
        .into_owned()
    }

    fn angle_msg(vertex: DVec3, a: DVec3, b: DVec3) -> Option<String> {
        Self::angle_degrees(a - vertex, b - vertex).map(Self::angle_value_msg)
    }
}

impl CadCommand for MeasureGeomCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match self.mode {
            Mode::Choose => {
                t!("MEASUREGEOM  Enter an option [Distance/Radius/Angle/ARea]:").into_owned()
            }
            Mode::Distance => {
                if self.points.is_empty() {
                    t!("MEASUREGEOM  Specify first point:").into_owned()
                } else {
                    t!("MEASUREGEOM  Specify second point:").into_owned()
                }
            }
            Mode::Area => {
                if self.points.is_empty() {
                    t!("MEASUREGEOM  Specify first corner point (Enter to cancel):").into_owned()
                } else {
                    let n = self.points.len();
                    t!(
                        "MEASUREGEOM  Specify next point (%{n} picked, Enter to calculate):",
                        n = n
                    )
                    .into_owned()
                }
            }
            Mode::Angle => match self.points.len() {
                0 => return crate::tr!("measurement", "angle-start", command = self.name),
                1 => t!("MEASUREGEOM  Specify first ray point:").into_owned(),
                _ => t!("MEASUREGEOM  Specify second ray point:").into_owned(),
            },
            Mode::Radius => t!("MEASUREGEOM  Select arc or circle:").into_owned(),
            Mode::AngleLines => {
                return if self.first_line.is_none() {
                    crate::tr!("measurement", "first-line", command = self.name)
                } else {
                    crate::tr!("measurement", "second-line", command = self.name)
                }
            }
        }
        .replace("MEASUREGEOM", self.name)
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn wants_text_input(&self) -> bool {
        self.mode == Mode::Choose || self.point_step_accepts_keywords()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        (self.mode == Mode::Angle && self.points.is_empty())
            || (self.mode == Mode::AngleLines && self.first_line.is_none())
    }

    fn has_measurement_readout(&self) -> bool {
        true
    }

    fn measurement_readout(&self, cursor: DVec3) -> Option<String> {
        match self.mode {
            Mode::Distance if !self.points.is_empty() && cursor.is_finite() => Some(crate::tr!(
                "measurement",
                "distance",
                value = crate::entities::common::format_length((cursor - self.points[0]).length())
            )),
            Mode::Area if self.points.len() >= 2 && cursor.is_finite() => {
                let mut points = self.points.clone();
                points.push(cursor);
                Some(Self::area_msg(self.plane, &points))
            }
            Mode::Angle if self.points.len() >= 2 => {
                Self::angle_msg(self.points[0], self.points[1], cursor)
            }
            Mode::Radius => self
                .hovered
                .as_ref()
                .and_then(|(_, e)| Self::radius_of(e))
                .map(Self::radius_msg),
            Mode::AngleLines => {
                let (first_handle, first) = self.first_line?;
                let (handle, entity) = self.hovered.as_ref()?;
                if first_handle == *handle {
                    return None;
                }
                Self::line_angle(first, Self::line_direction(entity)?).map(Self::angle_value_msg)
            }
            _ => None,
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.mode != Mode::Choose {
            return match self.mode {
                Mode::Angle if self.points.is_empty() => vec![CmdOption {
                    label: crate::tr!("measurement", "lines"),
                    keyword: "LINES".into(),
                }],
                Mode::AngleLines if self.first_line.is_none() => vec![CmdOption {
                    label: crate::tr!("measurement", "points"),
                    keyword: "POINTS".into(),
                }],
                _ => Vec::new(),
            };
        }
        vec![
            CmdOption::new("Distance", "D"),
            CmdOption::new("Radius", "R"),
            CmdOption::new("Angle", "A"),
            CmdOption::new("ARea", "AR"),
        ]
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim().to_ascii_uppercase();
        if self.mode == Mode::Angle
            && self.points.is_empty()
            && matches!(input.as_str(), "L" | "LINES")
        {
            self.mode = Mode::AngleLines;
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == Mode::AngleLines
            && self.first_line.is_none()
            && matches!(input.as_str(), "P" | "POINTS")
        {
            self.mode = Mode::Angle;
            self.hovered = None;
            return Some(CmdResult::NeedPoint);
        }
        if self.mode != Mode::Choose {
            return None;
        }
        let t = text.trim().to_uppercase();
        self.mode = match t.as_str() {
            "D" | "DISTANCE" => Mode::Distance,
            "R" | "RADIUS" => Mode::Radius,
            "A" | "ANGLE" => Mode::Angle,
            "AR" | "AREA" => Mode::Area,
            _ => return Some(CmdResult::NeedPoint), // re-prompt on unknown keyword
        };
        Some(CmdResult::NeedPoint)
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, Mode::Radius | Mode::AngleLines)
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn wants_hover_entity(&self, handle: Handle) -> bool {
        self.needs_entity_pick()
            && !handle.is_null()
            && self.hovered.as_ref().map(|(h, _)| *h) != Some(handle)
    }

    fn inject_hover_entity(&mut self, handle: Handle, entity: EntityType) {
        self.hovered = Some((handle, entity));
    }

    fn on_hover_entity(&mut self, handle: Handle, _point: DVec3) -> Vec<WireModel> {
        if handle.is_null() || self.hovered.as_ref().map(|(h, _)| *h) != Some(handle) {
            self.hovered = None;
        }
        Vec::new()
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        // Never reuse an entity injected for a previous pick.
        let picked = self.picked.take();
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            Mode::Radius => match picked.as_ref().and_then(Self::radius_of) {
                Some(radius) => CmdResult::Measurement(Self::radius_msg(radius)),
                None => CmdResult::ReportError(crate::tr!("measurement", "invalid-radius")),
            },
            Mode::AngleLines => {
                let Some(direction) = picked.as_ref().and_then(Self::line_direction) else {
                    return CmdResult::ReportError(crate::tr!("measurement", "invalid-line"));
                };
                if let Some((first_handle, first)) = self.first_line {
                    if first_handle == handle {
                        return CmdResult::ReportError(crate::tr!("measurement", "different-line"));
                    }
                    match Self::line_angle(first, direction) {
                        Some(angle) => CmdResult::Measurement(Self::angle_value_msg(angle)),
                        None => CmdResult::ReportError(crate::tr!("measurement", "invalid-line")),
                    }
                } else {
                    self.first_line = Some((handle, direction));
                    CmdResult::NeedPoint
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if matches!(self.mode, Mode::Distance | Mode::Angle | Mode::Area) && !pt.is_finite() {
            return CmdResult::ReportError(crate::tr!("measurement", "invalid-point"));
        }
        if self.mode == Mode::Angle
            && !self.points.is_empty()
            && Self::direction(pt - self.points[0]).is_none()
        {
            return CmdResult::ReportError(crate::tr!("measurement", "degenerate-ray"));
        }
        match self.mode {
            Mode::Distance => {
                self.points.push(pt);
                if self.points.len() == 2 {
                    CmdResult::Measurement(Self::distance_msg(
                        self.plane,
                        self.points[0],
                        self.points[1],
                    ))
                } else {
                    CmdResult::NeedPoint
                }
            }
            Mode::Angle => {
                self.points.push(pt);
                if self.points.len() == 3 {
                    match Self::angle_msg(self.points[0], self.points[1], self.points[2]) {
                        Some(text) => CmdResult::Measurement(text),
                        None => {
                            self.points.pop();
                            CmdResult::ReportError(crate::tr!("measurement", "degenerate-ray"))
                        }
                    }
                } else {
                    CmdResult::NeedPoint
                }
            }
            Mode::Area => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            // Choose / Radius do not take point picks.
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.mode {
            Mode::Area => {
                if self.points.len() < 3 {
                    CmdResult::Cancel
                } else {
                    CmdResult::Measurement(Self::area_msg(self.plane, &self.points))
                }
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let f = |p: DVec3| [p.x as f32, p.y as f32, p.z as f32];
        match self.mode {
            Mode::Distance | Mode::Angle => {
                if self.points.is_empty() {
                    return None;
                }
                let mut pts: Vec<[f32; 3]> = self.points.iter().map(|p| f(*p)).collect();
                if self.mode == Mode::Angle && self.points.len() >= 2 {
                    pts = vec![f(self.points[1]), f(self.points[0])];
                }
                pts.push(f(pt));
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            Mode::Area => {
                if self.points.is_empty() {
                    return None;
                }
                let mut pts: Vec<[f32; 3]> = self.points.iter().map(|p| f(*p)).collect();
                pts.push(f(pt));
                pts.push(f(self.points[0]));
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            _ => None,
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["MEASUREGEOM", "MEA", "MEASURERADIUS", "MEASUREANGLE"]
}); // MeasureGeomCommand

#[cfg(test)]
mod measurement_tests {
    use super::*;
    use crate::entities::common::{set_unit_context, unit_context};
    use codec::{
        entities::{Arc, Circle, Line},
        types::Vector3,
    };

    fn units() {
        let mut context = unit_context();
        context.lunits = 2;
        context.luprec = 4;
        context.auprec = 2;
        set_unit_context(context);
    }
    fn result(result: CmdResult) -> String {
        let CmdResult::Measurement(text) = result else {
            panic!("expected measurement");
        };
        text
    }
    fn pick(command: &mut MeasureGeomCommand, entity: EntityType, id: u64) -> CmdResult {
        command.inject_picked_entity(entity);
        command.on_entity_pick(Handle::new(id), DVec3::ZERO)
    }
    fn line(end: Vector3) -> EntityType {
        EntityType::Line(Line::from_points(Vector3::ZERO, end))
    }

    #[test]
    fn radius_circle_and_arc_respect_length_precision() {
        units();
        for entity in [
            EntityType::Circle(Circle::from_coords(500000.0, 100000.0, 0.0, 2.34567)),
            EntityType::Arc(Arc::from_center_radius_angles(
                Vector3::ZERO,
                2.34567,
                0.0,
                std::f64::consts::FRAC_PI_2,
            )),
        ] {
            let text = result(pick(&mut MeasureGeomCommand::radius(), entity, 1));
            assert!(text.contains("2.3457"), "{text}");
            assert!(text.contains("4.6913"), "{text}");
        }
    }

    #[test]
    fn radius_rejects_invalid_entities_and_stale_injection() {
        let mut command = MeasureGeomCommand::radius();
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::ReportError(_)
        ));
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                pick(
                    &mut command,
                    EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, radius)),
                    1
                ),
                CmdResult::ReportError(_)
            ));
        }
        command.inject_picked_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)));
        assert!(matches!(
            command.on_entity_pick(Handle::NULL, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_entity_pick(Handle::new(2), DVec3::ZERO),
            CmdResult::ReportError(_)
        ));
    }

    #[test]
    fn known_angles_extreme_scales_and_degenerate_rays() {
        for degrees in [0.0_f64, 30.0, 45.0, 90.0, 135.0, 180.0] {
            let radians = degrees.to_radians();
            let ray = DVec3::new(radians.cos(), radians.sin(), 0.0);
            for scale in [1e-200, 1.0, 1e200] {
                let actual =
                    MeasureGeomCommand::angle_degrees(DVec3::X * scale, ray * scale).unwrap();
                assert!((actual - degrees).abs() < 1e-10);
            }
        }
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::X, DVec3::Z),
            Some(90.0)
        );
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::ZERO, DVec3::X),
            None
        );
        assert_eq!(
            MeasureGeomCommand::angle_degrees(DVec3::NAN, DVec3::X),
            None
        );
    }

    #[test]
    fn invalid_angle_points_do_not_advance_and_survey_precision_is_retained() {
        units();
        let mut command = MeasureGeomCommand::angle();
        let vertex = DVec3::new(500000.001, 600000.002, 100.0);
        assert!(matches!(
            command.on_point(DVec3::NAN),
            CmdResult::ReportError(_)
        ));
        assert!(command.points.is_empty());
        command.on_point(vertex);
        assert!(matches!(
            command.on_point(vertex),
            CmdResult::ReportError(_)
        ));
        assert_eq!(command.points.len(), 1);
        command.on_point(vertex + DVec3::X * 0.001);
        assert!(matches!(
            command.on_point(vertex),
            CmdResult::ReportError(_)
        ));
        assert_eq!(command.points.len(), 2);
        assert!(result(command.on_point(vertex + DVec3::Y * 0.001)).contains("90.00°"));
    }

    #[test]
    fn angle_preview_two_rays_and_live_value() {
        units();
        let mut command = MeasureGeomCommand::angle();
        command.on_point(DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::Y).is_none());
        command.on_point(DVec3::X);
        assert!(command
            .measurement_readout(DVec3::Y)
            .unwrap()
            .contains("90.00°"));
        assert!(command
            .measurement_readout(DVec3::X + DVec3::Y)
            .unwrap()
            .contains("45.00°"));
        assert_eq!(
            command.on_mouse_move(DVec3::Y).unwrap().points,
            vec![[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
    }

    #[test]
    fn two_lines_smaller_angle_and_invalid_selections() {
        units();
        let mut command = MeasureGeomCommand::angle();
        command.on_text_input("LINES");
        assert!(command.needs_entity_pick());
        assert!(matches!(
            pick(&mut command, line(Vector3::ZERO), 1),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            pick(&mut command, line(Vector3::UNIT_X), 1),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(
            pick(
                &mut command,
                EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 1.0)),
                2
            ),
            CmdResult::ReportError(_)
        ));
        assert!(
            result(pick(&mut command, line(Vector3::new(-1.0, -1.0, 0.0)), 2)).contains("45.00°")
        );
        assert_eq!(
            MeasureGeomCommand::line_angle(DVec3::X, -DVec3::X),
            Some(0.0)
        );
    }

    #[test]
    fn hover_clears_on_miss_without_completing_measurement() {
        units();
        let mut command = MeasureGeomCommand::radius();
        let handle = Handle::new(5);
        assert!(command.wants_hover_entity(handle));
        command.inject_hover_entity(
            handle,
            EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)),
        );
        command.on_hover_entity(handle, DVec3::ZERO);
        assert!(command
            .measurement_readout(DVec3::ZERO)
            .unwrap()
            .contains("2.0000"));
        assert!(!command.wants_hover_entity(handle));
        assert!(command.picked.is_none());
        command.on_hover_entity(Handle::NULL, DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
        command.inject_hover_entity(handle, line(Vector3::UNIT_X));
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
    }

    #[test]
    fn measuregeom_all_four_modes_remain_available() {
        units();
        assert_eq!(MeasureGeomCommand::new().options().len(), 4);
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("D");
        command.on_point(DVec3::ZERO);
        assert!(result(command.on_point(DVec3::new(3.0, 4.0, 0.0))).contains("5.0000"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("AR");
        for p in [DVec3::ZERO, DVec3::X * 4.0, DVec3::Y * 3.0] {
            command.on_point(p);
        }
        let text = result(command.on_enter());
        assert!(text.contains("6.0000") && text.contains("12.0000"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("A");
        command.on_point(DVec3::ZERO);
        command.on_point(DVec3::X);
        assert!(result(command.on_point(DVec3::Y)).contains("90.00°"));
        let mut command = MeasureGeomCommand::new();
        command.on_text_input("R");
        assert!(result(pick(
            &mut command,
            EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 3.0)),
            1
        ))
        .contains("3.0000"));
    }
}
