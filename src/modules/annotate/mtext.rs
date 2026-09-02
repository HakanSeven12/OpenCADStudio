use acadrust::entities::mtext::AttachmentPoint;
use acadrust::types::Vector3;
use acadrust::MText;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, DynField, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/mtext.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MTEXT",
        label: "MText",
        icon: ICON,
        event: ModuleEvent::Command("MTEXT".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    FirstCorner,
    OppositeCorner,
    Height,
    Justify,
    LineSpacing,
    Rotation,
    Style,
    Width,
    ColumnMode,
    ColumnCount,
    ColumnWidth,
    ColumnGutter,
}

pub struct MTextCommand {
    step: Step,
    first: Option<DVec3>,
    plane: WorkingPlane,
    height: f64,
    style: String,
    attachment: AttachmentPoint,
    line_spacing: f64,
    rotation: f64,
    width: Option<f64>,
    column_type: i16,
    column_count: i32,
    column_width: f64,
    column_gutter: f64,
}

impl MTextCommand {
    pub fn with_defaults(height: f64, style: String) -> Self {
        Self {
            step: Step::FirstCorner,
            first: None,
            plane: WorkingPlane::default(),
            height: height.max(1e-6),
            style,
            attachment: AttachmentPoint::TopLeft,
            line_spacing: 1.0,
            rotation: 0.0,
            width: None,
            column_type: 0,
            column_count: 2,
            column_width: 0.0,
            column_gutter: 0.0,
        }
    }

    fn resume_point_step(&mut self) {
        self.step = if self.first.is_some() {
            Step::OppositeCorner
        } else {
            Step::FirstCorner
        };
    }

    fn point_options() -> Vec<CmdOption> {
        vec![
            CmdOption::new("Height", "HEIGHT"),
            CmdOption::new("Justify", "JUSTIFY"),
            CmdOption::new("Line spacing", "LINESPACING"),
            CmdOption::new("Rotation", "ROTATION"),
            CmdOption::new("Style", "STYLE"),
            CmdOption::new("Width", "WIDTH"),
            CmdOption::new("Columns", "COLUMNS"),
        ]
    }

    fn begin_option(&mut self, keyword: &str) -> bool {
        self.step = match keyword {
            "H" | "HEIGHT" => Step::Height,
            "J" | "JUSTIFY" => Step::Justify,
            "L" | "LINESPACING" | "LINE SPACING" => Step::LineSpacing,
            "R" | "ROTATION" => Step::Rotation,
            "S" | "STYLE" => Step::Style,
            "W" | "WIDTH" => Step::Width,
            "C" | "COLUMNS" => Step::ColumnMode,
            _ => return false,
        };
        true
    }

    fn boundary_geometry(&self, opposite: DVec3) -> (DVec3, f64, f64) {
        let first = self.first.unwrap_or(opposite);
        let local = self.plane.vector_to_local(opposite - first);
        let (sin, cos) = self.rotation.sin_cos();
        let picked_horizontal = local.x * cos + local.y * sin;
        let vertical = -local.x * sin + local.y * cos;
        let width = self.width.unwrap_or(picked_horizontal.abs());
        let horizontal = if self.width.is_some() {
            width.copysign(if picked_horizontal.abs() > 1e-12 {
                picked_horizontal
            } else {
                1.0
            })
        } else {
            picked_horizontal
        };
        let (xmin, xmax) = (horizontal.min(0.0), horizontal.max(0.0));
        let (ymin, ymax) = (vertical.min(0.0), vertical.max(0.0));
        let horizontal_anchor = match self.attachment {
            AttachmentPoint::TopLeft
            | AttachmentPoint::MiddleLeft
            | AttachmentPoint::BottomLeft => 0.0,
            AttachmentPoint::TopCenter
            | AttachmentPoint::MiddleCenter
            | AttachmentPoint::BottomCenter => 0.5,
            AttachmentPoint::TopRight
            | AttachmentPoint::MiddleRight
            | AttachmentPoint::BottomRight => 1.0,
        };
        let vertical_anchor = match self.attachment {
            AttachmentPoint::TopLeft
            | AttachmentPoint::TopCenter
            | AttachmentPoint::TopRight => 1.0,
            AttachmentPoint::MiddleLeft
            | AttachmentPoint::MiddleCenter
            | AttachmentPoint::MiddleRight => 0.5,
            AttachmentPoint::BottomLeft
            | AttachmentPoint::BottomCenter
            | AttachmentPoint::BottomRight => 0.0,
        };
        let anchor_x = xmin + (xmax - xmin) * horizontal_anchor;
        let anchor_y = ymin + (ymax - ymin) * vertical_anchor;
        let offset = DVec3::new(
            anchor_x * cos - anchor_y * sin,
            anchor_x * sin + anchor_y * cos,
            0.0,
        );
        (
            first + self.plane.vector_to_world(offset),
            width,
            vertical.abs(),
        )
    }

    fn open_editor(&self, opposite: DVec3) -> CmdResult {
        let (insertion, width, boundary_height) = self.boundary_geometry(opposite);
        let mut template = MText::default();
        template.insertion_point = Vector3::new(insertion.x, insertion.y, insertion.z);
        template.height = self.height;
        template.rectangle_width = width.max(0.0);
        template.rectangle_height = (boundary_height > 1e-9).then_some(boundary_height);
        template.rotation = self.rotation;
        template.style = self.style.clone();
        template.attachment_point = self.attachment;
        template.line_spacing_factor = self.line_spacing;
        template.column_data.column_type = self.column_type;
        if self.column_type != 0 {
            template.column_data.column_count =
                crate::entities::text_support::clamp_mtext_column_count(self.column_count);
            template.column_data.auto_height = self.column_type == 2;
            template.column_data.width = if self.column_width > 0.0 {
                self.column_width
            } else if width > 0.0 {
                width
            } else {
                self.height * 10.0
            };
            template.column_data.gutter = if self.column_gutter > 0.0 {
                self.column_gutter
            } else {
                self.height
            };
        }
        CmdResult::OpenMTextEditor {
            pos: insertion,
            handle: None,
            initial: String::new(),
            height: self.height,
            template: Some(Box::new(template)),
        }
    }
}

impl CadCommand for MTextCommand {
    fn name(&self) -> &'static str {
        "MTEXT"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::FirstCorner => t!("MTEXT  Specify first corner:").into_owned(),
            Step::OppositeCorner => t!("MTEXT  Specify opposite corner:").into_owned(),
            Step::Height => crate::tf!("MTEXT  Specify text height <{}>:", self.height).into_owned(),
            Step::Justify => t!("MTEXT  Enter justification [TL / TC / TR / ML / MC / MR / BL / BC / BR] <TL>:").into_owned(),
            Step::LineSpacing => crate::tf!("MTEXT  Enter line spacing factor (0.25-4.00) <{}>:", self.line_spacing).into_owned(),
            Step::Rotation => crate::tf!("MTEXT  Specify rotation angle <{}>:", self.rotation.to_degrees()).into_owned(),
            Step::Style => crate::tf!("MTEXT  Enter text style <{}>:", self.style).into_owned(),
            Step::Width => crate::tf!("MTEXT  Specify boundary width <{}>:", self.width.unwrap_or(0.0)).into_owned(),
            Step::ColumnMode => t!("MTEXT  Columns [None / Static / Dynamic] <None>:").into_owned(),
            Step::ColumnCount => crate::tf!("MTEXT  Enter column count <{}>:", self.column_count).into_owned(),
            Step::ColumnWidth => crate::tf!("MTEXT  Enter column width <{}>:", self.column_width).into_owned(),
            Step::ColumnGutter => crate::tf!("MTEXT  Enter column gutter <{}>:", self.column_gutter).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::FirstCorner | Step::OppositeCorner => Self::point_options(),
            Step::Justify => ["TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"]
                .into_iter()
                .map(|value| CmdOption::new(value, value))
                .collect(),
            Step::ColumnMode => vec![
                CmdOption::new("None", "NONE"),
                CmdOption::new("Static", "STATIC"),
                CmdOption::new("Dynamic", "DYNAMIC"),
            ],
            _ => Vec::new(),
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::FirstCorner | Step::OppositeCorner)
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, Step::Style) {
            InputKind::FreeText
        } else {
            InputKind::SingleToken
        }
    }

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::Rotation => DynField::Angle,
            Step::Height
            | Step::LineSpacing
            | Step::Width
            | Step::ColumnCount
            | Step::ColumnWidth
            | Step::ColumnGutter => DynField::Scalar,
            _ => DynField::Point,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let trimmed = text.trim();
        let upper = trimmed.to_ascii_uppercase();
        if matches!(self.step, Step::FirstCorner | Step::OppositeCorner)
            && self.begin_option(&upper)
        {
            return Some(CmdResult::NeedPoint);
        }

        match self.step {
            Step::Height => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value <= 0.0 {
                    return None;
                }
                self.height = value;
                self.resume_point_step();
            }
            Step::Justify => {
                self.attachment = match upper.as_str() {
                    "TL" => AttachmentPoint::TopLeft,
                    "TC" => AttachmentPoint::TopCenter,
                    "TR" => AttachmentPoint::TopRight,
                    "ML" => AttachmentPoint::MiddleLeft,
                    "MC" => AttachmentPoint::MiddleCenter,
                    "MR" => AttachmentPoint::MiddleRight,
                    "BL" => AttachmentPoint::BottomLeft,
                    "BC" => AttachmentPoint::BottomCenter,
                    "BR" => AttachmentPoint::BottomRight,
                    _ => return None,
                };
                self.resume_point_step();
            }
            Step::LineSpacing => {
                let value = trimmed.replace(',', ".").parse::<f64>().ok()?;
                if !(0.25..=4.0).contains(&value) {
                    return None;
                }
                self.line_spacing = value;
                self.resume_point_step();
            }
            Step::Rotation => {
                self.rotation = crate::entities::common::parse_typed_direction(trimmed)?;
                self.resume_point_step();
            }
            Step::Style => {
                if trimmed.is_empty() {
                    return None;
                }
                self.style = trimmed.to_string();
                self.resume_point_step();
            }
            Step::Width => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value < 0.0 {
                    return None;
                }
                self.width = Some(value);
                self.resume_point_step();
            }
            Step::ColumnMode => {
                self.column_type = match upper.as_str() {
                    "N" | "NONE" => 0,
                    "S" | "STATIC" => 1,
                    "D" | "DYNAMIC" => 2,
                    _ => return None,
                };
                if self.column_type == 0 {
                    self.resume_point_step();
                } else {
                    self.step = Step::ColumnCount;
                }
            }
            Step::ColumnCount => {
                let value = trimmed.parse::<i32>().ok()?;
                if !(1..=crate::entities::text_support::MAX_MTEXT_COLUMNS).contains(&value) {
                    return None;
                }
                self.column_count = value;
                self.step = Step::ColumnWidth;
            }
            Step::ColumnWidth => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value <= 0.0 {
                    return None;
                }
                self.column_width = value;
                self.step = Step::ColumnGutter;
            }
            Step::ColumnGutter => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value < 0.0 {
                    return None;
                }
                self.column_gutter = value;
                self.resume_point_step();
            }
            Step::FirstCorner | Step::OppositeCorner => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::FirstCorner => {
                self.first = Some(point);
                self.step = Step::OppositeCorner;
                CmdResult::NeedPoint
            }
            Step::OppositeCorner => self.open_editor(point),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Height
            | Step::Justify
            | Step::LineSpacing
            | Step::Rotation
            | Step::Style
            | Step::Width
            | Step::ColumnMode
            | Step::ColumnCount
            | Step::ColumnWidth
            | Step::ColumnGutter => {
                self.resume_point_step();
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, _point: DVec3) -> Option<WireModel> {
        None
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["MTEXT"] });
