// MLEADER: pick the arrow and elbow, then edit the text.

use codec::entities::{LeaderContentType, MultiLeader, MultiLeaderPathType};
use codec::types::Vector3;
use codec::EntityType;
use glam::{DVec3, Mat4, Vec3};

use std::sync::atomic::{AtomicU8, Ordering};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
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

/// The creation order and the last Options choice carry over to the next
/// MLEADER of the session.
static LAST_ORDER: AtomicU8 = AtomicU8::new(CreationOrder::ArrowFirst as u8);
static LAST_OPTION: AtomicU8 = AtomicU8::new(MlOption::Exit as u8);

pub struct MLeaderCommand {
    verts: Vec<DVec3>,
    /// Content first: the width between the text box corners.
    box_width: Option<f64>,
    /// The Options prompt's default (the option chosen last).
    last_option: MlOption,
    plane: WorkingPlane,
    style: Option<codec::objects::MultiLeaderStyle>,
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
    /// MLEADERLAYER as the laYer option shows it (empty: the current layer).
    layer: String,
    /// The laYer option changed MLEADERLAYER.
    layer_set: bool,
    text: String,
    picked_entity: Option<EntityType>,
    selected_mtext: Option<codec::entities::MText>,
    block_sources: Vec<(String, codec::Handle)>,
    block_handle: Option<codec::Handle>,
    layers: Vec<String>,
    text_styles: Vec<(String, codec::Handle)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationOrder {
    ArrowFirst,
    LandingFirst,
    ContentFirst,
}

impl CreationOrder {
    fn last() -> Self {
        match LAST_ORDER.load(Ordering::Relaxed) {
            1 => Self::LandingFirst,
            2 => Self::ContentFirst,
            _ => Self::ArrowFirst,
        }
    }
}

/// The MLEADER Options prompt's choices, in prompt order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MlOption {
    LeaderType,
    Landing,
    ContentType,
    MaxPoints,
    FirstAngle,
    SecondAngle,
    Layer,
    Exit,
}

impl MlOption {
    fn last() -> Self {
        match LAST_OPTION.load(Ordering::Relaxed) {
            0 => Self::LeaderType,
            1 => Self::Landing,
            2 => Self::ContentType,
            3 => Self::MaxPoints,
            4 => Self::FirstAngle,
            5 => Self::SecondAngle,
            6 => Self::Layer,
            _ => Self::Exit,
        }
    }
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
            box_width: None,
            last_option: MlOption::last(),
            plane: WorkingPlane::default(),
            style: None,
            display_scale: 1.0,
            order: CreationOrder::last(),
            step: Step::PickPoints,
            content_type: LeaderContentType::MText,
            path_type: MultiLeaderPathType::StraightLineSegments,
            enable_landing: true,
            landing_length: 2.5,
            max_points: 2,
            first_angle: None,
            second_angle: None,
            layer: String::new(),
            layer_set: false,
            text: String::new(),
            picked_entity: None,
            selected_mtext: None,
            block_sources: Vec::new(),
            block_handle: None,
            layers: Vec::new(),
            text_styles: Vec::new(),
        }
    }

    pub fn with_style(style: codec::objects::MultiLeaderStyle, annotation_multiplier: f64) -> Self {
        let display_scale = if style.is_annotative {
            annotation_multiplier
        } else {
            style.scale_factor
        };
        Self {
            verts: Vec::new(),
            box_width: None,
            last_option: MlOption::last(),
            plane: WorkingPlane::default(),
            style: Some(style.clone()),
            display_scale,
            order: CreationOrder::last(),
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
            layer_set: false,
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
        block_sources: Vec<(String, codec::Handle)>,
        layers: Vec<String>,
        text_styles: Vec<(String, codec::Handle)>,
    ) -> Self {
        self.block_sources = block_sources;
        self.layers = layers;
        self.text_styles = text_styles;
        self
    }

    /// Start from the drawing's MLEADERLAYER (`None`: the current layer).
    pub fn with_layer(mut self, layer: Option<String>) -> Self {
        self.layer = layer.unwrap_or_default();
        self
    }

    fn set_layer(&mut self, layer: &str) {
        self.layer = layer.to_string();
        self.layer_set = true;
    }

    fn point_limit(&self) -> usize {
        if self.order == CreationOrder::ContentFirst {
            2
        } else if self.max_points == 0 {
            usize::MAX
        } else {
            self.max_points.max(2)
        }
    }

    /// Run an Options choice; it becomes the prompt's default.
    fn choose_option(&mut self, option: MlOption) {
        self.last_option = option;
        LAST_OPTION.store(option as u8, Ordering::Relaxed);
        self.step = match option {
            MlOption::LeaderType => Step::LeaderType,
            MlOption::Landing => Step::Landing,
            MlOption::ContentType => Step::ContentType,
            MlOption::MaxPoints => Step::MaxPoints,
            MlOption::FirstAngle => Step::FirstAngle,
            MlOption::SecondAngle => Step::SecondAngle,
            MlOption::Layer => Step::Layer,
            MlOption::Exit => Step::PickPoints,
        };
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
                let mut content = local[0];
                let arrow = local[1];
                let sign = if content.x >= arrow.x { 1.0 } else { -1.0 };
                // Text left of the arrow hangs from the box's right edge.
                if sign < 0.0 {
                    content.x += self.box_width.unwrap_or(0.0);
                }
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
        // New multileaders reach their text and keep no wrap width unless
        // the content-first box gave one; the text flows by its style.
        ml.extend_leader_to_text = true;
        ml.context.text_width = self.box_width.unwrap_or(0.0);
        ml.context.text_flow_direction =
            codec::entities::multileader::FlowDirectionType::ByStyle;
        ml.enable_landing = self.enable_landing;
        ml.enable_dogleg = self.enable_landing;
        ml.dogleg_length = self.landing_length;
        if self.content_type == LeaderContentType::Block {
            ml.block_content_handle = self.block_handle;
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
            ml.context.text_direction = source
                .dwg_x_direction
                .unwrap_or_else(|| Vector3::new(source.rotation.cos(), source.rotation.sin(), 0.0));
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
                codec::entities::mtext::DrawingDirection::LeftToRight => {
                    codec::entities::multileader::FlowDirectionType::Horizontal
                }
                codec::entities::mtext::DrawingDirection::TopToBottom => {
                    codec::entities::multileader::FlowDirectionType::Vertical
                }
                codec::entities::mtext::DrawingDirection::ByStyle => {
                    codec::entities::multileader::FlowDirectionType::ByStyle
                }
            };
            let (attachment, alignment) = match source.attachment_point {
                codec::entities::mtext::AttachmentPoint::TopCenter
                | codec::entities::mtext::AttachmentPoint::MiddleCenter
                | codec::entities::mtext::AttachmentPoint::BottomCenter => (
                    codec::entities::multileader::TextAttachmentPointType::Center,
                    codec::entities::TextAlignmentType::Center,
                ),
                codec::entities::mtext::AttachmentPoint::TopRight
                | codec::entities::mtext::AttachmentPoint::MiddleRight
                | codec::entities::mtext::AttachmentPoint::BottomRight => (
                    codec::entities::multileader::TextAttachmentPointType::Right,
                    codec::entities::TextAlignmentType::Right,
                ),
                _ => (
                    codec::entities::multileader::TextAttachmentPointType::Left,
                    codec::entities::TextAlignmentType::Left,
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
        let entity = self
            .plane
            .place_entity(EntityType::MultiLeader(Box::new(ml)));
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
        let candidates = [
            angle,
            -angle,
            std::f64::consts::PI - angle,
            angle - std::f64::consts::PI,
        ];
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
        use crate::tr;
        let num = |v: f64| format!("{v:.4}");
        let angle = |a: Option<f64>| {
            a.map_or("0".to_string(), |r| format!("{}", r.to_degrees().round()))
        };
        match self.step {
            Step::Options => {
                let default = match self.last_option {
                    MlOption::LeaderType => tr!("mleader-prompt", "opt-leader-type"),
                    MlOption::Landing => tr!("mleader-prompt", "opt-landing"),
                    MlOption::ContentType => tr!("mleader-prompt", "opt-content-type"),
                    MlOption::MaxPoints => tr!("mleader-prompt", "opt-maxpoints"),
                    MlOption::FirstAngle => tr!("mleader-prompt", "opt-first-angle"),
                    MlOption::SecondAngle => tr!("mleader-prompt", "opt-second-angle"),
                    MlOption::Layer => tr!("mleader-prompt", "opt-layer"),
                    MlOption::Exit => tr!("mleader-prompt", "opt-exit"),
                };
                tr!("mleader-prompt", "options", default = default)
            }
            Step::LeaderType => {
                let current = match self.path_type {
                    MultiLeaderPathType::Spline => tr!("mleader-prompt", "spline"),
                    MultiLeaderPathType::Invisible => tr!("mleader-prompt", "none"),
                    _ => tr!("mleader-prompt", "straight"),
                };
                tr!("mleader-prompt", "leader-type", current = current)
            }
            Step::Landing => {
                let current = if self.enable_landing {
                    tr!("mleader-prompt", "yes")
                } else {
                    tr!("mleader-prompt", "no")
                };
                tr!("mleader-prompt", "use-landing", current = current)
            }
            Step::LandingLength => {
                tr!("mleader-prompt", "landing-distance", current = num(self.landing_length))
            }
            Step::ContentType => {
                let current = match self.content_type {
                    LeaderContentType::Block => tr!("mleader-prompt", "block"),
                    LeaderContentType::None => tr!("mleader-prompt", "none"),
                    _ => tr!("mleader-prompt", "mtext"),
                };
                tr!("mleader-prompt", "content-type", current = current)
            }
            Step::MaxPoints => {
                tr!("mleader-prompt", "max-points", current = self.max_points.to_string())
            }
            Step::FirstAngle => {
                tr!("mleader-prompt", "first-angle", current = angle(self.first_angle))
            }
            Step::SecondAngle => {
                tr!("mleader-prompt", "second-angle", current = angle(self.second_angle))
            }
            Step::Layer => {
                let current = if self.layer.is_empty() {
                    tr!("mleader-cmd", "use-current")
                } else {
                    self.layer.clone()
                };
                tr!("mleader-cmd", "layer", current = current)
            }
            Step::PreEnterText => t!("MLEADER  Enter text:").into_owned(),
            Step::SelectMText => t!("MLEADER  Select an MText object:").into_owned(),
            Step::BlockSource => tr!("mleader-prompt", "block-name"),
            Step::PickPoints if self.verts.is_empty() => {
                let text = !self.text.is_empty();
                match (self.order, text) {
                    (CreationOrder::ArrowFirst, false) => tr!("mleader-prompt", "arrow-first"),
                    (CreationOrder::ArrowFirst, true) => {
                        tr!("mleader-prompt", "arrow-first-text")
                    }
                    (CreationOrder::LandingFirst, false) => {
                        tr!("mleader-prompt", "landing-first")
                    }
                    (CreationOrder::LandingFirst, true) => {
                        tr!("mleader-prompt", "landing-first-text")
                    }
                    (CreationOrder::ContentFirst, false) => {
                        tr!("mleader-prompt", "content-first")
                    }
                    (CreationOrder::ContentFirst, true) => {
                        tr!("mleader-prompt", "content-first-text")
                    }
                }
            }
            Step::PickPoints if self.order == CreationOrder::ContentFirst => {
                if self.box_width.is_none() {
                    tr!("mleader-prompt", "opposite-corner")
                } else {
                    tr!("mleader-prompt", "arrowhead")
                }
            }
            Step::PickPoints if self.verts.len() + 1 >= self.point_limit() => {
                if self.order == CreationOrder::LandingFirst {
                    tr!("mleader-prompt", "arrowhead")
                } else {
                    tr!("mleader-prompt", "landing")
                }
            }
            Step::PickPoints => tr!("mleader-prompt", "next-point"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        let keys = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(label, key)| CmdOption::new(label, key))
                .collect::<Vec<_>>()
        };
        match self.step {
            Step::PickPoints if self.verts.is_empty() => {
                let mut list: Vec<(&str, &str)> = Vec::new();
                if self.text.is_empty() {
                    list.push(("pre enter Text", "T"));
                }
                match self.order {
                    CreationOrder::ArrowFirst => {
                        list.extend([("leader Landing first", "L"), ("Content first", "C")])
                    }
                    CreationOrder::LandingFirst => {
                        list.extend([("leader arrowHead first", "H"), ("Content first", "C")])
                    }
                    CreationOrder::ContentFirst => list.extend([
                        ("select Mtext", "M"),
                        ("leader arrowHead first", "H"),
                        ("leader Landing first", "L"),
                    ]),
                }
                list.push(("Options", "O"));
                keys(&list)
            }
            Step::Options => keys(&[
                ("Leader type", "L"),
                ("leader lAnding", "A"),
                ("Content type", "C"),
                ("Maxpoints", "M"),
                ("First angle", "F"),
                ("Second angle", "S"),
                ("laYer", "Y"),
                ("eXit options", "X"),
            ]),
            Step::LeaderType => keys(&[("Straight", "S"), ("sPline", "P"), ("None", "N")]),
            Step::Landing => keys(&[("Yes", "Y"), ("No", "N")]),
            Step::ContentType => keys(&[("Block", "B"), ("Mtext", "M"), ("None", "N")]),
            Step::BlockSource => self
                .block_sources
                .iter()
                .map(|(name, _)| CmdOption::new(name, name))
                .collect(),
            Step::Layer => [CmdOption::new("?", "?"), CmdOption::new("eXit", "X")]
                .into_iter()
                .chain(self.layers.iter().map(|name| CmdOption::new(name, name)))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step != Step::PickPoints {
            return CmdResult::NeedPoint;
        }
        // Content first: the text box's two corners, then the arrowhead.
        if self.order == CreationOrder::ContentFirst
            && self.verts.len() == 1
            && self.box_width.is_none()
        {
            let first = self.plane.to_local(self.verts[0]);
            let other = self.plane.to_local(pt);
            self.box_width = Some((other.x - first.x).abs());
            // The text sits at the box's top-left corner.
            self.verts[0] = self.plane.to_world(DVec3::new(
                first.x.min(other.x),
                first.y.max(other.y),
                first.z,
            ));
            return CmdResult::NeedPoint;
        }
        self.verts.push(self.constrained_point(pt));

        if self.verts.len() < self.point_limit() {
            return CmdResult::NeedPoint;
        }
        self.finish()
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Options => {
                let option = self.last_option;
                self.choose_option(option);
                CmdResult::NeedPoint
            }
            Step::LeaderType
            | Step::Landing
            | Step::LandingLength
            | Step::ContentType
            | Step::MaxPoints
            | Step::FirstAngle
            | Step::SecondAngle
            | Step::Layer => {
                self.step = Step::Options;
                CmdResult::NeedPoint
            }
            Step::BlockSource => {
                CmdResult::ReportError(crate::tr!("mleader-prompt", "invalid-input"))
            }
            Step::PreEnterText | Step::SelectMText => {
                self.step = Step::PickPoints;
                CmdResult::NeedPoint
            }
            Step::PickPoints if self.verts.is_empty() => {
                self.step = Step::Options;
                CmdResult::NeedPoint
            }
            Step::PickPoints if self.verts.len() >= 2 => self.finish(),
            Step::PickPoints => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::PickPoints || self.verts.is_empty() {
            return None;
        }
        if self.order == CreationOrder::ContentFirst && self.box_width.is_none() {
            // The text box being spanned.
            let a = self.plane.to_local(self.verts[0]);
            let b = self.plane.to_local(pt);
            let corners = [
                DVec3::new(a.x, a.y, a.z),
                DVec3::new(b.x, a.y, a.z),
                DVec3::new(b.x, b.y, a.z),
                DVec3::new(a.x, b.y, a.z),
                DVec3::new(a.x, a.y, a.z),
            ];
            let mut preview = preview_wire(&[], 0.0);
            preview.points = corners
                .iter()
                .map(|p| self.plane.to_world(*p).as_vec3().to_array())
                .collect();
            return Some(preview);
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

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, Step::Layer | Step::PreEnterText) {
            InputKind::FreeText
        } else if matches!(self.step, Step::PickPoints | Step::SelectMText) {
            InputKind::Point
        } else {
            InputKind::SingleToken
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PickPoints | Step::Layer)
    }

    fn on_text_input(&mut self, input: &str) -> Option<CmdResult> {
        let text = input.trim();
        let upper = text.to_ascii_uppercase();
        let upper = upper.trim_start_matches('_');
        let invalid_keyword = || Some(CmdResult::ReportError(t!("Invalid option keyword.").into_owned()));
        let invalid_input =
            || Some(CmdResult::ReportError(crate::tr!("mleader-prompt", "invalid-input")));
        match self.step {
            Step::PickPoints if self.verts.is_empty() => {
                let order = match (upper, self.order) {
                    ("T" | "TEXT" | "PRE ENTER TEXT", _) if self.text.is_empty() => {
                        self.step = Step::PreEnterText;
                        return Some(CmdResult::NeedPoint);
                    }
                    ("O" | "OPTIONS", _) => {
                        self.step = Step::Options;
                        return Some(CmdResult::NeedPoint);
                    }
                    ("M" | "MTEXT" | "SELECT MTEXT", CreationOrder::ContentFirst) => {
                        self.step = Step::SelectMText;
                        return Some(CmdResult::NeedPoint);
                    }
                    ("H" | "ARROWHEAD" | "LEADER ARROWHEAD FIRST", order)
                        if order != CreationOrder::ArrowFirst =>
                    {
                        CreationOrder::ArrowFirst
                    }
                    ("L" | "LANDING" | "LEADER LANDING FIRST", order)
                        if order != CreationOrder::LandingFirst =>
                    {
                        CreationOrder::LandingFirst
                    }
                    ("C" | "CONTENT" | "CONTENT FIRST", order)
                        if order != CreationOrder::ContentFirst =>
                    {
                        CreationOrder::ContentFirst
                    }
                    _ => return None,
                };
                self.order = order;
                LAST_ORDER.store(order as u8, Ordering::Relaxed);
            }
            Step::Options => {
                let option = match upper {
                    "L" | "LEADER TYPE" => MlOption::LeaderType,
                    "A" | "LANDING" | "LEADER LANDING" => MlOption::Landing,
                    "C" | "CONTENT TYPE" => MlOption::ContentType,
                    "M" | "MAXPOINTS" => MlOption::MaxPoints,
                    "F" | "FIRST ANGLE" => MlOption::FirstAngle,
                    "S" | "SECOND ANGLE" => MlOption::SecondAngle,
                    "Y" | "LAYER" => MlOption::Layer,
                    "X" | "EXIT" | "EXIT OPTIONS" => MlOption::Exit,
                    _ => return invalid_keyword(),
                };
                self.choose_option(option);
            }
            Step::LeaderType => {
                self.path_type = match upper {
                    "S" | "STRAIGHT" => MultiLeaderPathType::StraightLineSegments,
                    "P" | "SPLINE" => MultiLeaderPathType::Spline,
                    "N" | "NONE" => MultiLeaderPathType::Invisible,
                    _ => return invalid_keyword(),
                };
                self.step = Step::Options;
            }
            Step::Landing => match upper {
                "Y" | "YES" => {
                    self.enable_landing = true;
                    self.step = Step::LandingLength;
                }
                "N" | "NO" => {
                    self.enable_landing = false;
                    self.step = Step::Options;
                }
                _ => return invalid_keyword(),
            },
            Step::LandingLength => {
                let Some(value) = crate::entities::common::parse_typed_length(text) else {
                    return invalid_input();
                };
                if value < 0.0 {
                    return invalid_input();
                }
                self.landing_length = value;
                self.step = Step::Options;
            }
            Step::ContentType => {
                self.content_type = match upper {
                    "B" | "BLOCK" => LeaderContentType::Block,
                    "M" | "MTEXT" => LeaderContentType::MText,
                    "N" | "NONE" => LeaderContentType::None,
                    _ => return invalid_keyword(),
                };
                self.step = if self.content_type == LeaderContentType::Block {
                    Step::BlockSource
                } else {
                    Step::Options
                };
            }
            Step::MaxPoints => {
                // 0 leaves the count open (Enter ends the leader).
                let Some(value) = text.parse::<usize>().ok().filter(|v| *v == 0 || *v >= 2)
                else {
                    return invalid_input();
                };
                self.max_points = value;
                self.step = Step::Options;
            }
            Step::FirstAngle | Step::SecondAngle => {
                let Some(degrees) = text
                    .parse::<f64>()
                    .ok()
                    .filter(|d| [0.0, 15.0, 30.0, 45.0, 60.0, 90.0, 180.0].contains(d))
                else {
                    return Some(CmdResult::ReportError(crate::tr!(
                        "mleader-prompt",
                        "invalid-angle"
                    )));
                };
                let angle = (degrees != 0.0).then(|| degrees.to_radians());
                if self.step == Step::FirstAngle {
                    self.first_angle = angle;
                } else {
                    self.second_angle = angle;
                }
                self.step = Step::Options;
            }
            Step::Layer => {
                match upper {
                    "" => return None,
                    "X" | "EXIT" => self.step = Step::Options,
                    "?" => {
                        return Some(CmdResult::ReportMeasurement(self.layers.join("\n")));
                    }
                    "." => self.set_layer(""),
                    _ => self.set_layer(text.trim_matches('"')),
                }
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
                    return invalid_input();
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
        matches!(self.step, Step::SelectMText | Step::Layer)
    }

    fn drawing_variable_update(&self) -> Option<(&'static str, String)> {
        self.layer_set.then(|| {
            let value = if self.layer.is_empty() { "." } else { &self.layer };
            ("MLEADERLAYER", value.to_string())
        })
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: codec::Handle, _point: DVec3) -> CmdResult {
        if self.step == Step::Layer {
            // A picked object lends its layer.
            if let Some(entity) = self.picked_entity.take() {
                let layer = entity.as_entity().layer().to_string();
                self.set_layer(&layer);
            }
            return CmdResult::NeedPoint;
        }
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
        self.selected_mtext = Some(*text);
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
    style: Option<&codec::objects::MultiLeaderStyle>,
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
    crate::entities::multileader::set_mleader_text_side(&mut ml, to_right);

    ml.context.text_rotation = (ux.y as f64).atan2(ux.x as f64);
    ml.context.text_direction = Vector3::new(ux.x as f64, ux.y as f64, 0.0);

    if let Some(root) = ml.context.leader_roots.first_mut() {
        root.direction = Vector3::new(landing.x as f64, landing.y as f64, 0.0);

        root.connection_point = elbow_v3;
        root.landing_distance = landing_distance;
    }

    // Place text beyond the landing and gap.
    let off = landing * (landing_distance + landing_gap) as f32;

    let mut text_location = content_point.map(v3).unwrap_or_else(|| {
        Vector3::new(
            elbow_v3.x + off.x as f64,
            elbow_v3.y + off.y as f64,
            elbow_v3.z,
        )
    });
    // The text location is the text's top-left corner; level text sits so
    // its attachment line meets the landing. A content-first box gives that
    // corner, so the landing comes to it, one landing and gap away.
    let level = ux.y.abs() < 1.0e-6 && ux.x > 0.0;
    let top = if level {
        crate::entities::multileader::text_top_offset(&ml)
    } else {
        0.0
    };
    if content_point.is_some() {
        if let Some(root) = ml.context.leader_roots.first_mut() {
            root.connection_point.x =
                text_location.x - sign * (landing_distance + landing_gap) * f64::from(ux.x);
            root.connection_point.y = text_location.y - top;
        }
    } else {
        text_location.y += top;
    }
    // The content base is the landing's end.
    let base = ml.context.leader_roots.first().map_or(text_location, |root| {
        Vector3::new(
            root.connection_point.x + root.direction.x * landing_distance,
            root.connection_point.y + root.direction.y * landing_distance,
            root.connection_point.z,
        )
    });

    ml.context.text_location = text_location;
    ml.context.content_base_point = base;

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

        ..Default::default()
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
inventory::submit!(crate::command::CommandRegistration {
    names: &["MLEADER"]
}); // MLeaderCommand
