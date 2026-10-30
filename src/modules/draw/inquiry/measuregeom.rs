// MEASUREGEOM command (alias MEA) — multi-mode geometry measurement.
//
// First step prompts for a mode keyword:
//   Quick / Distance / Radius / Angle / ARea
// then collects the geometry the mode needs and prints a one-line readout
// via `CmdResult::Measurement`, ending the command.
//
//   DISTANCE — two points → distance, delta X/Y/Z, angle in the XY plane.
//   AREA     — points until Enter → area (f64 shoelace relative to the first
//              vertex, mirroring inquiry/area.rs for precision) + perimeter.
//   ANGLE    — three points (vertex + two ray endpoints) → angle in degrees.
//   RADIUS   — pick a Circle or Arc → radius + diameter.
//   QUICK    — no picks: as the cursor moves, the nearest geometry along the
//              working plane's X and Y axes is measured live; a click
//              reports the current readout and Enter ends the command.
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
    Quick,
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
    /// QUICK readout for the last hovered cursor position.
    quick: std::cell::RefCell<Option<String>>,
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
            quick: std::cell::RefCell::new(None),
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
    ///
    /// Keeps full f64 coordinates through [`WireModel::solid_f64`]: previews
    /// cast straight to f32 drift about half a metre away from their snapped
    /// points on UTM-scale drawings.
    fn preview_wire(name: &str, pts: Vec<DVec3>) -> WireModel {
        WireModel::solid_f64(
            name.to_string(),
            pts.into_iter().map(|point| point.to_array()).collect(),
            WireModel::CYAN,
            false,
        )
    }

    fn radius_at(entity: &EntityType, point: DVec3) -> Option<f64> {
        match entity {
            EntityType::Circle(circle) => {
                let radius = circle.radius;
                (radius.is_finite() && radius > 0.0 && (radius * 2.0).is_finite()).then_some(radius)
            }
            EntityType::Arc(arc) => {
                let radius = arc.radius;
                (radius.is_finite() && radius > 0.0 && (radius * 2.0).is_finite()).then_some(radius)
            }
            EntityType::Region(_) => {
                let source = crate::scene::dimension_assoc::radial_source_at(
                    entity,
                    codec::types::Vector3::new(point.x, point.y, point.z),
                )?;
                (source.radius.is_finite()
                    && source.radius > 0.0
                    && (source.radius * 2.0).is_finite())
                .then_some(source.radius)
            }
            _ => None,
        }
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

    /// Signed offset along local `axis` (0 = X, 1 = Y) from `c` to where the
    /// segment `a`–`b` crosses the line through `c` along that axis.
    fn crossing(a: DVec3, b: DVec3, c: DVec3, axis: usize) -> Option<f64> {
        let other = 1 - axis;
        let (da, db) = (a[other] - c[other], b[other] - c[other]);
        if da * db > 0.0 || da == db {
            return None;
        }
        let t = da / (da - db);
        Some(a[axis] + t * (b[axis] - a[axis]) - c[axis])
    }

    /// Nearest crossings of the drawn geometry with the working plane's X and
    /// Y lines through `cursor`, as local offsets `[-X, +X, -Y, +Y]`.
    fn quick_hits(
        scene: &crate::scene::Scene,
        plane: WorkingPlane,
        cursor: DVec3,
    ) -> [Option<f64>; 4] {
        let c = plane.to_local(cursor);
        // Long enough to leave the drawing from anywhere the cursor can be.
        let reach = scene
            .model_space_extents()
            .map(|(lo, hi)| {
                let (lo, hi) = (lo.as_dvec3(), hi.as_dvec3());
                (hi - lo).length() + (cursor - (lo + hi) * 0.5).length()
            })
            .unwrap_or(0.0)
            + 1.0;
        let mut hits = [None; 4];
        for (axis, direction) in [plane.x, plane.y].into_iter().enumerate() {
            let (a, b) = (cursor - direction * reach, cursor + direction * reach);
            let pad = reach * 1e-9;
            let candidates = scene.interaction_candidates_xy([
                a.x.min(b.x) - pad,
                a.y.min(b.y) - pad,
                a.x.max(b.x) + pad,
                a.y.max(b.y) + pad,
            ]);
            for wire in candidates.iter() {
                if !wire.display_visible
                    || wire.snap_only
                    || wire.point_marker.is_some()
                    || !wire.text_verts.is_empty()
                {
                    continue;
                }
                let mut previous: Option<DVec3> = None;
                for (index, point) in wire.points.iter().enumerate() {
                    if point[0].is_nan() || point[1].is_nan() {
                        previous = None;
                        continue;
                    }
                    let local = plane.to_local(wire.point_world(index, 0.0));
                    if let Some(offset) = previous
                        .and_then(|previous| Self::crossing(previous, local, c, axis))
                        .filter(|offset| offset.abs() > pad)
                    {
                        let slot = &mut hits[axis * 2 + usize::from(offset > 0.0)];
                        if slot.is_none_or(|best: f64| offset.abs() < best.abs()) {
                            *slot = Some(offset);
                        }
                    }
                    previous = Some(local);
                }
            }
        }
        hits
    }

    /// QUICK measurement lines (world) and readout for crossings `hits`.
    fn quick_result(
        plane: WorkingPlane,
        cursor: DVec3,
        hits: [Option<f64>; 4],
    ) -> (Vec<WireModel>, Option<String>) {
        let c = plane.to_local(cursor);
        let mut wires = Vec::new();
        let mut lines = Vec::new();
        for axis in 0..2 {
            let (low, high) = (hits[axis * 2], hits[axis * 2 + 1]);
            if low.is_none() && high.is_none() {
                continue;
            }
            let (from, to) = (low.unwrap_or(0.0), high.unwrap_or(0.0));
            let mut start = c;
            let mut end = c;
            start[axis] += from;
            end[axis] += to;
            wires.push(Self::preview_wire(
                "measuregeom_quick",
                vec![plane.to_world(start), plane.to_world(end)],
            ));
            let value = crate::entities::common::format_length(to - from);
            lines.push(if axis == 0 {
                crate::tr!("measurement", "quick-horizontal", value = value)
            } else {
                crate::tr!("measurement", "quick-vertical", value = value)
            });
        }
        let readout = (!lines.is_empty()).then(|| lines.join("\n"));
        (wires, readout)
    }
}

impl CadCommand for MeasureGeomCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match self.mode {
            Mode::Choose => {
                t!("MEASUREGEOM  Enter an option [Quick/Distance/Radius/Angle/ARea]:").into_owned()
            }
            Mode::Quick => return crate::tr!("measurement", "quick-prompt", command = self.name),
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
            Mode::Radius => {
                t!("MEASUREGEOM  Select arc, circle, or circular region edge:").into_owned()
            }
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
            Mode::Quick => self.quick.borrow().clone(),
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
                .and_then(|(_, e)| Self::radius_at(e, cursor))
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
            CmdOption {
                label: crate::tr!("measurement", "quick"),
                keyword: "Q".into(),
            },
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
            "Q" | "QUICK" => Mode::Quick,
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

    // QUICK reads the scene under the cursor; it never resolves an object, so
    // every click lands in `on_point`.
    fn needs_structure_point_pick(&self) -> bool {
        self.mode == Mode::Quick
    }

    fn object_pick_hover_previews(
        &self,
        scene: &crate::scene::Scene,
        cursor: DVec3,
    ) -> Vec<WireModel> {
        if self.mode != Mode::Quick || !cursor.is_finite() {
            return Vec::new();
        }
        let hits = Self::quick_hits(scene, self.plane, cursor);
        let (wires, readout) = Self::quick_result(self.plane, cursor, hits);
        *self.quick.borrow_mut() = readout;
        wires
    }

    fn object_pick_miss_message(&self) -> &'static str {
        ""
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

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        // Never reuse an entity injected for a previous pick.
        let picked = self.picked.take();
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            Mode::Radius => match picked.as_ref().and_then(|e| Self::radius_at(e, point)) {
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
        if self.mode == Mode::Quick {
            return match self.quick.borrow().clone() {
                Some(text) => CmdResult::ReportMeasurement(text),
                None => CmdResult::NeedPoint,
            };
        }
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
        match self.mode {
            Mode::Distance | Mode::Angle => {
                if self.points.is_empty() {
                    return None;
                }
                let mut pts = self.points.clone();
                if self.mode == Mode::Angle && self.points.len() >= 2 {
                    pts = vec![self.points[1], self.points[0]];
                }
                pts.push(pt);
                Some(Self::preview_wire("measuregeom_preview", pts))
            }
            Mode::Area => {
                if self.points.is_empty() {
                    return None;
                }
                let mut pts = self.points.clone();
                pts.push(pt);
                pts.push(self.points[0]);
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
    fn region_radius_uses_the_hovered_and_picked_boundary() {
        units();
        use kernel::geom2d::{Circle as KernelCircle, Curve};
        let region = crate::scene::model::presspull_model::region_from_loops(
            &[
                vec![Curve::Circle(KernelCircle {
                    centre: [0.0, 0.0],
                    radius: 10.0,
                })],
                vec![Curve::Circle(KernelCircle {
                    centre: [0.0, 0.0],
                    radius: 2.0,
                })],
            ],
            WorkingPlane::default(),
        )
        .unwrap();
        let before = format!("{region:?}");
        let mut command = MeasureGeomCommand::radius();
        let handle = Handle::new(1);
        command.inject_hover_entity(handle, region.clone());
        for radius in [10.0, 2.0] {
            let point = DVec3::X * radius;
            command.on_hover_entity(handle, point);
            assert_eq!(
                command.measurement_readout(point),
                Some(MeasureGeomCommand::radius_msg(radius))
            );
            command.inject_picked_entity(region.clone());
            assert_eq!(
                result(command.on_entity_pick(handle, point)),
                MeasureGeomCommand::radius_msg(radius)
            );
        }
        assert_eq!(format!("{region:?}"), before);
        command.on_hover_entity(Handle::NULL, DVec3::ZERO);
        assert!(command.measurement_readout(DVec3::ZERO).is_none());
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
        assert_eq!(MeasureGeomCommand::new().options().len(), 5);
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
