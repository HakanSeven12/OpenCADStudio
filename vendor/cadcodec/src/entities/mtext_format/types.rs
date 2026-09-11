//! Data types representing parsed MTEXT formatted content.
//!
//! MTEXT uses an RTF-like formatting syntax enclosed in braces `{...}`.
//! This module provides a structured representation of the parsed content
//! as paragraphs containing styled text spans.
//!
//! For the full specification of supported control codes, see the [`mtext_format`] module.
//!
//! [`mtext_format`]: crate::entities::mtext_format

use std::fmt::Write;

// ============================================================================
// Special Characters
// ============================================================================

/// AutoCAD special character codes (`%%...`).
///
/// These are escape sequences in MTEXT/TEXT strings that represent special symbols.
/// Only `%%c` (diameter Ø), `%%d` (degree °), and `%%p` (plus-minus ±) are
/// valid special characters. `%%%%` produces a literal percent sign.
///
/// Note: `%%o`, `%%u`, `%%k` are TEXT entity formatting codes and are NOT
/// valid in MTEXT. MTEXT uses `\L`/`\l`, `\O`/`\o`, `\K`/`\k` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpecialChar {
    /// Degree symbol (°) — `%%d`
    Degree,
    /// Plus-minus symbol (±) — `%%p`
    PlusMinus,
    /// Diameter symbol (Ø) — `%%c`
    Diameter,
    /// Literal percent sign (%) — `%%%%`
    Percent,
}

impl SpecialChar {
    /// The character this special code renders as.
    pub fn to_char(self) -> char {
        match self {
            SpecialChar::Degree => '°',
            SpecialChar::PlusMinus => '±',
            SpecialChar::Diameter => 'Ø',
            SpecialChar::Percent => '%',
        }
    }

    /// Parse from the single character following `%%`.
    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            'd' | 'D' => Some(SpecialChar::Degree),
            'p' | 'P' => Some(SpecialChar::PlusMinus),
            'c' | 'C' => Some(SpecialChar::Diameter),
            '%' => Some(SpecialChar::Percent),
            _ => None,
        }
    }

    /// Serialize back to the `%%...` string.
    pub fn to_string(self) -> &'static str {
        match self {
            SpecialChar::Degree => "%%d",
            SpecialChar::PlusMinus => "%%p",
            SpecialChar::Diameter => "%%c",
            SpecialChar::Percent => "%%%%",
        }
    }
}

impl Default for SpecialChar {
    fn default() -> Self {
        SpecialChar::Degree
    }
}

// ============================================================================
// Stroke Flags (Underline, Overline, Strikethrough)
// ============================================================================

/// Bitflags for text stroke decorations in MTEXT.
///
/// Multiple strokes can be active simultaneously (e.g. underline + strikethrough).
/// Set via `\L`/`\l` (underline), `\O`/`\o` (overline), `\K`/`\k` (strikethrough).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextStroke {
    bits: u8,
}

impl MTextStroke {
    const UNDERLINE: u8 = 1;
    const STRIKE_THROUGH: u8 = 2;
    const OVERLINE: u8 = 4;

    pub fn new() -> Self {
        Self { bits: 0 }
    }

    #[inline]
    pub fn underline(&self) -> bool {
        self.bits & Self::UNDERLINE != 0
    }

    #[inline]
    pub fn set_underline(&mut self, value: bool) {
        if value {
            self.bits |= Self::UNDERLINE;
        } else {
            self.bits &= !Self::UNDERLINE;
        }
    }

    #[inline]
    pub fn strikethrough(&self) -> bool {
        self.bits & Self::STRIKE_THROUGH != 0
    }

    #[inline]
    pub fn set_strikethrough(&mut self, value: bool) {
        if value {
            self.bits |= Self::STRIKE_THROUGH;
        } else {
            self.bits &= !Self::STRIKE_THROUGH;
        }
    }

    #[inline]
    pub fn overline(&self) -> bool {
        self.bits & Self::OVERLINE != 0
    }

    #[inline]
    pub fn set_overline(&mut self, value: bool) {
        if value {
            self.bits |= Self::OVERLINE;
        } else {
            self.bits &= !Self::OVERLINE;
        }
    }

    #[inline]
    pub fn has_any(&self) -> bool {
        self.bits != 0
    }
}

// ============================================================================
// Paragraph Alignment
// ============================================================================

/// Paragraph alignment from `\p...q<letter>...;` codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextParagraphAlignment {
    #[default]
    Default = 0,
    Left = 1,
    Right = 2,
    Center = 3,
    Justified = 4,
    Distributed = 5,
}

impl MTextParagraphAlignment {
    /// Parse from the character after `q` in `\p...q<char>...;`
    pub fn from_char(ch: char) -> Self {
        match ch.to_ascii_lowercase() {
            'l' => MTextParagraphAlignment::Left,
            'r' => MTextParagraphAlignment::Right,
            'c' => MTextParagraphAlignment::Center,
            'j' => MTextParagraphAlignment::Justified,
            'd' => MTextParagraphAlignment::Distributed,
            // AutoCAD serializes the paragraph *Justify* option as `\pq*` (the
            // `*` that means "reset" in the other sub-codes), so a `q*` reads as
            // justified rather than an alignment reset.
            '*' => MTextParagraphAlignment::Justified,
            _ => MTextParagraphAlignment::Default,
        }
    }
}

// ============================================================================
// Line Alignment (Vertical within line)
// ============================================================================

/// Vertical alignment of text within a line from `\A<n>;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextLineAlignment {
    /// Bottom / baseline (default)
    #[default]
    Bottom = 0,
    /// Middle / center
    Middle = 1,
    /// Top
    Top = 2,
}

impl MTextLineAlignment {
    /// Parse from numeric code.
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => MTextLineAlignment::Middle,
            2 => MTextLineAlignment::Top,
            _ => MTextLineAlignment::Bottom,
        }
    }
}

// ============================================================================
// Stacking Type (for \S fractions/limits)
// ============================================================================

/// The visual stacking style for fractions and limits from `\S<n>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StackingType {
    /// Horizontal fraction with a line: `\S<num>/<den>;`
    #[default]
    Horizontal = 0,
    /// Diagonal fraction: `\S<num>#<den>;`
    Diagonal = 1,
    /// Stacked limit (no line): `\S<num>^<den>;`
    Limit = 2,
}

impl StackingType {
    /// Parse from the separator character.
    pub fn from_char(ch: char) -> Self {
        match ch {
            '/' => StackingType::Horizontal,
            '#' => StackingType::Diagonal,
            '^' => StackingType::Limit,
            _ => StackingType::Horizontal,
        }
    }
}

/// Data for a stacking (fraction/limit) expression from `\S`.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StackingData {
    /// Text above the line / first element
    pub numerator: String,
    /// Text below the line / second element
    pub denominator: String,
    /// Visual stacking style
    pub stacking_type: StackingType,
}

impl StackingData {
    pub fn is_empty(&self) -> bool {
        self.numerator.is_empty() && self.denominator.is_empty()
    }

    pub fn to_plain_text(&self) -> String {
        match self.stacking_type {
            StackingType::Horizontal => format!("{}/{}", self.numerator, self.denominator),
            StackingType::Diagonal => format!("{}/{}", self.numerator, self.denominator),
            StackingType::Limit => format!("{} {}", self.numerator, self.denominator),
        }
    }
}

// ============================================================================
// Paragraph Properties
// ============================================================================

/// Paragraph-level properties parsed from `\p...;` codes.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParagraphProperties {
    /// Paragraph alignment from `q<char>`.
    pub alignment: Option<MTextParagraphAlignment>,
    /// First-line indent from `i<n>`.
    pub first_line_indent: Option<f64>,
    /// Left margin from `l<n>`.
    pub left_margin: Option<f64>,
    /// Right margin from `r<n>`.
    pub right_margin: Option<f64>,
    /// Spacing before paragraph from `b<n>` (inches).
    pub spacing_before: Option<f64>,
    /// Spacing after paragraph from `a<n>` (inches).
    pub spacing_after: Option<f64>,
    /// Line spacing from `se<n>` (exact spacing in inches) or `sm<n>` (multiple of font height).
    pub line_spacing: Option<MTextLineSpacing>,
    /// Tab stops from `t<n>` (comma-separated, each optionally prefixed by
    /// `c`/`r`/`D` for center/right/decimal alignment).
    pub tab_stops: Vec<TabStop>,
}

/// Line spacing mode from `se` (exact) or `sm` (multiple) inside `\p...;`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextLineSpacing {
    /// Default (automatic line spacing).
    #[default]
    Default,
    /// Exact spacing in inches: `se<n>`.
    Exact(f64),
    /// Multiple of font height: `sm<n>`.
    Multiple(f64),
}

// ============================================================================
// Tab Stop
// ============================================================================

/// A tab stop position and alignment. The prefix in `\p…t…;` selects the
/// kind: no prefix = left, `c` = center, `r` = right, `D` = decimal (the text
/// after the tab aligns its decimal point on the stop).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TabStop {
    /// Left-aligned tab at position
    Left(f64),
    /// Center-aligned tab at position
    Center(f64),
    /// Right-aligned tab at position
    Right(f64),
    /// Decimal (dot-aligned) tab at position (`D` prefix)
    Decimal(f64),
}

impl TabStop {
    /// The stop's position along the paragraph.
    pub fn position(&self) -> f64 {
        match self {
            TabStop::Left(p) | TabStop::Center(p) | TabStop::Right(p) | TabStop::Decimal(p) => *p,
        }
    }
}

impl core::fmt::Display for TabStop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TabStop::Left(p) => write!(f, "{}", p),
            TabStop::Center(p) => write!(f, "c{}", p),
            TabStop::Right(p) => write!(f, "r{}", p),
            TabStop::Decimal(p) => write!(f, "D{}", p),
        }
    }
}

// ============================================================================
// Color
// ============================================================================

/// Represents an AutoCAD Color Index (ACI) color, true-color RGB, or ByLayer/ByBlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextColor {
    /// No color specified (inherits from entity or parent)
    None,
    /// AutoCAD Color Index (1–255)
    Index(u16),
    /// True color value as packed 24-bit integer (r << 16 | g << 8 | b)
    TrueColor(u32),
    /// Explicit `\C256;` — use the layer color.
    ByLayer,
    /// Explicit `\C0;` — use the block/entity color.
    ByBlock,
}

impl Default for MTextColor {
    fn default() -> Self {
        MTextColor::None
    }
}

impl MTextColor {
    /// Create from an ACI index (0–256).
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => MTextColor::ByBlock,
            256 => MTextColor::ByLayer,
            1..=255 => MTextColor::Index(index as u16),
            _ => MTextColor::None,
        }
    }

    /// Create from a packed true color value.
    pub fn from_true_color(value: i32) -> Self {
        MTextColor::TrueColor(value as u32)
    }

    /// Create from BGR packed value (as used in `\c<n>;`).
    pub fn from_bgr_packed(packed: u32) -> Self {
        let b = (packed & 0xFF) as u8;
        let g = ((packed >> 8) & 0xFF) as u8;
        let r = ((packed >> 16) & 0xFF) as u8;
        let rgb = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        MTextColor::TrueColor(rgb)
    }

    /// Get the ACI index if this is an index color.
    pub fn as_index(&self) -> Option<u16> {
        match self {
            MTextColor::Index(i) => Some(*i),
            _ => None,
        }
    }

    /// Get the true color value if this is a true color.
    pub fn as_true_color(&self) -> Option<u32> {
        match self {
            MTextColor::TrueColor(v) => Some(*v),
            _ => None,
        }
    }

    /// Get RGB components if this is a true color.
    pub fn as_rgb(&self) -> Option<(u8, u8, u8)> {
        match self {
            MTextColor::TrueColor(v) => {
                let r = ((v >> 16) & 0xFF) as u8;
                let g = ((v >> 8) & 0xFF) as u8;
                let b = (v & 0xFF) as u8;
                Some((r, g, b))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for MTextColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            MTextColor::None => write!(f, "None"),
            MTextColor::Index(i) => write!(f, "Index({})", i),
            MTextColor::TrueColor(v) => write!(f, "TrueColor({})", v),
            MTextColor::ByLayer => write!(f, "ByLayer"),
            MTextColor::ByBlock => write!(f, "ByBlock"),
        }
    }
}

// ============================================================================
// Font
// ============================================================================

/// Font specification: font family name and optional pipe-separated flags.
///
/// Flags the format model does not know about are kept verbatim in
/// [`extra`](Self::extra) so parse→serialize round-trips stay faithful even
/// for flags added by newer AutoCAD versions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextFont {
    /// Font family name (e.g. "Arial", "romans.shx")
    pub name: String,
    /// Font style name (e.g. "Bold Italic", "") — legacy format.
    pub style: String,
    /// Bold flag from `\f...|b1|...;`
    pub bold: bool,
    /// Italic flag from `\f...|i1|...;`
    pub italic: bool,
    /// Whether a `|b0`/`|b1` flag was present (explicit vs inherited).
    #[cfg_attr(feature = "serde", serde(default))]
    pub bold_explicit: bool,
    /// Whether a `|i0`/`|i1` flag was present (explicit vs inherited).
    #[cfg_attr(feature = "serde", serde(default))]
    pub italic_explicit: bool,
    /// Character set / code page from `|c<n>;` (e.g. `134` = GB/Chinese).
    #[cfg_attr(feature = "serde", serde(default))]
    pub charset: Option<u16>,
    /// Pitch / font family from `|p<n>;` (0 = default, 1 = fixed, 2 = variable).
    #[cfg_attr(feature = "serde", serde(default))]
    pub pitch: Option<u8>,
    /// Any other pipe flags (e.g. `|e<n>`) preserved verbatim.
    #[cfg_attr(feature = "serde", serde(default))]
    pub extra: Vec<String>,
}

impl MTextFont {
    /// Create a new font specification.
    pub fn new(name: impl Into<String>, style: impl Into<String>) -> Self {
        MTextFont {
            name: name.into(),
            style: style.into(),
            ..Default::default()
        }
    }

    /// Create from just a font name.
    pub fn from_name(name: impl Into<String>) -> Self {
        MTextFont {
            name: name.into(),
            style: String::new(),
            ..Default::default()
        }
    }

    /// Create with bold/italic flags.
    pub fn with_flags(name: impl Into<String>, bold: bool, italic: bool) -> Self {
        MTextFont {
            name: name.into(),
            style: String::new(),
            bold,
            italic,
            bold_explicit: true,
            italic_explicit: true,
            ..Default::default()
        }
    }
}

// ============================================================================
// Span Properties
// ============================================================================

/// A height set by `\H`: either a multiplier on the current height
/// (`\H<v>x;`, relative) or an absolute drawing-unit height (`\H<v>;`).
///
/// The two are kept distinct because a consumer resolves them differently: an
/// absolute height is divided by the entity's text height to obtain a factor,
/// while a relative factor applies directly. A prior absolute height carries
/// through a following relative factor (`\H5;\H2x;` → `Absolute(10)`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MTextScalar {
    /// Multiplier relative to the current height (`\H<v>x;`).
    Factor(f64),
    /// Absolute drawing-unit height (`\H<v>;`).
    Absolute(f64),
}

impl MTextScalar {
    /// The numeric value, regardless of kind.
    pub fn value(self) -> f64 {
        match self {
            MTextScalar::Factor(v) | MTextScalar::Absolute(v) => v,
        }
    }

    /// `true` for a relative (`\H…x;`) factor.
    pub fn is_relative(self) -> bool {
        matches!(self, MTextScalar::Factor(_))
    }
}

/// Properties applied to a text span.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpanProperties {
    /// Text color (ACI index). `None` means inherit.
    pub color: Option<MTextColor>,

    /// True-color RGB override. When set, overrides `color`.
    pub color_rgb: Option<(u8, u8, u8)>,

    /// Second text color (for gradient effects). `None` means single color.
    pub second_color: Option<MTextColor>,

    /// Font specification. Empty name means inherit.
    pub font: Option<MTextFont>,

    /// Height from `\H`: a relative factor (`\H<v>x;`) or an absolute
    /// drawing-unit height (`\H<v>;`). `None` means inherit (default 1.0).
    pub height: Option<MTextScalar>,

    /// Stroke decorations (underline, overline, strikethrough).
    pub stroke: MTextStroke,

    /// Vertical alignment within the line from `\A<n>;`.
    /// `None` means bottom (baseline).
    pub line_align: Option<MTextLineAlignment>,

    /// Character tracking in 1/1000th of character width.
    /// Positive = spread apart, negative = condensed.
    pub tracking: Option<f64>,

    /// Character width factor. 1.0 = normal, >1.0 = wider.
    pub width_factor: Option<f64>,

    /// Oblique angle in degrees. Positive = lean right.
    pub oblique_angle: Option<f64>,
}

impl SpanProperties {
    /// Returns `true` if all properties are unset/default.
    pub fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.color_rgb.is_none()
            && self.second_color.is_none()
            && self.font.is_none()
            && self.height.is_none()
            && !self.stroke.has_any()
            && self.line_align.is_none()
            && self.tracking.is_none()
            && self.width_factor.is_none()
            && self.oblique_angle.is_none()
    }

    /// Merge another set of properties into this one, overriding only the non-default values.
    pub fn merge(&mut self, other: &SpanProperties) {
        if let Some(ref c) = other.color {
            self.color = Some(*c);
        }
        if let Some(ref c) = other.color_rgb {
            self.color_rgb = Some(*c);
        }
        if let Some(ref c) = other.second_color {
            self.second_color = Some(*c);
        }
        if let Some(ref f) = other.font {
            self.font = Some(f.clone());
        }
        if let Some(h) = other.height {
            self.height = Some(h);
        }
        // Stroke flags
        if other.stroke.underline() {
            self.stroke.set_underline(true);
        }
        if other.stroke.strikethrough() {
            self.stroke.set_strikethrough(true);
        }
        if other.stroke.overline() {
            self.stroke.set_overline(true);
        }
        if let Some(v) = other.line_align {
            self.line_align = Some(v);
        }
        if let Some(v) = other.tracking {
            self.tracking = Some(v);
        }
        if let Some(v) = other.width_factor {
            self.width_factor = Some(v);
        }
        if let Some(v) = other.oblique_angle {
            self.oblique_angle = Some(v);
        }
    }

    // ── Convenience accessors ──

    /// Whether underline is active.
    pub fn underline(&self) -> bool {
        self.stroke.underline()
    }

    /// Whether overline is active.
    pub fn overline(&self) -> bool {
        self.stroke.overline()
    }

    /// Whether strikethrough is active.
    pub fn strikethrough(&self) -> bool {
        self.stroke.strikethrough()
    }

    /// Set underline.
    pub fn set_underline(&mut self, value: bool) {
        self.stroke.set_underline(value);
    }

    /// Set overline.
    pub fn set_overline(&mut self, value: bool) {
        self.stroke.set_overline(value);
    }

    /// Set strikethrough.
    pub fn set_strikethrough(&mut self, value: bool) {
        self.stroke.set_strikethrough(value);
    }
}

// ============================================================================
// MTextSpan
// ============================================================================

/// A styled text segment within a paragraph.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextSpan {
    /// The text content of this span.
    pub text: String,

    /// Formatting properties applied to this span.
    pub properties: SpanProperties,

    /// Stacking data (for fractions/limits). Present only when this span
    /// represents a stacking element from `\S`.
    pub stacking: Option<StackingData>,
}

impl MTextSpan {
    /// Create a new span with text and properties.
    pub fn new(text: impl Into<String>, properties: SpanProperties) -> Self {
        MTextSpan {
            text: text.into(),
            properties,
            stacking: None,
        }
    }

    /// Create a plain span with no special formatting.
    pub fn plain(text: impl Into<String>) -> Self {
        MTextSpan {
            text: text.into(),
            properties: SpanProperties::default(),
            stacking: None,
        }
    }

    /// Create a stacking span with the given properties.
    pub fn stacking(
        text: impl Into<String>,
        properties: SpanProperties,
        stacking: StackingData,
    ) -> Self {
        MTextSpan {
            text: text.into(),
            properties,
            stacking: Some(stacking),
        }
    }

    /// Returns `true` if this span has no special formatting.
    pub fn is_plain(&self) -> bool {
        self.properties.is_empty() && self.stacking.is_none()
    }
}

// ============================================================================
// MTextParagraph
// ============================================================================

/// A paragraph within an MTEXT document.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextParagraph {
    /// Paragraph-level properties (alignment, indent, margins).
    pub properties: ParagraphProperties,

    /// Styled text spans in this paragraph.
    pub spans: Vec<MTextSpan>,

    /// Whether this paragraph opens a new column — the `\N` break, as opposed
    /// to `\P`, which only starts a new line.
    ///
    /// The two are otherwise identical (span properties carry across both), so
    /// without this the distinction is gone the moment the string is parsed:
    /// a reader lays the columns out as one stacked run, and
    /// [`to_mtext_string`](MTextDocument::to_mtext_string) writes every break
    /// back as `\P`, quietly collapsing a multi-column MTEXT on round-trip.
    pub starts_column: bool,
}

impl MTextParagraph {
    /// Create a new empty paragraph.
    pub fn new() -> Self {
        MTextParagraph {
            properties: ParagraphProperties::default(),
            spans: Vec::new(),
            starts_column: false,
        }
    }

    /// Create a paragraph from plain text.
    pub fn from_text(text: impl Into<String>) -> Self {
        MTextParagraph {
            properties: ParagraphProperties::default(),
            spans: vec![MTextSpan::plain(text)],
            starts_column: false,
        }
    }

    /// Returns `true` if the paragraph contains no spans.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Get the plain text content of this paragraph (concatenating all spans).
    /// For stacking spans, derives text from the stacking data.
    pub fn to_plain_text(&self) -> String {
        self.spans
            .iter()
            .map(|s| {
                if let Some(ref stack) = s.stacking {
                    stack.to_plain_text()
                } else {
                    s.text.clone()
                }
            })
            .collect()
    }

    /// Add a span to the end of the paragraph.
    pub fn push_span(&mut self, span: MTextSpan) {
        self.spans.push(span);
    }

    /// Try to merge the last span into `new_span` if they share the same properties.
    pub fn push_span_merged(&mut self, new_span: MTextSpan) {
        if let Some(last) = self.spans.last_mut() {
            if last.properties == new_span.properties
                && last.stacking.is_none()
                && new_span.stacking.is_none()
            {
                last.text.push_str(&new_span.text);
                return;
            }
        }
        self.spans.push(new_span);
    }
}

impl Default for MTextParagraph {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MTextDocument
// ============================================================================

/// The root type representing a fully parsed MTEXT document.
///
/// An MTEXT document consists of one or more paragraphs, each containing
/// zero or more styled text spans.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextDocument {
    /// The paragraphs in this document.
    pub paragraphs: Vec<MTextParagraph>,
}

/// Returns `true` if the paragraph properties contain any non-default values
/// that should be serialized into `\p...;` format.
fn para_has_props(p: &ParagraphProperties) -> bool {
    p.alignment
        .as_ref()
        .map_or(false, |a| !matches!(a, MTextParagraphAlignment::Default))
        || p.first_line_indent.is_some()
        || p.left_margin.is_some()
        || p.right_margin.is_some()
        || p.spacing_before.is_some()
        || p.spacing_after.is_some()
        || p.line_spacing.is_some()
        || !p.tab_stops.is_empty()
}

impl MTextDocument {
    /// Create a new empty document.
    pub fn new() -> Self {
        MTextDocument {
            paragraphs: Vec::new(),
        }
    }

    /// Create a single-paragraph document from plain text.
    pub fn from_text(text: impl Into<String>) -> Self {
        MTextDocument {
            paragraphs: vec![MTextParagraph::from_text(text)],
        }
    }

    /// Returns `true` if the document has no paragraphs.
    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }

    /// Get the total plain text content (paragraphs joined by newlines).
    pub fn to_plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.to_plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get the total number of spans across all paragraphs.
    pub fn span_count(&self) -> usize {
        self.paragraphs.iter().map(|p| p.spans.len()).sum()
    }

    /// Returns `true` if ALL paragraphs contain only plain spans.
    pub fn is_all_plain(&self) -> bool {
        self.paragraphs
            .iter()
            .all(|p| p.spans.iter().all(|s| s.is_plain()))
    }

    /// Add a paragraph to the end of the document.
    pub fn push_paragraph(&mut self, paragraph: MTextParagraph) {
        self.paragraphs.push(paragraph);
    }

    /// Serialize this document back to an MTEXT format string.
    pub fn to_mtext_string(&self) -> String {
        // Simple case: single paragraph with only plain text and no paragraph properties
        if self.paragraphs.len() == 1 {
            let para = &self.paragraphs[0];
            let has_para_props = para_has_props(&para.properties);
            if para.spans.iter().all(|s| s.is_plain()) && !has_para_props {
                let text = self.to_plain_text();
                if text.contains('{') || text.contains('}') || text.contains('\\') {
                    let mut escaped = String::with_capacity(text.len() + 4);
                    escaped.push('{');
                    for ch in text.chars() {
                        match ch {
                            '\\' => escaped.push_str("\\\\"),
                            '{' => escaped.push_str("\\{"),
                            '}' => escaped.push_str("\\}"),
                            _ => escaped.push(ch),
                        }
                    }
                    escaped.push('}');
                    return escaped;
                }
                return text;
            }
        }

        let mut result = String::from("{");

        for (pi, paragraph) in self.paragraphs.iter().enumerate() {
            if pi > 0 {
                // `\N` opens a column, `\P` only a line — writing `\P` for both
                // would flatten a multi-column MTEXT every time it round-trips.
                result.push_str(if paragraph.starts_column {
                    "\\N"
                } else {
                    "\\P"
                });
            }

            // Emit paragraph properties
            let para_props = &paragraph.properties;
            if para_has_props(para_props) {
                result.push_str("\\p");
                if let Some(ref align) = para_props.alignment {
                    match align {
                        MTextParagraphAlignment::Left => result.push_str("ql"),
                        MTextParagraphAlignment::Right => result.push_str("qr"),
                        MTextParagraphAlignment::Center => result.push_str("qc"),
                        MTextParagraphAlignment::Justified => result.push_str("qj"),
                        MTextParagraphAlignment::Distributed => result.push_str("qd"),
                        MTextParagraphAlignment::Default => {}
                    }
                }
                if let Some(v) = para_props.first_line_indent {
                    write!(result, ",i{}", v).ok();
                }
                if let Some(v) = para_props.left_margin {
                    write!(result, ",l{}", v).ok();
                }
                if let Some(v) = para_props.right_margin {
                    write!(result, ",r{}", v).ok();
                }
                if let Some(v) = para_props.spacing_before {
                    write!(result, ",b{}", v).ok();
                }
                if let Some(v) = para_props.spacing_after {
                    write!(result, ",a{}", v).ok();
                }
                if let Some(ref ls) = para_props.line_spacing {
                    match ls {
                        MTextLineSpacing::Default => result.push_str(",s*"),
                        MTextLineSpacing::Exact(v) => {
                            write!(result, ",se{}", v).ok();
                        }
                        MTextLineSpacing::Multiple(v) => {
                            write!(result, ",sm{}", v).ok();
                        }
                    }
                }
                if !para_props.tab_stops.is_empty() {
                    result.push_str(",t");
                    for (idx, ts) in para_props.tab_stops.iter().enumerate() {
                        if idx > 0 {
                            result.push(',');
                        }
                        write!(result, "{}", ts).ok();
                    }
                }
                result.push(';');
            }

            for span in &paragraph.spans {
                // Each styled span gets its own group. Closing the group restores
                // every inherited MTEXT property at once; relying on stateful
                // transitions cannot express resets for color, font, height,
                // tracking, width, oblique angle, or line alignment and makes a
                // preceding span's formatting leak into the following text.
                let scoped = !span.properties.is_empty();
                if scoped {
                    result.push('{');
                    Self::emit_properties(
                        &mut result,
                        &SpanProperties::default(),
                        &span.properties,
                    );
                }

                if let Some(ref stack) = span.stacking {
                    let sep = match stack.stacking_type {
                        StackingType::Horizontal => '/',
                        StackingType::Diagonal => '#',
                        StackingType::Limit => '^',
                    };
                    write!(
                        result,
                        "\\S{}{}{};",
                        stack.numerator, sep, stack.denominator
                    )
                    .ok();
                } else {
                    for ch in span.text.chars() {
                        match ch {
                            '\\' => result.push_str("\\\\"),
                            '{' => result.push_str("\\{"),
                            '}' => result.push_str("\\}"),
                            _ => result.push(ch),
                        }
                    }
                }
                if scoped {
                    result.push('}');
                }
            }
        }

        result.push('}');
        result
    }

    fn emit_properties(
        result: &mut String,
        current_props: &SpanProperties,
        props: &SpanProperties,
    ) {
        // Color (ACI) — only emit when changed
        if props.color != current_props.color {
            if let Some(ref color) = props.color {
                result.push_str("\\C");
                match color {
                    MTextColor::Index(i) => {
                        write!(result, "{};", i).ok();
                    }
                    MTextColor::TrueColor(v) => {
                        write!(result, "{};", v).ok();
                    }
                    MTextColor::ByLayer => {
                        result.push_str("256;");
                    }
                    MTextColor::ByBlock => {
                        result.push_str("0;");
                    }
                    MTextColor::None => {
                        result.push_str("0;");
                    }
                }
            }
        }

        // Second color — only emit when changed
        if props.second_color != current_props.second_color {
            if let Some(ref sc) = props.second_color {
                match sc {
                    MTextColor::Index(i) => {
                        write!(result, "{};", i).ok();
                    }
                    MTextColor::TrueColor(v) => {
                        write!(result, "{};", v).ok();
                    }
                    MTextColor::ByLayer => {
                        result.push_str("256;");
                    }
                    MTextColor::ByBlock => {
                        result.push_str("0;");
                    }
                    MTextColor::None => {
                        result.push(';');
                    }
                }
            }
        }

        // True-color RGB — only emit when changed
        if props.color_rgb != current_props.color_rgb {
            if let Some((r, g, b)) = props.color_rgb {
                let packed: u32 = (r as u32) | ((g as u32) << 8) | ((b as u32) << 16);
                write!(result, "\\c{};", packed).ok();
            }
        }

        // Font — only emit when changed
        if props.font.as_ref() != current_props.font.as_ref() {
            if let Some(ref font) = props.font {
                if !font.name.is_empty() {
                    result.push_str("\\f");
                    result.push_str(&font.name);
                    if font.bold_explicit {
                        result.push_str(if font.bold { "|b1" } else { "|b0" });
                    }
                    if font.italic_explicit {
                        result.push_str(if font.italic { "|i1" } else { "|i0" });
                    }
                    if let Some(cs) = font.charset {
                        result.push('|');
                        result.push('c');
                        result.push_str(&cs.to_string());
                    }
                    if let Some(p) = font.pitch {
                        result.push('|');
                        result.push('p');
                        result.push_str(&p.to_string());
                    }
                    if !font.style.is_empty() {
                        result.push('|');
                        result.push_str(&font.style);
                    }
                    for flag in &font.extra {
                        result.push('|');
                        result.push_str(flag);
                    }
                    result.push(';');
                }
            }
        }

        // Height — only emit when changed. A relative `\H…x;` MULTIPLIES the
        // current height: the parser compounds it (`\H0.2x;\H1.25x;` →
        // Factor(0.25)) and stores the cumulative factor. So the serializer must
        // emit the RATIO from the running height to the target, NOT the stored
        // cumulative factor — writing `\H0.25x;` there would compound a second
        // time on re-parse (0.25 × current), shrinking the height on every
        // round-trip until the text collapses toward zero.
        if props.height != current_props.height {
            match (&current_props.height, &props.height) {
                // Factor → Factor: emit target/current so re-parse multiplies
                // back to exactly `target`.
                (Some(MTextScalar::Factor(c)), Some(MTextScalar::Factor(t))) if c.abs() > 1e-12 => {
                    write!(result, "\\H{}x;", t / c).ok();
                }
                // Fresh factor over the implicit 1.0.
                (None, Some(MTextScalar::Factor(t))) => {
                    write!(result, "\\H{}x;", t).ok();
                }
                // Absolute always resets — no compounding, emit it directly.
                (_, Some(MTextScalar::Absolute(t))) => {
                    write!(result, "\\H{};", t).ok();
                }
                // Factor reached from an absolute (or a zero current) can't be
                // expressed by a relative code; emit it as a fresh factor. This
                // mix does not arise from parsed input.
                (_, Some(MTextScalar::Factor(t))) => {
                    write!(result, "\\H{}x;", t).ok();
                }
                (_, None) => {}
            }
        }

        // Stroke decorations — emit ON/OFF codes when the state changes
        if props.stroke.underline() != current_props.stroke.underline() {
            if props.stroke.underline() {
                result.push_str("\\L");
            } else {
                result.push_str("\\l");
            }
        }
        if props.stroke.overline() != current_props.stroke.overline() {
            if props.stroke.overline() {
                result.push_str("\\O");
            } else {
                result.push_str("\\o");
            }
        }
        if props.stroke.strikethrough() != current_props.stroke.strikethrough() {
            if props.stroke.strikethrough() {
                result.push_str("\\K");
            } else {
                result.push_str("\\k");
            }
        }

        // Line alignment — only emit when changed
        if props.line_align != current_props.line_align {
            if let Some(v) = props.line_align {
                write!(result, "\\A{};", v as u8).ok();
            }
        }

        // Tracking — only emit when changed
        if props.tracking != current_props.tracking {
            if let Some(t) = props.tracking {
                write!(result, "\\T{};", t).ok();
            }
        }

        // Width factor — only emit when changed
        if props.width_factor != current_props.width_factor {
            if let Some(w) = props.width_factor {
                write!(result, "\\W{};", w).ok();
            }
        }

        // Oblique angle — only emit when changed
        if props.oblique_angle != current_props.oblique_angle {
            if let Some(a) = props.oblique_angle {
                write!(result, "\\Q{};", a).ok();
            }
        }
    }
}

impl Default for MTextDocument {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::mtext_format::parse_mtext;

    /// Relative `\H…x;` heights compound, so serialize→parse must reproduce the
    /// SAME cumulative factors. Regression for the bug where the serializer
    /// re-emitted the stored cumulative factor as a relative code, compounding
    /// it a second time and shrinking the text toward zero on every round-trip
    /// (budweiser2018.dwg handle 50019 rendered its editor preview at collapsed
    /// / huge sizes because of this).
    #[test]
    fn relative_height_round_trips() {
        let src = "{\\H0.2x;a\\H1.25x;b\\H0.8x;c}";
        let factors = |v: &str| -> Vec<f64> {
            parse_mtext(v, true)
                .paragraphs
                .iter()
                .flat_map(|p| p.spans.iter())
                .filter_map(|s| match s.properties.height {
                    Some(MTextScalar::Factor(f)) => Some(f),
                    _ => None,
                })
                .collect()
        };
        let f0 = factors(src);
        assert_eq!(f0, vec![0.2, 0.25, 0.2], "parse compounds: {f0:?}");
        // Round-trip repeatedly — factors must stay put, not decay.
        let mut v = parse_mtext(src, true).to_mtext_string();
        for _ in 0..3 {
            let f = factors(&v);
            assert_eq!(f, f0, "height decayed on round-trip: {f:?} (value {v:?})");
            v = parse_mtext(&v, true).to_mtext_string();
        }
    }

    #[test]
    fn test_special_char_to_char() {
        assert_eq!(SpecialChar::Degree.to_char(), '°');
        assert_eq!(SpecialChar::PlusMinus.to_char(), '±');
        assert_eq!(SpecialChar::Diameter.to_char(), 'Ø');
        assert_eq!(SpecialChar::Percent.to_char(), '%');
    }

    #[test]
    fn test_special_char_from_char() {
        assert_eq!(SpecialChar::from_char('d'), Some(SpecialChar::Degree));
        assert_eq!(SpecialChar::from_char('D'), Some(SpecialChar::Degree));
        assert_eq!(SpecialChar::from_char('p'), Some(SpecialChar::PlusMinus));
        assert_eq!(SpecialChar::from_char('P'), Some(SpecialChar::PlusMinus));
        assert_eq!(SpecialChar::from_char('c'), Some(SpecialChar::Diameter));
        assert_eq!(SpecialChar::from_char('C'), Some(SpecialChar::Diameter));
        assert_eq!(SpecialChar::from_char('%'), Some(SpecialChar::Percent));
        assert_eq!(SpecialChar::from_char('x'), None);
    }

    #[test]
    fn test_special_char_to_string() {
        assert_eq!(SpecialChar::Degree.to_string(), "%%d");
        assert_eq!(SpecialChar::PlusMinus.to_string(), "%%p");
        assert_eq!(SpecialChar::Diameter.to_string(), "%%c");
        assert_eq!(SpecialChar::Percent.to_string(), "%%%%");
    }

    #[test]
    fn test_mtext_stroke() {
        let mut stroke = MTextStroke::new();
        assert!(!stroke.underline());
        assert!(!stroke.overline());
        assert!(!stroke.strikethrough());
        assert!(!stroke.has_any());

        stroke.set_underline(true);
        assert!(stroke.underline());
        assert!(stroke.has_any());

        stroke.set_strikethrough(true);
        assert!(stroke.strikethrough());

        stroke.set_overline(true);
        assert!(stroke.overline());

        stroke.set_underline(false);
        assert!(!stroke.underline());
        assert!(stroke.has_any()); // still has others
    }

    #[test]
    fn test_paragraph_alignment_from_char() {
        assert_eq!(
            MTextParagraphAlignment::from_char('l'),
            MTextParagraphAlignment::Left
        );
        assert_eq!(
            MTextParagraphAlignment::from_char('r'),
            MTextParagraphAlignment::Right
        );
        assert_eq!(
            MTextParagraphAlignment::from_char('c'),
            MTextParagraphAlignment::Center
        );
        assert_eq!(
            MTextParagraphAlignment::from_char('j'),
            MTextParagraphAlignment::Justified
        );
        assert_eq!(
            MTextParagraphAlignment::from_char('d'),
            MTextParagraphAlignment::Distributed
        );
    }

    #[test]
    fn test_line_alignment_from_code() {
        assert_eq!(MTextLineAlignment::from_code(0), MTextLineAlignment::Bottom);
        assert_eq!(MTextLineAlignment::from_code(1), MTextLineAlignment::Middle);
        assert_eq!(MTextLineAlignment::from_code(2), MTextLineAlignment::Top);
        assert_eq!(
            MTextLineAlignment::from_code(99),
            MTextLineAlignment::Bottom
        );
    }

    #[test]
    fn test_stacking_type_from_char() {
        assert_eq!(StackingType::from_char('/'), StackingType::Horizontal);
        assert_eq!(StackingType::from_char('#'), StackingType::Diagonal);
        assert_eq!(StackingType::from_char('^'), StackingType::Limit);
        assert_eq!(StackingType::from_char('x'), StackingType::Horizontal);
    }

    #[test]
    fn test_color_from_bgr_packed() {
        // BLUE: BGR packed 255 = (B=255,G=0,R=0) → RGB (0,0,255)
        let color = MTextColor::from_bgr_packed(255);
        assert_eq!(color.as_rgb(), Some((0, 0, 255)));
        // GREEN: BGR packed 65280 = (B=0,G=255,R=0) → RGB (0,255,0)
        let color = MTextColor::from_bgr_packed(65280);
        assert_eq!(color.as_rgb(), Some((0, 255, 0)));
        // RED: BGR packed 16711680 = (B=0,G=0,R=255) → RGB (255,0,0)
        let color = MTextColor::from_bgr_packed(16711680);
        assert_eq!(color.as_rgb(), Some((255, 0, 0)));
    }

    #[test]
    fn test_span_properties_is_empty() {
        let props = SpanProperties::default();
        assert!(props.is_empty());

        let mut props = SpanProperties::default();
        props.color = Some(MTextColor::Index(1));
        assert!(!props.is_empty());

        let mut props = SpanProperties::default();
        props.set_underline(true);
        assert!(!props.is_empty());
    }

    #[test]
    fn test_span_properties_merge() {
        let mut props = SpanProperties::default();
        let mut other = SpanProperties::default();
        other.color = Some(MTextColor::Index(3));
        other.height = Some(MTextScalar::Absolute(0.5));
        other.set_underline(true);

        props.merge(&other);
        assert_eq!(props.color, Some(MTextColor::Index(3)));
        assert_eq!(props.height, Some(MTextScalar::Absolute(0.5)));
        assert!(props.underline());
    }

    #[test]
    fn test_stacking_data() {
        let data = StackingData {
            numerator: "1".into(),
            denominator: "4".into(),
            stacking_type: StackingType::Horizontal,
        };
        assert!(!data.is_empty());
        assert_eq!(data.to_plain_text(), "1/4");
    }

    #[test]
    fn test_mtext_document_roundtrip_formatted() {
        let mut doc = MTextDocument::new();
        let mut para = MTextParagraph::new();

        let mut props = SpanProperties::default();
        props.color = Some(MTextColor::Index(1));
        para.push_span(MTextSpan::new("Red", props));

        para.push_span(MTextSpan::plain(" Normal"));

        doc.push_paragraph(para);

        let s = doc.to_mtext_string();
        assert!(s.starts_with("{"));
        assert!(s.ends_with("}"));
        assert!(s.contains("\\C1;"));
    }

    #[test]
    fn test_mtext_document_roundtrip_stroke() {
        let mut doc = MTextDocument::new();
        let mut para = MTextParagraph::new();

        let mut props = SpanProperties::default();
        props.set_underline(true);
        para.push_span(MTextSpan::new("Underlined", props));

        para.push_span(MTextSpan::plain(" normal"));

        doc.push_paragraph(para);

        let s = doc.to_mtext_string();
        assert!(s.contains("\\L"));
    }

    #[test]
    fn test_serialize_preserves_same_color_across_paragraphs() {
        let mut doc = MTextDocument::new();
        let mut props = SpanProperties::default();
        props.color = Some(MTextColor::Index(1));

        let mut para1 = MTextParagraph::new();
        para1.push_span(MTextSpan::new("First", props.clone()));
        doc.push_paragraph(para1);

        let mut para2 = MTextParagraph::new();
        para2.push_span(MTextSpan::new("Second", props));
        doc.push_paragraph(para2);

        let serialized = doc.to_mtext_string();
        let reparsed = crate::entities::mtext_format::parse_mtext(&serialized, false);

        assert_eq!(reparsed.paragraphs.len(), 2);
        assert_eq!(reparsed.paragraphs[0].to_plain_text(), "First");
        assert_eq!(reparsed.paragraphs[1].to_plain_text(), "Second");
        assert_eq!(
            reparsed.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            reparsed.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_serialize_color_change_between_paragraphs() {
        let mut doc = MTextDocument::new();

        let mut props1 = SpanProperties::default();
        props1.color = Some(MTextColor::Index(1));
        let mut para1 = MTextParagraph::new();
        para1.push_span(MTextSpan::new("Red", props1));
        doc.push_paragraph(para1);

        let mut props2 = SpanProperties::default();
        props2.color = Some(MTextColor::Index(5));
        let mut para2 = MTextParagraph::new();
        para2.push_span(MTextSpan::new("Blue", props2));
        doc.push_paragraph(para2);

        let s = doc.to_mtext_string();
        // Both color codes must appear
        assert!(s.contains("\\C1;"));
        assert!(s.contains("\\C5;"));
    }

    #[test]
    fn test_roundtrip_preserves_inherited_color_across_paragraphs() {
        // Parse → serialize → parse should produce the same structure
        let input = "{\\C1;First\\PSecond}";
        let doc = crate::entities::mtext_format::parse_mtext(input, false);

        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );

        let serialized = doc.to_mtext_string();
        let reparsed = crate::entities::mtext_format::parse_mtext(&serialized, false);

        assert_eq!(reparsed.paragraphs.len(), 2);
        assert_eq!(reparsed.paragraphs[0].to_plain_text(), "First");
        assert_eq!(reparsed.paragraphs[1].to_plain_text(), "Second");
        assert_eq!(
            reparsed.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            reparsed.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_roundtrip_stable_no_growth() {
        // Serializing and re-parsing should not create extra paragraphs
        let input = "{\\C1;First\\PSecond\\PThird}";
        let doc = crate::entities::mtext_format::parse_mtext(input, false);
        assert_eq!(doc.paragraphs.len(), 3);

        let s1 = doc.to_mtext_string();
        let doc2 = crate::entities::mtext_format::parse_mtext(&s1, false);
        assert_eq!(doc2.paragraphs.len(), 3);

        let s2 = doc2.to_mtext_string();
        // Should be identical (stable roundtrip)
        assert_eq!(s1, s2);
    }
}
