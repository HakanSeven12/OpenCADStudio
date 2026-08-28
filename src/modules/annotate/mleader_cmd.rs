// MLEADER: pick the arrow and elbow, then edit the text.

use acadrust::entities::{LeaderContentType, MultiLeader, MultiLeaderPathType};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Mat4, Vec3};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
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
    order: CreationOrder,
    step: Step,
    content_type: LeaderContentType,
    path_type: MultiLeaderPathType,
    enable_landing: bool,
    landing_length: f64,
    max_points: usize,
    first_angle: Option<f64>,
    second_angle: Option<f64>,
    layer: String,
    text: String,
    picked_entity: Option<EntityType>,
    selected_mtext: Option<acadrust::entities::MText>,
    block_sources: Vec<(String, acadrust::Handle)>,
    block_handle: Option<acadrust::Handle>,
    layers: Vec<String>,
    text_styles: Vec<(String, acadrust::Handle)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationOrder {
    ArrowFirst,
    LandingFirst,
    ContentFirst,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    PickPoints,
    Options,
    LeaderType,
    Landing,
    LandingLength,
    ContentType,
    MaxPoints,
    FirstAngle,
    SecondAngle,
    Layer,
    PreEnterText,
    SelectMText,
    BlockSource,
}

impl MLeaderCommand {
    pub fn new() -> Self {
        Self {
            verts: Vec::new(),
            plane: WorkingPlane::default(),
            style: None,
            display_scale: 1.0,
            order: CreationOrder::ArrowFirst,
            step: Step::PickPoints,
            content_type: LeaderContentType::MText,
            path_type: MultiLeaderPathType::StraightLineSegments,
            enable_landing: true,
            landing_length: 2.5,
            max_points: 2,
            first_angle: None,
            second_angle: None,
            layer: String::new(),
            text: String::new(),
            picked_entity: None,
            selected_mtext: None,
            block_sources: Vec::new(),
            block_handle: None,
            layers: Vec::new(),
            text_styles: Vec::new(),
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
            style: Some(style.clone()),
            display_scale,
            order: CreationOrder::ArrowFirst,
            step: Step::PickPoints,
            content_type: (style.content_type as i16).into(),
            path_type: (style.path_type as i16).into(),
            enable_landing: style.enable_landing,
            landing_length: style.landing_distance,
            max_points: style.max_leader_points.max(2) as usize,
            first_angle: (style.first_segment_angle.abs() > 1.0e-12)
                .then_some(style.first_segment_angle),
            second_angle: (style.second_segment_angle.abs() > 1.0e-12)
                .then_some(style.second_segment_angle),
            layer: String::new(),
            text: style.default_text.clone(),
            picked_entity: None,
            selected_mtext: None,
            block_sources: Vec::new(),
            block_handle: style.block_content_handle,
            layers: Vec::new(),
            text_styles: Vec::new(),
        }
    }

    pub fn with_drawing_resources(
        mut self,
        block_sources: Vec<(String, acadrust::Handle)>,
        layers: Vec<String>,
        text_styles: Vec<(String, acadrust::Handle)>,
    ) -> Self {
        self.block_sources = block_sources;
        self.layers = layers;
        self.text_styles = text_styles;
        self
    }

    fn point_limit(&self) -> usize {
        if self.order == CreationOrder::ContentFirst {
            2
        } else {
            self.max_points.max(2)
        }
    }

    fn finish(&self) -> CmdResult {
        if self.verts.len() < 2 {
            return CmdResult::Cancel;
        }
        let local: Vec<DVec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        let (leader_points, content_point) = match self.order {
            CreationOrder::ArrowFirst => (local, None),
            CreationOrder::LandingFirst => {
                let mut points = local;
                points.reverse();
                (points, None)
            }
            CreationOrder::ContentFirst => {
                let content = local[0];
                let arrow = local[1];
                let sign = if content.x >= arrow.x { 1.0 } else { -1.0 };
                let elbow = DVec3::new(
                    content.x - sign * self.landing_length * self.display_scale,
                    content.y,
                    content.z,
                );
                (vec![arrow, elbow], Some(content))
            }
        };
        let mut ml = build_mleader(
            &self.text,
            &leader_points,
            content_point,
            Mat4::IDENTITY,
            self.style.as_ref(),
            self.display_scale,
        );
        ml.content_type = self.content_type;
        ml.path_type = self.path_type;
        ml.enable_landing = self.enable_landing;
        ml.enable_dogleg = self.enable_landing;
        ml.dogleg_length = self.landing_length;
        if self.content_type == LeaderContentType::Block {
            ml.block_content_handle = self.block_handle;
        }
        if !self.layer.trim().is_empty() {
            ml.common.layer = format!("__MLEADER_LAYER__{}", self.layer.trim());
        }
        for root in &mut ml.context.leader_roots {
            root.landing_distance = self.landing_length * self.display_scale;
            for line in &mut root.lines {
                line.path_type = self.path_type;
            }
        }
        normalize_content_context(&mut ml);
        if let Some(source) = &self.selected_mtext {
            let scale = self.display_scale.max(1.0e-12);
            ml.text_height = source.height / scale;
            ml.context.text_height = source.height;
            ml.context.text_width = source.rectangle_width;
            ml.context.text_rotation = source.rotation;
            ml.context.text_direction = source.dwg_x_direction.unwrap_or_else(|| {
                Vector3::new(source.rotation.cos(), source.rotation.sin(), 0.0)
            });
            ml.context.line_spacing_factor = source.line_spacing_factor;
            ml.context.line_spacing_style = source.line_spacing_style;
            ml.text_color = source.common.color;
            ml.context.text_color = source.common.color;
            ml.text_frame = source.background_fill_flags & 0x10 != 0;
            ml.context.background_fill_enabled = source.background_fill_flags & 0x01 != 0;
            ml.context.background_mask_fill_on = source.background_fill_flags & 0x02 != 0;
            ml.context.background_scale_factor = source.background_scale;
            ml.context.background_fill_color = source.background_color;
            ml.context.background_transparency = source.background_transparency;
            ml.context.text_flow_direction = match source.drawing_direction {
                acadrust::entities::mtext::DrawingDirection::LeftToRight => {
                    acadrust::entities::multileader::FlowDirectionType::Horizontal
                }
                acadrust::entities::mtext::DrawingDirection::TopToBottom => {
                    acadrust::entities::multileader::FlowDirectionType::Vertical
                }
                acadrust::entities::mtext::DrawingDirection::ByStyle => {
                    acadrust::entities::multileader::FlowDirectionType::ByStyle
                }
            };
            let (attachment, alignment) = match source.attachment_point {
                acadrust::entities::mtext::AttachmentPoint::TopCenter
                | acadrust::entities::mtext::AttachmentPoint::MiddleCenter
                | acadrust::entities::mtext::AttachmentPoint::BottomCenter => {
                    (
                        acadrust::entities::multileader::TextAttachmentPointType::Center,
                        acadrust::entities::TextAlignmentType::Center,
                    )
                }
                acadrust::entities::mtext::AttachmentPoint::TopRight
                | acadrust::entities::mtext::AttachmentPoint::MiddleRight
                | acadrust::entities::mtext::AttachmentPoint::BottomRight => {
                    (
                        acadrust::entities::multileader::TextAttachmentPointType::Right,
                        acadrust::entities::TextAlignmentType::Right,
                    )
                }
                _ => (
                    acadrust::entities::multileader::TextAttachmentPointType::Left,
                    acadrust::entities::TextAlignmentType::Left,
                ),
            };
            ml.context.text_attachment_point = attachment;
            ml.text_alignment = alignment;
            ml.context.text_alignment = alignment;
            let style_handle = self
                .text_styles
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&source.style))
                .map(|(_, handle)| *handle);
            ml.text_style_handle = style_handle;
            ml.context.text_style_handle = style_handle;
        }
        let entity = self.plane.place_entity(EntityType::MultiLeader(ml));
        if self.content_type == LeaderContentType::MText && self.text.is_empty() {
            CmdResult::CommitAndEditText(entity)
        } else {
            CmdResult::CommitAndExit(entity)
        }
    }

    fn constrained_point(&self, point: DVec3) -> DVec3 {
        let Some(previous) = self.verts.last().copied() else {
            return point;
        };
        let segment_index = self.verts.len() - 1;
        let constraint = match (self.order, segment_index) {
            (CreationOrder::LandingFirst, 0) => self.second_angle,
            (CreationOrder::LandingFirst, 1) => self.first_angle,
            (_, 0) => self.first_angle,
            (_, 1) => self.second_angle,
            _ => None,
        };
        let Some(angle) = constraint else {
            return point;
        };
        let previous_local = self.plane.to_local(previous);
        let point_local = self.plane.to_local(point);
        let delta = point_local - previous_local;
        let length = delta.truncate().length();
        if length <= 1.0e-12 {
            return point;
        }
        let candidates = [angle, -angle, std::f64::consts::PI - angle, angle - std::f64::consts::PI];
        let current = delta.y.atan2(delta.x);
        let chosen = candidates
            .into_iter()
            .min_by(|a, b| {
                angle_delta(current, *a)
                    .partial_cmp(&angle_delta(current, *b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(angle);
        let local = DVec3::new(
            previous_local.x + length * chosen.cos(),
            previous_local.y + length * chosen.sin(),
            point_local.z,
        );
        self.plane.to_world(local)
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
        match self.step {
            Step::Options => t!("MLEADER  Options [Leader type/Landing/Content type/Max points/First angle/Second angle/Layer/Exit]:").into_owned(),
            Step::LeaderType => t!("MLEADER  Leader type [Straight/Spline/None]:").into_owned(),
            Step::Landing => t!("MLEADER  Leader landing [Yes/No]:").into_owned(),
            Step::LandingLength => t!("MLEADER  Specify landing distance:").into_owned(),
            Step::ContentType => t!("MLEADER  Content type [MText/Block/None]:").into_owned(),
            Step::MaxPoints => t!("MLEADER  Enter maximum leader points:").into_owned(),
            Step::FirstAngle => t!("MLEADER  First segment angle [Any/15/30/45/60/90]:").into_owned(),
            Step::SecondAngle => t!("MLEADER  Second segment angle [Any/15/30/45/60/90]:").into_owned(),
            Step::Layer => t!("MLEADER  Enter layer name:").into_owned(),
            Step::PreEnterText => t!("MLEADER  Enter text:").into_owned(),
            Step::SelectMText => t!("MLEADER  Select an MText object:").into_owned(),
            Step::BlockSource => t!("MLEADER  Enter source block name:").into_owned(),
            Step::PickPoints if self.verts.is_empty() => match self.order {
                CreationOrder::ArrowFirst => t!("MLEADER  Specify arrowhead point or [Landing first/Content first/Text/Select MText/Options]:").into_owned(),
                CreationOrder::LandingFirst => t!("MLEADER  Specify landing point or [Arrowhead first/Content first/Text/Select MText/Options]:").into_owned(),
                CreationOrder::ContentFirst => t!("MLEADER  Specify content location or [Arrowhead first/Landing first/Text/Select MText/Options]:").into_owned(),
            },
            Step::PickPoints if self.order == CreationOrder::ContentFirst => {
                t!("MLEADER  Specify arrowhead point:").into_owned()
            }
            Step::PickPoints => t!("MLEADER  Specify next leader point or press Enter to finish:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::PickPoints if self.verts.is_empty() => vec![
                CmdOption::new("Arrowhead first", "A"),
                CmdOption::new("Landing first", "L"),
                CmdOption::new("Content first", "C"),
                CmdOption::new("Text", "T"),
                CmdOption::new("Select MText", "S"),
                CmdOption::new("Options", "O"),
            ],
            Step::Options => vec![
                CmdOption::new("Leader type", "LT"),
                CmdOption::new("Landing", "LD"),
                CmdOption::new("Content type", "CT"),
                CmdOption::new("Max points", "M"),
                CmdOption::new("First angle", "F"),
                CmdOption::new("Second angle", "S"),
                CmdOption::new("Layer", "LA"),
                CmdOption::new("Exit", "X"),
            ],
            Step::LeaderType => ["Straight", "Spline", "None"].into_iter().map(|v| CmdOption::new(v, v)).collect(),
            Step::Landing => ["Yes", "No"].into_iter().map(|v| CmdOption::new(v, v)).collect(),
            Step::ContentType => ["MText", "Block", "None"].into_iter().map(|v| CmdOption::new(v, v)).collect(),
            Step::BlockSource => self
                .block_sources
                .iter()
                .map(|(name, _)| CmdOption::new(name, name))
                .collect(),
            Step::Layer => self
                .layers
                .iter()
                .map(|name| CmdOption::new(name, name))
                .collect(),
            Step::FirstAngle | Step::SecondAngle => ["Any", "15", "30", "45", "60", "90"].into_iter().map(|v| CmdOption::new(v, v)).collect(),
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step != Step::PickPoints {
            return CmdResult::NeedPoint;
        }
        self.verts.push(self.constrained_point(pt));

        if self.verts.len() < self.point_limit() {
            return CmdResult::NeedPoint;
        }
        self.finish()
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step != Step::PickPoints || self.verts.len() < 2 {
            return CmdResult::Cancel;
        }
        self.finish()
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::PickPoints || self.verts.is_empty() {
            return None;
        }
        // Preview / rubber-band is GPU screen-space: downcast to f32 here.
        let mut local_points: Vec<DVec3> = self
            .verts
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        local_points.push(self.plane.to_local(self.constrained_point(pt)));
        let pts: Vec<Vec3> = match self.order {
            CreationOrder::ArrowFirst => local_points,
            CreationOrder::LandingFirst => {
                local_points.reverse();
                local_points
            }
            CreationOrder::ContentFirst => {
                let content = local_points[0];
                let arrow = *local_points.last().unwrap_or(&content);
                let sign = if content.x >= arrow.x { 1.0 } else { -1.0 };
                let elbow = DVec3::new(
                    content.x - sign * self.landing_length * self.display_scale,
                    content.y,
                    content.z,
                );
                vec![arrow, elbow]
            }
        }
        .into_iter()
        .map(DVec3::as_vec3)
        .collect();
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

    fn wants_text_input(&self) -> bool {
        true
    }

    fn wants_text_with_spaces(&self) -> bool {
        matches!(self.step, Step::Layer | Step::PreEnterText)
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::PickPoints
    }

    fn on_text_input(&mut self, input: &str) -> Option<CmdResult> {
        let text = input.trim();
        let upper = text.to_ascii_uppercase();
        match self.step {
            Step::PickPoints if self.verts.is_empty() => match upper.as_str() {
                "A" | "ARROWHEAD" | "ARROWHEAD FIRST" => self.order = CreationOrder::ArrowFirst,
                "L" | "LANDING" | "LANDING FIRST" => self.order = CreationOrder::LandingFirst,
                "C" | "CONTENT" | "CONTENT FIRST" => self.order = CreationOrder::ContentFirst,
                "T" | "TEXT" => self.step = Step::PreEnterText,
                "S" | "SELECT" | "SELECT MTEXT" => self.step = Step::SelectMText,
                "O" | "OPTIONS" => self.step = Step::Options,
                _ => return None,
            },
            Step::Options => match upper.as_str() {
                "LT" | "LEADER TYPE" => self.step = Step::LeaderType,
                "LD" | "LANDING" => self.step = Step::Landing,
                "CT" | "CONTENT TYPE" => self.step = Step::ContentType,
                "M" | "MAX" | "MAX POINTS" => self.step = Step::MaxPoints,
                "F" | "FIRST" | "FIRST ANGLE" => self.step = Step::FirstAngle,
                "S" | "SECOND" | "SECOND ANGLE" => self.step = Step::SecondAngle,
                "LA" | "LAYER" => self.step = Step::Layer,
                "X" | "EXIT" => self.step = Step::PickPoints,
                _ => return None,
            },
            Step::LeaderType => {
                self.path_type = match upper.as_str() {
                    "SPLINE" => MultiLeaderPathType::Spline,
                    "NONE" => MultiLeaderPathType::Invisible,
                    "STRAIGHT" => MultiLeaderPathType::StraightLineSegments,
                    _ => return None,
                };
                self.step = Step::Options;
            }
            Step::Landing => match upper.as_str() {
                "Y" | "YES" => {
                    self.enable_landing = true;
                    self.step = Step::LandingLength;
                }
                "N" | "NO" => {
                    self.enable_landing = false;
                    self.step = Step::Options;
                }
                _ => return None,
            },
            Step::LandingLength => {
                let value = crate::entities::common::parse_typed_length(text)?;
                if value <= 0.0 {
                    return None;
                }
                self.landing_length = value;
                self.step = Step::Options;
            }
            Step::ContentType => {
                self.content_type = match upper.as_str() {
                    "MTEXT" => LeaderContentType::MText,
                    "BLOCK" => LeaderContentType::Block,
                    "NONE" => LeaderContentType::None,
                    _ => return None,
                };
                self.step = if self.content_type == LeaderContentType::Block
                    && self.block_handle.is_none()
                {
                    Step::BlockSource
                } else {
                    Step::Options
                };
            }
            Step::MaxPoints => {
                let value = text.parse::<usize>().ok()?;
                if value < 2 {
                    return None;
                }
                self.max_points = value;
                self.step = Step::Options;
            }
            Step::FirstAngle | Step::SecondAngle => {
                let angle = if upper == "ANY" {
                    None
                } else {
                    let degrees = text.parse::<f64>().ok()?;
                    if !matches!(degrees as i32, 15 | 30 | 45 | 60 | 90) {
                        return None;
                    }
                    Some(degrees.to_radians())
                };
                if self.step == Step::FirstAngle {
                    self.first_angle = angle;
                } else {
                    self.second_angle = angle;
                }
                self.step = Step::Options;
            }
            Step::Layer => {
                if text.is_empty() {
                    return None;
                }
                let Some(layer) = self
                    .layers
                    .iter()
                    .find(|layer| layer.eq_ignore_ascii_case(text))
                    .cloned()
                else {
                    return None;
                };
                self.layer = layer;
                self.step = Step::Options;
            }
            Step::PreEnterText => {
                self.text = input.to_string();
                self.content_type = LeaderContentType::MText;
                self.step = Step::PickPoints;
            }
            Step::SelectMText => return None,
            Step::BlockSource => {
                let Some((_, handle)) = self
                    .block_sources
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(text))
                else {
                    return None;
                };
                self.block_handle = Some(*handle);
                self.content_type = LeaderContentType::Block;
                self.step = Step::Options;
            }
            Step::PickPoints => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::SelectMText
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: acadrust::Handle, _point: DVec3) -> CmdResult {
        let Some(EntityType::MText(text)) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        self.verts.clear();
        self.verts.push(DVec3::new(
            text.insertion_point.x,
            text.insertion_point.y,
            text.insertion_point.z,
        ));
        self.text = text.value.clone();
        self.selected_mtext = Some(text);
        self.content_type = LeaderContentType::MText;
        self.order = CreationOrder::ContentFirst;
        self.step = Step::PickPoints;
        CmdResult::NeedPoint
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

fn build_mleader(
    text: &str,
    verts: &[DVec3],
    content_point: Option<DVec3>,
    ucs: Mat4,
    style: Option<&acadrust::objects::MultiLeaderStyle>,
    display_scale: f64,
) -> MultiLeader {
    // First point is the arrow, last point is the elbow. Intermediate points
    // are leader vertices governed by the configured maximum-point count.
    let arrow_pt = verts[0];
    let elbow_pt = *verts.last().unwrap_or(&verts[0]);

    let elbow_v3 = v3(elbow_pt);

    let leader_vertices = verts[..verts.len().saturating_sub(1)]
        .iter()
        .copied()
        .map(v3)
        .collect();
    let mut ml = MultiLeader::with_text(text, elbow_v3, leader_vertices);
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
    // Align the landing and text with the active UCS X axis.
    let ux = ucs.transform_vector3(Vec3::X).normalize_or(Vec3::X);
    // Which side of the leader the text sits on, measured along the UCS X axis.
    let to_right = (elbow_pt - arrow_pt).dot(ux.as_dvec3()) >= 0.0;
    let sign = if to_right { 1.0 } else { -1.0 };
    let landing = ux * (sign as f32);
    ml.context.text_attachment_point =
        if to_right {
            acadrust::entities::multileader::TextAttachmentPointType::Left
        } else {
            acadrust::entities::multileader::TextAttachmentPointType::Right
        };

    ml.context.text_rotation = (ux.y as f64).atan2(ux.x as f64);
    ml.context.text_direction = Vector3::new(ux.x as f64, ux.y as f64, 0.0);

    if let Some(root) = ml.context.leader_roots.first_mut() {
        root.direction =
            Vector3::new(landing.x as f64, landing.y as f64, 0.0);

        root.connection_point = elbow_v3;
        root.landing_distance = landing_distance;
    }

    // Place text beyond the landing and gap.
    let off = landing * (landing_distance + landing_gap) as f32;

    let text_location = content_point.map(v3).unwrap_or_else(|| {
        Vector3::new(
            elbow_v3.x + off.x as f64,
            elbow_v3.y + off.y as f64,
            elbow_v3.z,
        )
    });

    ml.context.text_location = text_location;
    ml.context.content_base_point = text_location;

    ml
}

fn normalize_content_context(ml: &mut MultiLeader) {
    match ml.content_type {
        LeaderContentType::MText => {
            ml.context.has_text_contents = true;
            ml.context.has_block_contents = false;
        }
        LeaderContentType::Block => {
            ml.context.has_text_contents = false;
            ml.context.has_block_contents = ml.block_content_handle.is_some();
            ml.context.block_content_handle = ml.block_content_handle;
            ml.context.block_content_location = ml.context.content_base_point;
            ml.context.block_content_scale = ml.block_scale;
            ml.context.block_rotation = ml.block_rotation;
            ml.context.block_content_color = ml.block_content_color;
            ml.context.block_connection_type = ml.block_connection_type;
        }
        _ => {
            ml.context.has_text_contents = false;
            ml.context.has_block_contents = false;
        }
    }
}

fn angle_delta(a: f64, b: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    ((a - b + std::f64::consts::PI).rem_euclid(two_pi) - std::f64::consts::PI).abs()
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
