// MLEADER command
//
// Flow:
//   1. CollectPoints — click arrowhead, then bend points; Enter (≥2) to finish
//   2. AskText       — wants_text_input; blank Enter = no text
//   → commit single MultiLeader entity

use acadrust::entities::MultiLeader;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Mat4, Vec3};

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/mleader.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MLEADER",
        label: "MLeader",
        icon: ICON,
        event: ModuleEvent::Command("MLEADER".to_string()),
    }
}

pub struct MLeaderCommand {
    verts: Vec<DVec3>,
    plane: WorkingPlane,
    style: Option<acadrust::objects::MultiLeaderStyle>,
    display_scale: f64,
}

impl MLeaderCommand {
    pub fn new() -> Self {
        Self {
            verts: Vec::new(),
            plane: WorkingPlane::default(),
            style: None,
            display_scale: 1.0,
        }
    }

    pub fn with_style(
        style: acadrust::objects::MultiLeaderStyle,
        annotation_multiplier: f64,
    ) -> Self {
        let display_scale = if style.is_annotative {
            annotation_multiplier
        } else {
            style.scale_factor
        };
        Self {
            verts: Vec::new(),
            plane: WorkingPlane::default(),
            style: Some(style),
            display_scale,
        }
    }
}

impl CadCommand for MLeaderCommand {
    fn name(&self) -> &'static str {
        "MLEADER"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        if self.verts.is_empty() {
            t!("MLEADER  Specify arrowhead point:").into_owned()
        } else {
            t!(
                "MLEADER  Specify next point [%{count} pts — Enter to place text]:",
                count = self.verts.len()
            )
            .into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.verts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.verts.len() < 2 {
            return CmdResult::Cancel;
        }
        // Place the leader with empty text, then open the in-place MText editor
        // so the user types the annotation into the rich editor.
        let local: Vec<DVec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        let ml = build_mleader(
            "",
            &local,
            Mat4::IDENTITY,
            self.style.as_ref(),
            self.display_scale,
        );
        CmdResult::CommitAndEditText(
            self.plane.place_entity(EntityType::MultiLeader(ml)),
        )
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.verts.is_empty() {
            return None;
        }
        // Preview / rubber-band is GPU screen-space: downcast to f32 here.
        let mut pts: Vec<Vec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point).as_vec3())
            .collect();
        pts.push(self.plane.to_local(pt).as_vec3());
        let arrow_size = self
            .style
            .as_ref()
            .map_or(2.5, |style| style.arrowhead_size)
            * self.display_scale;
        let mut preview = preview_wire(&pts, arrow_size as f32);
        preview.points = preview
            .points
            .iter()
            .map(|point| {
                if point[0].is_nan() {
                    *point
                } else {
                    self.plane
                        .to_world(Vec3::from_array(*point).as_dvec3())
                        .as_vec3()
                        .to_array()
                }
            })
            .collect();
        Some(preview)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn build_mleader(
    text: &str,
    verts: &[DVec3],
    ucs: Mat4,
    style: Option<&acadrust::objects::MultiLeaderStyle>,
    display_scale: f64,
) -> MultiLeader {
    // Last vertex = content/text location; remaining = leader line points
    let (leader_pts, content_pt) = verts.split_at(verts.len() - 1);
    let content_pt = content_pt[0];

    let leader_v3: Vec<Vector3> = leader_pts.iter().map(|p| v3(*p)).collect();
    let content_v3 = v3(content_pt);

    let mut ml = MultiLeader::with_text(text, content_v3, leader_v3);
    if let Some(style) = style {
        crate::scene::annotative::apply_mleader_style(&mut ml, style);
    } else {
        ml.text_height = 2.5;
        ml.context.text_height = 2.5;
        ml.arrowhead_size = 2.5;
        ml.context.arrowhead_size = 2.5;
        ml.dogleg_length = 2.5;
    }

    let landing_distance = ml.dogleg_length * display_scale;
    let landing_gap = ml.context.landing_gap * display_scale;
    ml.context.scale_factor = display_scale;
    ml.context.text_height = ml.text_height * display_scale;
    ml.context.arrowhead_size = ml.arrowhead_size * display_scale;
    ml.context.landing_gap = landing_gap;
    // "Horizontal" landing + text run along the active UCS X axis (identity =
    // world), so the annotation reads square to the user's coordinate system.
    let ux = ucs.transform_vector3(Vec3::X).normalize_or(Vec3::X);
    // Which side of the leader the text sits on, measured along the UCS X axis.
    let last_leader = leader_pts.last().copied().unwrap_or(content_pt);
    let to_right = (content_pt - last_leader).dot(ux.as_dvec3()) >= 0.0;
    let sign = if to_right { 1.0 } else { -1.0 };
    let landing = ux * (sign as f32);

    // Text + landing read along the UCS X axis. text_direction is what the
    // renderer consults first, so set both.
    ml.context.text_rotation = (ux.y as f64).atan2(ux.x as f64);
    ml.context.text_direction = Vector3::new(ux.x as f64, ux.y as f64, 0.0);

    if let Some(root) = ml.context.leader_roots.first_mut() {
        // Leader ends at the clicked point; the landing runs from there toward
        // the text along the UCS X axis.
        root.direction = Vector3::new(landing.x as f64, landing.y as f64, 0.0);
        root.connection_point = content_v3;
        root.landing_distance = landing_distance;
    }

    // Seed the text one landing-length past the leader end, on the side the
    // user dragged toward, offset along the UCS X axis.
    let off = landing * (landing_distance + landing_gap) as f32;
    ml.context.text_location =
        Vector3::new(content_v3.x + off.x as f64, content_v3.y + off.y as f64, content_v3.z);

    ml
}

fn preview_wire(pts: &[Vec3], arrow_size: f32) -> WireModel {
    let mut points: Vec<[f32; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    if pts.len() >= 2 {
        let [w1, w2] = arrowhead_wings(pts[0], pts[1], arrow_size);
        points.push([f32::NAN; 3]);
        points.push([w1.x, w1.y, w1.z]);
        points.push([pts[0].x, pts[0].y, pts[0].z]);
        points.push([w2.x, w2.y, w2.z]);
    }
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
        name: "mleader_preview".into(),
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

fn arrowhead_wings(tip: Vec3, next: Vec3, size: f32) -> [Vec3; 2] {
    let d = next - tip;
    let len = (d.x * d.x + d.y * d.y).sqrt().max(1e-9);
    let (dx, dy) = (d.x / len, d.y / len);
    let angle = std::f32::consts::PI / 6.0;
    let (s, c) = angle.sin_cos();
    [
        Vec3::new(
            tip.x + (dx * c - dy * s) * size,
            tip.y + (dx * s + dy * c) * size,
            tip.z,
        ),
        Vec3::new(
            tip.x + (dx * c + dy * s) * size,
            tip.y + (-dx * s + dy * c) * size,
            tip.z,
        ),
    ]
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["MLEADER"] });  // MLeaderCommand
