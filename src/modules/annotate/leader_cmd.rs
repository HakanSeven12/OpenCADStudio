// LEADER command
//
// Flow:
//   1. Click the arrowhead point, then one or more bend points.
//   2. Enter (≥2 points) places the leader line plus a linked, empty MText
//      annotation at the landing, and opens the in-place MText editor so the
//      user types the annotation. Escape leaves the leader without text.
//
// The MText is a separate entity referenced by the leader's annotation_handle
// (DXF group 340); editing/erasing them stays in sync via that link.

use acadrust::entities::mtext::AttachmentPoint;
use acadrust::entities::{Leader, LeaderCreationType, LeaderPathType, MText};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Mat4, Vec3};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/leader.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LEADER",
        label: "Leader",
        icon: ICON,
        event: ModuleEvent::Command("LEADER".to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Points,
    Format,
    Annotation,
    AnnotationOptions,
}

pub struct LeaderCommand {
    verts: Vec<DVec3>,
    step: Step,
    path_type: LeaderPathType,
    arrow_enabled: bool,
    plane: WorkingPlane,
    dimension_style: String,
    text_style: String,
    text_height: f64,
    display_scale: f64,
    gap: f64,
    arrow_size: f64,
    annotative: bool,
    annotation_lines: Vec<String>,
}

impl LeaderCommand {
    pub fn with_defaults(
        defaults: crate::scene::creation_style::DimensionCreationDefaults,
        annotation_multiplier: f64,
    ) -> Self {
        let display_scale = if defaults.annotative {
            annotation_multiplier
        } else {
            defaults.scale
        };

        Self {
            verts: Vec::new(),
            step: Step::Points,
            path_type: LeaderPathType::StraightLine,
            arrow_enabled: true,
            plane: WorkingPlane::default(),
            dimension_style: defaults.style_name,
            text_style: defaults.text_style_name,
            text_height: defaults.text_height,
            display_scale,
            gap: defaults.gap,
            arrow_size: defaults.arrow_size,
            annotative: defaults.annotative,
            annotation_lines: Vec::new(),
        }
    }

    fn finish(&self, annotation: Option<&str>, open_editor: bool) -> CmdResult {
        if self.verts.len() < 2 {
            return CmdResult::Cancel;
        }

        let local: Vec<DVec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();

        let displayed_height = self.text_height * self.display_scale;

        let displayed_landing = if self.arrow_size > 1.0e-9 {
            self.arrow_size * self.display_scale
        } else {
            displayed_height * 1.5
        };

        // The user supplies only the arrow point and elbow. The horizontal
        // landing is stored as a real third LEADER vertex so it can have its own grip.
        let mut leader_points = local.clone();

        let first = local[0];
        let elbow = *local.last().unwrap_or(&first);
        let sign = if elbow.x >= first.x { 1.0 } else { -1.0 };

        let landing_end = DVec3::new(elbow.x + sign * displayed_landing, elbow.y, elbow.z);

        leader_points.push(landing_end);

        let mut leader = build_leader(
            &leader_points,
            Mat4::IDENTITY,
            &self.dimension_style,
            self.text_height,
            self.gap,
            self.arrow_size,
        );
        leader.path_type = self.path_type;
        leader.arrow_enabled = self.arrow_enabled;

        // The MTEXT starts at the real end of the landing.
        let (anchor, attach) = annotation_anchor(&leader_points, 0.0, Mat4::IDENTITY);

        // Store the MTEXT at its native model-space size for the current
        // annotation scale. Its annotation contexts then scale it relatively
        // when another representation becomes active.
        let mtext_height = displayed_height;

        if annotation.is_none() {
            leader.creation_type = LeaderCreationType::NoAnnotation;
            return CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Leader(leader)));
        }

        let mtext = build_mtext(
            annotation.unwrap_or_default(),
            anchor,
            mtext_height,
            attach,
            Mat4::IDENTITY,
            &self.text_style,
            self.annotative,
        );

        CmdResult::CommitManyAndEditText {
            entities: vec![
                self.plane.place_entity(EntityType::Leader(leader)),
                self.plane.place_entity(EntityType::MText(mtext)),
            ],
            edit_index: 1,
            open_editor,
        }
    }
}

impl CadCommand for LeaderCommand {
    fn name(&self) -> &'static str {
        "LEADER"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        if self.step == Step::Format {
            return t!(
                "LEADER  Enter leader formatting option [Spline/Straight/Arrow/None] <exit>:"
            )
            .into_owned();
        }
        if self.step == Step::AnnotationOptions {
            return t!("LEADER  Enter an annotation option [None/Mtext]:").into_owned();
        }
        if self.step == Step::Annotation {
            return if self.annotation_lines.is_empty() {
                t!("LEADER  Enter first line of annotation text or <options>:").into_owned()
            } else {
                t!("LEADER  Enter next line of annotation text:").into_owned()
            };
        }
        match self.verts.len() {
            0 => t!("LEADER  Specify leader start point:").into_owned(),
            1 => t!("LEADER  Specify next point:").into_owned(),
            _ => t!("LEADER  Specify next point or [Annotation/Format/Undo] <Annotation>:")
                .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.step == Step::Format {
            return vec![
                CmdOption::new("Spline", "SPLINE"),
                CmdOption::new("Straight", "STRAIGHT"),
                CmdOption::new("Arrow", "ARROW"),
                CmdOption::new("None", "NONE"),
            ];
        }
        if self.step == Step::AnnotationOptions {
            return vec![
                CmdOption::new("None", "NONE"),
                CmdOption::new("Mtext", "MTEXT"),
            ];
        }
        if self.verts.len() >= 2 {
            vec![
                CmdOption::new("Annotation", "ANNOTATION"),
                CmdOption::new("Format", "FORMAT"),
                CmdOption::new("Undo", "UNDO"),
            ]
        } else {
            Vec::new()
        }
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, Step::Format | Step::AnnotationOptions) {
            InputKind::SingleToken
        } else if self.step == Step::Annotation {
            InputKind::FreeText
        } else {
            InputKind::Point
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::Points && self.verts.len() >= 2
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        if self.step == Step::Annotation {
            self.annotation_lines.push(text.to_string());
            return Some(CmdResult::NeedPoint);
        }
        if self.step == Step::AnnotationOptions {
            return match keyword.as_str() {
                "N" | "NONE" => Some(self.finish(None, false)),
                "M" | "MTEXT" => Some(self.finish(Some(""), true)),
                _ => None,
            };
        }
        if self.step == Step::Format {
            match keyword.as_str() {
                "S" | "SPLINE" => self.path_type = LeaderPathType::Spline,
                "ST" | "STRAIGHT" => self.path_type = LeaderPathType::StraightLine,
                "A" | "ARROW" => self.arrow_enabled = true,
                "N" | "NONE" => self.arrow_enabled = false,
                _ => return None,
            }
            self.step = Step::Points;
            return Some(CmdResult::NeedPoint);
        }
        if self.verts.len() < 2 {
            return None;
        }
        match keyword.as_str() {
            "A" | "ANNOTATION" => {
                self.step = Step::Annotation;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FORMAT" => {
                self.step = Step::Format;
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => {
                self.verts.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step != Step::Points {
            return CmdResult::NeedPoint;
        }
        self.verts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step == Step::Format {
            self.step = Step::Points;
            CmdResult::NeedPoint
        } else if self.step == Step::Annotation {
            if self.annotation_lines.is_empty() {
                self.step = Step::AnnotationOptions;
                CmdResult::NeedPoint
            } else {
                let text = self.annotation_lines.join("\n");
                self.finish(Some(&text), false)
            }
        } else if self.step == Step::AnnotationOptions {
            CmdResult::NeedPoint
        } else {
            self.step = Step::Annotation;
            CmdResult::NeedPoint
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::Points || self.verts.is_empty() {
            return None;
        }
        let mut pts: Vec<Vec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point).as_vec3())
            .collect();
        pts.push(self.plane.to_local(pt).as_vec3());
        let mut preview = preview_wire(&pts, (self.arrow_size * self.display_scale) as f32);
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

fn dv3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn build_leader(
    verts: &[DVec3],
    ucs: Mat4,
    dimension_style: &str,
    text_height: f64,
    gap: f64,
    arrow_size: f64,
) -> Leader {
    let mut l = Leader::from_vertices(verts.iter().map(|p| dv3(*p)).collect());
    l.creation_type = LeaderCreationType::WithText;

    // New leaders store the landing as their final real vertex, so the renderer
    // must not append a second synthetic hookline.
    l.hookline_enabled = false;
    l.dimension_style = dimension_style.to_string();
    l.text_height = text_height;
    l.dimension_gap = gap;
    l.arrow_size = arrow_size;
    // The hookline / text read along the UCS X axis (DXF "horizontal direction
    // for text"); the renderer uses it instead of world horizontal.
    let ux = ucs.transform_vector3(Vec3::X).normalize_or(Vec3::X);
    l.horizontal_direction = Vector3::new(ux.x as f64, ux.y as f64, 0.0);
    l
}

/// Text anchor at the end of the landing line, and the attachment point that
/// keeps the text reading away from the leader (text to the right of a
/// left-pointing landing, to the left of a right-pointing one).
fn annotation_anchor(verts: &[DVec3], landing_length: f64, ucs: Mat4) -> (DVec3, AttachmentPoint) {
    let last = *verts.last().unwrap();
    let prev = verts[verts.len() - 2];

    let ux = ucs
        .transform_vector3(Vec3::X)
        .normalize_or(Vec3::X)
        .as_dvec3();

    let to_right = (last - prev).dot(ux) >= 0.0;
    let sign = if to_right { 1.0_f64 } else { -1.0_f64 };

    let anchor = last + ux * (sign * landing_length);

    let attach = if to_right {
        AttachmentPoint::MiddleLeft
    } else {
        AttachmentPoint::MiddleRight
    };

    (anchor, attach)
}

fn build_mtext(
    text: &str,
    pos: DVec3,
    height: f64,
    attach: AttachmentPoint,
    ucs: Mat4,
    text_style: &str,
    annotative: bool,
) -> MText {
    let mut m = MText::new();
    m.value = text.to_string();
    m.insertion_point = dv3(pos);
    m.height = height;
    m.style = text_style.to_string();
    m.is_annotative = annotative;
    m.attachment_point = attach;
    // Text reads along the UCS X axis.
    let ux = ucs.transform_vector3(Vec3::X);
    m.rotation = (ux.y as f64).atan2(ux.x as f64);
    m
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
        name: "leader_preview".into(),
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

pub fn arrowhead_wings(tip: Vec3, next: Vec3, size: f32) -> [Vec3; 2] {
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
inventory::submit!(crate::command::CommandRegistration { names: &["LEADER"] }); // LeaderCommand
