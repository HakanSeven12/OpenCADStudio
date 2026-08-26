use acadrust::entities::{
    Text, TextHorizontalAlignment as HA, TextVerticalAlignment as VA,
};
use acadrust::tables::TextStyle;
use acadrust::types::Vector3;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::creation_style::TextCreationDefaults;
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/text.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TEXT",
        label: "Text",
        icon: ICON,
        event: ModuleEvent::Command("TEXT".to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Justification,
    Style,
    Height,
    Rotation,
    SecondPoint,
}

pub struct TextCommand {
    step: Step,
    plane: WorkingPlane,
    first_point: Option<DVec3>,
    second_point: Option<DVec3>,
    horizontal: HA,
    vertical: VA,
    style_name: String,
    styles: Vec<TextStyle>,
    height: f64,
    rotation: f64,
    width_factor: f64,
    oblique_angle: f64,
    fixed_height: bool,
    annotative: bool,
    generation_flags: i16,
    last_entity: Option<Text>,
}

impl TextCommand {
    pub fn with_defaults(defaults: TextCreationDefaults, styles: Vec<TextStyle>) -> Self {
        let current_height = defaults.height;
        let mut command = Self {
            step: Step::Start,
            plane: WorkingPlane::default(),
            first_point: None,
            second_point: None,
            horizontal: HA::Left,
            vertical: VA::Baseline,
            style_name: defaults.style_name,
            styles,
            height: defaults.height,
            rotation: 0.0,
            width_factor: defaults.width_factor,
            oblique_angle: defaults.oblique_angle,
            fixed_height: false,
            annotative: false,
            generation_flags: 0,
            last_entity: None,
        };
        let style = command.style_name.clone();
        command.select_style(&style);
        if !command.fixed_height {
            command.height = current_height;
        }
        command
    }

    fn select_style(&mut self, name: &str) -> bool {
        let Some(style) = self
            .styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return false;
        };
        self.style_name = style.name;
        self.fixed_height = style.height > 1.0e-9;
        if self.fixed_height {
            self.height = style.height;
        } else if style.last_height > 1.0e-9 {
            self.height = style.last_height;
        }
        self.width_factor = style.width_factor.max(0.01);
        self.oblique_angle = style.oblique_angle.clamp(
            -85.0_f64.to_radians(),
            85.0_f64.to_radians(),
        );
        self.annotative = style.annotative;
        self.generation_flags = (if style.flags.backward { 2 } else { 0 })
            | (if style.flags.upside_down { 4 } else { 0 });
        true
    }

    fn set_justification(&mut self, value: &str) -> bool {
        let normalized = value.trim().to_ascii_uppercase().replace([' ', '-'], "");
        let alignment = match normalized.as_str() {
            "L" | "LEFT" => (HA::Left, VA::Baseline),
            "C" | "CENTER" => (HA::Center, VA::Baseline),
            "R" | "RIGHT" => (HA::Right, VA::Baseline),
            "A" | "ALIGNED" | "ALIGN" => (HA::Aligned, VA::Baseline),
            "M" | "MIDDLE" => (HA::Middle, VA::Baseline),
            "F" | "FIT" => (HA::Fit, VA::Baseline),
            "TL" | "TOPLEFT" => (HA::Left, VA::Top),
            "TC" | "TOPCENTER" => (HA::Center, VA::Top),
            "TR" | "TOPRIGHT" => (HA::Right, VA::Top),
            "ML" | "MIDDLELEFT" => (HA::Left, VA::Middle),
            "MC" | "MIDDLECENTER" => (HA::Center, VA::Middle),
            "MR" | "MIDDLERIGHT" => (HA::Right, VA::Middle),
            "BL" | "BOTTOMLEFT" => (HA::Left, VA::Bottom),
            "BC" | "BOTTOMCENTER" => (HA::Center, VA::Bottom),
            "BR" | "BOTTOMRIGHT" => (HA::Right, VA::Bottom),
            _ => return false,
        };
        self.horizontal = alignment.0;
        self.vertical = alignment.1;
        true
    }

    fn is_two_point(&self) -> bool {
        matches!(self.horizontal, HA::Aligned | HA::Fit)
    }

    fn after_first_point(&mut self) -> CmdResult {
        if self.is_two_point() {
            self.step = Step::SecondPoint;
        } else if self.fixed_height {
            self.step = Step::Rotation;
        } else {
            self.step = Step::Height;
        }
        CmdResult::NeedPoint
    }

    fn after_second_point(&mut self) -> CmdResult {
        if matches!(self.horizontal, HA::Fit) && !self.fixed_height {
            self.step = Step::Height;
            CmdResult::NeedPoint
        } else {
            self.open_editor()
        }
    }

    fn make_entity(&self) -> Option<Text> {
        let first = self.plane.to_local(self.first_point?);
        let mut text = Text::with_value("", Vector3::new(first.x, first.y, first.z))
            .with_height(self.height.max(1.0e-9));
        text.style = self.style_name.clone();
        text.width_factor = self.width_factor.max(0.01);
        text.oblique_angle = self.oblique_angle;
        text.rotation = self.rotation;
        text.horizontal_alignment = self.horizontal;
        text.vertical_alignment = self.vertical;
        text.generation_flags = self.generation_flags;
        text.alignment_point = if self.is_two_point() {
            let second = self.plane.to_local(self.second_point?);
            Some(Vector3::new(second.x, second.y, second.z))
        } else if matches!((self.horizontal, self.vertical), (HA::Left, VA::Baseline)) {
            None
        } else {
            Some(Vector3::new(first.x, first.y, first.z))
        };
        Some(text)
    }

    fn open_editor(&mut self) -> CmdResult {
        let Some(entity) = self.make_entity() else {
            return CmdResult::NeedPoint;
        };
        let pos = self.first_point.unwrap_or(DVec3::ZERO);
        self.last_entity = Some(entity.clone());
        CmdResult::SuspendForTextInput { pos, entity }
    }

    fn open_next_line(&mut self) -> CmdResult {
        let Some(mut entity) = self.last_entity.take() else {
            return CmdResult::Cancel;
        };
        let angle = if matches!(entity.horizontal_alignment, HA::Aligned | HA::Fit) {
            entity.alignment_point.map_or(entity.rotation, |point| {
                (point.y - entity.insertion_point.y)
                    .atan2(point.x - entity.insertion_point.x)
            })
        } else {
            entity.rotation
        };
        let spacing = entity.height.max(1.0e-9) * 1.666_666_666_7;
        let delta = Vector3::new(angle.sin() * spacing, -angle.cos() * spacing, 0.0);
        entity.insertion_point = entity.insertion_point + delta;
        if let Some(point) = entity.alignment_point.as_mut() {
            *point = *point + delta;
        }
        entity.value.clear();
        let local = DVec3::new(
            entity.insertion_point.x,
            entity.insertion_point.y,
            entity.insertion_point.z,
        );
        let pos = self.plane.to_world(local);
        self.last_entity = Some(entity.clone());
        CmdResult::SuspendForTextInput { pos, entity }
    }
}

impl CadCommand for TextCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TEXT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Start => format!(
                "{}\n{}",
                crate::tf!(
                    "TEXT  Current style: {}, Height: {}, Annotative: {}",
                    self.style_name,
                    self.height,
                    if self.annotative { "Yes" } else { "No" }
                ),
                t!("TEXT  Specify start point or [Justify/Style]:")
            ),
            Step::Justification => t!(
                "TEXT  Enter justification [Left/Center/Right/Aligned/Middle/Fit/TL/TC/TR/ML/MC/MR/BL/BC/BR]:"
            )
            .into_owned(),
            Step::Style => crate::tf!("TEXT  Enter style name <{}>:", self.style_name).into_owned(),
            Step::Height if self.annotative => {
                crate::tf!("TEXT  Specify paper text height <{}>:", self.height).into_owned()
            }
            Step::Height => crate::tf!("TEXT  Specify height <{}>:", self.height).into_owned(),
            Step::Rotation => crate::tf!(
                "TEXT  Specify rotation angle <{}>:",
                self.rotation.to_degrees()
            )
            .into_owned(),
            Step::SecondPoint => t!("TEXT  Specify second endpoint:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Start => vec![
                CmdOption::new(t!("Justify").as_ref(), "J"),
                CmdOption::new(t!("Style").as_ref(), "ST"),
            ],
            Step::Justification => [
                ("Left", "L"), ("Center", "C"), ("Right", "R"),
                ("Aligned", "A"), ("Middle", "M"), ("Fit", "F"),
                ("TL", "TL"), ("TC", "TC"), ("TR", "TR"),
                ("ML", "ML"), ("MC", "MC"), ("MR", "MR"),
                ("BL", "BL"), ("BC", "BC"), ("BR", "BR"),
            ]
            .into_iter()
            .map(|(label, keyword)| CmdOption::new(label, keyword))
            .collect(),
            Step::Style => self
                .styles
                .iter()
                .map(|style| CmdOption::new(&style.name, &style.name))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            Step::Justification | Step::Style | Step::Height | Step::Rotation
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim();
        let upper = token.to_ascii_uppercase();
        match self.step {
            Step::Start => match upper.as_str() {
                "J" | "JUSTIFY" | "JUSTIFICATION" => self.step = Step::Justification,
                "S" | "ST" | "STYLE" => self.step = Step::Style,
                _ => return None,
            },
            Step::Justification => {
                if !self.set_justification(token) {
                    return None;
                }
                self.step = Step::Start;
            }
            Step::Style => {
                if !self.select_style(token) {
                    return None;
                }
                self.step = Step::Start;
            }
            Step::Height => {
                let value = token.replace(',', ".").parse::<f64>().ok()?;
                if !value.is_finite() || value <= 1.0e-9 {
                    return None;
                }
                self.height = value;
                if self.is_two_point() {
                    return Some(self.open_editor());
                }
                self.step = Step::Rotation;
            }
            Step::Rotation => {
                let value = token.replace(',', ".").parse::<f64>().ok()?;
                if !value.is_finite() {
                    return None;
                }
                self.rotation = value.to_radians();
                return Some(self.open_editor());
            }
            Step::SecondPoint => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Start => {
                self.first_point = Some(point);
                self.second_point = None;
                self.after_first_point()
            }
            Step::Height => {
                let Some(first) = self.first_point else {
                    return CmdResult::NeedPoint;
                };
                let value = self
                    .plane
                    .vector_to_local(point - first)
                    .truncate()
                    .length();
                if value <= 1.0e-9 {
                    return CmdResult::NeedPoint;
                }
                self.height = value;
                if self.is_two_point() {
                    self.open_editor()
                } else {
                    self.step = Step::Rotation;
                    CmdResult::NeedPoint
                }
            }
            Step::Rotation => {
                let Some(first) = self.first_point else {
                    return CmdResult::NeedPoint;
                };
                let Some(angle) = self.plane.angle(first, point) else {
                    return CmdResult::NeedPoint;
                };
                self.rotation = angle;
                self.open_editor()
            }
            Step::SecondPoint => {
                let Some(first) = self.first_point else {
                    return CmdResult::NeedPoint;
                };
                if self
                    .plane
                    .vector_to_local(point - first)
                    .truncate()
                    .length()
                    <= 1.0e-9
                {
                    return CmdResult::NeedPoint;
                }
                self.second_point = Some(point);
                self.after_second_point()
            }
            Step::Justification | Step::Style => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Start => CmdResult::Cancel,
            Step::Justification | Step::Style => {
                self.step = Step::Start;
                CmdResult::NeedPoint
            }
            Step::Height => {
                if self.is_two_point() {
                    self.open_editor()
                } else {
                    self.step = Step::Rotation;
                    CmdResult::NeedPoint
                }
            }
            Step::Rotation => self.open_editor(),
            Step::SecondPoint => CmdResult::NeedPoint,
        }
    }

    fn on_editor_closed(&mut self, committed: bool) -> CmdResult {
        if committed {
            self.open_next_line()
        } else {
            CmdResult::Cancel
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, _point: DVec3) -> Option<WireModel> {
        None
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["TEXT"] });
