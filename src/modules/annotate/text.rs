use codec::entities::{Text, TextHorizontalAlignment as HA, TextVerticalAlignment as VA};
use codec::tables::TextStyle;
use codec::types::Vector3;
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

/// What the next TEXT starts from: the rotation last typed and the last
/// line written, which Enter at the start point continues below.
struct Session {
    rotation: f64,
    last: Option<Text>,
}

static SESSION: std::sync::Mutex<Session> = std::sync::Mutex::new(Session {
    rotation: 0.0,
    last: None,
});

fn session() -> std::sync::MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// A value the way the prompts show a default (no trailing zeros).
pub fn number_text(value: f64) -> String {
    angle_text(value)
}

/// An angle in degrees the way the prompts show it (no trailing zeros).
fn angle_text(degrees: f64) -> String {
    let text = format!("{degrees:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// A height the way TEXT shows it (four decimals).
fn length4(value: f64) -> String {
    format!("{value:.4}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Justification,
    Style,
    /// Style's `?`: which styles to list.
    StyleList,
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
    annotation_multiplier: f64,
    last_display_height: Option<f64>,
    last_entity: Option<Text>,
    /// A justification was chosen: the point prompt names its point.
    justified: bool,
}

impl TextCommand {
    pub fn with_defaults(
        defaults: TextCreationDefaults,
        styles: Vec<TextStyle>,
        annotation_multiplier: f64,
    ) -> Self {
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
            rotation: session().rotation,
            width_factor: defaults.width_factor,
            oblique_angle: defaults.oblique_angle,
            fixed_height: false,
            annotative: false,
            annotation_multiplier: annotation_multiplier.max(1.0e-9),
            last_display_height: None,
            last_entity: None,
            justified: false,
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
        self.oblique_angle = style
            .oblique_angle
            .clamp(-85.0_f64.to_radians(), 85.0_f64.to_radians());
        self.annotative = style.annotative;
        true
    }

    /// TEXT's `?` at the style name: every style with its font and settings.
    fn style_listing(&self, pattern: &str) -> String {
        let mut out = vec![crate::tr!("text-cmd", "styles-title")];
        let listed: Vec<&TextStyle> = self
            .styles
            .iter()
            .filter(|style| {
                pattern
                    .split(',')
                    .any(|part| crate::io::xref_model::wildcard_match(&style.name, part.trim()))
            })
            .collect();
        let width = listed.iter().map(|s| s.name.chars().count() + 2).max().unwrap_or(0);
        for style in listed {
            let name = format!("\"{}\"", style.name);
            let font = if style.true_type_font.trim().is_empty() {
                let file = style.font_file.trim();
                let stem = file
                    .strip_suffix(".shx")
                    .or_else(|| file.strip_suffix(".SHX"))
                    .unwrap_or(file);
                format!("Font files: {stem}")
            } else {
                format!("Font typeface: {}", style.true_type_font.trim())
            };
            out.push(format!("Style name: {name:<width$}  {font}"));
            out.push(format!(
                "   Height: {:.4}  Width factor: {:.4}  Obliquing angle: {}",
                style.height,
                style.width_factor,
                angle_text(style.oblique_angle.to_degrees())
            ));
            out.push("   Generation: Normal".to_string());
        }
        out.push(format!("Current text style: \"{}\"", self.style_name));
        out.join("\n")
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

    fn justify_label(&self) -> &'static str {
        match (self.horizontal, self.vertical) {
            (HA::Aligned, _) => "Align",
            (HA::Fit, _) => "Fit",
            (HA::Middle, _) => "Middle",
            (HA::Left, VA::Baseline) => "Left",
            (HA::Center, VA::Baseline) => "Center",
            (HA::Right, VA::Baseline) => "Right",
            (HA::Left, VA::Top) => "TL",
            (HA::Center, VA::Top) => "TC",
            (HA::Right, VA::Top) => "TR",
            (HA::Left, VA::Middle) => "ML",
            (HA::Center, VA::Middle) => "MC",
            (HA::Right, VA::Middle) => "MR",
            (HA::Left, VA::Bottom) => "BL",
            (HA::Center, VA::Bottom) => "BC",
            (HA::Right, VA::Bottom) => "BR",
        }
    }

    /// The point a chosen justification asks for.
    fn point_prompt(&self) -> String {
        match (self.horizontal, self.vertical) {
            (HA::Aligned | HA::Fit, _) => crate::tr!("text-cmd", "first-endpoint"),
            (HA::Middle, _) | (HA::Center, VA::Middle) => crate::tr!("text-cmd", "middle-point"),
            (HA::Left, VA::Baseline) => crate::tr!("text-cmd", "start-point"),
            (HA::Center, VA::Baseline) => crate::tr!("text-cmd", "center-point"),
            (HA::Right, VA::Baseline) => crate::tr!("text-cmd", "right-endpoint"),
            (HA::Left, VA::Top) => crate::tr!("text-cmd", "top-left"),
            (HA::Center, VA::Top) => crate::tr!("text-cmd", "top-center"),
            (HA::Right, VA::Top) => crate::tr!("text-cmd", "top-right"),
            (HA::Left, VA::Middle) => crate::tr!("text-cmd", "middle-left"),
            (HA::Right, VA::Middle) => crate::tr!("text-cmd", "middle-right"),
            (HA::Left, VA::Bottom) => crate::tr!("text-cmd", "bottom-left"),
            (HA::Center, VA::Bottom) => crate::tr!("text-cmd", "bottom-center"),
            (HA::Right, VA::Bottom) => crate::tr!("text-cmd", "bottom-right"),
        }
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
        self.last_display_height = None;
        self.last_entity = Some(entity.clone());
        CmdResult::SuspendForTextInput { pos, entity }
    }

    fn open_next_line(&mut self) -> CmdResult {
        let Some(mut entity) = self.last_entity.take() else {
            return CmdResult::Cancel;
        };
        let angle = if matches!(entity.horizontal_alignment, HA::Aligned | HA::Fit) {
            entity.alignment_point.map_or(entity.rotation, |point| {
                (point.y - entity.insertion_point.y).atan2(point.x - entity.insertion_point.x)
            })
        } else {
            entity.rotation
        };
        let fallback_height = entity.height.max(1.0e-9)
            * if self.annotative {
                self.annotation_multiplier
            } else {
                1.0
            };
        let spacing = self
            .last_display_height
            .take()
            .unwrap_or(fallback_height)
            .max(1.0e-9)
            * 1.666_666_666_7;
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
        let text = match self.step {
            Step::Start if self.justified => self.point_prompt(),
            Step::Start => {
                let header = crate::tr!(
                    "text-cmd",
                    "header",
                    style = self.style_name.clone(),
                    height = length4(self.height),
                    annotative = if self.annotative { t!("Yes") } else { t!("No") }.into_owned(),
                    justify = self.justify_label()
                );
                format!("TEXT  {header}\nTEXT  {}", crate::tr!("text-cmd", "start"))
            }
            Step::Justification => crate::tr!("text-cmd", "justify"),
            Step::Style => crate::tr!("text-cmd", "style", default = self.style_name.clone()),
            Step::StyleList => crate::tr!("text-cmd", "style-list"),
            Step::Height if self.annotative => {
                crate::tf!("Specify paper text height <{}>:", length4(self.height)).into_owned()
            }
            Step::Height => crate::tr!("text-cmd", "height", default = length4(self.height)),
            Step::Rotation => crate::tr!(
                "text-cmd",
                "rotation",
                default = angle_text(self.rotation.to_degrees())
            ),
            Step::SecondPoint => crate::tr!("text-cmd", "second-endpoint"),
        };
        if text.starts_with("TEXT  ") {
            text
        } else {
            format!("TEXT  {text}")
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Start if self.justified => Vec::new(),
            Step::Start => vec![
                CmdOption::new(t!("Justify").as_ref(), "J"),
                CmdOption::new(t!("Style").as_ref(), "S"),
            ],
            Step::Justification => [
                ("Left", "L"),
                ("Center", "C"),
                ("Right", "R"),
                ("Align", "A"),
                ("Middle", "M"),
                ("Fit", "F"),
                ("TL", "TL"),
                ("TC", "TC"),
                ("TR", "TR"),
                ("ML", "ML"),
                ("MC", "MC"),
                ("MR", "MR"),
                ("BL", "BL"),
                ("BC", "BC"),
                ("BR", "BR"),
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
            Step::Justification | Step::Style | Step::StyleList | Step::Height | Step::Rotation
        )
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim();
        let upper = token.to_ascii_uppercase();
        let upper = upper.trim_start_matches(['_', '.']).to_string();
        match self.step {
            Step::Start if self.justified => return None,
            Step::Start => match upper.as_str() {
                "J" | "JUSTIFY" | "JUSTIFICATION" => self.step = Step::Justification,
                "S" | "ST" | "STYLE" => self.step = Step::Style,
                _ => return None,
            },
            Step::Justification => {
                if !self.set_justification(&upper) {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "invalid-keyword")));
                }
                self.justified = true;
                self.step = Step::Start;
            }
            Step::Style => {
                if upper == "?" {
                    self.step = Step::StyleList;
                    return Some(CmdResult::NeedPoint);
                }
                if !self.select_style(token) {
                    return Some(CmdResult::ReportError(crate::tr!(
                        "text-cmd",
                        "no-style",
                        name = token.to_string()
                    )));
                }
                self.step = Step::Start;
            }
            // The listing ends the Style option.
            Step::StyleList => {
                let pattern = if token.is_empty() { "*" } else { token };
                self.step = Step::Start;
                return Some(CmdResult::ReportMeasurement(self.style_listing(pattern)));
            }
            Step::Height => {
                let Ok(value) = token.replace(',', ".").parse::<f64>() else {
                    return Some(CmdResult::ReportError(
                        crate::t!("Requires numeric distance or second point.").into_owned(),
                    ));
                };
                if !value.is_finite() || value <= 1.0e-9 {
                    return Some(CmdResult::ReportError(crate::tr!("text-cmd", "positive")));
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
                session().rotation = self.rotation;
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
                session().rotation = angle;
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
            Step::Justification | Step::Style | Step::StyleList => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Enter continues one line below the last text written.
            Step::Start if !self.justified => match session().last.clone() {
                Some(last) => {
                    self.last_entity = Some(last);
                    self.open_next_line()
                }
                None => CmdResult::Cancel,
            },
            Step::Start => CmdResult::NeedPoint,
            Step::StyleList => {
                self.step = Step::Start;
                CmdResult::ReportMeasurement(self.style_listing("*"))
            }
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
            session().last = self.last_entity.clone();
            self.open_next_line()
        } else {
            CmdResult::Cancel
        }
    }

    fn on_editor_display_height(&mut self, height: f64) {
        if height.is_finite() && height > 1.0e-9 {
            self.last_display_height = Some(height);
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
