use codec::entities::mtext::AttachmentPoint;
use codec::entities::LineSpacingStyle;
use codec::types::Vector3;
use codec::MText;
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
    LineSpacingType,
    LineSpacing,
    Rotation,
    Style,
    Width,
    ColumnMode,
    TotalWidth,
    ColumnCount,
    ColumnWidth,
    ColumnGutter,
    ColumnHeight,
}

pub struct MTextCommand {
    step: Step,
    first: Option<DVec3>,
    plane: WorkingPlane,
    height: f64,
    style: String,
    attachment: AttachmentPoint,
    line_spacing: f64,
    line_spacing_style: LineSpacingStyle,
    rotation: f64,
    width: Option<f64>,
    column_type: i16,
    column_count: i32,
    column_width: f64,
    column_gutter: f64,
    column_height: f64,
    total_width: f64,
    annotative: bool,
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
            line_spacing_style: LineSpacingStyle::AtLeast,
            rotation: 0.0,
            width: None,
            column_type: 0,
            column_count: 2,
            column_width: 0.0,
            column_gutter: 0.0,
            column_height: 0.0,
            total_width: 0.0,
            annotative: false,
        }
    }

    /// Whether the current text style is annotative (shown in the header).
    pub fn with_annotative(mut self, annotative: bool) -> Self {
        self.annotative = annotative;
        self
    }

    fn attachment_label(&self) -> &'static str {
        match self.attachment {
            AttachmentPoint::TopLeft => "TL",
            AttachmentPoint::TopCenter => "TC",
            AttachmentPoint::TopRight => "TR",
            AttachmentPoint::MiddleLeft => "ML",
            AttachmentPoint::MiddleCenter => "MC",
            AttachmentPoint::MiddleRight => "MR",
            AttachmentPoint::BottomLeft => "BL",
            AttachmentPoint::BottomCenter => "BC",
            AttachmentPoint::BottomRight => "BR",
        }
    }

    /// Column defaults scale with the text height (measured at 2.5: column
    /// width 75, total width 200, gutter 12.5, height 25).
    fn column_default(&self, step: Step) -> f64 {
        let h = self.height;
        match step {
            Step::TotalWidth if self.total_width > 0.0 => self.total_width,
            Step::TotalWidth => h * 80.0,
            Step::ColumnWidth if self.column_width > 0.0 => self.column_width,
            Step::ColumnWidth => h * 30.0,
            Step::ColumnGutter if self.column_gutter > 0.0 => self.column_gutter,
            Step::ColumnGutter => h * 5.0,
            Step::ColumnHeight if self.column_height > 0.0 => self.column_height,
            Step::ColumnHeight => h * 10.0,
            _ => 0.0,
        }
    }

    /// Static columns share the total width; both kinds go back to the box.
    fn finish_columns(&mut self) {
        if self.column_type == 1 {
            let count = self.column_count.max(1) as f64;
            let gutter = self.column_default(Step::ColumnGutter);
            let total = self.column_default(Step::TotalWidth);
            self.column_width = ((total - gutter * (count - 1.0)) / count).max(1e-6);
        }
        self.resume_point_step();
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
            "L" | "LINESPACING" | "LINE SPACING" => Step::LineSpacingType,
            "R" | "ROTATION" => Step::Rotation,
            "S" | "STYLE" => Step::Style,
            "W" | "WIDTH" => Step::Width,
            "C" | "COLUMNS" => Step::ColumnMode,
            _ => return false,
        };
        true
    }

    /// Signed text-area extents along the rotated boundary axes from the
    /// first corner to `opposite`; a typed width overrides the picked one.
    fn boundary_extents(&self, opposite: DVec3) -> (f64, f64) {
        let first = self.first.unwrap_or(opposite);
        let local = self.plane.vector_to_local(opposite - first);
        let (sin, cos) = self.rotation.sin_cos();
        let picked_horizontal = local.x * cos + local.y * sin;
        let vertical = -local.x * sin + local.y * cos;
        let horizontal = match self.width {
            Some(width) => width.copysign(if picked_horizontal.abs() > 1e-12 {
                picked_horizontal
            } else {
                1.0
            }),
            None => picked_horizontal,
        };
        (horizontal, vertical)
    }

    fn boundary_geometry(&self, opposite: DVec3) -> (DVec3, f64, f64) {
        let first = self.first.unwrap_or(opposite);
        let (sin, cos) = self.rotation.sin_cos();
        let (horizontal, vertical) = self.boundary_extents(opposite);
        let width = horizontal.abs();
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
            AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => {
                1.0
            }
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
        template.line_spacing_style = self.line_spacing_style;
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
            if self.column_height > 0.0 {
                template.rectangle_height = Some(self.column_height);
            }
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
        let num = crate::modules::annotate::text::number_text;
        let text = match self.step {
            Step::FirstCorner => format!(
                "{}\nMTEXT  {}",
                crate::tr!(
                    "mtext-cmd",
                    "header",
                    style = self.style.clone(),
                    height = num(self.height),
                    annotative = if self.annotative { t!("Yes") } else { t!("No") }.into_owned()
                ),
                crate::tr!("mtext-cmd", "first-corner")
            ),
            Step::OppositeCorner => crate::tr!("mtext-cmd", "opposite-corner"),
            Step::Height => crate::tr!("mtext-cmd", "height", default = num(self.height)),
            Step::Justify => {
                crate::tr!("mtext-cmd", "justify", default = self.attachment_label())
            }
            Step::LineSpacingType => crate::tr!(
                "mtext-cmd",
                "spacing-type",
                default = match self.line_spacing_style {
                    LineSpacingStyle::Exactly => "Exactly",
                    _ => "At least",
                }
            ),
            Step::LineSpacing => crate::tr!(
                "mtext-cmd",
                "spacing",
                default = format!("{}x", num(self.line_spacing))
            ),
            Step::Rotation => crate::tr!(
                "mtext-cmd",
                "rotation",
                default = num(self.rotation.to_degrees())
            ),
            Step::Style => crate::tr!("mtext-cmd", "style", default = self.style.clone()),
            Step::Width => crate::tr!("mtext-cmd", "width"),
            Step::ColumnMode => crate::tr!("mtext-cmd", "column-type"),
            Step::TotalWidth => crate::tr!(
                "mtext-cmd",
                "total-width",
                default = num(self.column_default(Step::TotalWidth))
            ),
            Step::ColumnCount => {
                crate::tr!("mtext-cmd", "column-count", default = self.column_count.to_string())
            }
            Step::ColumnWidth => crate::tr!(
                "mtext-cmd",
                "column-width",
                default = num(self.column_default(Step::ColumnWidth))
            ),
            Step::ColumnGutter => crate::tr!(
                "mtext-cmd",
                "gutter",
                default = num(self.column_default(Step::ColumnGutter))
            ),
            Step::ColumnHeight => crate::tr!(
                "mtext-cmd",
                "column-height",
                default = num(self.column_default(Step::ColumnHeight))
            ),
        };
        if text.starts_with("MTEXT  ") {
            text
        } else {
            format!("MTEXT  {text}")
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
                CmdOption::new("Dynamic", "D"),
                CmdOption::new("Static", "S"),
                CmdOption::new("No columns", "N"),
            ],
            Step::LineSpacingType => vec![
                CmdOption::new("At least", "A"),
                CmdOption::new("Exactly", "E"),
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
        } else if matches!(self.step, Step::FirstCorner | Step::OppositeCorner) {
            InputKind::Point
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
            | Step::TotalWidth
            | Step::ColumnCount
            | Step::ColumnWidth
            | Step::ColumnGutter
            | Step::ColumnHeight => DynField::Scalar,
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
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
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
                    _ => return Some(CmdResult::ReportError(crate::tr!("text-cmd", "invalid-keyword"))),
                };
                self.resume_point_step();
            }
            Step::LineSpacingType => {
                let word = upper.trim_start_matches(['_', '.']);
                self.line_spacing_style = if "AT LEAST".starts_with(word) {
                    LineSpacingStyle::AtLeast
                } else if "EXACTLY".starts_with(word) {
                    LineSpacingStyle::Exactly
                } else {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "invalid-keyword")));
                };
                self.step = Step::LineSpacing;
            }
            Step::LineSpacing => {
                // `1.5x` is a factor of the single spacing; a bare value a
                // distance between baselines (5/3 of the height is single).
                let lower = trimmed.to_ascii_lowercase();
                let value = match lower.strip_suffix('x') {
                    Some(factor) => factor.trim().replace(',', ".").parse::<f64>().ok()?,
                    None => {
                        crate::entities::common::parse_typed_length(trimmed)?
                            / (self.height * 5.0 / 3.0)
                    }
                };
                if !(0.25..=4.0).contains(&value) {
                    return Some(CmdResult::ReportError(crate::tr!("mtext-cmd", "spacing-range")));
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
            // The width ends the box: the editor opens at once, the text
            // area as high as its text.
            Step::Width => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value < 0.0 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.width = Some(value);
                let first = self.first?;
                return Some(self.open_editor(first));
            }
            Step::ColumnMode => {
                let word = upper.trim_start_matches(['_', '.']);
                self.column_type = if "DYNAMIC".starts_with(word) {
                    2
                } else if "STATIC".starts_with(word) {
                    1
                } else if "NO COLUMNS".starts_with(word) {
                    0
                } else {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "invalid-keyword")));
                };
                self.step = match self.column_type {
                    0 => {
                        self.resume_point_step();
                        self.step
                    }
                    1 => Step::TotalWidth,
                    _ => Step::ColumnWidth,
                };
            }
            Step::TotalWidth => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value <= 0.0 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.total_width = value;
                self.step = Step::ColumnCount;
            }
            Step::ColumnCount => {
                let value = trimmed.parse::<i32>().ok()?;
                if !(1..=crate::entities::text_support::MAX_MTEXT_COLUMNS).contains(&value) {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.column_count = value;
                self.step = Step::ColumnGutter;
            }
            Step::ColumnWidth => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value <= 0.0 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.column_width = value;
                self.step = Step::ColumnGutter;
            }
            Step::ColumnGutter => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value < 0.0 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.column_gutter = value;
                self.step = Step::ColumnHeight;
            }
            Step::ColumnHeight => {
                let value = crate::entities::common::parse_typed_length(trimmed)?;
                if value <= 0.0 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
                }
                self.column_height = value;
                self.finish_columns();
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
            Step::LineSpacingType => {
                self.step = Step::LineSpacing;
                CmdResult::NeedPoint
            }
            // Enter takes the offered default and goes on.
            Step::ColumnMode => {
                self.column_type = 2;
                self.step = Step::ColumnWidth;
                CmdResult::NeedPoint
            }
            Step::TotalWidth => {
                self.total_width = self.column_default(Step::TotalWidth);
                self.step = Step::ColumnCount;
                CmdResult::NeedPoint
            }
            Step::ColumnCount => {
                self.step = Step::ColumnGutter;
                CmdResult::NeedPoint
            }
            Step::ColumnWidth => {
                self.column_width = self.column_default(Step::ColumnWidth);
                self.step = Step::ColumnGutter;
                CmdResult::NeedPoint
            }
            Step::ColumnGutter => {
                self.column_gutter = self.column_default(Step::ColumnGutter);
                self.step = Step::ColumnHeight;
                CmdResult::NeedPoint
            }
            Step::ColumnHeight => {
                self.column_height = self.column_default(Step::ColumnHeight);
                self.finish_columns();
                CmdResult::NeedPoint
            }
            Step::Height
            | Step::Justify
            | Step::LineSpacing
            | Step::Rotation
            | Step::Style
            | Step::Width => {
                self.resume_point_step();
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if self.step != Step::OppositeCorner {
            return None;
        }
        let first = self.first?;
        let (h, v) = self.boundary_extents(point);
        let (sin, cos) = self.rotation.sin_cos();
        let points = [(0.0, 0.0), (h, 0.0), (h, v), (0.0, v), (0.0, 0.0)]
            .into_iter()
            .map(|(x, y)| {
                let offset = DVec3::new(x * cos - y * sin, x * sin + y * cos, 0.0);
                let world = first + self.plane.vector_to_world(offset);
                [world.x, world.y, world.z]
            })
            .collect();
        Some(WireModel::solid_f64(
            "rubber_band".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["MTEXT"] });
