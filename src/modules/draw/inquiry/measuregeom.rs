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

use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::t;

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
}

// ── Command implementation ──────────────────────────────────────────────────

pub struct MeasureGeomCommand {
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
            mode: Mode::Choose,
            points: vec![],
            picked: None,
            plane: WorkingPlane::default(),
        }
    }

    /// Build a cyan preview wire connecting the picked points and the cursor.
    fn preview_wire(name: &str, pts: Vec<[f32; 3]>) -> WireModel {
        WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
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
        }
    }

    /// Extract (radius) from a Circle or Arc; `None` for anything else.
    fn radius_of(entity: &EntityType) -> Option<f64> {
        match entity {
            EntityType::Circle(c) => Some(c.radius),
            EntityType::Arc(a) => Some(a.radius),
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
        let dist_s = format!("{dist:.4}");
        let angle_xy_s = format!("{angle_xy:.4}");
        let dx_s = format!("{dx:.4}");
        let dy_s = format!("{dy:.4}");
        let dz_s = format!("{dz:.4}");
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
        let points = points.iter().map(|point| plane.to_local(*point)).collect::<Vec<_>>();
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
        let area_s = format!("{area:.4}");
        let perimeter_s = format!("{perimeter:.4}");
        t!(
            "Area = %{area},  Perimeter = %{perimeter}",
            area = area_s,
            perimeter = perimeter_s
        )
        .into_owned()
    }

    /// ANGLE readout: angle at `vertex` between the rays to `a` and `b`.
    fn angle_msg(vertex: DVec3, a: DVec3, b: DVec3) -> String {
        let va = a - vertex;
        let vb = b - vertex;
        let la = va.length();
        let lb = vb.length();
        if la == 0.0 || lb == 0.0 {
            return t!("Angle = 0.0000° (degenerate rays)").into_owned();
        }
        let cos = (va.dot(vb) / (la * lb)).clamp(-1.0, 1.0);
        let angle = cos.acos().to_degrees();
        let angle_s = format!("{angle:.4}");
        t!("Angle = %{angle}°", angle = angle_s).into_owned()
    }
}

impl CadCommand for MeasureGeomCommand {
    fn name(&self) -> &'static str {
        "MEASUREGEOM"
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
                0 => t!("MEASUREGEOM  Specify vertex point:").into_owned(),
                1 => t!("MEASUREGEOM  Specify first ray point:").into_owned(),
                _ => t!("MEASUREGEOM  Specify second ray point:").into_owned(),
            },
            Mode::Radius => t!("MEASUREGEOM  Select arc or circle:").into_owned(),
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn wants_text_input(&self) -> bool {
        // Only the opening mode-keyword step reads a typed token.
        self.mode == Mode::Choose
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
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
        self.mode == Mode::Radius
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.picked.as_ref().and_then(Self::radius_of) {
            Some(radius) => {
                let diameter = radius * 2.0;
                let radius_s = format!("{radius:.4}");
                let diameter_s = format!("{diameter:.4}");
                CmdResult::Measurement(
                    t!(
                        "Radius = %{radius},  Diameter = %{diameter}",
                        radius = radius_s,
                        diameter = diameter_s
                    )
                    .into_owned(),
                )
            }
            // Picked something that is not a circle or arc — keep prompting.
            None => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
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
                    CmdResult::Measurement(Self::angle_msg(
                        self.points[0],
                        self.points[1],
                        self.points[2],
                    ))
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
    names: &["MEASUREGEOM", "MEA"]
}); // MeasureGeomCommand
