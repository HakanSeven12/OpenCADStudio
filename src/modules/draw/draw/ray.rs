// Ray / XLine draw commands.
//
//  RAY   — semi-infinite line: click base point, then click direction point.
//          Produces a Ray entity; repeats until Enter/Esc.
//  XLINE — infinite construction line: same two-click pattern, yields XLine.

use crate::t;
use acadrust::entities::{Ray as RayEnt, XLine as XLineEnt};
use acadrust::types::Vector3;
use acadrust::EntityType;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

const DISPLAY_EXTENT: f32 = 1_000_000.0;

// ── RAY ───────────────────────────────────────────────────────────────────

pub struct RayCommand {
    base: Option<DVec3>,
}

impl RayCommand {
    pub fn new() -> Self {
        Self { base: None }
    }
}

impl CadCommand for RayCommand {
    fn name(&self) -> &'static str {
        "RAY"
    }

    fn prompt(&self) -> String {
        if self.base.is_none() {
            crate::t!("RAY  Specify start point:").into_owned()
        } else {
            crate::t!("RAY  Specify through point:").into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.base.is_some() {
            vec![CmdOption::enter(t!("Done").as_ref())]
        } else {
            vec![]
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(base) = self.base {
            let dir = pt - base;
            let len = dir.length();
            if len < 1e-6 {
                return CmdResult::NeedPoint;
            }
            let dir_n = dir / len;
            let ray = RayEnt::new(
                Vector3::new(base.x, base.y, base.z),
                Vector3::new(dir_n.x, dir_n.y, dir_n.z),
            );
            // Repeated through points share the original start point.
            CmdResult::CommitEntity(EntityType::Ray(ray))
        } else {
            self.base = Some(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let pt = pt.as_vec3();
        let base = self.base?.as_vec3();
        let dir = (pt - base).normalize_or_zero();
        let far = base + dir * DISPLAY_EXTENT;
        Some(WireModel {
            bg_adapt: None,
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
            name: "ray_preview".into(),
            points: vec![[base.x, base.y, base.z], [far.x, far.y, far.z]],
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
        })
    }
}

// ── XLINE ─────────────────────────────────────────────────────────────────

pub struct XLineCommand {
    base: Option<DVec3>,
    mode: XLineMode,
    plane: WorkingPlane,
    reference: Option<(DVec3, DVec3)>,
    picked: Option<EntityType>,
    offset: Option<f64>,
    value_origin: Option<DVec3>,
}

#[derive(Clone, Copy)]
enum XLineMode {
    Points,
    Direction(DVec3),
    Angle,
    AngleReference,
    Bisect,
    OffsetDistance,
    OffsetPick,
    OffsetSide,
}

impl XLineCommand {
    pub fn new() -> Self {
        Self {
            base: None,
            mode: XLineMode::Points,
            plane: WorkingPlane::default(),
            reference: None,
            picked: None,
            offset: None,
            value_origin: None,
        }
    }

    fn geometry(&self, pt: DVec3) -> Option<(DVec3, DVec3)> {
        if !pt.is_finite() { return None; }
        let (base, direction) = match self.mode {
            XLineMode::Points => (self.base?, pt - self.base?),
            XLineMode::Direction(dir) => (pt, dir),
            XLineMode::Bisect => {
                let base = self.base?;
                let first = self.reference?.1;
                let last = (pt - base).try_normalize()?;
                (base, first + last)
            }
            XLineMode::OffsetSide => {
                let (base, dir) = self.reference?;
                if let Some(distance) = self.offset {
                    let normal = self.plane.z.cross(dir).try_normalize()?;
                    let side = (pt - base).dot(normal);
                    if side.abs() < 1e-10 { return None; }
                    (base + normal * distance * side.signum(), dir)
                } else {
                    (pt, dir)
                }
            }
            _ => return None,
        };
        Some((base, direction.try_normalize()?))
    }
}

impl CadCommand for XLineCommand {
    fn name(&self) -> &'static str {
        "XLINE"
    }

    fn prompt(&self) -> String {
        if self.value_origin.is_some() { return "XLINE  Specify second point:".into(); }
        match self.mode {
            XLineMode::Points if self.base.is_none() => "XLINE  Specify a point or [Hor/Ver/Ang/Bisect/Offset]:",
            XLineMode::Angle => "XLINE  Enter angle of xline <0> or [Reference]:",
            XLineMode::AngleReference | XLineMode::OffsetPick => "XLINE  Select a line object:",
            XLineMode::Bisect if self.base.is_none() => "XLINE  Specify angle vertex point:",
            XLineMode::Bisect if self.reference.is_none() => "XLINE  Specify angle start point:",
            XLineMode::Bisect => "XLINE  Specify angle end point:",
            XLineMode::OffsetDistance => "XLINE  Specify offset distance or [Through] <Through>:",
            XLineMode::OffsetSide if self.offset.is_some() => "XLINE  Specify side to offset:",
            _ => "XLINE  Specify through point:",
        }.into()
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.value_origin.is_some() { return vec![]; }
        match self.mode {
            XLineMode::Points if self.base.is_none() => vec![
                CmdOption::new("Hor", "H"), CmdOption::new("Ver", "V"),
                CmdOption::new("Ang", "A"), CmdOption::new("Bisect", "B"),
                CmdOption::new("Offset", "O"),
            ],
            XLineMode::Angle => vec![CmdOption::new("Reference", "R")],
            XLineMode::OffsetDistance => vec![CmdOption::new("Through", "T")],
            _ => vec![CmdOption::enter(t!("Done").as_ref())],
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if !pt.is_finite() { return CmdResult::NeedPoint; }
        if matches!(self.mode, XLineMode::Angle | XLineMode::OffsetDistance) {
            if self.value_origin.is_none() { self.value_origin = Some(pt); }
            else if let Some(value) = self.dyn_live_value(pt) {
                return self.on_text_input(&value.to_string()).unwrap_or(CmdResult::NeedPoint);
            }
            return CmdResult::NeedPoint;
        }
        if let Some((base, dir)) = self.geometry(pt) {
            let entity = XLineEnt::new(
                Vector3::new(base.x, base.y, base.z),
                Vector3::new(dir.x, dir.y, dir.z),
            );
            if matches!(self.mode, XLineMode::OffsetSide) {
                self.mode = XLineMode::OffsetPick;
                self.reference = None;
            }
            return CmdResult::CommitEntity(EntityType::XLine(entity));
        }
        match self.mode {
            XLineMode::Points | XLineMode::Bisect if self.base.is_none() => self.base = Some(pt),
            XLineMode::Bisect if self.reference.is_none() => {
                if let Some(dir) = (pt - self.base.unwrap()).try_normalize() {
                    self.reference = Some((self.base.unwrap(), dir));
                }
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.value_origin.is_some() { return CmdResult::NeedPoint; }
        match self.mode {
            XLineMode::Angle => self.on_text_input("0").unwrap_or(CmdResult::NeedPoint),
            XLineMode::OffsetDistance => {
                self.mode = XLineMode::OffsetPick;
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) { self.plane = plane; }
    fn wants_text_input(&self) -> bool { true }
    fn point_step_accepts_keywords(&self) -> bool { true }
    fn dyn_field(&self) -> crate::command::DynField {
        match self.mode {
            XLineMode::Angle => crate::command::DynField::Angle,
            XLineMode::OffsetDistance => crate::command::DynField::Distance,
            _ => crate::command::DynField::Point,
        }
    }
    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.mode, XLineMode::Angle | XLineMode::OffsetDistance)
    }
    fn dyn_live_value(&self, cursor: DVec3) -> Option<f64> {
        let origin = self.value_origin?;
        if !cursor.is_finite() { return None; }
        match self.mode {
            XLineMode::Angle => {
                let delta = self.plane.vector_to_local(cursor - origin);
                (delta.x.hypot(delta.y) > 1e-10).then(|| delta.y.atan2(delta.x).to_degrees())
            }
            XLineMode::OffsetDistance => {
                let distance = cadkernel::space::Vec3::from(cursor.to_array()).distance(origin.to_array().into());
                (distance.is_finite() && distance > 0.0).then_some(distance)
            }
            _ => None,
        }
    }
    fn dyn_auto_sign_angle(&self) -> bool { false }
    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, XLineMode::AngleReference | XLineMode::OffsetPick)
    }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) { self.picked = Some(entity); }
    fn on_entity_pick(&mut self, _handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        let xyz = |v: Vector3| DVec3::new(v.x, v.y, v.z);
        let reference = match self.picked.take() {
            Some(EntityType::Line(line)) => Some((xyz(line.start), xyz(line.end) - xyz(line.start))),
            Some(EntityType::Ray(line)) => Some((xyz(line.base_point), xyz(line.direction))),
            Some(EntityType::XLine(line)) => Some((xyz(line.base_point), xyz(line.direction))),
            _ => None,
        };
        if let Some((base, dir)) = reference {
            if let Some(dir) = dir.try_normalize() {
                self.reference = Some((base, dir));
                self.mode = if matches!(self.mode, XLineMode::AngleReference) {
                    XLineMode::Angle
                } else { XLineMode::OffsetSide };
            }
        }
        CmdResult::NeedPoint
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let key = text.trim().to_ascii_uppercase();
        match self.mode {
            XLineMode::Points if self.base.is_none() => {
                self.mode = match key.as_str() {
                    "H" | "HOR" => XLineMode::Direction(self.plane.x),
                    "V" | "VER" => XLineMode::Direction(self.plane.y),
                    "A" | "ANG" => XLineMode::Angle,
                    "B" | "BISECT" => XLineMode::Bisect,
                    "O" | "OFFSET" => XLineMode::OffsetDistance,
                    _ => return None,
                };
            }
            XLineMode::Angle => {
                if key == "R" || key == "REFERENCE" {
                    if self.value_origin.is_some() { return None; }
                    self.mode = XLineMode::AngleReference;
                } else {
                    let angle: f64 = key.parse().ok().filter(|v: &f64| v.is_finite())?;
                    let (sin, cos) = angle.to_radians().sin_cos();
                    let axis = self.reference.map_or(self.plane.x, |(_, dir)| dir);
                    self.mode = XLineMode::Direction(axis * cos + self.plane.z.cross(axis) * sin);
                }
            }
            XLineMode::OffsetDistance => {
                self.offset = if key == "T" || key == "THROUGH" {
                    if self.value_origin.is_some() { return None; }
                    None
                } else {
                    Some(key.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)?)
                };
                self.mode = XLineMode::OffsetPick;
            }
            _ => return None,
        }
        self.value_origin = None;
        Some(CmdResult::NeedPoint)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let (base, dir) = self.geometry(pt)?;
        let base = base.as_vec3();
        let dir = dir.as_vec3();
        let far_pos = base + dir * DISPLAY_EXTENT;
        let far_neg = base - dir * DISPLAY_EXTENT;
        Some(WireModel {
            bg_adapt: None,
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
            name: "xline_preview".into(),
            points: vec![
                [far_neg.x, far_neg.y, far_neg.z],
                [far_pos.x, far_pos.y, far_pos.z],
            ],
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_rays_and_xlines_keep_their_first_point() {
        let base = DVec3::new(2.0, 3.0, 4.0);
        let mut ray = RayCommand::new();
        let mut xline = XLineCommand::new();
        assert!(matches!(ray.on_point(base), CmdResult::NeedPoint));
        assert!(matches!(xline.on_point(base), CmdResult::NeedPoint));

        for through in [DVec3::new(3.0, 3.0, 4.0), DVec3::new(2.0, 5.0, 4.0)] {
            let CmdResult::CommitEntity(EntityType::Ray(entity)) = ray.on_point(through) else {
                panic!("ray was not committed");
            };
            assert_eq!(entity.base_point, Vector3::new(base.x, base.y, base.z));

            let CmdResult::CommitEntity(EntityType::XLine(entity)) = xline.on_point(through) else {
                panic!("construction line was not committed");
            };
            assert_eq!(entity.base_point, Vector3::new(base.x, base.y, base.z));
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["RAY"] }); // RayCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["CONSTRUCTIONLINE", "XLINE"]
}); // XLineCommand
