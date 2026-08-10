use acadrust::types::aci_table::aci_to_rgb;
use acadrust::CadDocument;

use crate::scene::convert::acad_to_render::{GlyphRun, TextStroke};
use crate::scene::text::font_face::Face;
use crate::scene::text::lff;

pub struct ResolvedTextStyle {
    pub font_name: String,
    pub width_factor: f32,
    pub oblique_angle: f32,
    pub is_backward: bool,
    pub is_upside_down: bool,
}

pub fn resolve_text_style(style_name: &str, document: &CadDocument) -> ResolvedTextStyle {
    let style = document.text_styles.iter().find(|entry| {
        entry.name.eq_ignore_ascii_case(style_name)
            || (style_name.trim().is_empty() && entry.name.eq_ignore_ascii_case("Standard"))
    });

    let mut font_name = if let Some(style) = style {
        if !style.true_type_font.trim().is_empty() {
            style.true_type_font.trim().to_string()
        } else if !style.font_file.trim().is_empty() {
            let file = style.font_file.trim();
            // A .shx font that resolves on disk (as stored, or next to the
            // drawing) renders its REAL stroke glyphs — pass the absolute
            // path through so `Face::resolve` picks the SHX face. Only an
            // unresolvable file falls back to the stem's LFF substitute.
            let shx_path = file
                .to_ascii_lowercase()
                .ends_with(".shx")
                .then(|| {
                    let base = document
                        .source_path
                        .as_deref()
                        .map(std::path::Path::new)
                        .and_then(|p| p.parent());
                    crate::io::resolve_image_file(file, base)
                })
                .flatten();
            if let Some(p) = shx_path {
                p
            } else {
                let basename = file.rsplit(['/', '\\']).next().unwrap_or(file);
                let stem = basename.split('.').next().unwrap_or(basename).trim();
                if !stem.is_empty() {
                    stem.to_string()
                } else if !style.name.trim().is_empty() {
                    style.name.trim().to_string()
                } else {
                    "Standard".to_string()
                }
            }
        } else if !style.name.trim().is_empty() {
            style.name.trim().to_string()
        } else {
            "Standard".to_string()
        }
    } else if style_name.trim().is_empty() {
        "Standard".to_string()
    } else {
        style_name.trim().to_string()
    };

    if !lff::is_builtin(&font_name) {
        if let Some(canonical) = crate::scene::text::sysfont::canonical_family_name(&font_name) {
            font_name = canonical;
        }
    }

    ResolvedTextStyle {
        font_name,
        width_factor: style.map(|s| s.width_factor as f32).unwrap_or(1.0),
        oblique_angle: style.map(|s| s.oblique_angle as f32).unwrap_or(0.0),
        is_backward: style.map(|s| s.is_backward()).unwrap_or(false),
        is_upside_down: style.map(|s| s.is_upside_down()).unwrap_or(false),
    }
}

pub struct TextLocalBounds {
    /// Inked extent (glyph strokes only) — drives vertical alignment, where
    /// the cap / baseline geometry is what matters.
    pub ink_min: [f32; 2],
    pub ink_max: [f32; 2],
    /// Pen advance along the baseline, including leading / trailing spaces and
    /// inter-glyph spacing. Drives horizontal alignment so spaces in the string
    /// keep their width instead of collapsing to the first / last inked glyph.
    pub advance: f32,
}

pub fn text_local_bounds(
    font_name: &str,
    text: &str,
    height: f32,
    width_factor: f32,
    oblique_angle: f32,
) -> Option<TextLocalBounds> {
    if text.is_empty() || height <= 0.0 {
        return None;
    }

    let face = Face::resolve(font_name);
    let scale = height / 9.0;
    let wf = width_factor.abs().clamp(0.01, 100.0);
    let ob = oblique_angle.tan();
    let mut cursor_x = 0.0_f32;
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for ch in text.chars() {
        if ch == ' ' {
            cursor_x += face.word_spacing();
            continue;
        }
        match face.glyph(ch) {
            Some(glyph) => {
                for stroke in &glyph.strokes {
                    for &[gx, gy] in stroke {
                        let sx = (cursor_x + gx) * scale * wf + gy * scale * ob;
                        let sy = gy * scale;
                        min_x = min_x.min(sx);
                        max_x = max_x.max(sx);
                        min_y = min_y.min(sy);
                        max_y = max_y.max(sy);
                    }
                }
                cursor_x += glyph.advance + face.letter_spacing();
            }
            None => {
                cursor_x += 6.0 + face.letter_spacing();
            }
        }
    }

    // Pen advance is measured at the baseline, so oblique shear (which skews x
    // only by gy) does not enter it. Valid even for an all-space string.
    let advance = cursor_x * scale * wf;

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(TextLocalBounds {
            ink_min: [min_x, min_y],
            ink_max: [max_x, max_y],
            advance,
        })
    } else {
        None
    }
}

/// Expand DXF `%%x` special-character sequences that appear in both TEXT and MTEXT values:
/// - `%%d` / `%%D` → `°`
/// - `%%p` / `%%P` → `±`
/// - `%%c` / `%%C` → `⌀`
/// - `%%u` / `%%U` → underline toggle (stripped — not renderable with stroke fonts)
/// - `%%o` / `%%O` → overline toggle (stripped)
/// - `%%%%` → `%`
/// - `%%nnn` (3 decimal digits) → Unicode scalar `nnn`
/// Any unrecognised `%%x` is passed through unchanged.
pub fn resolve_dxf_special_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '%' || chars.peek() != Some(&'%') {
            out.push(c);
            continue;
        }
        chars.next(); // consume second '%'
        match chars.peek().map(|c| c.to_ascii_lowercase()) {
            Some('d') => {
                chars.next();
                out.push('°');
            }
            Some('p') => {
                chars.next();
                out.push('±');
            }
            Some('c') => {
                chars.next();
                out.push('⌀');
            }
            Some('u') | Some('o') => {
                chars.next();
            } // toggle codes — strip silently
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some(d) if d.is_ascii_digit() => {
                let mut digits = String::with_capacity(3);
                for _ in 0..3 {
                    match chars.peek() {
                        Some(&ch) if ch.is_ascii_digit() => {
                            digits.push(chars.next().unwrap());
                        }
                        _ => break,
                    }
                }
                if digits.len() == 3 {
                    if let Ok(n) = digits.parse::<u32>() {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                            continue;
                        }
                    }
                }
                out.push('%');
                out.push('%');
                out.push_str(&digits);
            }
            _ => {
                out.push('%');
                out.push('%');
            }
        }
    }

    out
}

// ──────────────────────────────────────────────────────────────────────────
// Rich MTEXT parser — full inline format-code coverage
//
// Recognised codes (DXF MTEXT inline):
//   Escapes:  \\  \{  \}  \~  \t  \P  \n  \N  \U+XXXX  \u+XXXX
//   Toggles:  \L\l  \O\o  \K\k  (underline / overline / strike)
//   State:    \H<v>[x];  \W<v>[x];  \Q<v>;  \T<v>[x];  \A<n>;
//             \C<aci>;   \c<rgb>;
//             \f<name>|b<0/1>|i<0/1>|c<n>|p<n>;   \F<file>;
//             \M+<n>;    \X   \S<u><sep><l>;
//   Paragraph: \p[xi<v>,l<v>,r<v>,q[lcrjd],t<positions>,s<v>...];
//   Scope:    { ... }   push/pop full state
// ──────────────────────────────────────────────────────────────────────────

/// Paragraph alignment encoded inline via `\p...q[lcrjd]...;`.
/// `Justify` / `Distribute` render as `Left` (full inter-word redistribution
/// is not implemented in the stroke renderer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParagraphAlign {
    Left,
    Center,
    Right,
    Justify,
    Distribute,
}

/// Inline colour override (`\C` ACI or `\c` 24-bit true colour). Resolved to
/// linear RGB at render time via the document's ACI table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InlineColor {
    Aci(u8),
    True([f32; 3]),
}

/// Tab-stop alignment kind (from `\pt<L|C|R><pos>` entries). `Center` / `Right`
/// are DXF-spec kinds the parser does not emit yet (only `Left` is produced).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKind {
    Left,
    Center,
    Right,
    /// Dot-aligned: the text after the tab places its decimal point on the stop.
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub position: f32,
    pub kind: TabKind,
}

/// Per-run formatting state. All fields are multipliers / overrides relative
/// to the entity-level defaults; the renderer composes them with the resolved
/// text style at draw time.
#[derive(Clone, Debug, PartialEq)]
pub struct RunState {
    /// Multiplier on entity text height (`\H<v>x;` → ×v; `\H<v>;` → v / entity_h)
    pub height_mul: f32,
    /// Multiplier on the (signed) style width-factor (`\W<v>;` → set, `\Wx;` → ×)
    pub width_mul: f32,
    /// Absolute oblique angle override in radians (`\Q<deg>;`)
    pub oblique_rad: f32,
    /// Tracking multiplier on `font.letter_spacing` (`\T<v>;`)
    pub tracking: f32,
    /// Vertical alignment of the run within its line box (0=baseline / 1=center
    /// / 2=top). Mainly used for fractions and superscript-like layout (`\A`).
    pub valign: u8,
    /// Font-name override, `None` ⇒ inherit the resolved style font.
    pub font: Option<String>,
    /// Colour override, `None` ⇒ inherit entity colour.
    pub color: Option<InlineColor>,
    pub underline: bool,
    pub overline: bool,
    pub strike: bool,
    /// Bold run — rendered with a wider SDF pen (thicker strokes).
    pub bold: bool,
}

impl Default for RunState {
    fn default() -> Self {
        Self {
            height_mul: 1.0,
            width_mul: 1.0,
            oblique_rad: 0.0,
            tracking: 1.0,
            valign: 0,
            font: None,
            color: None,
            underline: false,
            overline: false,
            strike: false,
            bold: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MTextRunKind {
    /// Renderable glyph text (DXF specials resolved, decoration markers stripped).
    Glyphs(String),
    /// `\t` — jump the cursor to the next tab stop (or default tab interval).
    /// Handled in the layout matches but not emitted by the parser yet.
    #[allow(dead_code)]
    Tab,
    /// A `\S` stack: `numerator` over `denominator`, sharing one horizontal
    /// slot. `bar` draws the fraction rule between them (`\S1/2;`); without it
    /// the two just sit one above the other (`\S1^2;`, a limit).
    Stack {
        numerator: String,
        denominator: String,
        bar: bool,
    },
}

#[derive(Clone, Debug)]
pub struct MTextRun {
    pub kind: MTextRunKind,
    pub state: RunState,
}

/// One paragraph of MTEXT after parsing. Each line is a sequence of runs that
/// share text content + a snapshot of formatting state, plus paragraph-level
/// layout (alignment, indents, tab stops). `\P` / `\n` / `\N` start a new
/// line; paragraph properties carry forward until the next `\p...;` block.
#[derive(Clone, Debug, Default)]
pub struct MTextLine {
    pub runs: Vec<MTextRun>,
    pub align: Option<ParagraphAlign>,
    pub indent_first: f32,
    pub indent_left: f32,
    pub indent_right: f32,
    pub tab_stops: Vec<TabStop>,
    /// Set when this line opened a new column (`\N`, not `\P`). Layout starts a
    /// fresh column here when the entity defines any; with no columns it reads
    /// as a plain line break, which is what `\N` degrades to anyway.
    pub starts_column: bool,
    /// Blank space above the paragraph, in drawing units (`\p…b#;`). Widens the
    /// gap before the paragraph's first line so stacked paragraphs read apart.
    pub space_before: f32,
    /// Blank space below the paragraph, in drawing units (`\p…a#;`).
    pub space_after: f32,
    /// Per-paragraph line spacing (`\psm#` multiple / `\pse#` exact), overriding
    /// the entity-level factor for this paragraph's own lines.
    pub line_spacing: Option<ParaLineSpacing>,
}

/// A paragraph's own line-spacing rule from `\psm#` / `\pse#`, overriding the
/// entity's DXF-44 factor for that paragraph.
#[derive(Clone, Copy, Debug)]
pub enum ParaLineSpacing {
    /// Multiple of the default baseline gap (`sm#`).
    Multiple(f32),
    /// Exact baseline-to-baseline distance in drawing units (`se#`).
    Exact(f32),
}

impl MTextLine {
    pub fn is_blank(&self) -> bool {
        self.runs.iter().all(|r| match &r.kind {
            MTextRunKind::Glyphs(t) => t.trim().is_empty(),
            MTextRunKind::Tab => false,
            MTextRunKind::Stack { .. } => false,
        })
    }
}


/// Font-name stem: drop any path and extension, so a `\F` font path like
/// `C:\\fonts\\arial.ttf` and a plain `\f` name `arial` resolve to the same font.
fn font_stem(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .split('.')
        .next()
        .unwrap_or(name)
        .to_string()
}

/// Parse an MTEXT string into the layout's `Vec<MTextLine>`, using acadrust's
/// structured `mtext_format::parse_mtext` — OCS keeps only the layout engine
/// (`layout_mtext` and callers read `MTextLine`/`RunState`), not a second MTEXT
/// inline parser.
///
/// Representation notes:
///  - DXF `%%d`/`%%p`/`%%c` arrive already resolved to Unicode from acadrust;
///    the stroke tokenizer treats those as ordinary glyphs.
///  - Stacking (`\S`) is flattened inline to `num<sep>den` (`^` for limit, else
///    `/`) since the stroke path has no fraction layout.
///  - `\H`: a relative factor (`\Hx`) applies directly; an absolute height is
///    divided by the entity height. See `acadrust::…::MTextScalar`.
pub fn adapt_mtext_paragraphs(
    s: &str,
    entity_height: f32,
    trim_blank_edges: bool,
) -> Vec<MTextLine> {
    use acadrust::entities::mtext_format::{
        parse_mtext, MTextColor, MTextLineAlignment, MTextLineSpacing, MTextParagraphAlignment,
        MTextScalar, ParagraphProperties, SpanProperties, StackingType,
    };

    let entity_height = entity_height.max(1e-6);
    let doc = parse_mtext(s, true);

    fn map_align(a: MTextParagraphAlignment) -> Option<ParagraphAlign> {
        match a {
            MTextParagraphAlignment::Left => Some(ParagraphAlign::Left),
            MTextParagraphAlignment::Right => Some(ParagraphAlign::Right),
            MTextParagraphAlignment::Center => Some(ParagraphAlign::Center),
            MTextParagraphAlignment::Justified => Some(ParagraphAlign::Justify),
            MTextParagraphAlignment::Distributed => Some(ParagraphAlign::Distribute),
            MTextParagraphAlignment::Default => None,
        }
    }
    fn color_of(p: &SpanProperties) -> Option<InlineColor> {
        if let Some((r, g, b)) = p.color_rgb {
            return Some(InlineColor::True([
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ]));
        }
        match p.color {
            Some(MTextColor::Index(n)) => Some(InlineColor::Aci(n.min(255) as u8)),
            _ => None,
        }
    }

    // A `\p...;` block sets the paragraph's alignment/indents/tabs and stays in
    // force until the next `\p` — a paragraph opened by a bare `\P` / `\N`
    // inherits it. The parser records only the literal codes (a paragraph with
    // no `\p` carries none), so persistence is applied here: without it, the
    // second line of a centred or right column (`\pxqc;col2\P≡`) would fall back
    // to the left edge while its heading stayed aligned.
    let has_own_props = |p: &ParagraphProperties| {
        p.alignment
            .as_ref()
            .is_some_and(|a| !matches!(a, MTextParagraphAlignment::Default))
            || p.first_line_indent.is_some()
            || p.left_margin.is_some()
            || p.right_margin.is_some()
            || !p.tab_stops.is_empty()
    };

    let mut lines: Vec<MTextLine> = Vec::new();
    let mut carried: Option<(Option<ParagraphAlign>, f32, f32, f32, Vec<TabStop>)> = None;
    // Paragraph spacing (`b`/`a`) and line spacing (`sm`/`se`) are sticky
    // per-field: a `\p…;` block that names some codes leaves the rest at their
    // previous value, so spacing set once (`\pxb0.25;`) applies to every later
    // paragraph until re-specified. The alignment/indent carry above is
    // all-or-nothing, which would drop spacing at the first `\p` that only
    // touches alignment — so spacing is tracked independently here.
    let mut carried_sb = 0.0_f32;
    let mut carried_sa = 0.0_f32;
    let mut carried_ls: Option<ParaLineSpacing> = None;
    for para in &doc.paragraphs {
        let props = &para.properties;
        if let Some(v) = props.spacing_before {
            carried_sb = v as f32;
        }
        if let Some(v) = props.spacing_after {
            carried_sa = v as f32;
        }
        match props.line_spacing {
            Some(MTextLineSpacing::Multiple(m)) => {
                carried_ls = Some(ParaLineSpacing::Multiple(m as f32))
            }
            Some(MTextLineSpacing::Exact(e)) => carried_ls = Some(ParaLineSpacing::Exact(e as f32)),
            Some(MTextLineSpacing::Default) => carried_ls = None,
            None => {}
        }
        let (align, indent_first, indent_left, indent_right, tab_stops) =
            if has_own_props(props) || carried.is_none() {
                let v = (
                    props.alignment.and_then(map_align),
                    props.first_line_indent.unwrap_or(0.0) as f32,
                    props.left_margin.unwrap_or(0.0) as f32,
                    props.right_margin.unwrap_or(0.0) as f32,
                    props
                        .tab_stops
                        .iter()
                        .map(|ts| {
                            use acadrust::entities::mtext_format::TabStop as ATab;
                            let kind = match ts {
                                ATab::Left(_) => TabKind::Left,
                                ATab::Center(_) => TabKind::Center,
                                ATab::Right(_) => TabKind::Right,
                                ATab::Decimal(_) => TabKind::Decimal,
                            };
                            TabStop {
                                position: ts.position() as f32,
                                kind,
                            }
                        })
                        .collect::<Vec<_>>(),
                );
                carried = Some(v.clone());
                v
            } else {
                carried.clone().unwrap()
            };
        let mut line = MTextLine {
            align,
            starts_column: para.starts_column,
            indent_first,
            indent_left,
            indent_right,
            tab_stops,
            runs: Vec::new(),
            space_before: carried_sb,
            space_after: carried_sa,
            line_spacing: carried_ls,
        };
        for span in &para.spans {
            let p = &span.properties;
            let state = RunState {
                // Relative `\H…x;` is a factor applied directly; absolute `\H…;`
                // is a drawing-unit height resolved against the entity height.
                height_mul: match p.height {
                    Some(MTextScalar::Factor(f)) => f as f32,
                    Some(MTextScalar::Absolute(a)) => a as f32 / entity_height,
                    None => 1.0,
                },
                width_mul: p.width_factor.map(|w| w as f32).unwrap_or(1.0),
                // Explicit `\Q` oblique wins; otherwise the font italic flag
                // (`\f…|i1;`) renders as the same ~15° slant the editor's Italic
                // button applies, so imported italic text isn't shown upright.
                oblique_rad: match p.oblique_angle {
                    Some(q) => (q as f32).to_radians(),
                    None if p.font.as_ref().is_some_and(|f| f.italic) => {
                        15.0_f32.to_radians()
                    }
                    None => 0.0,
                },
                tracking: p.tracking.map(|t| t as f32).unwrap_or(1.0),
                valign: match p.line_align {
                    Some(MTextLineAlignment::Middle) => 1,
                    Some(MTextLineAlignment::Top) => 2,
                    _ => 0,
                },
                font: p.font.as_ref().map(|f| font_stem(&f.name)),
                bold: p.font.as_ref().map(|f| f.bold).unwrap_or(false),
                color: color_of(p),
                underline: p.stroke.underline(),
                overline: p.stroke.overline(),
                strike: p.stroke.strikethrough(),
            };
            if let Some(st) = &span.stacking {
                // `\S` stacks the two halves in one slot. A diagonal stack
                // (`\S1#2;`) is the exception — it reads as an inline `num/den`
                // and is drawn that way rather than as a slanted rule.
                if matches!(st.stacking_type, StackingType::Diagonal) {
                    let mut t = st.numerator.clone();
                    if !st.denominator.is_empty() {
                        t.push('/');
                        t.push_str(&st.denominator);
                    }
                    if !t.is_empty() {
                        line.runs.push(MTextRun {
                            kind: MTextRunKind::Glyphs(t),
                            state,
                        });
                    }
                } else if !st.numerator.is_empty() || !st.denominator.is_empty() {
                    line.runs.push(MTextRun {
                        kind: MTextRunKind::Stack {
                            numerator: st.numerator.clone(),
                            denominator: st.denominator.clone(),
                            bar: matches!(st.stacking_type, StackingType::Horizontal),
                        },
                        state,
                    });
                }
                continue;
            }
            let text = span.text.clone();
            if text.is_empty() {
                continue;
            }
            line.runs.push(MTextRun {
                kind: MTextRunKind::Glyphs(text),
                state,
            });
        }
        lines.push(line);
    }

    if !trim_blank_edges {
        return lines;
    }
    let start = lines.iter().position(|l| !l.is_blank()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.is_blank())
        .map(|i| i + 1)
        .unwrap_or(0);
    lines[start..end].to_vec()
}

// Legacy MText helpers (`strip_mtext_codes`, `split_mtext_lines`,
// `measure_mtext_chars`, `word_wrap`) were removed when every text-bearing
// entity switched to the run-aware pipeline below. The pipeline now owns
// per-run width measurement and word-wrap; MTEXT inline parsing now comes from
// acadrust via `adapt_mtext_paragraphs`. The supported surface for callers is
// `adapt_mtext_paragraphs`, `layout_mtext`, `mtext_line_count`,
// `text_local_bounds`, and `resolve_dxf_special_chars`.

// ──────────────────────────────────────────────────────────────────────────────
// Shared MText layout / render pipeline
// ──────────────────────────────────────────────────────────────────────────────
//
// `layout_mtext` is the entry point used by every text-bearing entity that
// stores MText-formatted content (MTEXT, MLEADER text content, TABLE cell,
// ATTRIB / ATTDEF with `mtext_flag` set, and DIMENSION `text_override` when
// it carries inline codes).
//
// The pipeline mirrors the MTEXT renderer:
//   1. Parse — via `adapt_mtext_paragraphs` (acadrust `parse_mtext`).
//   2. Atomise — turn each MTextLine.runs into a flat sequence of atoms
//      (Word / Space / Tab) so the wrapper operates at break boundaries
//      while keeping per-character formatting state.
//   3. Wrap — accumulate atoms into sub-lines using paragraph indents and
//      tab stops; each Tab jumps the cursor to the next user-defined stop
//      (or a 4-em default grid).
//   4. Render — for each sub-line: pick paragraph alignment + indent, walk
//      atoms left → right, emit one TextStroke per Word using the atom's
//      RunState (height / width / oblique / tracking / font / colour /
//      decorations / valign).
//
// In addition to the strokes, the helper returns enough geometry (line
// widths, line height, v_offset, h_anchor) for the caller to draw a frame /
// background rectangle, run a low-detail LOD path, or compute snap bounds.

#[derive(Clone)]
pub enum AtomKind {
    Word(String),
    Space,
    Tab,
    /// A `\S` stack — both halves share this one slot, so it measures as wide
    /// as the wider of them rather than as the two laid end to end.
    Stack {
        numerator: String,
        denominator: String,
        bar: bool,
    },
}

// A `\S` stack is measured against the height of the run it sits in, and that
// run has usually already been dropped for the purpose — `\H0.7x;\S1/2;\H1.429x;`
// is how a 70 %-scaled fraction is spelled, so the 0.7 *is* the fraction's size.
// Shrinking again on top of it is what buries the halves.
//
// So a half is near the full run height and the stack ends up about twice that:
// it is meant to grow past the line, numerator above the cap and denominator on
// the baseline — not to be squeezed into one line's worth of height.
//
// The three must stay ordered — denominator cap, then rule, then numerator
// baseline — or the rule cuts through a half.
/// Each half of a `\S` stack, as a fraction of the run's own height.
pub const STACK_HALF_SCALE: f32 = 0.85;
/// Height of the fraction rule above the denominator's baseline, as a fraction
/// of the run's height. Sits just clear of the denominator's cap.
pub const STACK_BAR_Y: f32 = 0.95;
/// Baseline of the numerator above the denominator's, as a fraction of the run's
/// height. Clears the rule.
pub const STACK_RAISE: f32 = 1.05;

#[derive(Clone)]
pub struct LayoutAtom {
    pub kind: AtomKind,
    pub state: RunState,
}

pub fn run_scale(state: &RunState, entity_h: f32, base_wf: f32) -> f32 {
    (state.height_mul * entity_h / 9.0) * (state.width_mul * base_wf.abs())
}

pub fn resolve_font<'a>(state: &'a RunState, base: &'a str) -> std::borrow::Cow<'a, str> {
    let Some(font) = state.font.as_deref().map(str::trim).filter(|f| !f.is_empty()) else {
        return std::borrow::Cow::Borrowed(base);
    };
    if lff::is_builtin(font) {
        return std::borrow::Cow::Borrowed(font);
    }
    if let Some(canonical) = crate::scene::text::sysfont::canonical_family_name(font) {
        std::borrow::Cow::Owned(canonical)
    } else {
        std::borrow::Cow::Borrowed(base)
    }
}

pub fn measure_word(
    text: &str,
    state: &RunState,
    entity_h: f32,
    base_wf: f32,
    base_font: &str,
) -> f32 {
    let scale = run_scale(state, entity_h, base_wf);
    let font_name = resolve_font(state, base_font);
    let face = Face::resolve(&font_name);
    let mut w = 0.0_f32;
    for ch in text.chars() {
        w += match face.glyph(ch) {
            Some(g) => (g.advance + face.letter_spacing() * state.tracking) * scale,
            None => (6.0 + face.letter_spacing() * state.tracking) * scale,
        };
    }
    w
}

pub fn measure_space(state: &RunState, entity_h: f32, base_wf: f32, base_font: &str) -> f32 {
    let scale = run_scale(state, entity_h, base_wf);
    let font_name = resolve_font(state, base_font);
    Face::resolve(&font_name).word_spacing() * scale
}

pub fn atom_width(atom: &LayoutAtom, entity_h: f32, base_wf: f32, base_font: &str) -> f32 {
    match &atom.kind {
        AtomKind::Word(t) => measure_word(t, &atom.state, entity_h, base_wf, base_font),
        AtomKind::Space => measure_space(&atom.state, entity_h, base_wf, base_font),
        AtomKind::Tab => 0.0,
        AtomKind::Stack {
            numerator,
            denominator,
            ..
        } => {
            // Both halves start at the slot's left edge, so the slot is as wide
            // as the wider one — measured at the shrunk height they draw at.
            let mut half = atom.state.clone();
            half.height_mul *= STACK_HALF_SCALE;
            let n = measure_word(numerator, &half, entity_h, base_wf, base_font);
            let d = measure_word(denominator, &half, entity_h, base_wf, base_font);
            n.max(d)
        }
    }
}

/// Cursor position after a `\t` atom: advance to the next user-defined tab
/// stop that lies past `cur_x`, falling back to a 4-em default grid when no
/// stop is reached.
pub fn next_tab_position(
    cur_x: f32,
    tab_stops: &[TabStop],
    indent_left: f32,
    entity_h: f32,
) -> f32 {
    let local = cur_x - indent_left;
    for ts in tab_stops {
        if ts.position > local + 1e-4 {
            return indent_left + ts.position;
        }
    }
    let default_interval = entity_h * 4.0;
    let n = (local / default_interval).floor() + 1.0;
    indent_left + n * default_interval
}

/// Break a flat MText paragraph atom stream into wrap-fit sub-lines.
pub fn wrap_paragraph(
    atoms: Vec<LayoutAtom>,
    rect_w: f32,
    indent_first: f32,
    indent_left: f32,
    indent_right: f32,
    tab_stops: &[TabStop],
    entity_h: f32,
    base_wf: f32,
    base_font: &str,
) -> Vec<Vec<LayoutAtom>> {
    if rect_w <= 0.0 {
        return vec![atoms];
    }
    let mut sublines: Vec<Vec<LayoutAtom>> = Vec::new();
    let mut cur: Vec<LayoutAtom> = Vec::new();
    let mut cur_w = 0.0_f32;
    let mut subline_idx: usize = 0;
    // The field after a center/right/decimal tab is aligned on the stop, so it
    // reaches only part of its width to the right — don't wrap it off the line
    // by its full width as a left-flushed word would be.
    let mut after_align_tab = false;
    let line_start_x = |idx: usize| if idx == 0 { indent_first } else { indent_left };
    let line_max_w = |idx: usize| (rect_w - indent_right - line_start_x(idx)).max(0.0);

    for atom in atoms {
        match &atom.kind {
            // A stack is one unbreakable slot — it wraps exactly like a word.
            AtomKind::Word(_) | AtomKind::Stack { .. } => {
                let w = atom_width(&atom, entity_h, base_wf, base_font);
                let max_w = line_max_w(subline_idx);
                if !cur.is_empty() && cur_w + w > max_w && !after_align_tab {
                    while matches!(cur.last().map(|a| &a.kind), Some(AtomKind::Space)) {
                        cur.pop();
                    }
                    sublines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                    subline_idx += 1;
                }
                after_align_tab = false;
                cur.push(atom);
                cur_w += w;
            }
            AtomKind::Space => {
                after_align_tab = false;
                if cur.is_empty() {
                    continue;
                }
                cur_w += atom_width(&atom, entity_h, base_wf, base_font);
                cur.push(atom);
            }
            AtomKind::Tab => {
                let start_x = line_start_x(subline_idx);
                let local = cur_w + start_x - indent_left;
                let is_align = tab_stops
                    .iter()
                    .find(|ts| ts.position > local + 1e-4)
                    .map(|ts| !matches!(ts.kind, TabKind::Left))
                    .unwrap_or(false);
                let new_w = next_tab_position(cur_w + start_x, tab_stops, indent_left, entity_h)
                    - start_x;
                let max_w = line_max_w(subline_idx);
                if new_w > max_w && !cur.is_empty() {
                    sublines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                    subline_idx += 1;
                } else {
                    cur.push(atom);
                    cur_w = new_w.min(max_w);
                }
                after_align_tab = is_align;
            }
        }
    }
    if !cur.is_empty() {
        sublines.push(cur);
    }
    if sublines.is_empty() {
        sublines.push(Vec::new());
    }
    sublines
}

pub fn line_total_width(
    atoms: &[LayoutAtom],
    entity_h: f32,
    base_wf: f32,
    base_font: &str,
    line_start_x: f32,
    indent_left: f32,
    tab_stops: &[TabStop],
) -> f32 {
    let mut x = line_start_x;
    for atom in atoms {
        match atom.kind {
            AtomKind::Tab => {
                x = next_tab_position(x, tab_stops, indent_left, entity_h);
            }
            _ => x += atom_width(atom, entity_h, base_wf, base_font),
        }
    }
    x - line_start_x
}

pub fn resolve_inline_color(c: &InlineColor) -> Option<[f32; 3]> {
    match c {
        InlineColor::Aci(idx) => aci_to_rgb(*idx).map(|(r, g, b)| {
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
        }),
        InlineColor::True(rgb) => Some(*rgb),
    }
}

/// Wrap a run's glyph text with MTEXT decoration markers so lff's
/// `tessellate_text_run` emits the underline / overline / strikethrough
/// strokes for us — keeps decoration geometry in one place rather than
/// duplicating the y-position constants.
fn decorated(text: &str, state: &RunState) -> String {
    if !(state.underline || state.overline || state.strike) {
        return text.to_string();
    }
    let mut s = String::with_capacity(text.len() + 6);
    if state.underline {
        s.push_str("\\L");
    }
    if state.overline {
        s.push_str("\\O");
    }
    if state.strike {
        s.push_str("\\K");
    }
    s.push_str(text);
    if state.underline {
        s.push_str("\\l");
    }
    if state.overline {
        s.push_str("\\o");
    }
    if state.strike {
        s.push_str("\\k");
    }
    s
}

#[derive(Clone, Copy, Debug)]
pub enum MTextVAnchor {
    /// Block top edge at insertion (first line's cap = insertion.y).
    Top,
    /// Block midpoint at insertion.
    Middle,
    /// Block bottom edge at insertion (last line's baseline = insertion.y).
    Bottom,
    /// MLEADER `MiddleOfTopLine` — first line's vertical centre at insertion.
    MiddleOfTopLine,
    /// MLEADER `MiddleOfBottomLine` — last line's vertical centre at insertion.
    MiddleOfBottomLine,
    /// MLEADER `BottomOfTopLineUnderline*` — first line's baseline at insertion.
    BottomOfTopLine,
}

/// Flow axis + signed attachment-side factor for a text box's width/wrap
/// grip. ONE place for every text-bearing entity (MTEXT, MLEADER, …):
///
/// - `dir`: the flow axis — the rotated baseline for horizontal text, DOWN
///   the column for top-to-bottom text.
/// - `k`: which side of the insertion the box actually forms on, from the
///   attachment that governs the flow axis: start-attached `+1`, centre
///   `±half`, end-attached `-1` (a right-attached horizontal text forms LEFT
///   of its insertion; a bottom-anchored vertical text forms UP).
///
/// Grip position = `insertion + dir · (k · width)`; a drag re-derives
/// `width = proj(dir) / k`, so pulling the grip on either side widens the
/// same box.
pub fn flow_grip_axis(
    rotation: f64,
    vertical: bool,
    h_anchor: f32,
    v_anchor: MTextVAnchor,
) -> (glam::DVec3, f64) {
    let (c, s) = (rotation.cos(), rotation.sin());
    let dir = if vertical {
        glam::DVec3::new(s, -c, 0.0)
    } else {
        glam::DVec3::new(c, s, 0.0)
    };
    let k = if vertical {
        match v_anchor {
            MTextVAnchor::Top
            | MTextVAnchor::MiddleOfTopLine
            | MTextVAnchor::BottomOfTopLine => 1.0,
            MTextVAnchor::Middle => 0.5,
            MTextVAnchor::Bottom | MTextVAnchor::MiddleOfBottomLine => -1.0,
        }
    } else if h_anchor >= 0.75 {
        -1.0
    } else if h_anchor >= 0.25 {
        0.5
    } else {
        1.0
    };
    (dir, k)
}

/// Render inputs for [`layout_mtext`]. The caller resolves the text style
/// once and feeds the entity's geometry; the helper handles the entire
/// parse → wrap → render pipeline and returns both the rendered strokes and
/// the layout metrics (so callers can also draw frames / fills / LOD
/// substitutes from the same numbers).
pub struct MTextRenderOpts<'a> {
    /// Raw MText-formatted value (the string the parser walks).
    pub value: &'a str,
    /// World-space insertion point — strokes are emitted with this as their
    /// origin (after the per-sub-line rotation + cursor offset).
    pub insertion: [f64; 3],
    /// Entity text height in world units.
    pub height: f32,
    /// Box width for word-wrap (0 → no wrap; lines flow at the insertion).
    pub rect_w: f32,
    /// Final rotation in radians (already composed with `is_upside_down`).
    pub rotation: f32,
    /// Resolved style (font + width factor + oblique). Width factor sign
    /// honours `is_backward` (negative → mirror).
    pub style: &'a ResolvedTextStyle,
    /// Horizontal anchor of the text block at the insertion point:
    /// 0.0 = left, 0.5 = center, 1.0 = right.
    pub attach_h_anchor: f32,
    /// Vertical anchor of the text block at the insertion point.
    pub v_anchor: MTextVAnchor,
    /// DXF code 44 — multiplier on the default 5/3-em baseline gap.
    pub line_spacing_factor: f32,
    /// `true` when the entity is laid out top-to-bottom (DXF code 71 = 2).
    pub vertical_text: bool,
    /// When true, `layout_mtext` also fills `MTextLayout::glyph_boxes` with
    /// one world-space box per visible character (used by the MText editor's
    /// click-to-select preview). Off in the hot render path.
    pub want_glyph_boxes: bool,
    /// The entity's column layout. `Default` (count 0) = a single column, which
    /// is what everything except a multi-column MTEXT wants.
    pub columns: MTextColumns,
}

/// An MTEXT's column layout, flattened from its `column_data`.
///
/// Content moves to the next column at a `\N` break. Filling a column and
/// spilling into the next on its own is not modelled: `heights` / `auto_height`
/// are not read, so a column runs as long as its content does.
#[derive(Clone, Copy, Debug, Default)]
pub struct MTextColumns {
    /// Column count. 0 or 1 both mean "no column layout".
    pub count: usize,
    /// Width of one column, world units.
    pub width: f32,
    /// Gap between two adjacent columns, world units.
    pub gutter: f32,
}

impl MTextColumns {
    /// Whether this describes a real multi-column layout.
    pub fn active(&self) -> bool {
        self.count > 1 && self.width > 1e-6
    }

    /// Left edge of column `i`, relative to the text block's own left edge.
    pub fn offset_of(&self, i: usize) -> f32 {
        i as f32 * (self.width + self.gutter)
    }
}

/// One selectable character in the laid-out text: its world-space AABB plus
/// the running index of visible characters (in reading order) so the editor
/// can map a clicked box back to an offset in the value.
#[derive(Clone, Copy, Debug)]
pub struct GlyphBox {
    pub vis: usize,
    pub xmin: f32,
    pub xmax: f32,
    pub ymin: f32,
    pub ymax: f32,
}

/// Output of [`layout_mtext`]: stroke groups + the geometry the caller
/// needs for surrounding chrome (frame / fill / LOD baseline-or-rect).
pub struct MTextLayout {
    /// One TextStroke per Word atom (Tab / Space contribute only to cursor
    /// advance). The `color` field on each stroke carries the inline
    /// `\C` / `\c` override when one was set, otherwise `None`.
    pub strokes: Vec<TextStroke>,
    /// Per-sub-line width in entity-local (pre-rotation) units. Includes
    /// any trailing whitespace that survived the trim — kept in sync with
    /// the cursor advance so the alignment numbers and the visible glyphs
    /// line up.
    pub line_widths: Vec<f32>,
    /// Sub-line count (≥ 1; an entity with an empty value still reports 1).
    pub line_count: usize,
    /// Baseline-to-baseline gap used when stepping between sub-lines.
    pub line_height: f32,
    /// Y of the first sub-line's baseline relative to the insertion point
    /// (in the entity-local, pre-rotation frame).
    pub v_offset: f32,
    /// One world-space AABB per visible character — only populated when
    /// `MTextRenderOpts::want_glyph_boxes` is set.
    pub glyph_boxes: Vec<GlyphBox>,
    /// Pre-rotation glyph bounds in entity-local coordinates relative to the
    /// insertion point: [min_x, min_y, max_x, max_y]. Inverted (min > max)
    /// when the layout produced no glyphs. Valid for horizontal AND vertical
    /// flow, so frame/background boxes can wrap what was actually drawn.
    pub local_bounds: [f32; 4],
}

/// Lay out and render an MText-formatted value, returning the stroke
/// groups plus the layout metrics needed by callers that draw chrome
/// (text frame, background fill, low-detail LOD substitutes) around the
/// text block.
pub fn layout_mtext(opts: &MTextRenderOpts) -> MTextLayout {
    let base_font_name = opts.style.font_name.clone();
    let base_font = Face::resolve(&base_font_name);
    let base_wf_abs = opts.style.width_factor.max(0.01);
    let base_wf = if opts.style.is_backward { -base_wf_abs } else { base_wf_abs };
    let base_oblique = opts.style.oblique_angle;
    let entity_h = opts.height;
    let rect_w = opts.rect_w;

    // ── 1. Parse ─────────────────────────────────────────────────────────
    // The editor (want_glyph_boxes) keeps blank edges so a freshly typed
    // trailing newline yields an empty paragraph the caret can sit on.
    let paragraphs = adapt_mtext_paragraphs(opts.value, entity_h, !opts.want_glyph_boxes);

    // ── 2. Atomise + wrap each paragraph into sub-lines ──────────────────
    struct SubLine {
        atoms: Vec<LayoutAtom>,
        align: Option<ParagraphAlign>,
        indent_first: f32,
        indent_left: f32,
        indent_right: f32,
        tab_stops: Vec<TabStop>,
        is_first_in_paragraph: bool,
        /// Which column this line lives in; always 0 without a column layout.
        column: usize,
        /// Extra gap above (paragraph's first wrapped line only) / below (last
        /// line only), in drawing units, from `\p…b#/a#;`.
        space_before: f32,
        space_after: f32,
        /// Paragraph line-spacing override applied to this line's advance.
        line_spacing: Option<ParaLineSpacing>,
    }

    let cols = opts.columns;
    // A `\N` past the last column has nowhere to go — keep those lines in the
    // last one rather than laying them out beyond the block.
    let last_col = cols.count.saturating_sub(1);
    let mut column = 0usize;

    let mut sub_lines: Vec<SubLine> = Vec::new();
    for para in &paragraphs {
        if cols.active() && para.starts_column {
            column = (column + 1).min(last_col);
        }
        let mut atoms: Vec<LayoutAtom> = Vec::new();
        for run in &para.runs {
            match &run.kind {
                MTextRunKind::Glyphs(text) => {
                    let mut word = String::new();
                    for ch in text.chars() {
                        if ch == '\u{00A0}' {
                            // Non-breaking space (`\~`): keep it inside the word
                            // so the line can't wrap here. It still renders as a
                            // space — the font advances over the ' ' — but the
                            // words it joins stay together.
                            word.push(' ');
                        } else if ch == ' ' || ch == '\t' {
                            if !word.is_empty() {
                                atoms.push(LayoutAtom {
                                    kind: AtomKind::Word(std::mem::take(&mut word)),
                                    state: run.state.clone(),
                                });
                            }
                            // A literal tab (acadrust keeps `^I` / `\t` as a tab
                            // char in the span) advances to the paragraph's next
                            // tab stop, aligning the field that follows it.
                            atoms.push(LayoutAtom {
                                kind: if ch == '\t' {
                                    AtomKind::Tab
                                } else {
                                    AtomKind::Space
                                },
                                state: run.state.clone(),
                            });
                        } else {
                            word.push(ch);
                        }
                    }
                    if !word.is_empty() {
                        atoms.push(LayoutAtom {
                            kind: AtomKind::Word(word),
                            state: run.state.clone(),
                        });
                    }
                }
                MTextRunKind::Tab => {
                    atoms.push(LayoutAtom {
                        kind: AtomKind::Tab,
                        state: run.state.clone(),
                    });
                }
                MTextRunKind::Stack {
                    numerator,
                    denominator,
                    bar,
                } => {
                    atoms.push(LayoutAtom {
                        kind: AtomKind::Stack {
                            numerator: numerator.clone(),
                            denominator: denominator.clone(),
                            bar: *bar,
                        },
                        state: run.state.clone(),
                    });
                }
            }
        }

        // Trim leading + trailing Space atoms so line_w / cursor_start agree
        // on the paragraph's visible content. Without this a stray trailing
        // space measures wider than it draws and centring / right-alignment
        // is off by half a space-width.
        //
        // Skipped when emitting glyph boxes (the MText editor) so a space the
        // user just typed at the end keeps a selectable box and the caret can
        // sit after it.
        if !opts.want_glyph_boxes {
            let first_word = atoms
                .iter()
                .position(|a| !matches!(a.kind, AtomKind::Space))
                .unwrap_or(atoms.len());
            atoms.drain(..first_word);
            while matches!(atoms.last().map(|a| &a.kind), Some(AtomKind::Space)) {
                atoms.pop();
            }
        }

        // Wrap to the column the text actually flows down, not to the block:
        // measuring against the full width would let a line run across the
        // gutter and over its neighbour. Vertical flow never pre-wraps
        // horizontally — there rect_w is the COLUMN length, applied by the
        // render's own down-the-column word wrap.
        let wrap_w = if opts.vertical_text {
            0.0
        } else if cols.active() {
            cols.width
        } else {
            rect_w
        };
        let wrapped = wrap_paragraph(
            atoms,
            wrap_w,
            para.indent_first,
            para.indent_left,
            para.indent_right,
            &para.tab_stops,
            entity_h,
            base_wf,
            &base_font_name,
        );
        let n_wrapped = wrapped.len();
        for (idx, atoms) in wrapped.into_iter().enumerate() {
            sub_lines.push(SubLine {
                atoms,
                align: para.align,
                indent_first: para.indent_first,
                indent_left: para.indent_left,
                indent_right: para.indent_right,
                tab_stops: para.tab_stops.clone(),
                is_first_in_paragraph: idx == 0,
                column,
                // The gap above rides the first wrapped line; the gap below the
                // last, so an interior wrap keeps normal single spacing.
                space_before: if idx == 0 { para.space_before } else { 0.0 },
                space_after: if idx + 1 == n_wrapped {
                    para.space_after
                } else {
                    0.0
                },
                line_spacing: para.line_spacing,
            });
        }
    }
    if sub_lines.is_empty() {
        sub_lines.push(SubLine {
            atoms: Vec::new(),
            align: None,
            indent_first: 0.0,
            indent_left: 0.0,
            indent_right: 0.0,
            tab_stops: Vec::new(),
            is_first_in_paragraph: true,
            column: 0,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        });
    }

    // ── 3. Block geometry (line spacing, attachment, rotation) ───────────
    let ls_factor = if opts.line_spacing_factor > 0.0 {
        opts.line_spacing_factor
    } else {
        1.0
    };
    // DXF code 44 — multiplier on the default 5/3-em baseline-to-baseline gap.
    let line_h = entity_h * ls_factor * (5.0 / 3.0) * base_font.line_spacing();
    let h = entity_h;
    // How far the first line's glyphs actually reach above the baseline, when
    // that is further than the line's own cap height.
    //
    // Ordinary letters stop at the cap, so this is the first line's nominal cap
    // and nothing below moves. Two exceptions push it: a symbol font whose
    // glyphs straddle the cap box (the diameter sign's stroke overhangs it top
    // and bottom by design), and — the reason the floor is the *line's* cap, not
    // the entity height — a small heading (`\H0.2x;First column`): anchoring by
    // the full entity height opened a whole blank line above it.
    let first_line_cap = sub_lines
        .first()
        .map(|line| {
            line.atoms
                .iter()
                .filter_map(|a| match &a.kind {
                    AtomKind::Word(_) => Some(a.state.height_mul * entity_h),
                    _ => None,
                })
                .fold(0.0_f32, f32::max)
        })
        .filter(|c| *c > 0.0)
        .unwrap_or(h);
    let first_line_ascent = sub_lines
        .first()
        .map(|line| {
            line.atoms
                .iter()
                .filter_map(|atom| {
                    let AtomKind::Word(text) = &atom.kind else {
                        return None;
                    };
                    let font = resolve_font(&atom.state, &base_font_name);
                    text_local_bounds(
                        &font,
                        text,
                        atom.state.height_mul * entity_h,
                        atom.state.width_mul * base_wf.abs(),
                        atom.state.oblique_rad,
                    )
                    .map(|b| b.ink_max[1])
                })
                .fold(first_line_cap, f32::max)
        })
        .unwrap_or(h);
    // Per-line vertical advance: a varying-height MText (inline `\H`) must
    // space each line by its own content height, not the entity height —
    // otherwise a small line still leaves a full-height gap and the block
    // grows several times too tall. Accumulated per column (columns stack
    // independently). Uniform-height text reduces to the constant `line_h`.
    // A blank line (an empty `\P` paragraph) carries no glyph to size it, so it
    // takes the height of the text around it — the last non-empty line — not the
    // full entity height, which would blow a small-text blank line up several
    // times too tall and shove everything below it down the page.
    let mut last_line_h = entity_h;
    let per_line_h: Vec<f32> = sub_lines
        .iter()
        .map(|s| {
            let mh = s
                .atoms
                .iter()
                .filter_map(|a| match &a.kind {
                    AtomKind::Word(_) => Some(a.state.height_mul * entity_h),
                    _ => None,
                })
                .fold(0.0_f32, f32::max);
            let mh = if mh > 0.0 {
                last_line_h = mh;
                mh
            } else {
                last_line_h
            };
            // A paragraph's own `\psm#`/`\pse#` overrides the entity factor for
            // its lines; `Exact` fixes the baseline gap outright.
            match s.line_spacing {
                Some(ParaLineSpacing::Exact(e)) if e > 0.0 => e,
                Some(ParaLineSpacing::Multiple(m)) if m > 0.0 => {
                    mh * m * (5.0 / 3.0) * base_font.line_spacing()
                }
                _ => mh * ls_factor * (5.0 / 3.0) * base_font.line_spacing(),
            }
        })
        .collect();
    // Per-line text height — the tallest Word on the line, an empty line
    // inheriting the previous line's height (same rule as `per_line_h`). Unlike
    // `line_max_h` it has no `entity_h` floor, so it reflects the ACTUAL text
    // size; used to size the empty-line caret slot to the surrounding text
    // rather than the raw entity height (which can be many times the visible
    // text when `\H…x;` runs shrink it).
    let line_text_h: Vec<f32> = {
        let mut last = entity_h;
        sub_lines
            .iter()
            .map(|s| {
                let mh = s
                    .atoms
                    .iter()
                    .filter_map(|a| match &a.kind {
                        AtomKind::Word(_) => Some(a.state.height_mul * entity_h),
                        _ => None,
                    })
                    .fold(0.0_f32, f32::max);
                if mh > 0.0 {
                    last = mh;
                    mh
                } else {
                    last
                }
            })
            .collect()
    };
    let line_y_offset: Vec<f32> = {
        let mut col_y = vec![0.0_f32; cols.count.max(1)];
        let mut started = vec![false; cols.count.max(1)];
        // Space owed below the previous paragraph in each column, paid into the
        // gap at the next paragraph's first line (alongside its space-before).
        let mut pending_after = vec![0.0_f32; cols.count.max(1)];
        sub_lines
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let c = s.column.min(col_y.len().saturating_sub(1));
                if started[c] {
                    col_y[c] -= per_line_h[i];
                    if s.is_first_in_paragraph {
                        col_y[c] -= s.space_before + pending_after[c];
                    }
                } else {
                    started[c] = true;
                    // The column's first paragraph still opens its space-before as
                    // a top margin — that gap is the entity's own `\p…b#;`, not a
                    // heuristic, so the heading sits where the drawing places it.
                    col_y[c] -= s.space_before;
                }
                pending_after[c] = s.space_after;
                col_y[c]
            })
            .collect()
    };
    let total_depth = -line_y_offset.iter().copied().fold(0.0_f32, f32::min);

    let v_offset = match opts.v_anchor {
        MTextVAnchor::Top => -first_line_ascent,
        MTextVAnchor::Middle => (total_depth - h) * 0.5,
        MTextVAnchor::Bottom => total_depth,
        MTextVAnchor::MiddleOfTopLine => -h * 0.5,
        MTextVAnchor::MiddleOfBottomLine => total_depth - h * 0.5,
        MTextVAnchor::BottomOfTopLine => 0.0,
    };
    let attach_h_anchor = opts.attach_h_anchor;
    let box_left = -attach_h_anchor * rect_w;
    let rot = opts.rotation;
    let (cos_r, sin_r) = (rot.cos(), rot.sin());
    let ins_x = opts.insertion[0];
    let ins_y = opts.insertion[1];

    // ── 4. Render each sub-line ──────────────────────────────────────────
    let mut all_strokes: Vec<TextStroke> = Vec::new();
    let mut local_bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    // Vertical-flow block metrics: each sub-line is a COLUMN (1.2h step), each
    // atom cell 1.4h tall. The justification anchors place this block the way
    // the horizontal path places its line stack.
    // With a defined width (the flow grip, measuring DOWN the column for
    // vertical text), a column wraps: characters past the limit spill into a
    // fresh column to the right.
    let v_cell = entity_h * 1.4;
    let v_col_limit: usize = if opts.vertical_text && rect_w > 0.0 {
        ((rect_w / v_cell).floor() as usize).max(1)
    } else {
        usize::MAX
    };
    let (v_block_w, v_block_h) = if opts.vertical_text {
        // Mirror the render's WORD-boundary column wrap exactly (a word that
        // would overrun moves whole to the next column; an over-long word
        // overflows its own column; a space at a column start/break is
        // consumed) — a char-count estimate here under-counts columns and
        // over/under-sizes the block, which drags every non-start-anchored
        // text away from its attachment.
        let sim_line = |sub: &SubLine| -> (usize, usize) {
            let mut cols = 1usize;
            let mut cur = 0usize;
            let mut tallest = 0usize;
            for a in &sub.atoms {
                match &a.kind {
                    AtomKind::Word(t) => {
                        let w = t.chars().count();
                        if v_col_limit != usize::MAX && cur > 0 && cur + w > v_col_limit {
                            cols += 1;
                            cur = 0;
                        }
                        cur += w;
                        tallest = tallest.max(cur);
                    }
                    _ => {
                        if cur == 0 {
                            continue;
                        }
                        if v_col_limit != usize::MAX && cur + 1 > v_col_limit {
                            cols += 1;
                            cur = 0;
                        } else {
                            cur += 1;
                            tallest = tallest.max(cur);
                        }
                    }
                }
            }
            (cols, tallest.max(1))
        };
        let mut total_cols = 0usize;
        let mut tallest = 0usize;
        for sub in &sub_lines {
            let (c, t) = sim_line(sub);
            total_cols += c;
            tallest = tallest.max(t);
        }
        (
            total_cols.max(1) as f32 * entity_h * 1.2,
            tallest as f32 * v_cell,
        )
    } else {
        (0.0, 0.0)
    };
    // Running column index across sub-lines (vertical flow): overflow columns
    // shift every later paragraph's column right.
    let mut v_col: usize = 0;
    let mut line_widths: Vec<f32> = Vec::with_capacity(sub_lines.len());
    let mut glyph_boxes: Vec<GlyphBox> = Vec::new();
    let mut vis: usize = 0;
    // Transform an entity-local point to world space (mirrors the stroke
    // origin maths) so glyph boxes line up with the drawn glyphs.
    let to_world = |line_base_x: f32, line_base_y: f32, lx: f32, ly: f32| -> (f32, f32) {
        let wdx = lx * cos_r - ly * sin_r;
        let wdy = lx * sin_r + ly * cos_r;
        (
            ins_x as f32 + line_base_x + wdx,
            ins_y as f32 + line_base_y + wdy,
        )
    };
    for (i, sub) in sub_lines.iter().enumerate() {
        // The block anchors on its full width; each column then sits at its own
        // offset inside it and wraps to its own width, not the block's.
        let (box_left, rect_w) = if cols.active() {
            (box_left + cols.offset_of(sub.column), cols.width)
        } else {
            (box_left, rect_w)
        };
        if opts.vertical_text && i > 0 {
            // Each paragraph starts a fresh column after the previous
            // paragraph's (possibly wrapped) columns.
            v_col += 1;
        }
        let v_col_start = v_col;
        let (line_lx, line_ly) = if opts.vertical_text {
            // Anchor the column block: X by the attachment's horizontal
            // anchor over the block width, Y so the block hangs below /
            // straddles / sits above the insertion per the vertical anchor
            // (columns run DOWN from their line_ly).
            let top_y = match opts.v_anchor {
                MTextVAnchor::Top | MTextVAnchor::MiddleOfTopLine => 0.0,
                MTextVAnchor::BottomOfTopLine => entity_h * 1.4,
                MTextVAnchor::Middle => v_block_h * 0.5,
                MTextVAnchor::Bottom | MTextVAnchor::MiddleOfBottomLine => v_block_h,
            };
            (
                -attach_h_anchor * v_block_w + v_col as f32 * entity_h * 1.2,
                top_y,
            )
        } else {
            (0.0, line_y_offset[i] + v_offset)
        };
        let line_base_x = line_lx * cos_r - line_ly * sin_r;
        let line_base_y = line_lx * sin_r + line_ly * cos_r;

        let content_left = if rect_w > 0.0 {
            box_left
                + if sub.is_first_in_paragraph {
                    sub.indent_first
                } else {
                    sub.indent_left
                }
        } else {
            0.0
        };
        let content_right = if rect_w > 0.0 {
            box_left + rect_w - sub.indent_right
        } else {
            0.0
        };

        let line_anchor: f32 = match sub.align {
            Some(ParagraphAlign::Left)
            | Some(ParagraphAlign::Justify)
            | Some(ParagraphAlign::Distribute) => 0.0,
            Some(ParagraphAlign::Center) => 0.5,
            Some(ParagraphAlign::Right) => 1.0,
            None => attach_h_anchor,
        };

        let line_w = line_total_width(
            &sub.atoms,
            entity_h,
            base_wf,
            &base_font_name,
            0.0,
            sub.indent_left,
            &sub.tab_stops,
        );
        line_widths.push(line_w);

        let cursor_start = if rect_w > 0.0 {
            let content_w = (content_right - content_left).max(0.0);
            content_left + (content_w - line_w) * line_anchor
        } else if line_anchor > 0.0 {
            -line_w * line_anchor
        } else {
            0.0
        };

        let line_max_h = sub
            .atoms
            .iter()
            .map(|a| a.state.height_mul * entity_h)
            .fold(entity_h, f32::max);

        // A paragraph break (explicit `\n` / `\P`) that started this line gets
        // a zero-width caret slot at the line start, so the MText editor can
        // place the caret on a fresh/empty line. Size it to the line's own text
        // height (the surrounding text for an empty line), not the raw entity
        // height — otherwise a blank line's caret is entity-tall, which is many
        // times the visible text when `\H…x;` runs shrink it.
        if opts.want_glyph_boxes && i > 0 && sub.is_first_in_paragraph {
            let caret_h = line_text_h.get(i).copied().unwrap_or(entity_h);
            let (ax, ay) = to_world(line_base_x, line_base_y, cursor_start, 0.0);
            let (_, by) = to_world(line_base_x, line_base_y, cursor_start, caret_h);
            glyph_boxes.push(GlyphBox {
                vis,
                xmin: ax,
                xmax: ax,
                ymin: ay.min(by),
                ymax: ay.max(by),
            });
            vis += 1;
        }

        // Justify / distribute fill the line to the content width. `qj` spreads
        // only the word gaps and leaves the paragraph's last line ragged; `qd`
        // spreads every glyph gap (letters and words) on every line. The slack
        // is shared as one gap `δ`: added to each space, and — for distribute —
        // to each glyph via letter tracking, so `δ·(chars+spaces) = slack`.
        let is_last_in_para = sub_lines
            .get(i + 1)
            .is_none_or(|n| n.is_first_in_paragraph || n.column != sub.column);
        let (spread_letters, gap) = {
            let justify =
                matches!(sub.align, Some(ParagraphAlign::Justify)) && !is_last_in_para;
            let distribute = matches!(sub.align, Some(ParagraphAlign::Distribute));
            let slack = (content_right - content_left).max(0.0) - line_w;
            if rect_w > 0.0 && (justify || distribute) && slack > 1e-6 {
                let spaces = sub
                    .atoms
                    .iter()
                    .filter(|a| matches!(a.kind, AtomKind::Space))
                    .count();
                if distribute {
                    let chars: usize = sub
                        .atoms
                        .iter()
                        .filter_map(|a| match &a.kind {
                            AtomKind::Word(t) => Some(t.chars().count()),
                            _ => None,
                        })
                        .sum();
                    let n = chars + spaces;
                    (true, if n > 0 { slack / n as f32 } else { 0.0 })
                } else if spaces > 0 {
                    (false, slack / spaces as f32)
                } else {
                    (false, 0.0)
                }
            } else {
                (false, 0.0)
            }
        };

        let mut cursor_x = if opts.vertical_text { 0.0 } else { cursor_start };
        for (ai, atom) in sub.atoms.iter().enumerate() {
            match &atom.kind {
                AtomKind::Word(text) => {
                    let run_h = atom.state.height_mul * entity_h;
                    let signed_wf =
                        base_wf.signum() * atom.state.width_mul * base_wf.abs();
                    let oblique = base_oblique + atom.state.oblique_rad;
                    let font_name = resolve_font(&atom.state, &base_font_name);
                    if opts.vertical_text {
                        // Vertical (top-to-bottom) text: every character sits
                        // in its own cell stacked DOWN the column — the line
                        // cursor runs down, not across. Each char becomes its
                        // own single-glyph run so the SDF path draws it at its
                        // cell origin; 1.4×height is the classic vertical cell
                        // advance.
                        let color =
                            atom.state.color.as_ref().and_then(resolve_inline_color);
                        let cell = run_h * 1.4;
                        // Word wrap, vertically: a word that would overrun the
                        // column limit moves WHOLE to the next column (the
                        // vertical analogue of space-boundary wrapping). A word
                        // longer than the limit overflows its own column, like
                        // a long word overflows a narrow horizontal box.
                        let word_len = text.chars().count() as f32 * cell;
                        if v_col_limit != usize::MAX
                            && cursor_x > 0.0
                            && cursor_x + word_len > v_col_limit as f32 * v_cell + 1e-3
                        {
                            v_col += 1;
                            cursor_x = 0.0;
                        }
                        for ch in text.chars() {
                            let col_dx = (v_col - v_col_start) as f32 * entity_h * 1.2;
                            let s = ch.to_string();
                            let (strokes, fill_tris) = lff::tessellate_text_run(
                                [0.0, 0.0],
                                run_h,
                                rot,
                                signed_wf,
                                oblique,
                                atom.state.tracking,
                                &font_name,
                                &s,
                            );
                            let ly = -(cursor_x + cell);
                            let world_dx = col_dx * cos_r - ly * sin_r;
                            let world_dy = col_dx * sin_r + ly * cos_r;
                            let origin: [f64; 2] = [
                                ins_x + (line_base_x + world_dx) as f64,
                                ins_y + (line_base_y + world_dy) as f64,
                            ];
                            all_strokes.push(TextStroke {
                                strokes,
                                origin,
                                color,
                                fill_tris,
                                run: Some(GlyphRun {
                                    text: s,
                                    font: font_name.to_string(),
                                    height: run_h,
                                    rotation: rot,
                                    width_factor: signed_wf,
                                    oblique,
                                    tracking: atom.state.tracking,
                                    bold: atom.state.bold,
                                }),
                            });
                            let gx = line_lx + col_dx;
                            let (gy0, gy1) = (line_ly + ly, line_ly + ly + run_h);
                            local_bounds[0] = local_bounds[0].min(gx);
                            local_bounds[1] = local_bounds[1].min(gy0);
                            local_bounds[2] = local_bounds[2].max(gx + run_h);
                            local_bounds[3] = local_bounds[3].max(gy1);
                            vis += 1;
                            cursor_x += cell;
                        }
                        continue;
                    }
                    // Distribute widens letter tracking so each glyph advances an
                    // extra `gap`; `tracking` scales the font's letter_spacing, so
                    // invert that to land the exact world gap. A zero-spacing font
                    // (TTF) can't take letter tracking — its words stay solid and
                    // only the word gaps spread.
                    let tracking = if spread_letters {
                        let l = Face::resolve(&font_name).letter_spacing();
                        let wf = signed_wf.abs().max(1e-6);
                        let scale = run_h / 9.0;
                        if l.abs() > 1e-6 && scale > 0.0 {
                            atom.state.tracking + gap / (l * scale * wf)
                        } else {
                            atom.state.tracking
                        }
                    } else {
                        atom.state.tracking
                    };
                    let valign_dy = match atom.state.valign {
                        1 => (line_max_h - run_h) * 0.5,
                        2 => line_max_h - run_h,
                        _ => 0.0,
                    };
                    let color = atom.state.color.as_ref().and_then(resolve_inline_color);

                    let lx = cursor_x;
                    let ly = valign_dy;
                    let world_dx = lx * cos_r - ly * sin_r;
                    let world_dy = lx * sin_r + ly * cos_r;
                    let origin: [f64; 2] = [
                        ins_x + (line_base_x + world_dx) as f64,
                        ins_y + (line_base_y + world_dy) as f64,
                    ];
                    // Glyphs from the plain word — the SDF run and the stroke
                    // fallback both draw clean characters.
                    let (strokes, fill_tris) = lff::tessellate_text_run(
                        [0.0, 0.0],
                        run_h,
                        rot,
                        signed_wf,
                        oblique,
                        tracking,
                        &font_name,
                        text,
                    );
                    let glyph_n = strokes.len();
                    all_strokes.push(TextStroke {
                        strokes,
                        origin,
                        color,
                        fill_tris,
                        run: Some(GlyphRun {
                            text: text.clone(),
                            font: font_name.to_string(),
                            height: run_h,
                            rotation: rot,
                            width_factor: signed_wf,
                            oblique,
                            tracking,
                            bold: atom.state.bold,
                        }),
                    });
                    // Underline / overline / strike are lines, not glyphs; a
                    // run-group's strokes are suppressed by the SDF path, so
                    // emit the decorations in their own RUN-LESS group. Reuse
                    // lff's exact positions by tessellating the decorated word
                    // and taking the strokes it appends after the glyphs.
                    if atom.state.underline || atom.state.overline || atom.state.strike {
                        let body = decorated(text, &atom.state);
                        let (deco, _) = lff::tessellate_text_run(
                            [0.0, 0.0],
                            run_h,
                            rot,
                            signed_wf,
                            oblique,
                            tracking,
                            &font_name,
                            &body,
                        );
                        if deco.len() > glyph_n {
                            all_strokes.push(TextStroke {
                                strokes: deco[glyph_n..].to_vec(),
                                origin,
                                color,
                                fill_tris: vec![],
                                run: None,
                            });
                        }
                    }
                    if opts.want_glyph_boxes {
                        // Per-character boxes, advancing exactly as
                        // `measure_word` does so they track the glyphs.
                        let scale = run_scale(&atom.state, entity_h, base_wf);
                        let face = Face::resolve(&font_name);
                        let mut cx = cursor_x;
                        for ch in text.chars() {
                            let adv = match face.glyph(ch) {
                                Some(g) => {
                                    (g.advance + face.letter_spacing() * tracking) * scale
                                }
                                None => (6.0 + face.letter_spacing() * tracking) * scale,
                            };
                            let (ax, ay) = to_world(line_base_x, line_base_y, cx, ly);
                            let (bx, by) = to_world(line_base_x, line_base_y, cx + adv, ly + run_h);
                            glyph_boxes.push(GlyphBox {
                                vis,
                                xmin: ax.min(bx),
                                xmax: ax.max(bx),
                                ymin: ay.min(by),
                                ymax: ay.max(by),
                            });
                            vis += 1;
                            cx += adv;
                        }
                    }
                    // Advance by the same tracking the glyphs drew with, so a
                    // distributed word's letters and the atoms after it stay put.
                    let word_x0 = lx;
                    if tracking != atom.state.tracking {
                        let mut st = atom.state.clone();
                        st.tracking = tracking;
                        cursor_x += measure_word(text, &st, entity_h, base_wf, &base_font_name);
                    } else {
                        cursor_x +=
                            measure_word(text, &atom.state, entity_h, base_wf, &base_font_name);
                    }
                    local_bounds[0] = local_bounds[0].min(word_x0);
                    local_bounds[1] = local_bounds[1].min(line_ly + ly);
                    local_bounds[2] = local_bounds[2].max(cursor_x);
                    local_bounds[3] = local_bounds[3].max(line_ly + ly + run_h);
                }
                AtomKind::Stack {
                    numerator,
                    denominator,
                    bar,
                } => {
                    let run_h = atom.state.height_mul * entity_h;
                    let signed_wf = base_wf.signum() * atom.state.width_mul * base_wf.abs();
                    let oblique = base_oblique + atom.state.oblique_rad;
                    let font_name = resolve_font(&atom.state, &base_font_name);
                    let tracking = atom.state.tracking;
                    let color = atom.state.color.as_ref().and_then(resolve_inline_color);
                    let valign_dy = match atom.state.valign {
                        1 => (line_max_h - run_h) * 0.5,
                        2 => line_max_h - run_h,
                        _ => 0.0,
                    };

                    // Both halves draw at the shrunk height; the numerator sits
                    // a fixed lift above the denominator's baseline, so the two
                    // occupy one slot instead of running on horizontally.
                    let mut half_state = atom.state.clone();
                    half_state.height_mul *= STACK_HALF_SCALE;
                    let half_h = run_h * STACK_HALF_SCALE;

                    let slot_w = atom_width(atom, entity_h, base_wf, &base_font_name);

                    let mut emit = |text: &str, ly: f32| {
                        if text.is_empty() {
                            return;
                        }
                        let lx = cursor_x;
                        let world_dx = lx * cos_r - ly * sin_r;
                        let world_dy = lx * sin_r + ly * cos_r;
                        let origin: [f64; 2] = [
                            ins_x + (line_base_x + world_dx) as f64,
                            ins_y + (line_base_y + world_dy) as f64,
                        ];
                        let (strokes, fill_tris) = lff::tessellate_text_run(
                            [0.0, 0.0],
                            half_h,
                            rot,
                            signed_wf,
                            oblique,
                            tracking,
                            &font_name,
                            text,
                        );
                        all_strokes.push(TextStroke {
                            strokes,
                            origin,
                            color,
                            fill_tris,
                            run: Some(GlyphRun {
                                text: text.to_string(),
                                font: font_name.to_string(),
                                height: half_h,
                                rotation: rot,
                                width_factor: signed_wf,
                                oblique,
                                tracking,
                                bold: atom.state.bold,
                            }),
                        });
                    };
                    emit(denominator, valign_dy);
                    emit(numerator, valign_dy + run_h * STACK_RAISE);

                    if *bar && slot_w > 0.0 {
                        // The rule is plain geometry, not a glyph, so it rides a
                        // run-less group — those keep their strokes, while a
                        // group carrying a `GlyphRun` renders as SDF quads and
                        // has its strokes suppressed.
                        let by = valign_dy + run_h * STACK_BAR_Y;
                        let p = |lx: f32, ly: f32| -> [f32; 2] {
                            [lx * cos_r - ly * sin_r, lx * sin_r + ly * cos_r]
                        };
                        let (ox, oy) = (
                            ins_x + (line_base_x + p(cursor_x, by)[0]) as f64,
                            ins_y + (line_base_y + p(cursor_x, by)[1]) as f64,
                        );
                        let end = p(slot_w, 0.0);
                        all_strokes.push(TextStroke {
                            strokes: vec![vec![[0.0, 0.0], end]],
                            origin: [ox, oy],
                            color,
                            fill_tris: Vec::new(),
                            run: None,
                        });
                    }
                    cursor_x += slot_w;
                }
                AtomKind::Space => {
                    if opts.vertical_text {
                        // One empty cell down the column; a space that lands
                        // on a column break is consumed by it (no leading
                        // blank cell at the top of a column).
                        let cell = atom.state.height_mul * entity_h * 1.4;
                        if cursor_x <= 1e-6 {
                            continue;
                        }
                        if v_col_limit != usize::MAX
                            && cursor_x + cell > v_col_limit as f32 * v_cell + 1e-3
                        {
                            v_col += 1;
                            cursor_x = 0.0;
                        } else {
                            cursor_x += cell;
                        }
                        continue;
                    }
                    // Justify / distribute widen every word gap by `gap`.
                    let adv =
                        measure_space(&atom.state, entity_h, base_wf, &base_font_name) + gap;
                    // A space inside a decorated span carries the underline /
                    // overline / strike state but draws no glyph, so without this
                    // the rule breaks at every inter-word gap. Emit the decoration
                    // for the blank the same way words do — lff advances a space by
                    // exactly `measure_space`, so the rule meets the neighbours'.
                    if atom.state.underline || atom.state.overline || atom.state.strike {
                        let run_h = atom.state.height_mul * entity_h;
                        let signed_wf =
                            base_wf.signum() * atom.state.width_mul * base_wf.abs();
                        let oblique = base_oblique + atom.state.oblique_rad;
                        let font_name = resolve_font(&atom.state, &base_font_name);
                        let tracking = atom.state.tracking;
                        let valign_dy = match atom.state.valign {
                            1 => (line_max_h - run_h) * 0.5,
                            2 => line_max_h - run_h,
                            _ => 0.0,
                        };
                        let color =
                            atom.state.color.as_ref().and_then(resolve_inline_color);
                        let lx = cursor_x;
                        let ly = valign_dy;
                        let origin: [f64; 2] = [
                            ins_x + (line_base_x + lx * cos_r - ly * sin_r) as f64,
                            ins_y + (line_base_y + lx * sin_r + ly * cos_r) as f64,
                        ];
                        let body = decorated(" ", &atom.state);
                        let (deco, _) = lff::tessellate_text_run(
                            [0.0, 0.0],
                            run_h,
                            rot,
                            signed_wf,
                            oblique,
                            tracking,
                            &font_name,
                            &body,
                        );
                        if !deco.is_empty() {
                            all_strokes.push(TextStroke {
                                strokes: deco,
                                origin,
                                color,
                                fill_tris: vec![],
                                run: None,
                            });
                        }
                    }
                    if opts.want_glyph_boxes {
                        let run_h = atom.state.height_mul * entity_h;
                        let (ax, ay) = to_world(line_base_x, line_base_y, cursor_x, 0.0);
                        let (bx, by) =
                            to_world(line_base_x, line_base_y, cursor_x + adv, run_h);
                        glyph_boxes.push(GlyphBox {
                            vis,
                            xmin: ax.min(bx),
                            xmax: ax.max(bx),
                            ymin: ay.min(by),
                            ymax: ay.max(by),
                        });
                        vis += 1;
                    }
                    cursor_x += adv;
                }
                AtomKind::Tab => {
                    // A center/right/decimal stop aligns the field that follows
                    // the tab (the run of words up to the next space/tab) rather
                    // than just advancing the pen. Decimal places the first `.`
                    // on the stop; right the field's end; center its middle.
                    //
                    // Stop positions are relative to the paragraph's own left
                    // edge, so measure against `box_left` (the column's left,
                    // which `cursor_x` already carries) — not the raw indent, or
                    // a second column's tabs miss every stop and shoot off right.
                    let tab_base = box_left + sub.indent_left;
                    let local = cursor_x - tab_base;
                    let stop = sub
                        .tab_stops
                        .iter()
                        .find(|ts| ts.position > local + 1e-4)
                        .copied();
                    match stop.map(|s| s.kind) {
                        Some(TabKind::Center | TabKind::Right | TabKind::Decimal) => {
                            let sx = tab_base + stop.unwrap().position;
                            let mut field_w = 0.0_f32;
                            let mut before_dot: Option<f32> = None;
                            for next in &sub.atoms[ai + 1..] {
                                match &next.kind {
                                    AtomKind::Word(t) => {
                                        if before_dot.is_none() {
                                            if let Some(dp) = t.find('.') {
                                                let pre: String = t[..dp].to_string();
                                                before_dot = Some(
                                                    field_w
                                                        + measure_word(
                                                            &pre,
                                                            &next.state,
                                                            entity_h,
                                                            base_wf,
                                                            &base_font_name,
                                                        ),
                                                );
                                            }
                                        }
                                        field_w += measure_word(
                                            t,
                                            &next.state,
                                            entity_h,
                                            base_wf,
                                            &base_font_name,
                                        );
                                    }
                                    AtomKind::Stack { .. } => {
                                        field_w += atom_width(
                                            next,
                                            entity_h,
                                            base_wf,
                                            &base_font_name,
                                        );
                                    }
                                    _ => break,
                                }
                            }
                            let offset = match stop.unwrap().kind {
                                TabKind::Right => field_w,
                                TabKind::Center => field_w * 0.5,
                                // No dot in the field → align its end, as AutoCAD
                                // does for a decimal tab with no decimal point.
                                TabKind::Decimal => before_dot.unwrap_or(field_w),
                                TabKind::Left => 0.0,
                            };
                            // The stop wins even when the field is wider than the
                            // room before it — the dot/edge lands on the stop and
                            // the field overruns to the left, matching AutoCAD.
                            cursor_x = sx - offset;
                        }
                        _ => {
                            cursor_x = next_tab_position(
                                cursor_x,
                                &sub.tab_stops,
                                tab_base,
                                entity_h,
                            );
                        }
                    }
                }
            }
        }
    }

    MTextLayout {
        strokes: all_strokes,
        line_widths,
        line_count: sub_lines.len(),
        line_height: line_h,
        v_offset,
        glyph_boxes,
        local_bounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(font_name: &str) -> ResolvedTextStyle {
        ResolvedTextStyle {
            font_name: font_name.to_string(),
            width_factor: 1.0,
            oblique_angle: 0.0,
            is_backward: false,
            is_upside_down: false,
        }
    }

    fn stroke_point_count(layout: &MTextLayout) -> usize {
        layout
            .strokes
            .iter()
            .map(|s| s.strokes.iter().map(Vec::len).sum::<usize>() + s.fill_tris.len())
            .sum()
    }

    #[test]
    fn unresolved_inline_font_falls_back_to_style_font() {
        let base = "txt";
        let mut state = RunState::default();
        state.font = Some("__definitely_not_an_installed_font__".to_string());

        assert_eq!(resolve_font(&state, base), base);

        let layout = layout_mtext(&MTextRenderOpts {
            columns: Default::default(),
            value: "{\\f__definitely_not_an_installed_font__|b0|i0|c0|p34;Storage Units}",
            insertion: [0.0, 0.0, 0.0],
            height: 2.5,
            rect_w: 0.0,
            rotation: 0.0,
            style: &style(base),
            attach_h_anchor: 0.0,
            v_anchor: MTextVAnchor::Top,
            line_spacing_factor: 1.0,
            vertical_text: false,
            want_glyph_boxes: false,
        });

        assert!(
            stroke_point_count(&layout) > 0,
            "unresolvable inline \\f should render through the style font"
        );
    }

    #[test]
    fn block_style_font_name_from_ttf_file_renders_mtext() {
        let layout = layout_mtext(&MTextRenderOpts {
            columns: Default::default(),
            value: "FERRAGAMO",
            insertion: [0.0, 0.0, 0.0],
            height: 20.0,
            rect_w: 0.0,
            rotation: 0.0,
            style: &style("arial"),
            attach_h_anchor: 0.0,
            v_anchor: MTextVAnchor::Top,
            line_spacing_factor: 1.0,
            vertical_text: false,
            want_glyph_boxes: false,
        });

        assert!(
            stroke_point_count(&layout) > 0,
            "style font derived from arial.ttf should produce drawable block text"
        );
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    /// Glyph text + state of the first renderable run.
    fn first_run(lines: &[MTextLine]) -> (String, RunState) {
        for l in lines {
            for r in &l.runs {
                if let MTextRunKind::Glyphs(t) = &r.kind {
                    return (t.clone(), r.state.clone());
                }
            }
        }
        panic!("no glyph run produced");
    }

    #[test]
    fn plain_text_is_default_state() {
        let lines = adapt_mtext_paragraphs("Hello", 2.5, true);
        assert_eq!(lines.len(), 1);
        let (t, st) = first_run(&lines);
        assert_eq!(t, "Hello");
        assert_eq!(st, RunState::default());
    }

    #[test]
    fn relative_height_is_a_factor() {
        // `\H2x;` multiplies the current height → height_mul 2.0, independent of
        // the entity height. (This is the case that needed acadrust's MTextScalar.)
        let (_, st) = first_run(&adapt_mtext_paragraphs("\\H2x;big", 2.5, true));
        assert!((st.height_mul - 2.0).abs() < 1e-4, "got {}", st.height_mul);
    }

    #[test]
    fn absolute_height_resolves_against_entity_height() {
        // `\H5;` is an absolute height → divided by the entity height (2.5) → 2.0.
        let (_, st) = first_run(&adapt_mtext_paragraphs("\\H5;abs", 2.5, true));
        assert!((st.height_mul - 2.0).abs() < 1e-4, "got {}", st.height_mul);
    }

    #[test]
    fn color_font_oblique_valign_decoration() {
        let (_, c) = first_run(&adapt_mtext_paragraphs("\\C1;red", 2.5, true));
        assert_eq!(c.color, Some(InlineColor::Aci(1)));

        let (_, f) = first_run(&adapt_mtext_paragraphs("\\fArial;x", 2.5, true));
        assert_eq!(f.font.as_deref(), Some("Arial"));

        let (_, q) = first_run(&adapt_mtext_paragraphs("\\Q15;x", 2.5, true));
        assert!((q.oblique_rad - 15f32.to_radians()).abs() < 1e-4);

        let (_, w) = first_run(&adapt_mtext_paragraphs("\\W1.5;x", 2.5, true));
        assert!((w.width_mul - 1.5).abs() < 1e-4);

        let (_, a) = first_run(&adapt_mtext_paragraphs("\\A1;x", 2.5, true));
        assert_eq!(a.valign, 1);

        let (_, d) = first_run(&adapt_mtext_paragraphs("\\Lunder\\l", 2.5, true));
        assert!(d.underline);
    }

    #[test]
    fn font_italic_flag_renders_as_oblique() {
        // A `\f…|i1;` italic font has no true-italic glyphs in the stroke/SDF
        // faces, so it must render as the same slant the editor's Italic button
        // applies (~15°) — not upright.
        let (_, it) = first_run(&adapt_mtext_paragraphs("\\fArial|i1;x", 2.5, true));
        assert!(
            (it.oblique_rad - 15f32.to_radians()).abs() < 1e-4,
            "italic flag should slant, got {}",
            it.oblique_rad
        );
        // A non-italic font stays upright.
        let (_, up) = first_run(&adapt_mtext_paragraphs("\\fArial|i0;x", 2.5, true));
        assert!(up.oblique_rad.abs() < 1e-4, "got {}", up.oblique_rad);
        // An explicit `\Q` wins over the italic flag (no double slant).
        let (_, q) = first_run(&adapt_mtext_paragraphs("\\Q30;\\fArial|i1;x", 2.5, true));
        assert!((q.oblique_rad - 30f32.to_radians()).abs() < 1e-4, "got {}", q.oblique_rad);
    }

    #[test]
    fn stacking_keeps_its_halves_apart() {
        // `\S` reaches layout as a stack, not as flattened text: the halves
        // share one slot, which is what keeps a fraction from widening the line.
        let lines = adapt_mtext_paragraphs("\\S1/2;", 2.5, true);
        match &lines[0].runs[0].kind {
            MTextRunKind::Stack {
                numerator,
                denominator,
                bar,
            } => {
                assert_eq!((numerator.as_str(), denominator.as_str()), ("1", "2"));
                assert!(*bar, "`/` draws a fraction rule");
            }
            other => panic!("expected a stack run, got {other:?}"),
        }

        // `^` is the same stack without the rule.
        let lines = adapt_mtext_paragraphs("\\S1^2;", 2.5, true);
        match &lines[0].runs[0].kind {
            MTextRunKind::Stack { bar, .. } => assert!(!*bar, "`^` is a limit, no rule"),
            other => panic!("expected a stack run, got {other:?}"),
        }

        // Diagonal stays inline `num/den` — that is what a slanted fraction
        // reads as, and it needs no second baseline.
        let (t, _) = first_run(&adapt_mtext_paragraphs("\\S1#2;", 2.5, true));
        assert_eq!(t, "1/2");
    }

    #[test]
    fn paragraph_breaks_split_lines() {
        let lines = adapt_mtext_paragraphs("a\\Pb\\Pc", 2.5, true);
        assert_eq!(lines.len(), 3);
    }

}


#[cfg(test)]
mod v_anchor_tests {
    use super::*;

    /// The highest point the layout actually DRAWS, in entity-local units.
    /// `glyph_boxes` are the cap box the editor's caret rides on, not the ink,
    /// so they cannot answer this.
    fn ink_top(l: &MTextLayout) -> f32 {
        l.strokes
            .iter()
            .flat_map(|s| {
                s.strokes
                    .iter()
                    .flatten()
                    .map(move |p| s.origin[1] as f32 + p[1])
            })
            .fold(f32::MIN, f32::max)
    }

    fn layout_at_top(value: &str, height: f32) -> MTextLayout {
        let style = ResolvedTextStyle {
            font_name: "txt".to_string(),
            width_factor: 1.0,
            oblique_angle: 0.0,
            is_backward: false,
            is_upside_down: false,
        };
        layout_mtext(&MTextRenderOpts {
            columns: Default::default(),
            value,
            insertion: [0.0, 0.0, 0.0],
            height,
            rect_w: 0.0,
            rotation: 0.0,
            style: &style,
            attach_h_anchor: 0.0,
            v_anchor: MTextVAnchor::Top,
            line_spacing_factor: 1.0,
            vertical_text: false,
            want_glyph_boxes: true,
        })
    }

    /// The top of what a line draws sits at the insertion point.
    ///
    /// For ordinary letters that is the cap height and nothing moves. A symbol
    /// font's glyphs straddle the cap box on purpose — the diameter sign's
    /// stroke overhangs it — and anchoring by the nominal cap hung the glyph
    /// above where the drawing expects it, by the size of the overhang.
    #[test]
    fn a_glyph_that_overhangs_the_cap_still_starts_at_the_insertion_point() {
        // The benchmark's own case: a diameter sign at height 3, whose stroke
        // reaches 11.2117 of 9 cap units — 3.7372 in world units, not 3.
        let top = ink_top(&layout_at_top("{\\Fgdt.shx;n}", 3.0));
        assert!(
            top.abs() < 0.05,
            "the glyph draws down from {top}, not from the insertion point"
        );
    }

    /// …and plain text must not move: its letters stop at the cap, so the top
    /// anchor stays the cap height exactly as before.
    #[test]
    fn plain_text_still_hangs_from_its_cap_height() {
        for s in ["ABC", "abc", "Hello world"] {
            let top = ink_top(&layout_at_top(s, 3.0));
            assert!(top <= 0.05, "{s:?} drew above the insertion point ({top})");
            // An all-lowercase line stops at its x-height and must NOT be
            // pulled up to meet the insertion point.
            assert!(
                top > -3.0,
                "{s:?} was pushed down to {top}; the cap should still anchor it"
            );
        }
    }
}
