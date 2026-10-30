//! JUSTIFYTEXT, SCALETEXT and TEXTALIGN: text moved, rescaled or lined up
//! without the glyphs jumping.
//!
//! Each works on the box a TEXT or MTEXT occupies ([`TextFrame`]): a TEXT
//! runs along its baseline for its pen advance, from the font's descender
//! below the baseline up to its height; an MTEXT spans its widest line and
//! hangs from the first line's cap height to the last baseline.

use std::sync::Mutex;

use codec::entities::{AttachmentPoint, TextHorizontalAlignment as HA, TextVerticalAlignment as VA};
use codec::types::Vector3;
use codec::{Document, EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::entities::text_support::{descent_ratio, resolve_dxf_special_chars, text_local_bounds};

// ── Justification codes ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Justify {
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

const JUSTIFY_ALL: [Justify; 15] = [
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Baseline,
    Top,
    Mid,
    Bottom,
    /// Middle (72 = 4): the centre of the strokes.
    Ink,
}

impl Justify {
    pub fn label(self) -> &'static str {
        match self {
            Justify::Left => "Left",
            Justify::Align => "Align",
            Justify::Fit => "Fit",
            Justify::Center => "Center",
            Justify::Middle => "Middle",
            Justify::Right => "Right",
            Justify::TL => "TL",
            Justify::TC => "TC",
            Justify::TR => "TR",
            Justify::ML => "ML",
            Justify::MC => "MC",
            Justify::MR => "MR",
            Justify::BL => "BL",
            Justify::BC => "BC",
            Justify::BR => "BR",
        }
    }

    /// The option typed (`_` / `.` prefixes and case ignored; the word
    /// options take their first letter).
    pub fn parse(word: &str) -> Option<Self> {
        let word = keyword(word);
        if word.is_empty() {
            return None;
        }
        let by_word = |full: &str| full.starts_with(word.as_str());
        Some(match word.as_str() {
            "TL" => Justify::TL,
            "TC" => Justify::TC,
            "TR" => Justify::TR,
            "ML" => Justify::ML,
            "MC" => Justify::MC,
            "MR" => Justify::MR,
            "BL" => Justify::BL,
            "BC" => Justify::BC,
            "BR" => Justify::BR,
            _ if by_word("LEFT") => Justify::Left,
            _ if by_word("ALIGN") => Justify::Align,
            _ if by_word("FIT") => Justify::Fit,
            _ if by_word("CENTER") => Justify::Center,
            _ if by_word("MIDDLE") => Justify::Middle,
            _ if by_word("RIGHT") => Justify::Right,
            _ => return None,
        })
    }

    fn column(self) -> f64 {
        match self {
            Justify::Center | Justify::Middle | Justify::TC | Justify::MC | Justify::BC => 0.5,
            Justify::Right | Justify::TR | Justify::MR | Justify::BR => 1.0,
            _ => 0.0,
        }
    }

    fn row(self) -> Row {
        match self {
            Justify::Left | Justify::Align | Justify::Fit | Justify::Center | Justify::Right => {
                Row::Baseline
            }
            Justify::Middle => Row::Ink,
            Justify::TL | Justify::TC | Justify::TR => Row::Top,
            Justify::ML | Justify::MC | Justify::MR => Row::Mid,
            Justify::BL | Justify::BC | Justify::BR => Row::Bottom,
        }
    }

    pub fn of_text(h: HA, v: VA) -> Self {
        match (h, v) {
            (HA::Aligned, _) => Justify::Align,
            (HA::Fit, _) => Justify::Fit,
            (HA::Middle, _) => Justify::Middle,
            (HA::Left, VA::Baseline) => Justify::Left,
            (HA::Center, VA::Baseline) => Justify::Center,
            (HA::Right, VA::Baseline) => Justify::Right,
            (HA::Left, VA::Top) => Justify::TL,
            (HA::Center, VA::Top) => Justify::TC,
            (HA::Right, VA::Top) => Justify::TR,
            (HA::Left, VA::Middle) => Justify::ML,
            (HA::Center, VA::Middle) => Justify::MC,
            (HA::Right, VA::Middle) => Justify::MR,
            (HA::Left, VA::Bottom) => Justify::BL,
            (HA::Center, VA::Bottom) => Justify::BC,
            (HA::Right, VA::Bottom) => Justify::BR,
        }
    }

    fn to_text(self) -> (HA, VA) {
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

    /// An MTEXT has no baseline codes: Left, Align and Fit take its bottom
    /// left, Center and Right its bottom centre and right, Middle its centre
    /// (measured).
    fn attachment(self) -> AttachmentPoint {
        use AttachmentPoint as A;
        match self {
            Justify::Left | Justify::Align | Justify::Fit | Justify::BL => A::BottomLeft,
            Justify::Center | Justify::BC => A::BottomCenter,
            Justify::Right | Justify::BR => A::BottomRight,
            Justify::Middle | Justify::MC => A::MiddleCenter,
            Justify::TL => A::TopLeft,
            Justify::TC => A::TopCenter,
            Justify::TR => A::TopRight,
            Justify::ML => A::MiddleLeft,
            Justify::MR => A::MiddleRight,
        }
    }

    fn of_attachment(a: AttachmentPoint) -> Self {
        use AttachmentPoint as A;
        match a {
            A::TopLeft => Justify::TL,
            A::TopCenter => Justify::TC,
            A::TopRight => Justify::TR,
            A::MiddleLeft => Justify::ML,
            A::MiddleCenter => Justify::MC,
            A::MiddleRight => Justify::MR,
            A::BottomLeft => Justify::BL,
            A::BottomCenter => Justify::BC,
            A::BottomRight => Justify::BR,
        }
    }
}

fn keyword(text: &str) -> String {
    text.trim().trim_start_matches(['_', '.']).to_ascii_uppercase()
}

fn hex(handles: &[Handle]) -> String {
    handles
        .iter()
        .map(|h| format!("{:X}", h.value()))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn parse_handles(text: &str) -> Vec<Handle> {
    text.split(',')
        .filter_map(|s| u64::from_str_radix(s.trim(), 16).ok().map(Handle::new))
        .collect()
}

/// TEXT and MTEXT, the objects the three commands select.
pub fn is_text(entity: &EntityType) -> bool {
    matches!(entity, EntityType::Text(_) | EntityType::MText(_))
}

// ── The box a text occupies ────────────────────────────────────────────────

/// A TEXT or MTEXT box in its own frame: x along the baseline, y up.
pub struct TextFrame {
    origin: [f64; 2],
    rotation: f64,
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
    /// The y Left / Center / Right sit on: the baseline of a TEXT, the bottom
    /// of an MTEXT.
    base: f64,
    /// The y ML / MC / MR sit on: half the height of a TEXT (measured: "Alpha"
    /// at 2.5 centres 1.25 above its baseline), the middle of an MTEXT.
    mid: f64,
    /// The centre of the strokes (Middle).
    ink: f64,
}

impl TextFrame {
    pub fn of(entity: &EntityType, doc: &Document) -> Option<Self> {
        match entity {
            EntityType::Text(t) => {
                let p = crate::entities::text::text_run_placement_at_scale(t, doc, 1.0);
                let height = p.height as f64;
                let b = text_local_bounds(
                    &p.font,
                    &resolve_dxf_special_chars(&t.value),
                    p.height,
                    p.width_factor,
                    p.oblique_angle,
                );
                let advance = b.as_ref().map_or(0.0, |b| b.advance as f64)
                    * (p.width_factor as f64).signum();
                let ink = b
                    .as_ref()
                    .map_or(height * 0.5, |b| (b.ink_min[1] + b.ink_max[1]) as f64 * 0.5);
                Some(Self {
                    origin: p.origin,
                    rotation: p.rotation as f64,
                    left: 0.0,
                    right: advance,
                    bottom: -height * descent_ratio(&p.font) as f64,
                    top: height,
                    base: 0.0,
                    mid: height * 0.5,
                    ink,
                })
            }
            EntityType::MText(m) => {
                let layout = crate::entities::mtext::layout_of(m, doc, false);
                let width = layout.line_widths.iter().fold(0.0_f32, |a, w| a.max(*w)) as f64;
                let lines = layout.line_count.max(1);
                let block = m.height + (lines - 1) as f64 * layout.line_height as f64;
                let attachment = Justify::of_attachment(m.attachment_point);
                let left = -attachment.column() * width;
                let top = match attachment.row() {
                    Row::Mid => block / 2.0,
                    Row::Bottom => block,
                    _ => 0.0,
                };
                let bottom = top - block;
                Some(Self {
                    origin: [m.insertion_point.x, m.insertion_point.y],
                    rotation: m.rotation,
                    left,
                    right: left + width,
                    bottom,
                    top,
                    base: bottom,
                    mid: (top + bottom) / 2.0,
                    ink: (top + bottom) / 2.0,
                })
            }
            _ => None,
        }
    }

    fn local(&self, j: Justify) -> [f64; 2] {
        let x = self.left + (self.right - self.left) * j.column();
        let y = match j.row() {
            Row::Baseline => self.base,
            Row::Top => self.top,
            Row::Mid => self.mid,
            Row::Bottom => self.bottom,
            Row::Ink => self.ink,
        };
        [x, y]
    }

    fn world(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.rotation.sin_cos();
        [
            self.origin[0] + x * c - y * s,
            self.origin[1] + x * s + y * c,
        ]
    }

    /// The point justification `j` puts on this box, in the drawing.
    pub fn anchor(&self, j: Justify) -> [f64; 2] {
        self.world(self.local(j))
    }

    fn width(&self) -> f64 {
        (self.right - self.left).abs()
    }

}

fn v3(p: [f64; 2], z: f64) -> Vector3 {
    Vector3::new(p[0], p[1], z)
}

/// The justification the object has now.
pub fn current_justification(entity: &EntityType) -> Option<Justify> {
    match entity {
        EntityType::Text(t) => Some(Justify::of_text(t.horizontal_alignment, t.vertical_alignment)),
        EntityType::MText(m) => Some(Justify::of_attachment(m.attachment_point)),
        _ => None,
    }
}

/// `entity` justified by `j` with its glyphs where they were.
pub fn justified(entity: &EntityType, doc: &Document, j: Justify) -> Option<EntityType> {
    let frame = TextFrame::of(entity, doc)?;
    let mut out = entity.clone();
    match &mut out {
        EntityType::Text(t) => {
            let z = t.insertion_point.z;
            let (h, v) = j.to_text();
            t.horizontal_alignment = h;
            t.vertical_alignment = v;
            t.insertion_point = v3(frame.world([0.0, 0.0]), z);
            t.alignment_point = match j {
                Justify::Left => None,
                // The baseline from its start to its end.
                Justify::Align | Justify::Fit => Some(v3(frame.world([frame.right, 0.0]), z)),
                _ => Some(v3(frame.anchor(j), z)),
            };
        }
        EntityType::MText(m) => {
            let attachment = j.attachment();
            m.insertion_point = v3(
                frame.anchor(Justify::of_attachment(attachment)),
                m.insertion_point.z,
            );
            m.attachment_point = attachment;
        }
        _ => return None,
    }
    Some(out)
}

/// `entity` scaled by `factor` about the point justification `base` puts on
/// it (its own justification point when `None`). An MTEXT's boundary width
/// scales with it.
pub fn scaled(
    entity: &EntityType,
    doc: &Document,
    base: Option<Justify>,
    factor: f64,
) -> Option<EntityType> {
    let frame = TextFrame::of(entity, doc)?;
    let j = base.or_else(|| current_justification(entity))?;
    let a = frame.anchor(j);
    let about = |p: Vector3| Vector3::new(a[0] + (p.x - a[0]) * factor, a[1] + (p.y - a[1]) * factor, p.z);
    let mut out = entity.clone();
    match &mut out {
        EntityType::Text(t) => {
            t.height *= factor;
            t.insertion_point = about(t.insertion_point);
            t.alignment_point = t.alignment_point.map(about);
        }
        EntityType::MText(m) => {
            m.height *= factor;
            m.rectangle_width *= factor;
            m.rectangle_height = m.rectangle_height.map(|h| h * factor);
            m.insertion_point = about(m.insertion_point);
        }
        _ => return None,
    }
    Some(out)
}

/// `entity` moved by `delta`.
pub fn moved(entity: &EntityType, delta: [f64; 2]) -> Option<EntityType> {
    let shift = |p: Vector3| Vector3::new(p.x + delta[0], p.y + delta[1], p.z);
    let mut out = entity.clone();
    match &mut out {
        EntityType::Text(t) => {
            t.insertion_point = shift(t.insertion_point);
            t.alignment_point = t.alignment_point.map(shift);
        }
        EntityType::MText(m) => m.insertion_point = shift(m.insertion_point),
        _ => return None,
    }
    Some(out)
}

pub fn text_height(entity: &EntityType) -> Option<f64> {
    match entity {
        EntityType::Text(t) => Some(t.height),
        EntityType::MText(m) => Some(m.height),
        _ => None,
    }
}

// ── Remembered settings ────────────────────────────────────────────────────

/// SCALETEXT's base point option; JUSTIFYTEXT shares the last one chosen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BaseOption {
    Existing,
    At(Justify),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Spacing {
    Distribute,
    Set,
    CurrentVertical,
    CurrentHorizontal,
}

struct Settings {
    last_option: Option<BaseOption>,
    scale_height: Option<f64>,
    scale_factor: f64,
    align: Justify,
    spacing: Spacing,
    set_spacing: f64,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    last_option: None,
    scale_height: None,
    scale_factor: 2.0,
    align: Justify::Left,
    spacing: Spacing::CurrentVertical,
    set_spacing: 0.0,
});

fn settings() -> std::sync::MutexGuard<'static, Settings> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// TEXTALIGN's alignment option.
pub fn text_align_setting() -> Justify {
    settings().align
}

fn invalid_keyword() -> CmdResult {
    CmdResult::ReportError(crate::tr!("text-edit", "invalid-keyword"))
}

fn positive_error() -> CmdResult {
    CmdResult::ReportError(crate::tr!("text-edit", "positive"))
}

fn number(text: &str) -> Option<f64> {
    crate::entities::common::parse_typed_length(text.trim()).filter(|v| v.is_finite())
}

fn options(labels: &[&'static str]) -> Vec<CmdOption> {
    labels
        .iter()
        .map(|label| {
            let keyword: String = label.chars().filter(|c| c.is_ascii_uppercase()).collect();
            let keyword = if keyword.is_empty() { label.to_string() } else { keyword };
            CmdOption::new(label, &keyword)
        })
        .collect()
}

// ── JUSTIFYTEXT ────────────────────────────────────────────────────────────

/// `Enter a justification option` for the selected text.
pub struct JustifyTextCommand {
    handles: Vec<Handle>,
    retry: bool,
}

impl JustifyTextCommand {
    pub fn new(handles: Vec<Handle>) -> Self {
        Self { handles, retry: false }
    }

    fn default_option() -> Justify {
        match settings().last_option {
            Some(BaseOption::At(j)) => j,
            _ => Justify::Left,
        }
    }

    fn apply(&mut self, j: Justify) -> CmdResult {
        settings().last_option = Some(BaseOption::At(j));
        CmdResult::Dispatch(format!("_JUSTIFYTEXTAPPLY {} {}", j.label(), hex(&self.handles)))
    }
}

impl CadCommand for JustifyTextCommand {
    fn name(&self) -> &'static str {
        "JUSTIFYTEXT"
    }

    fn prompt(&self) -> String {
        let options = crate::tr!(
            "text-edit",
            "justify-options",
            default = Self::default_option().label()
        );
        if self.retry {
            options
        } else {
            format!("{}\n{}", crate::tr!("text-edit", "justify-title"), options)
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        JUSTIFY_ALL
            .iter()
            .map(|j| CmdOption::new(j.label(), j.label()))
            .collect()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if text.trim().is_empty() {
            return Some(self.on_enter());
        }
        match Justify::parse(text) {
            Some(j) => Some(self.apply(j)),
            None => {
                self.retry = true;
                Some(invalid_keyword())
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        let j = Self::default_option();
        self.apply(j)
    }
}

// ── SCALETEXT ──────────────────────────────────────────────────────────────

const SCALE_BASES: [Justify; 13] = [
    Justify::Left,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScaleStep {
    Base,
    Height,
    Paper,
    Match,
    Factor,
    RefLength,
    RefSecond,
    NewLength,
}

/// SCALETEXT for the selected text: base point option, then the new height
/// (model or paper), the height of another text or a scale factor.
pub struct ScaleTextCommand {
    handles: Vec<Handle>,
    step: ScaleStep,
    retry: bool,
    base: BaseOption,
    default_height: f64,
    ref_first: Option<DVec3>,
    ref_length: f64,
}

impl ScaleTextCommand {
    /// `text_size` is TEXTSIZE, the first height offered.
    pub fn new(handles: Vec<Handle>, text_size: f64) -> Self {
        let default_height = settings().scale_height.unwrap_or(text_size);
        Self {
            handles,
            step: ScaleStep::Base,
            retry: false,
            base: Self::default_base(),
            default_height,
            ref_first: None,
            ref_length: 1.0,
        }
    }

    fn default_base() -> BaseOption {
        match settings().last_option {
            Some(BaseOption::At(j)) if SCALE_BASES.contains(&j) => BaseOption::At(j),
            _ => BaseOption::Existing,
        }
    }

    fn base_label(base: BaseOption) -> &'static str {
        match base {
            BaseOption::Existing => "Existing",
            BaseOption::At(j) => j.label(),
        }
    }

    fn base_arg(&self) -> &'static str {
        Self::base_label(self.base)
    }

    fn finish(&self, mode: &str, value: f64) -> CmdResult {
        CmdResult::Dispatch(format!(
            "_SCALETEXTAPPLY {} {} {} {}",
            self.base_arg(),
            mode,
            value,
            hex(&self.handles)
        ))
    }

    fn set_height(&mut self, height: f64) -> CmdResult {
        settings().scale_height = Some(height);
        self.finish("H", height)
    }

    fn set_factor(&mut self, factor: f64) -> CmdResult {
        settings().scale_factor = factor;
        self.finish("S", factor)
    }
}

impl CadCommand for ScaleTextCommand {
    fn name(&self) -> &'static str {
        "SCALETEXT"
    }

    fn prompt(&self) -> String {
        match self.step {
            ScaleStep::Base => {
                let options = crate::tr!(
                    "text-edit",
                    "scale-base-options",
                    default = Self::base_label(Self::default_base())
                );
                if self.retry {
                    options
                } else {
                    format!("{}\n{}", crate::tr!("text-edit", "scale-base-title"), options)
                }
            }
            ScaleStep::Height => crate::tr!(
                "text-edit",
                "scale-height",
                default = fmt_number(self.default_height)
            ),
            ScaleStep::Paper => crate::tr!(
                "text-edit",
                "scale-paper-height",
                default = fmt_number(self.default_height)
            ),
            ScaleStep::Match => crate::tr!("text-edit", "scale-match"),
            ScaleStep::Factor => crate::tr!(
                "text-edit",
                "scale-factor",
                default = fmt_number(settings().scale_factor)
            ),
            ScaleStep::RefLength => crate::tr!(
                "text-edit",
                "scale-reference",
                default = fmt_number(self.ref_length)
            ),
            ScaleStep::RefSecond => crate::tr!("text-edit", "second-point"),
            ScaleStep::NewLength => crate::tr!("text-edit", "scale-new-length"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            ScaleStep::Base => std::iter::once(CmdOption::new("Existing", "E"))
                .chain(SCALE_BASES.iter().map(|j| CmdOption::new(j.label(), j.label())))
                .collect(),
            ScaleStep::Height => options(&["Paper height", "Match object", "Scale factor"]),
            ScaleStep::Factor => options(&["Reference"]),
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            ScaleStep::Base => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == ScaleStep::Match
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.step == ScaleStep::Match
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.step != ScaleStep::Match || handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!(
            "_SCALETEXTAPPLY {} M {:X} {}",
            self.base_arg(),
            handle.value(),
            hex(&self.handles)
        ))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = keyword(text);
        if word.is_empty() {
            return Some(self.on_enter());
        }
        Some(match self.step {
            ScaleStep::Base => {
                let base = if "EXISTING".starts_with(word.as_str()) {
                    Some(BaseOption::Existing)
                } else {
                    Justify::parse(&word)
                        .filter(|j| SCALE_BASES.contains(j))
                        .map(BaseOption::At)
                };
                match base {
                    Some(base) => {
                        self.base = base;
                        settings().last_option = Some(base);
                        self.step = ScaleStep::Height;
                        CmdResult::NeedPoint
                    }
                    None => {
                        self.retry = true;
                        invalid_keyword()
                    }
                }
            }
            ScaleStep::Height | ScaleStep::Paper => match number(text) {
                Some(v) if v > 0.0 => {
                    if self.step == ScaleStep::Paper {
                        settings().scale_height = Some(v);
                        self.finish("P", v)
                    } else {
                        self.set_height(v)
                    }
                }
                Some(_) => positive_error(),
                None if self.step == ScaleStep::Height && "PAPER".starts_with(word.as_str()) => {
                    self.step = ScaleStep::Paper;
                    CmdResult::NeedPoint
                }
                None if self.step == ScaleStep::Height && "MATCH".starts_with(word.as_str()) => {
                    self.step = ScaleStep::Match;
                    CmdResult::NeedPoint
                }
                None if self.step == ScaleStep::Height && "SCALE".starts_with(word.as_str()) => {
                    self.step = ScaleStep::Factor;
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError(crate::tr!("text-edit", "numeric-distance")),
            },
            ScaleStep::Factor => match number(text) {
                Some(v) if v > 0.0 => self.set_factor(v),
                Some(_) => positive_error(),
                None if "REFERENCE".starts_with(word.as_str()) => {
                    self.step = ScaleStep::RefLength;
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError(crate::tr!("text-edit", "numeric-distance")),
            },
            ScaleStep::RefLength => match number(text) {
                Some(v) if v > 0.0 => {
                    self.ref_length = v;
                    self.step = ScaleStep::NewLength;
                    CmdResult::NeedPoint
                }
                Some(_) => positive_error(),
                None => CmdResult::ReportError(crate::tr!("text-edit", "numeric-distance")),
            },
            ScaleStep::NewLength => match number(text) {
                Some(v) if v > 0.0 => self.set_factor(v / self.ref_length),
                Some(_) => positive_error(),
                None => CmdResult::ReportError(crate::tr!("text-edit", "numeric-distance")),
            },
            ScaleStep::Match | ScaleStep::RefSecond => return None,
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            ScaleStep::RefLength => {
                self.ref_first = Some(pt);
                self.step = ScaleStep::RefSecond;
                CmdResult::NeedPoint
            }
            ScaleStep::RefSecond => {
                let length = self.ref_first.map_or(0.0, |p| p.distance(pt));
                if length <= 1e-12 {
                    return positive_error();
                }
                self.ref_length = length;
                self.step = ScaleStep::NewLength;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            ScaleStep::Base => {
                let base = Self::default_base();
                self.base = base;
                settings().last_option = Some(base);
                self.step = ScaleStep::Height;
                CmdResult::NeedPoint
            }
            ScaleStep::Height => self.set_height(self.default_height),
            ScaleStep::Paper => self.finish("P", self.default_height),
            ScaleStep::Factor => {
                let factor = settings().scale_factor;
                self.set_factor(factor)
            }
            ScaleStep::RefLength => {
                self.step = ScaleStep::NewLength;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }
}

/// A value the way the prompts show a default (no trailing zeros).
pub fn fmt_number(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

// ── TEXTALIGN ──────────────────────────────────────────────────────────────

const ALIGN_OPTIONS: [Justify; 12] = [
    Justify::Left,
    Justify::Center,
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

/// The corner of each text TEXTALIGN lines up: Left, Center and Right work
/// on the top of the box (measured with Left).
pub fn align_anchor(align: Justify) -> Justify {
    match align {
        Justify::Left => Justify::TL,
        Justify::Center => Justify::TC,
        Justify::Right => Justify::TR,
        other => other,
    }
}

fn spacing_label(spacing: Spacing, value: f64) -> String {
    match spacing {
        Spacing::Distribute => crate::tr!("text-edit", "spacing-distribute"),
        Spacing::Set => crate::tr!("text-edit", "spacing-set", value = format!("{value:.6}")),
        Spacing::CurrentVertical => crate::tr!("text-edit", "spacing-vertical"),
        Spacing::CurrentHorizontal => crate::tr!("text-edit", "spacing-horizontal"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlignStep {
    Select,
    Alignment,
    Options,
    SetSpacing,
    Target,
    FirstPoint,
    SecondPoint,
    SecondOptions,
    SecondSetSpacing,
}

/// TEXTALIGN: line the selected text up on a text or along two points.
pub struct TextAlignCommand {
    step: AlignStep,
    handles: Vec<Handle>,
    target: Option<Handle>,
    first: Option<DVec3>,
}

impl TextAlignCommand {
    pub fn new() -> Self {
        Self {
            step: AlignStep::Select,
            handles: Vec::new(),
            target: None,
            first: None,
        }
    }

    /// `Current settings: Align = Left, Spacing Mode = Current Vertical`.
    pub fn current_settings() -> String {
        let s = settings();
        crate::tr!(
            "text-edit",
            "align-settings",
            align = s.align.label(),
            spacing = spacing_label(s.spacing, s.set_spacing)
        )
    }

    fn spacing_mode() -> String {
        let s = settings();
        crate::tr!("text-edit", "align-spacing-mode", spacing = spacing_label(s.spacing, s.set_spacing))
    }

    fn back_to_select(&mut self) -> CmdResult {
        self.step = AlignStep::Select;
        CmdResult::ReportMeasurement(Self::current_settings())
    }

    fn finish(&mut self, second: DVec3) -> CmdResult {
        let first = self.first.map(|p| [p.x, p.y]);
        let request = serde_json::json!({
            "handles": hex(&self.handles),
            "target": self.target.map(|h| format!("{:X}", h.value())),
            "first": first,
            "second": [second.x, second.y],
        });
        CmdResult::Dispatch(format!("_TEXTALIGNAPPLY {request}"))
    }

    /// The spacing option typed at `Enter an option`.
    fn spacing_option(word: &str) -> Option<Spacing> {
        if word.is_empty() {
            return None;
        }
        Some(match word {
            _ if "DISTRIBUTE".starts_with(word) => Spacing::Distribute,
            _ if "SET".starts_with(word) || "SET SPACING".starts_with(word) => Spacing::Set,
            _ if "VERTICAL".starts_with(word) || "CURRENT VERTICAL".starts_with(word) => {
                Spacing::CurrentVertical
            }
            _ if "HORIZONTAL".starts_with(word) || "CURRENT HORIZONTAL".starts_with(word) => {
                Spacing::CurrentHorizontal
            }
            _ => return None,
        })
    }

    fn after_options(&mut self) -> CmdResult {
        if self.step == AlignStep::SecondOptions || self.step == AlignStep::SecondSetSpacing {
            self.step = AlignStep::SecondPoint;
            return CmdResult::ReportMeasurement(Self::spacing_mode());
        }
        self.back_to_select()
    }
}

impl Default for TextAlignCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for TextAlignCommand {
    fn name(&self) -> &'static str {
        "TEXTALIGN"
    }

    fn prompt(&self) -> String {
        match self.step {
            AlignStep::Select => crate::tr!("text-edit", "align-select"),
            AlignStep::Alignment => crate::tr!(
                "text-edit",
                "align-direction",
                default = settings().align.label()
            ),
            AlignStep::Options | AlignStep::SecondOptions => {
                let s = settings();
                let default = match s.spacing {
                    Spacing::Distribute => "Distribute",
                    Spacing::Set => "Set spacing",
                    Spacing::CurrentVertical => "current Vertical",
                    Spacing::CurrentHorizontal => "current Horizontal",
                };
                crate::tr!("text-edit", "align-options", default = default)
            }
            AlignStep::SetSpacing | AlignStep::SecondSetSpacing => crate::tr!(
                "text-edit",
                "align-set-spacing",
                default = format!("{:.6}", settings().set_spacing)
            ),
            AlignStep::Target => crate::tr!("text-edit", "align-target"),
            AlignStep::FirstPoint => crate::tr!("text-edit", "align-first-point"),
            AlignStep::SecondPoint => crate::tr!("text-edit", "align-second-point"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            AlignStep::Select => vec![
                CmdOption::new("alIgnment", "I"),
                CmdOption::new("Options", "O"),
            ],
            AlignStep::Alignment => ALIGN_OPTIONS
                .iter()
                .map(|j| CmdOption::new(j.label(), j.label()))
                .collect(),
            AlignStep::Options | AlignStep::SecondOptions => vec![
                CmdOption::new("Distribute", "D"),
                CmdOption::new("Set spacing", "S"),
                CmdOption::new("current Vertical", "V"),
                CmdOption::new("current Horizontal", "H"),
            ],
            AlignStep::Target => vec![CmdOption::new("Point", "P")],
            AlignStep::SecondPoint => vec![CmdOption::new("Options", "O")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            AlignStep::Alignment | AlignStep::Options | AlignStep::SecondOptions => {
                InputKind::SingleToken
            }
            _ => InputKind::Point,
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == AlignStep::Select
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(is_text)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if self.step == AlignStep::Select {
            self.handles = handles;
        }
        CmdResult::NeedPoint
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == AlignStep::Target
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.step == AlignStep::Target
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.step != AlignStep::Target || handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        self.step = AlignStep::SecondPoint;
        CmdResult::ReportMeasurement(Self::spacing_mode())
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = keyword(text);
        if word.is_empty() {
            return Some(self.on_enter());
        }
        Some(match self.step {
            AlignStep::Select => match word.as_str() {
                "I" | "ALIGNMENT" => {
                    self.step = AlignStep::Alignment;
                    CmdResult::NeedPoint
                }
                _ if "OPTIONS".starts_with(word.as_str()) => {
                    self.step = AlignStep::Options;
                    CmdResult::NeedPoint
                }
                _ => return None,
            },
            AlignStep::Alignment => match Justify::parse(&word).filter(|j| ALIGN_OPTIONS.contains(j)) {
                Some(j) => {
                    settings().align = j;
                    self.back_to_select()
                }
                None => invalid_keyword(),
            },
            AlignStep::Options | AlignStep::SecondOptions => match Self::spacing_option(&word) {
                Some(Spacing::Set) => {
                    self.step = if self.step == AlignStep::Options {
                        AlignStep::SetSpacing
                    } else {
                        AlignStep::SecondSetSpacing
                    };
                    CmdResult::NeedPoint
                }
                Some(spacing) => {
                    settings().spacing = spacing;
                    self.after_options()
                }
                None => invalid_keyword(),
            },
            AlignStep::SetSpacing | AlignStep::SecondSetSpacing => match number(text) {
                Some(v) if v >= 0.0 => {
                    let mut s = settings();
                    s.spacing = Spacing::Set;
                    s.set_spacing = v;
                    drop(s);
                    self.after_options()
                }
                _ => CmdResult::ReportError(crate::tr!("text-edit", "numeric-distance")),
            },
            AlignStep::Target if "POINT".starts_with(word.as_str()) => {
                self.step = AlignStep::FirstPoint;
                CmdResult::NeedPoint
            }
            AlignStep::SecondPoint if "OPTIONS".starts_with(word.as_str()) => {
                self.step = AlignStep::SecondOptions;
                CmdResult::NeedPoint
            }
            _ => return None,
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            AlignStep::FirstPoint => {
                self.first = Some(pt);
                self.step = AlignStep::SecondPoint;
                CmdResult::ReportMeasurement(Self::spacing_mode())
            }
            AlignStep::SecondPoint => self.finish(pt),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            AlignStep::Select if self.handles.is_empty() => CmdResult::Cancel,
            AlignStep::Select => {
                self.step = AlignStep::Target;
                CmdResult::NeedPoint
            }
            AlignStep::Alignment => self.back_to_select(),
            AlignStep::Options | AlignStep::SecondOptions => self.after_options(),
            AlignStep::SetSpacing | AlignStep::SecondSetSpacing => {
                settings().spacing = Spacing::Set;
                self.after_options()
            }
            _ => CmdResult::NeedPoint,
        }
    }
}

/// Where TEXTALIGN puts each anchor, in the order of `anchors` (each the
/// anchor point and the box's width and height): along the line from
/// `first` towards `second`.
pub fn align_positions(anchors: &[([f64; 2], f64, f64)], first: [f64; 2], second: [f64; 2]) -> Vec<[f64; 2]> {
    let s = settings();
    let (spacing, gap) = (s.spacing, s.set_spacing);
    drop(s);
    let d = [second[0] - first[0], second[1] - first[1]];
    let len = d[0].hypot(d[1]);
    let u = if len > 1e-12 { [d[0] / len, d[1] / len] } else { [0.0, -1.0] };
    let project = |p: [f64; 2]| {
        let t = (p[0] - first[0]) * u[0] + (p[1] - first[1]) * u[1];
        [first[0] + u[0] * t, first[1] + u[1] * t]
    };
    // The texts in their order along the line.
    let mut order: Vec<usize> = (0..anchors.len()).collect();
    let along = |p: [f64; 2]| (p[0] - first[0]) * u[0] + (p[1] - first[1]) * u[1];
    order.sort_by(|&a, &b| along(anchors[a].0).total_cmp(&along(anchors[b].0)));
    let mut out = vec![[0.0; 2]; anchors.len()];
    match spacing {
        Spacing::CurrentVertical | Spacing::CurrentHorizontal => {
            for (k, (p, _, _)) in anchors.iter().enumerate() {
                out[k] = if spacing == Spacing::CurrentVertical && d[1].abs() > 1e-12 {
                    [first[0] + d[0] * (p[1] - first[1]) / d[1], p[1]]
                } else if spacing == Spacing::CurrentHorizontal && d[0].abs() > 1e-12 {
                    [p[0], first[1] + d[1] * (p[0] - first[0]) / d[0]]
                } else {
                    project(*p)
                };
            }
        }
        Spacing::Distribute => {
            let n = order.len();
            for (rank, &k) in order.iter().enumerate() {
                let t = if n > 1 { rank as f64 / (n - 1) as f64 } else { 0.0 };
                out[k] = [first[0] + d[0] * t, first[1] + d[1] * t];
            }
        }
        Spacing::Set => {
            // Each box follows the previous one with `gap` between them,
            // measured across the boxes along the line's main axis.
            let vertical = u[1].abs() >= u[0].abs();
            let mut at = first;
            for &k in &order {
                out[k] = at;
                let (_, w, h) = anchors[k];
                let step = if vertical { (h + gap) / u[1].abs() } else { (w + gap) / u[0].abs() };
                at = [at[0] + u[0] * step, at[1] + u[1] * step];
            }
        }
    }
    out
}

/// The box size TEXTALIGN spaces by: a TEXT counts from its baseline up
/// (measured: 2 high text 4 apart steps by 6).
pub fn frame_size(frame: &TextFrame) -> (f64, f64) {
    (frame.width(), frame.top - frame.base)
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["JUSTIFYTEXT", "SCALETEXT", "TEXTALIGN"]
});
