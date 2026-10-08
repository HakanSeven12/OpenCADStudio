// ATTDEF / -ATTDEF — attribute definitions.
//
// The modes (Invisible, Constant, Verify, Preset, Lock position, Multiple
// lines) persist for the session as AFLAGS; Annotative persists beside them.
// `-ATTDEF` asks everything on the command line; ATTDEF fills the same
// `AttdefSpec` from its dialog and only asks for the placement.

use std::sync::Mutex;

use codec::entities::attribute_definition::{
    HorizontalAlignment as HA, MTextFlag, VerticalAlignment as VA,
};
use codec::entities::{AttributeDefinition, MText};
use codec::tables::TextStyle;
use codec::types::Vector3;
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

pub const AFLAG_INVISIBLE: u8 = 1;
pub const AFLAG_CONSTANT: u8 = 2;
pub const AFLAG_VERIFY: u8 = 4;
pub const AFLAG_PRESET: u8 = 8;
pub const AFLAG_LOCK: u8 = 16;
pub const AFLAG_MULTILINE: u8 = 32;

/// Session state shared by ATTDEF and -ATTDEF: the AFLAGS modes, the
/// Annotative switch and the last definition placed (for "align below").
pub struct AttdefSession {
    pub aflags: u8,
    pub annotative: bool,
    pub last: Option<AttributeDefinition>,
    /// The dialog's "Specify on-screen" choice, kept for the session.
    pub on_screen: bool,
    /// Text the multi-line editor handed back to the dialog's Default field.
    pub editor_value: Option<String>,
}

pub static SESSION: Mutex<AttdefSession> = Mutex::new(AttdefSession {
    aflags: AFLAG_LOCK,
    annotative: false,
    last: None,
    on_screen: true,
    editor_value: None,
});

pub fn session() -> std::sync::MutexGuard<'static, AttdefSession> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// The fifteen justifications, in the order the dialog lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Left,
    Align,
    Fit,
    Center,
    Middle,
    Right,
    TL,
    TC,
    TR,
    ML,
    MC,
    MR,
    BL,
    BC,
    BR,
}

impl Justify {
    pub const ALL: [Justify; 15] = [
        Justify::Left,
        Justify::Align,
        Justify::Fit,
        Justify::Center,
        Justify::Middle,
        Justify::Right,
        Justify::TL,
        Justify::TC,
        Justify::TR,
        Justify::ML,
        Justify::MC,
        Justify::MR,
        Justify::BL,
        Justify::BC,
        Justify::BR,
    ];

    /// The name the dialog and the status line use.
    pub fn label(self) -> &'static str {
        match self {
            Justify::Left => "Left",
            Justify::Align => "Align",
            Justify::Fit => "Fit",
            Justify::Center => "Center",
            Justify::Middle => "Middle",
            Justify::Right => "Right",
            Justify::TL => "Top left",
            Justify::TC => "Top center",
            Justify::TR => "Top right",
            Justify::ML => "Middle left",
            Justify::MC => "Middle center",
            Justify::MR => "Middle right",
            Justify::BL => "Bottom left",
            Justify::BC => "Bottom center",
            Justify::BR => "Bottom right",
        }
    }

    pub fn from_keyword(word: &str) -> Option<Justify> {
        let w = word.trim().to_ascii_uppercase();
        let w = w.trim_start_matches('_');
        Some(match w {
            "L" | "LEFT" => Justify::Left,
            "A" | "ALIGN" => Justify::Align,
            "F" | "FIT" => Justify::Fit,
            "C" | "CENTER" => Justify::Center,
            "M" | "MIDDLE" => Justify::Middle,
            "R" | "RIGHT" => Justify::Right,
            "TL" => Justify::TL,
            "TC" => Justify::TC,
            "TR" => Justify::TR,
            "ML" => Justify::ML,
            "MC" => Justify::MC,
            "MR" => Justify::MR,
            "BL" => Justify::BL,
            "BC" => Justify::BC,
            "BR" => Justify::BR,
            _ => return None,
        })
    }

    pub fn from_alignment(h: HA, v: VA) -> Justify {
        match (h, v) {
            (HA::Aligned, _) => Justify::Align,
            (HA::Fit, _) => Justify::Fit,
            (HA::Middle, _) => Justify::Middle,
            (HA::Left, VA::Top) => Justify::TL,
            (HA::Center, VA::Top) => Justify::TC,
            (HA::Right, VA::Top) => Justify::TR,
            (HA::Left, VA::Middle) => Justify::ML,
            (HA::Center, VA::Middle) => Justify::MC,
            (HA::Right, VA::Middle) => Justify::MR,
            (HA::Left, VA::Bottom) => Justify::BL,
            (HA::Center, VA::Bottom) => Justify::BC,
            (HA::Right, VA::Bottom) => Justify::BR,
            (HA::Center, VA::Baseline) => Justify::Center,
            (HA::Right, VA::Baseline) => Justify::Right,
            (HA::Left, VA::Baseline) => Justify::Left,
        }
    }

    pub fn alignment(self) -> (HA, VA) {
        match self {
            Justify::Left => (HA::Left, VA::Baseline),
            Justify::Align => (HA::Aligned, VA::Baseline),
            Justify::Fit => (HA::Fit, VA::Baseline),
            Justify::Center => (HA::Center, VA::Baseline),
            Justify::Middle => (HA::Middle, VA::Baseline),
            Justify::Right => (HA::Right, VA::Baseline),
            Justify::TL => (HA::Left, VA::Top),
            Justify::TC => (HA::Center, VA::Top),
            Justify::TR => (HA::Right, VA::Top),
            Justify::ML => (HA::Left, VA::Middle),
            Justify::MC => (HA::Center, VA::Middle),
            Justify::MR => (HA::Right, VA::Middle),
            Justify::BL => (HA::Left, VA::Bottom),
            Justify::BC => (HA::Center, VA::Bottom),
            Justify::BR => (HA::Right, VA::Bottom),
        }
    }

    /// Align and Fit are placed by two baseline endpoints.
    pub fn two_point(self) -> bool {
        matches!(self, Justify::Align | Justify::Fit)
    }

    /// The point prompt after the justification is chosen.
    fn point_prompt(self) -> &'static str {
        match self {
            Justify::Left => "Specify start point of text:",
            Justify::Align | Justify::Fit => "Specify first endpoint of text baseline:",
            Justify::Center => "Specify center point of text:",
            Justify::Middle | Justify::MC => "Specify middle point of text:",
            Justify::Right => "Specify right endpoint of text baseline:",
            Justify::TL => "Specify top-left point of text:",
            Justify::TC => "Specify top-center point of text:",
            Justify::TR => "Specify top-right point of text:",
            Justify::ML => "Specify middle-left point of text:",
            Justify::MR => "Specify middle-right point of text:",
            Justify::BL => "Specify bottom-left point of text:",
            Justify::BC => "Specify bottom-center point of text:",
            Justify::BR => "Specify bottom-right point of text:",
        }
    }
}

/// Everything an attribute definition is made from, before it is placed.
#[derive(Debug, Clone)]
pub struct AttdefSpec {
    pub aflags: u8,
    pub annotative: bool,
    pub tag: String,
    pub prompt: String,
    /// Default (or constant) value; multi-line values joined with `\P`.
    pub value: String,
    pub justify: Justify,
    pub style: String,
    pub height: f64,
    /// Radians.
    pub rotation: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
    /// Multi-line boundary width (0 = none).
    pub boundary_width: f64,
    pub line_spacing: f64,
    /// A field the default value comes from: its code and referenced objects.
    pub field: Option<(String, Vec<codec::Handle>)>,
}

impl AttdefSpec {
    pub fn multiline(&self) -> bool {
        self.aflags & AFLAG_MULTILINE != 0
    }

    pub fn constant(&self) -> bool {
        self.aflags & AFLAG_CONSTANT != 0
    }
}

/// Tags are stored upper case.
pub fn normalize_tag(tag: &str) -> String {
    tag.trim().to_uppercase()
}

/// Build the entity. `first` is the start / justification point (world),
/// `second` the second baseline endpoint for Align / Fit.
pub fn build(
    spec: &AttdefSpec,
    plane: WorkingPlane,
    first: DVec3,
    second: Option<DVec3>,
) -> EntityType {
    let p = plane.to_local(first);
    let point = Vector3::new(p.x, p.y, p.z);
    let (h, v) = spec.justify.alignment();
    let mut attdef = AttributeDefinition {
        tag: normalize_tag(&spec.tag),
        prompt: if spec.constant() { String::new() } else { spec.prompt.clone() },
        default_value: spec.value.clone(),
        insertion_point: point,
        height: spec.height,
        rotation: spec.rotation,
        width_factor: spec.width_factor,
        oblique_angle: spec.oblique_angle,
        text_style: spec.style.clone(),
        horizontal_alignment: h,
        vertical_alignment: v,
        lock_position: spec.aflags & AFLAG_LOCK != 0,
        ..Default::default()
    };
    attdef.flags.invisible = spec.aflags & AFLAG_INVISIBLE != 0;
    attdef.flags.constant = spec.constant();
    attdef.flags.verify = spec.aflags & AFLAG_VERIFY != 0;
    attdef.flags.preset = spec.aflags & AFLAG_PRESET != 0;
    attdef.flags.annotative = spec.annotative;
    if spec.multiline() {
        // The reference writes MText flag 4 for every multi-line definition.
        attdef.mtext_flag = MTextFlag::ConstantMultiLine;
        attdef.is_multiline = true;
        attdef.line_count = spec.value.split("\\P").count().max(1) as i16;
        attdef.horizontal_alignment = HA::Left;
        attdef.vertical_alignment = VA::Top;
        attdef.alignment_point = point;
        let mut mtext = MText::new();
        mtext.value = spec.value.clone();
        mtext.insertion_point = point;
        mtext.height = spec.height;
        mtext.rectangle_width = spec.boundary_width;
        mtext.rotation = spec.rotation;
        mtext.style = spec.style.clone();
        mtext.line_spacing_factor = spec.line_spacing;
        attdef.embedded_mtext = Some(Box::new(mtext));
    } else if spec.justify != Justify::Left {
        let anchor = match (spec.justify.two_point(), second) {
            (true, Some(s)) => {
                let s = plane.to_local(s);
                attdef.rotation = (s.y - p.y).atan2(s.x - p.x);
                Vector3::new(s.x, s.y, s.z)
            }
            _ => point,
        };
        attdef.alignment_point = anchor;
        if !spec.justify.two_point() {
            attdef.insertion_point = crate::entities::attribute::definition_text_start(&attdef);
        }
    }
    plane.place_entity(EntityType::AttributeDefinition(attdef))
}

/// Where "align below the previous definition" puts the next one: one line
/// (12/7 of the text height, as the reference spaces it) down along the
/// previous definition's own direction.
pub fn below(previous: &AttributeDefinition) -> (Vector3, f64) {
    let step = previous.height * 12.0 / 7.0;
    let (sin, cos) = previous.rotation.sin_cos();
    let base = if matches!(previous.horizontal_alignment, HA::Left)
        && matches!(previous.vertical_alignment, VA::Baseline)
        || previous.is_multiline
    {
        previous.insertion_point
    } else {
        previous.alignment_point
    };
    (
        Vector3::new(base.x + sin * step, base.y - cos * step, base.z),
        step,
    )
}

/// Place `spec` directly below `previous` (same justification, height and
/// rotation).
pub fn build_below(spec: &AttdefSpec, previous: &AttributeDefinition) -> EntityType {
    let (point, _) = below(previous);
    // Justified definitions line up by their alignment points.
    let mut spec = spec.clone();
    spec.height = previous.height;
    spec.rotation = previous.rotation;
    spec.justify = Justify::from_alignment(previous.horizontal_alignment, previous.vertical_alignment);
    let mut entity = build(&spec, WorkingPlane::default(), DVec3::new(point.x, point.y, point.z), None);
    if let EntityType::AttributeDefinition(a) = &mut entity {
        a.normal = previous.normal;
        if spec.justify.two_point() {
            let (dx, dy) = (
                previous.alignment_point.x - previous.insertion_point.x,
                previous.alignment_point.y - previous.insertion_point.y,
            );
            a.alignment_point = Vector3::new(point.x + dx, point.y + dy, point.z);
            a.insertion_point = point;
        }
    }
    entity
}

/// Text height an Align definition gets from its baseline length.
fn aligned_height(spec: &AttdefSpec, length: f64) -> f64 {
    let font = crate::entities::common::style_font(&spec.style);
    let text = if spec.value.is_empty() { spec.tag.clone() } else { spec.value.clone() };
    crate::entities::text_support::text_local_bounds(&font, &text, 1.0, spec.width_factor as f32, 0.0)
        .map(|b| b.advance as f64)
        .filter(|w| *w > 1.0e-9)
        .map(|w| length / w)
        .unwrap_or(spec.height)
}

fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn yn(on: bool) -> char {
    if on { 'Y' } else { 'N' }
}

fn modes_line(aflags: u8, annotative: bool) -> String {
    format!(
        "Invisible={}  Constant={}  Verify={}  Preset={}  Lock position={}  Annotative={}  Multiple line={}",
        yn(aflags & AFLAG_INVISIBLE != 0),
        yn(aflags & AFLAG_CONSTANT != 0),
        yn(aflags & AFLAG_VERIFY != 0),
        yn(aflags & AFLAG_PRESET != 0),
        yn(aflags & AFLAG_LOCK != 0),
        yn(annotative),
        yn(aflags & AFLAG_MULTILINE != 0),
    )
}

/// Output the reference prints before the modes prompt.
pub fn modes_report(aflags: u8, annotative: bool) -> String {
    format!("Current attribute modes:\n{}", modes_line(aflags, annotative))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Modes,
    Tag,
    Prompt,
    Value,
    NextLine,
    Start,
    Justify,
    Style,
    JustifiedPoint,
    SecondPoint,
    Height,
    Rotation,
    // Multi-line placement.
    Location,
    Corner,
    MHeight,
    MJustify,
    MSpacingType,
    MSpacing,
    MRotation,
    MStyle,
    MWidth,
}

/// `-ATTDEF` on the command line.
pub struct AttdefCommand {
    step: Step,
    spec: AttdefSpec,
    lines: Vec<String>,
    styles: Vec<TextStyle>,
    plane: WorkingPlane,
    first: Option<DVec3>,
    second: Option<DVec3>,
    previous: Option<AttributeDefinition>,
    name: &'static str,
}

impl AttdefCommand {
    pub fn new(
        name: &'static str,
        style: String,
        height: f64,
        width_factor: f64,
        oblique_angle: f64,
        styles: Vec<TextStyle>,
    ) -> Self {
        let s = session();
        Self {
            step: Step::Modes,
            spec: AttdefSpec {
                aflags: s.aflags,
                annotative: s.annotative,
                tag: String::new(),
                prompt: String::new(),
                value: String::new(),
                justify: Justify::Left,
                style,
                height,
                rotation: 0.0,
                width_factor,
                oblique_angle,
                boundary_width: 0.0,
                line_spacing: 1.0,
                field: None,
            },
            lines: Vec::new(),
            styles,
            plane: WorkingPlane::default(),
            first: None,
            second: None,
            previous: s.last.clone(),
            name,
        }
    }

    /// The lines printed before the first prompt.
    pub fn intro(&self) -> String {
        modes_report(self.spec.aflags, self.spec.annotative)
    }

    fn status(&self) -> String {
        format!(
            "Current text style:  \"{}\"  Text height:  {:.4}  Justify:  {}",
            self.spec.style,
            self.spec.height,
            self.spec.justify.label()
        )
    }

    fn style_exists(&self, name: &str) -> Option<String> {
        self.styles
            .iter()
            .find(|s| !s.name.is_empty() && s.name.eq_ignore_ascii_case(name))
            .map(|s| s.name.clone())
    }

    fn report(&self, text: &str) -> CmdResult {
        CmdResult::ReportMeasurement(text.to_string())
    }

    /// Finish: build, remember and commit.
    fn finish(&mut self) -> CmdResult {
        let first = self.first.unwrap_or(DVec3::ZERO);
        let entity = build(&self.spec, self.plane, first, self.second);
        remember(&self.spec, &entity);
        CmdResult::CommitAndExit(entity)
    }

    fn after_point(&mut self) -> CmdResult {
        if self.spec.justify == Justify::Align {
            let (a, b) = (self.first.unwrap_or_default(), self.second.unwrap_or_default());
            let length = self.plane.to_local(b).truncate().distance(self.plane.to_local(a).truncate());
            self.spec.height = aligned_height(&self.spec, length);
            return self.finish();
        }
        self.step = Step::Height;
        CmdResult::NeedPoint
    }
}

/// Keep AFLAGS / Annotative and the placed definition for the next ATTDEF.
/// Multiple lines is not kept: the reference starts the next definition
/// single-line again.
pub fn remember(spec: &AttdefSpec, entity: &EntityType) {
    let mut s = session();
    s.aflags = spec.aflags & !AFLAG_MULTILINE;
    s.annotative = spec.annotative;
    if let EntityType::AttributeDefinition(a) = entity {
        s.last = Some(a.clone());
    }
}

const MODES_PROMPT: &str =
    "Enter an option to change [Invisible/Constant/Verify/Preset/Lock position/Annotative/Multiple lines] <done>:";

impl CadCommand for AttdefCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Modes => MODES_PROMPT.to_string(),
            Step::Tag => "Enter attribute tag name:".to_string(),
            Step::Prompt => "Enter attribute prompt:".to_string(),
            Step::Value if self.spec.constant() => "Enter attribute value:".to_string(),
            Step::Value => "Enter default attribute value:".to_string(),
            Step::NextLine => "Next line or <done>:".to_string(),
            Step::Start => "Specify start point of text or [Justify/Style]:".to_string(),
            Step::Justify => {
                "Enter an option [Left/Center/Right/Align/Middle/Fit/TL/TC/TR/ML/MC/MR/BL/BC/BR]:"
                    .to_string()
            }
            Step::Style | Step::MStyle => {
                format!("Enter style name or [?] <{}>:", self.spec.style)
            }
            Step::JustifiedPoint => self.spec.justify.point_prompt().to_string(),
            Step::SecondPoint => "Specify second endpoint of text baseline:".to_string(),
            Step::Height | Step::MHeight => format!("Specify height <{:.4}>:", self.spec.height),
            Step::Rotation => format!(
                "Specify rotation angle of text <{}>:",
                short(self.spec.rotation.to_degrees())
            ),
            Step::Location => "Specify location of multiline attribute:".to_string(),
            Step::Corner => {
                "Specify opposite corner or [Height/Justify/Line spacing/Rotation/Style/Width]:"
                    .to_string()
            }
            Step::MJustify => "Enter justification [TL/TC/TR/ML/MC/MR/BL/BC/BR] <TL>:".to_string(),
            Step::MSpacingType => "Enter line spacing type [At least/Exactly] <At least>:".to_string(),
            Step::MSpacing => format!(
                "Enter line spacing factor or distance <{}x>:",
                short(self.spec.line_spacing)
            ),
            Step::MRotation => format!(
                "Specify rotation angle <{}>:",
                short(self.spec.rotation.to_degrees())
            ),
            Step::MWidth => "Specify width:".to_string(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Prompt | Step::Value | Step::NextLine => InputKind::FreeText,
            Step::Modes | Step::Tag | Step::Justify | Step::Style | Step::MStyle | Step::MJustify
            | Step::MSpacingType => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start | Step::Corner)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let raw = text.trim();
        let word = raw.trim_start_matches('_').to_ascii_uppercase();
        match self.step {
            Step::Modes => {
                if word.is_empty() {
                    return Some(self.on_enter());
                }
                let bit = match word.as_str() {
                    "I" | "INVISIBLE" => Some(AFLAG_INVISIBLE),
                    "C" | "CONSTANT" => Some(AFLAG_CONSTANT),
                    "V" | "VERIFY" => Some(AFLAG_VERIFY),
                    "P" | "PRESET" => Some(AFLAG_PRESET),
                    "L" | "LOCK" | "LOCKPOSITION" => Some(AFLAG_LOCK),
                    "M" | "MULTIPLE" | "MULTIPLELINES" => Some(AFLAG_MULTILINE),
                    "A" | "ANNOTATIVE" => None,
                    _ => return Some(CmdResult::ReportError("Invalid option keyword.".into())),
                };
                match bit {
                    Some(bit) => self.spec.aflags ^= bit,
                    None => self.spec.annotative = !self.spec.annotative,
                }
                {
                    let mut s = session();
                    s.aflags = self.spec.aflags;
                    s.annotative = self.spec.annotative;
                }
                Some(self.report(&modes_report(self.spec.aflags, self.spec.annotative)))
            }
            Step::Tag => {
                if raw.is_empty() {
                    return Some(CmdResult::ReportError("Tag cannot be null".into()));
                }
                self.spec.tag = normalize_tag(raw);
                self.step = if self.spec.constant() { Step::Value } else { Step::Prompt };
                Some(CmdResult::NeedPoint)
            }
            Step::Prompt => {
                self.spec.prompt = text.to_string();
                self.step = Step::Value;
                Some(CmdResult::NeedPoint)
            }
            Step::Value => {
                if self.spec.multiline() {
                    if text.is_empty() {
                        self.lines_done();
                        return Some(CmdResult::NeedPoint);
                    }
                    self.lines.push(text.to_string());
                    self.step = Step::NextLine;
                } else {
                    self.spec.value = text.to_string();
                    self.step = Step::Start;
                    return Some(self.report(&self.status()));
                }
                Some(CmdResult::NeedPoint)
            }
            Step::NextLine => {
                if text.is_empty() {
                    self.lines_done();
                    return Some(CmdResult::NeedPoint);
                }
                self.lines.push(text.to_string());
                Some(CmdResult::NeedPoint)
            }
            Step::Start => match word.as_str() {
                "J" | "JUSTIFY" => {
                    self.step = Step::Justify;
                    Some(CmdResult::NeedPoint)
                }
                "S" | "STYLE" => {
                    self.step = Step::Style;
                    Some(CmdResult::NeedPoint)
                }
                "" => Some(self.on_enter()),
                _ => Some(CmdResult::ReportError("Point or option keyword required.".into())),
            },
            Step::Justify => match Justify::from_keyword(&word) {
                Some(j) => {
                    self.spec.justify = j;
                    self.step = Step::JustifiedPoint;
                    Some(CmdResult::NeedPoint)
                }
                None => Some(CmdResult::ReportError("Invalid option keyword.".into())),
            },
            Step::Style | Step::MStyle => {
                let back = if self.step == Step::Style { Step::Start } else { Step::Corner };
                if raw.is_empty() {
                    self.step = back;
                    return Some(if back == Step::Start {
                        self.report(&self.status())
                    } else {
                        CmdResult::NeedPoint
                    });
                }
                if raw == "?" {
                    let names: Vec<String> = self
                        .styles
                        .iter()
                        .filter(|s| !s.name.is_empty())
                        .map(|s| format!("Style name: \"{}\"  Font files: {}", s.name, s.font_file))
                        .collect();
                    return Some(self.report(&format!("Text styles:\n{}", names.join("\n"))));
                }
                match self.style_exists(raw) {
                    Some(name) => {
                        self.spec.style = name;
                        self.step = back;
                        Some(if back == Step::Start {
                            self.report(&self.status())
                        } else {
                            CmdResult::NeedPoint
                        })
                    }
                    None => Some(CmdResult::ReportError(format!("Cannot find text style \"{raw}\"."))),
                }
            }
            Step::Height | Step::MHeight => match raw.parse::<f64>() {
                Ok(v) if v > 0.0 => {
                    self.spec.height = v;
                    if self.step == Step::MHeight {
                        self.step = Step::Corner;
                        return Some(CmdResult::NeedPoint);
                    }
                    if self.spec.justify == Justify::Fit {
                        return Some(self.finish());
                    }
                    self.step = Step::Rotation;
                    Some(CmdResult::NeedPoint)
                }
                Ok(_) => Some(CmdResult::ReportError("Value must be positive and nonzero.".into())),
                Err(_) => Some(CmdResult::ReportError("Requires numeric distance or two points.".into())),
            },
            Step::Rotation | Step::MRotation => match raw.parse::<f64>() {
                Ok(v) => {
                    self.spec.rotation = v.to_radians();
                    if self.step == Step::MRotation {
                        self.step = Step::Corner;
                        return Some(CmdResult::NeedPoint);
                    }
                    Some(self.finish())
                }
                Err(_) => Some(CmdResult::ReportError("Requires valid numeric angle or second point.".into())),
            },
            Step::Corner => match word.as_str() {
                "H" | "HEIGHT" => {
                    self.step = Step::MHeight;
                    Some(CmdResult::NeedPoint)
                }
                "J" | "JUSTIFY" => {
                    self.step = Step::MJustify;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" | "LINESPACING" => {
                    self.step = Step::MSpacingType;
                    Some(CmdResult::NeedPoint)
                }
                "R" | "ROTATION" => {
                    self.step = Step::MRotation;
                    Some(CmdResult::NeedPoint)
                }
                "S" | "STYLE" => {
                    self.step = Step::MStyle;
                    Some(CmdResult::NeedPoint)
                }
                "W" | "WIDTH" => {
                    self.step = Step::MWidth;
                    Some(CmdResult::NeedPoint)
                }
                _ => Some(CmdResult::ReportError("2D point or option keyword required.".into())),
            },
            Step::MJustify => {
                // Multi-line definitions keep their attachment in the embedded
                // MText; only the keyword is validated here.
                if word.is_empty()
                    || ["TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"].contains(&word.as_str())
                {
                    self.step = Step::Corner;
                    Some(CmdResult::NeedPoint)
                } else {
                    Some(CmdResult::ReportError("Invalid option keyword.".into()))
                }
            }
            Step::MSpacingType => {
                if word.is_empty() || word.starts_with('A') || word.starts_with('E') {
                    self.step = Step::MSpacing;
                    Some(CmdResult::NeedPoint)
                } else {
                    Some(CmdResult::ReportError("Invalid option keyword.".into()))
                }
            }
            Step::MSpacing => {
                let t = raw.trim_end_matches(['x', 'X']);
                if raw.is_empty() {
                    self.step = Step::Corner;
                    return Some(CmdResult::NeedPoint);
                }
                match t.parse::<f64>() {
                    Ok(v) if v > 0.0 => {
                        self.spec.line_spacing = v;
                        self.step = Step::Corner;
                        Some(CmdResult::NeedPoint)
                    }
                    _ => Some(CmdResult::ReportError("Value must be positive and nonzero.".into())),
                }
            }
            Step::MWidth => match raw.parse::<f64>() {
                Ok(v) if v >= 0.0 => {
                    self.spec.boundary_width = v;
                    self.spec.value = self.lines.join("\\P");
                    Some(self.finish())
                }
                _ => Some(CmdResult::ReportError("Requires numeric distance or second point.".into())),
            },
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Start => {
                self.spec.justify = Justify::Left;
                self.first = Some(pt);
                self.after_point()
            }
            Step::JustifiedPoint => {
                self.first = Some(pt);
                if self.spec.justify.two_point() {
                    self.step = Step::SecondPoint;
                    return CmdResult::NeedPoint;
                }
                self.after_point()
            }
            Step::SecondPoint => {
                self.second = Some(pt);
                self.after_point()
            }
            Step::Height | Step::MHeight => {
                let base = self.first.unwrap_or(pt);
                let d = self.plane.to_local(pt).truncate().distance(self.plane.to_local(base).truncate());
                self.on_text_input(&d.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            Step::Rotation | Step::MRotation => {
                let base = self.plane.to_local(self.first.unwrap_or(pt));
                let p = self.plane.to_local(pt);
                let angle = (p.y - base.y).atan2(p.x - base.x).to_degrees();
                self.on_text_input(&angle.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            Step::Location => {
                self.first = Some(pt);
                self.step = Step::Corner;
                CmdResult::NeedPoint
            }
            Step::Corner | Step::MWidth => {
                let a = self.plane.to_local(self.first.unwrap_or(pt));
                let b = self.plane.to_local(pt);
                let (sin, cos) = self.spec.rotation.sin_cos();
                let width = ((b.x - a.x) * cos + (b.y - a.y) * sin).abs();
                self.spec.boundary_width = width;
                self.spec.value = self.lines.join("\\P");
                self.finish()
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Modes => {
                self.step = Step::Tag;
                CmdResult::NeedPoint
            }
            Step::Tag => CmdResult::ReportError("Tag cannot be null".into()),
            Step::Prompt | Step::Value | Step::NextLine | Step::Style | Step::MStyle
            | Step::MSpacingType | Step::MSpacing | Step::MJustify => {
                self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
            }
            Step::Start => match self.previous.clone() {
                Some(previous) => {
                    let entity = build_below(&self.spec, &previous);
                    remember(&self.spec, &entity);
                    CmdResult::CommitAndExit(entity)
                }
                None => CmdResult::ReportError("Point or option keyword required.".into()),
            },
            Step::Height | Step::MHeight => {
                let h = self.spec.height;
                self.on_text_input(&h.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            Step::Rotation | Step::MRotation => {
                let r = self.spec.rotation.to_degrees();
                self.on_text_input(&r.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.step, Step::Start | Step::JustifiedPoint | Step::Location) {
            return None;
        }
        Some(cross_preview(pt, self.plane))
    }
}

/// The value step after the modes: the multi-line value is collected line by
/// line, then the location is asked.
impl AttdefCommand {
    pub fn lines_done(&mut self) {
        self.spec.value = self.lines.join("\\P");
        self.step = Step::Location;
    }
}

/// Dialog placement: the spec is complete; only the start point (and the
/// second endpoint for Align / Fit) is asked.
pub struct AttdefPlaceCommand {
    spec: AttdefSpec,
    plane: WorkingPlane,
    first: Option<DVec3>,
}

impl AttdefPlaceCommand {
    pub fn new(spec: AttdefSpec) -> Self {
        Self { spec, plane: WorkingPlane::default(), first: None }
    }
}

impl CadCommand for AttdefPlaceCommand {
    fn name(&self) -> &'static str {
        "ATTDEF"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match (self.first, self.spec.justify.two_point()) {
            (None, true) => "Specify first endpoint of text baseline:".into(),
            (Some(_), _) => "Specify second endpoint of text baseline:".into(),
            (None, false) => "Specify start point:".into(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.spec.justify.two_point() && self.first.is_none() {
            self.first = Some(pt);
            return CmdResult::NeedPoint;
        }
        let (first, second) = match self.first {
            Some(first) => (first, Some(pt)),
            None => (pt, None),
        };
        if self.spec.justify == Justify::Align {
            let length = self.plane.to_local(pt).truncate().distance(self.plane.to_local(first).truncate());
            self.spec.height = aligned_height(&self.spec, length);
        }
        let entity = build(&self.spec, self.plane, first, second);
        remember(&self.spec, &entity);
        CmdResult::CommitAndExit(entity)
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        Some(cross_preview(pt, self.plane))
    }

    fn text_field(&self) -> Option<(String, Vec<codec::Handle>)> {
        self.spec.field.clone()
    }
}

/// A small cross at the cursor while a placement point is asked.
fn cross_preview(pt: DVec3, plane: WorkingPlane) -> WireModel {
    let d = 0.15;
    let points = [
        pt - plane.x * d,
        pt + plane.x * d,
        DVec3::splat(f64::NAN),
        pt - plane.y * d,
        pt + plane.y * d,
    ];
    WireModel::solid(
        "attdef_preview".into(),
        points.iter().map(|point| point.as_vec3().to_array()).collect(),
        WireModel::CYAN,
        false,
    )
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ATTDEF", "-ATTDEF", "AFLAGS"] });

/// A value the ATTDEF dialog takes from the drawing (its ⌖ buttons): typed,
/// or measured from two points. Hands the value back to reopen the dialog.
pub struct AttdefPickCommand {
    /// "H" height, "R" rotation, "W" boundary width.
    kind: &'static str,
    current: String,
    first: Option<DVec3>,
    plane: WorkingPlane,
}

impl AttdefPickCommand {
    pub fn new(kind: &'static str, current: String) -> Self {
        Self { kind, current, first: None, plane: WorkingPlane::default() }
    }

    fn back(&self, value: &str) -> CmdResult {
        CmdResult::Dispatch(format!("_ATTDEF_PICKED {} {}", self.kind, value))
    }
}

impl CadCommand for AttdefPickCommand {
    fn name(&self) -> &'static str {
        "ATTDEF"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match (self.kind, self.first.is_some()) {
            (_, true) => "Specify second point:".into(),
            ("H", _) => format!("Specify height <{}>:", self.current),
            ("R", _) => format!("Specify rotation angle <{}>:", self.current),
            _ => "Specify width:".into(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let raw = text.trim();
        if raw.is_empty() {
            return Some(self.on_enter());
        }
        match raw.parse::<f64>() {
            Ok(v) if self.kind == "R" || v > 0.0 => Some(self.back(&short(v))),
            Ok(_) => Some(CmdResult::ReportError("Value must be positive and nonzero.".into())),
            Err(_) => Some(CmdResult::ReportError("Requires numeric distance or two points.".into())),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(first) = self.first else {
            self.first = Some(pt);
            return CmdResult::NeedPoint;
        };
        let (a, b) = (self.plane.to_local(first), self.plane.to_local(pt));
        let value = if self.kind == "R" {
            (b.y - a.y).atan2(b.x - a.x).to_degrees()
        } else {
            a.truncate().distance(b.truncate())
        };
        self.back(&short(value))
    }

    fn on_enter(&mut self) -> CmdResult {
        let current = self.current.clone();
        self.back(&current)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("_ATTDEF_PICKED".into())
    }
}

/// ATTDISP: `Enter attribute visibility setting [Normal/ON/OFF] <current>:`.
/// Hands the chosen setting back as `ATTDISP <word>`; Enter keeps the current
/// one.
pub struct AttdispCommand {
    current: &'static str,
}

impl AttdispCommand {
    /// `mode` is ATTMODE: 0 OFF, 1 Normal, 2 ON.
    pub fn new(mode: i16) -> Self {
        Self { current: attdisp_word(mode) }
    }
}

/// The ATTDISP keyword for an ATTMODE value.
pub fn attdisp_word(mode: i16) -> &'static str {
    match mode {
        0 => "OFF",
        2 => "ON",
        _ => "Normal",
    }
}

impl CadCommand for AttdispCommand {
    fn name(&self) -> &'static str {
        "ATTDISP"
    }

    fn prompt(&self) -> String {
        format!("Enter attribute visibility setting [Normal/ON/OFF] <{}>:", self.current)
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if word.is_empty() {
            return Some(self.on_enter());
        }
        let mode = match word.as_str() {
            "N" | "NORMAL" => "NORMAL",
            "ON" => "ON",
            "OF" | "OFF" => "OFF",
            _ => return Some(CmdResult::ReportError("Invalid option keyword.".into())),
        };
        Some(CmdResult::Dispatch(format!("ATTDISP {mode}")))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// The dialog's "…" button: the default value is edited in the multi-line
/// text editor, then handed back to the dialog (`_ATTDEF_VALUE`).
pub struct AttdefValueCommand;

impl CadCommand for AttdefValueCommand {
    fn name(&self) -> &'static str {
        "ATTDEF"
    }

    fn prompt(&self) -> String {
        String::new()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_editor_text(&mut self, value: String) {
        session().editor_value = Some(value);
    }

    fn on_editor_closed(&mut self, _committed: bool) -> CmdResult {
        CmdResult::Dispatch("_ATTDEF_VALUE".into())
    }
}
