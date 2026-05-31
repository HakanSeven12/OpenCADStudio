use acadrust::types::aci_table::aci_to_rgb;
use acadrust::{CadDocument, EntityType};

use crate::scene::acad_to_truck::TextStroke;
use crate::scene::lff;

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

    let font_name = if let Some(style) = style {
        if !style.font_file.trim().is_empty() {
            let file = style.font_file.trim();
            let basename = file.rsplit(['/', '\\']).next().unwrap_or(file);
            let stem = basename.split('.').next().unwrap_or(basename).trim();
            if !stem.is_empty() {
                stem.to_string()
            } else if !style.true_type_font.trim().is_empty() {
                style.true_type_font.trim().to_string()
            } else if !style.name.trim().is_empty() {
                style.name.trim().to_string()
            } else {
                "Standard".to_string()
            }
        } else if !style.true_type_font.trim().is_empty() {
            style.true_type_font.trim().to_string()
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

    ResolvedTextStyle {
        font_name,
        width_factor: style.map(|s| s.width_factor as f32).unwrap_or(1.0),
        oblique_angle: style.map(|s| s.oblique_angle as f32).unwrap_or(0.0),
        is_backward: style.map(|s| s.is_backward()).unwrap_or(false),
        is_upside_down: style.map(|s| s.is_upside_down()).unwrap_or(false),
    }
}

pub fn text_local_bounds(
    font_name: &str,
    text: &str,
    height: f32,
    width_factor: f32,
    oblique_angle: f32,
) -> Option<([f32; 2], [f32; 2])> {
    if text.is_empty() || height <= 0.0 {
        return None;
    }

    let font = lff::get_font(font_name);
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
            cursor_x += font.word_spacing;
            continue;
        }
        match font.glyph(ch) {
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
                cursor_x += glyph.advance + font.letter_spacing;
            }
            None => {
                cursor_x += 6.0 + font.letter_spacing;
            }
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(([min_x, min_y], [max_x, max_y]))
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

/// Tab-stop alignment kind (from `\pt<L|C|R><pos>` entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKind {
    Left,
    Center,
    Right,
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
        }
    }
}

#[derive(Clone, Debug)]
pub enum MTextRunKind {
    /// Renderable glyph text (DXF specials resolved, decoration markers stripped).
    Glyphs(String),
    /// `\t` — jump the cursor to the next tab stop (or default tab interval).
    Tab,
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
}

impl MTextLine {
    pub fn is_blank(&self) -> bool {
        self.runs.iter().all(|r| match &r.kind {
            MTextRunKind::Glyphs(t) => t.trim().is_empty(),
            MTextRunKind::Tab => false,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct ParagraphProps {
    align: Option<ParagraphAlign>,
    indent_first: f32,
    indent_left: f32,
    indent_right: f32,
    tab_stops: Vec<TabStop>,
}

/// Parse a `\p...;` body. Comma-separated tokens, each with a single-letter
/// kind prefix. Unknown tokens are skipped — anything we don't understand is
/// silently dropped rather than poisoning the rest of the paragraph.
fn parse_paragraph_block(body: &str, props: &mut ParagraphProps) {
    // The legacy AutoCAD writer prefixes the block with a redundant `x`; skip
    // it so it doesn't confuse the kind matcher.
    let body = body.strip_prefix('x').unwrap_or(body);
    for token in body.split(',') {
        let token = token.trim();
        let mut chars = token.chars();
        let Some(kind) = chars.next() else { continue };
        let rest: String = chars.collect();
        match kind {
            'q' | 'Q' => {
                let sel = rest.chars().next().map(|c| c.to_ascii_lowercase());
                props.align = match sel {
                    Some('l') => Some(ParagraphAlign::Left),
                    Some('c') => Some(ParagraphAlign::Center),
                    Some('r') => Some(ParagraphAlign::Right),
                    Some('j') => Some(ParagraphAlign::Justify),
                    Some('d') => Some(ParagraphAlign::Distribute),
                    _ => props.align,
                };
            }
            'i' => {
                if let Ok(v) = rest.parse::<f32>() {
                    props.indent_first = v;
                }
            }
            'l' => {
                if let Ok(v) = rest.parse::<f32>() {
                    props.indent_left = v;
                }
            }
            'r' => {
                if let Ok(v) = rest.parse::<f32>() {
                    props.indent_right = v;
                }
            }
            't' => {
                // Tab list. Each entry may be prefixed with L / C / R to
                // pick the tab kind; default is Left. The remainder is the
                // position in drawing units.
                props.tab_stops.clear();
                for entry in rest.split(',') {
                    let entry = entry.trim();
                    if entry.is_empty() {
                        continue;
                    }
                    let (kind, num_str) = match entry.chars().next() {
                        Some(c @ ('L' | 'l')) => (TabKind::Left, &entry[c.len_utf8()..]),
                        Some(c @ ('C' | 'c')) => (TabKind::Center, &entry[c.len_utf8()..]),
                        Some(c @ ('R' | 'r')) => (TabKind::Right, &entry[c.len_utf8()..]),
                        _ => (TabKind::Left, entry),
                    };
                    if let Ok(p) = num_str.parse::<f32>() {
                        props.tab_stops.push(TabStop { position: p, kind });
                    }
                }
            }
            's' => {} // space-before — affects line spacing; ignored for now
            _ => {}
        }
    }
}

/// Parse an unsigned 24-bit true-colour value into linear-ish [r,g,b] floats.
/// AutoCAD packs `\c` as a 24-bit decimal: high byte = R, mid = G, low = B.
fn parse_true_color(s: &str) -> Option<InlineColor> {
    let n: u32 = s.trim().parse().ok()?;
    let r = ((n >> 16) & 0xFF) as f32 / 255.0;
    let g = ((n >> 8) & 0xFF) as f32 / 255.0;
    let b = (n & 0xFF) as f32 / 255.0;
    Some(InlineColor::True([r, g, b]))
}

/// Parse `\H` / `\W` / `\T` value. Returns `(value, is_relative)`; a trailing
/// `x` marks a multiplier on the current state, otherwise the value is the
/// absolute target (interpreted by the caller against entity defaults).
fn parse_scalar_with_x_suffix(body: &str) -> Option<(f32, bool)> {
    let body = body.trim();
    let (num, is_rel) = if let Some(stripped) = body.strip_suffix('x').or_else(|| body.strip_suffix('X')) {
        (stripped, true)
    } else {
        (body, false)
    };
    Some((num.trim().parse::<f32>().ok()?, is_rel))
}

/// Flush the glyph buffer as a `Glyphs` run on `line` using a snapshot of
/// `state`. Resolves DXF `%%x` specials (degree / diameter / `%%nnn` literal
/// unicode) so the renderer sees fully decoded text.
fn flush_glyph_buf(line: &mut MTextLine, buf: &mut String, state: &RunState) {
    if buf.is_empty() {
        return;
    }
    let text = resolve_dxf_special_chars(&std::mem::take(buf));
    line.runs.push(MTextRun {
        kind: MTextRunKind::Glyphs(text),
        state: state.clone(),
    });
}

/// Walk the MTEXT value string and produce one [`MTextLine`] per visible
/// paragraph (after stripping leading / trailing blank lines, as the legacy
/// `split_mtext_lines` does). Every inline format code listed in the module
/// header is recognised; unknown semicolon-terminated codes are stripped
/// silently so future / vendor-specific extensions don't pollute the text.
///
/// `entity_height` is needed to translate absolute `\H<v>;` declarations into
/// the height-multiplier representation carried in [`RunState`]; pass the
/// MTEXT entity's `height` field.
pub fn parse_mtext_paragraphs(s: &str, entity_height: f32) -> Vec<MTextLine> {
    let mut lines: Vec<MTextLine> = Vec::new();
    let mut current = MTextLine::default();
    let mut buf = String::new();
    let mut state = RunState::default();
    let mut state_stack: Vec<RunState> = Vec::new();
    let mut props = ParagraphProps::default();
    // No props_stack — paragraph props persist across braces by design.
    let entity_height = entity_height.max(1e-6);

    let mut chars = s.chars().peekable();

    let read_until_semi = |chars: &mut std::iter::Peekable<std::str::Chars>| -> String {
        let mut out = String::new();
        for c in chars.by_ref() {
            if c == ';' {
                break;
            }
            out.push(c);
        }
        out
    };

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek().copied() {
                // ── Line / paragraph break ────────────────────────────────
                Some('P') | Some('n') | Some('N') => {
                    chars.next();
                    flush_glyph_buf(&mut current, &mut buf, &state);
                    current.align = props.align;
                    current.indent_first = props.indent_first;
                    current.indent_left = props.indent_left;
                    current.indent_right = props.indent_right;
                    current.tab_stops = props.tab_stops.clone();
                    lines.push(std::mem::take(&mut current));
                }
                // ── Whitespace literals ───────────────────────────────────
                Some('~') => {
                    chars.next();
                    buf.push('\u{00A0}'); // nbsp — treated as a regular space by the wrap pass
                }
                Some('t') => {
                    chars.next();
                    flush_glyph_buf(&mut current, &mut buf, &state);
                    current.runs.push(MTextRun {
                        kind: MTextRunKind::Tab,
                        state: state.clone(),
                    });
                }
                // ── Unicode by hex code point ────────────────────────────
                Some('U') | Some('u') => {
                    chars.next();
                    if chars.peek() == Some(&'+') {
                        chars.next();
                        let mut hex = String::with_capacity(6);
                        for _ in 0..6 {
                            match chars.peek() {
                                Some(&c) if c.is_ascii_hexdigit() => {
                                    hex.push(chars.next().unwrap());
                                }
                                _ => break,
                            }
                        }
                        if chars.peek() == Some(&';') {
                            chars.next();
                        }
                        if let Ok(n) = u32::from_str_radix(&hex, 16) {
                            if let Some(c) = char::from_u32(n) {
                                buf.push(c);
                                continue;
                            }
                        }
                    } else {
                        // Bare `\U` / `\u` — strip to next `;`
                        let _ = read_until_semi(&mut chars);
                    }
                }
                // ── Stacked text \S<u><sep><l>; ──────────────────────────
                Some('S') | Some('s') => {
                    chars.next();
                    let mut upper = String::new();
                    let mut lower = String::new();
                    let mut sep = '/';
                    let mut in_lower = false;
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                        if !in_lower && (c == '/' || c == '^' || c == '#') {
                            sep = c;
                            in_lower = true;
                        } else if in_lower {
                            lower.push(c);
                        } else {
                            upper.push(c);
                        }
                    }
                    buf.push_str(&upper);
                    if !lower.is_empty() {
                        buf.push(if sep == '#' { '/' } else { sep });
                        buf.push_str(&lower);
                    }
                }
                // ── Decoration toggles (state, not markers) ──────────────
                Some('L') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.underline = true; }
                Some('l') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.underline = false; }
                Some('O') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.overline = true; }
                Some('o') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.overline = false; }
                Some('K') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.strike = true; }
                Some('k') => { chars.next(); flush_glyph_buf(&mut current, &mut buf, &state); state.strike = false; }
                // ── Literal backslash / braces ───────────────────────────
                Some('\\') => { chars.next(); buf.push('\\'); }
                Some('{') | Some('}') => { buf.push(chars.next().unwrap()); }
                // ── Paragraph props ──────────────────────────────────────
                Some('p') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    parse_paragraph_block(&body, &mut props);
                }
                // ── Height ───────────────────────────────────────────────
                Some('H') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Some((v, is_rel)) = parse_scalar_with_x_suffix(&body) {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        if is_rel {
                            state.height_mul *= v;
                        } else {
                            state.height_mul = v / entity_height;
                        }
                    }
                }
                // ── Width factor ─────────────────────────────────────────
                Some('W') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Some((v, is_rel)) = parse_scalar_with_x_suffix(&body) {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        if is_rel {
                            state.width_mul *= v;
                        } else {
                            state.width_mul = v;
                        }
                    }
                }
                // ── Oblique angle (degrees → radians) ────────────────────
                Some('Q') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Ok(deg) = body.trim().parse::<f32>() {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        state.oblique_rad = deg.to_radians();
                    }
                }
                // ── Tracking ─────────────────────────────────────────────
                Some('T') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Some((v, is_rel)) = parse_scalar_with_x_suffix(&body) {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        if is_rel {
                            state.tracking *= v;
                        } else {
                            state.tracking = v;
                        }
                    }
                }
                // ── Vertical alignment ───────────────────────────────────
                Some('A') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Ok(n) = body.trim().parse::<u8>() {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        state.valign = n.min(2);
                    }
                }
                // ── ACI colour ───────────────────────────────────────────
                Some('C') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Ok(n) = body.trim().parse::<u32>() {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        state.color = Some(InlineColor::Aci(n.min(255) as u8));
                    }
                }
                // ── True colour ──────────────────────────────────────────
                Some('c') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    if let Some(col) = parse_true_color(&body) {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        state.color = Some(col);
                    }
                }
                // ── Font (name + b/i/c/p flags) ──────────────────────────
                Some('f') | Some('F') => {
                    chars.next();
                    let body = read_until_semi(&mut chars);
                    // First `|`-separated field is the font name / file stem.
                    let name = body.split('|').next().unwrap_or("").trim();
                    if !name.is_empty() {
                        flush_glyph_buf(&mut current, &mut buf, &state);
                        // Strip extension if `\F` passed a file path.
                        let stem = name
                            .rsplit(['/', '\\'])
                            .next()
                            .unwrap_or(name)
                            .split('.')
                            .next()
                            .unwrap_or(name);
                        state.font = Some(stem.to_string());
                    }
                }
                // ── Multibyte / codepage marker — strip silently ─────────
                Some('M') => {
                    chars.next();
                    let _ = read_until_semi(&mut chars);
                }
                // ── Dimension MTEXT paragraph-end marker — strip silently
                Some('X') => {
                    chars.next();
                }
                // ── Unknown single-letter escape ─────────────────────────
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            // Scope push / pop — braces scope *character* state (font,
            // colour, height, decorations, …). Paragraph properties
            // (`\p...;`) are intentionally NOT scoped: AutoCAD treats them
            // as paragraph-level layout that persists across braces, and
            // real-world files routinely wrap inline `\pxqc;` inside a
            // `{\fArial;…}` block while expecting the alignment to apply
            // to the whole paragraph.
            '{' => {
                state_stack.push(state.clone());
            }
            '}' => {
                if let Some(prev) = state_stack.pop() {
                    flush_glyph_buf(&mut current, &mut buf, &state);
                    state = prev;
                }
            }
            '\r' => {}
            '\n' => {
                flush_glyph_buf(&mut current, &mut buf, &state);
                current.align = props.align;
                current.indent_first = props.indent_first;
                current.indent_left = props.indent_left;
                current.indent_right = props.indent_right;
                current.tab_stops = props.tab_stops.clone();
                lines.push(std::mem::take(&mut current));
            }
            other => buf.push(other),
        }
    }
    flush_glyph_buf(&mut current, &mut buf, &state);
    current.align = props.align;
    current.indent_first = props.indent_first;
    current.indent_left = props.indent_left;
    current.indent_right = props.indent_right;
    current.tab_stops = props.tab_stops.clone();
    lines.push(current);

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
// stripping, paragraph splitting, per-run width measurement and word-wrap;
// keep `parse_mtext_paragraphs`, `layout_mtext`, `mtext_line_count`,
// `text_local_bounds`, and `resolve_dxf_special_chars` as the supported
// surface for callers.

/// Total number of visible lines an MText renders to — explicit `\P` /
/// `\n` / `\N` breaks plus word-wrap induced sublines when
/// `rectangle_width > 0`. Drives LOD splits (greek-rect per row, baseline
/// counts) without re-running the full stroke tessellation; routes through
/// the same parse + atomise + wrap pipeline as `layout_mtext` so the LOD
/// row count and the rendered row count never disagree.
pub fn mtext_line_count(
    m: &acadrust::entities::MText,
    document: &CadDocument,
    anno_scale: f32,
) -> usize {
    let resolved = resolve_text_style(&m.style, document);
    let entity_h = (m.height as f32) * anno_scale;
    let base_wf_abs = resolved.width_factor.max(0.01);
    let base_wf = if resolved.is_backward { -base_wf_abs } else { base_wf_abs };
    let base_font_name = resolved.font_name.clone();
    let rect_w = (m.rectangle_width as f32) * anno_scale;

    let paragraphs = parse_mtext_paragraphs(&m.value, entity_h);
    let mut total = 0usize;
    for para in &paragraphs {
        let mut atoms: Vec<LayoutAtom> = Vec::new();
        for run in &para.runs {
            match &run.kind {
                MTextRunKind::Glyphs(text) => {
                    let mut word = String::new();
                    for ch in text.chars() {
                        if ch == ' ' || ch == '\u{00A0}' {
                            if !word.is_empty() {
                                atoms.push(LayoutAtom {
                                    kind: AtomKind::Word(std::mem::take(&mut word)),
                                    state: run.state.clone(),
                                });
                            }
                            atoms.push(LayoutAtom {
                                kind: AtomKind::Space,
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
                MTextRunKind::Tab => atoms.push(LayoutAtom {
                    kind: AtomKind::Tab,
                    state: run.state.clone(),
                }),
            }
        }
        // Same edge-trim the renderer applies — otherwise a trailing space
        // can inflate the wrap result by one extra sub-line.
        let first_word = atoms
            .iter()
            .position(|a| !matches!(a.kind, AtomKind::Space))
            .unwrap_or(atoms.len());
        atoms.drain(..first_word);
        while matches!(atoms.last().map(|a| &a.kind), Some(AtomKind::Space)) {
            atoms.pop();
        }
        let wrapped = wrap_paragraph(
            atoms,
            rect_w,
            para.indent_first,
            para.indent_left,
            para.indent_right,
            &para.tab_stops,
            entity_h,
            base_wf,
            &base_font_name,
        );
        total += wrapped.len().max(1);
    }
    total.max(1)
}

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
//   1. Parse — via `parse_mtext_paragraphs`.
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
}

#[derive(Clone)]
pub struct LayoutAtom {
    pub kind: AtomKind,
    pub state: RunState,
}

pub fn run_scale(state: &RunState, entity_h: f32, base_wf: f32) -> f32 {
    (state.height_mul * entity_h / 9.0) * (state.width_mul * base_wf.abs())
}

pub fn resolve_font<'a>(state: &'a RunState, base: &'a str) -> &'a str {
    state.font.as_deref().unwrap_or(base)
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
    let font = lff::get_font(font_name);
    let mut w = 0.0_f32;
    for ch in text.chars() {
        w += match font.glyph(ch) {
            Some(g) => (g.advance + font.letter_spacing * state.tracking) * scale,
            None => (6.0 + font.letter_spacing * state.tracking) * scale,
        };
    }
    w
}

pub fn measure_space(state: &RunState, entity_h: f32, base_wf: f32, base_font: &str) -> f32 {
    let scale = run_scale(state, entity_h, base_wf);
    let font_name = resolve_font(state, base_font);
    lff::get_font(font_name).word_spacing * scale
}

pub fn atom_width(atom: &LayoutAtom, entity_h: f32, base_wf: f32, base_font: &str) -> f32 {
    match &atom.kind {
        AtomKind::Word(t) => measure_word(t, &atom.state, entity_h, base_wf, base_font),
        AtomKind::Space => measure_space(&atom.state, entity_h, base_wf, base_font),
        AtomKind::Tab => 0.0,
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
    let line_start_x = |idx: usize| if idx == 0 { indent_first } else { indent_left };
    let line_max_w = |idx: usize| (rect_w - indent_right - line_start_x(idx)).max(0.0);

    for atom in atoms {
        match &atom.kind {
            AtomKind::Word(_) => {
                let w = atom_width(&atom, entity_h, base_wf, base_font);
                let max_w = line_max_w(subline_idx);
                if !cur.is_empty() && cur_w + w > max_w {
                    while matches!(cur.last().map(|a| &a.kind), Some(AtomKind::Space)) {
                        cur.pop();
                    }
                    sublines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                    subline_idx += 1;
                }
                cur.push(atom);
                cur_w += w;
            }
            AtomKind::Space => {
                if cur.is_empty() {
                    continue;
                }
                cur_w += atom_width(&atom, entity_h, base_wf, base_font);
                cur.push(atom);
            }
            AtomKind::Tab => {
                let start_x = line_start_x(subline_idx);
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
}

/// Lay out and render an MText-formatted value, returning the stroke
/// groups plus the layout metrics needed by callers that draw chrome
/// (text frame, background fill, low-detail LOD substitutes) around the
/// text block.
pub fn layout_mtext(opts: &MTextRenderOpts) -> MTextLayout {
    let base_font_name = opts.style.font_name.clone();
    let base_font = lff::get_font(&base_font_name);
    let base_wf_abs = opts.style.width_factor.max(0.01);
    let base_wf = if opts.style.is_backward { -base_wf_abs } else { base_wf_abs };
    let base_oblique = opts.style.oblique_angle;
    let entity_h = opts.height;
    let rect_w = opts.rect_w;

    // ── 1. Parse ─────────────────────────────────────────────────────────
    let paragraphs = parse_mtext_paragraphs(opts.value, entity_h);

    // ── 2. Atomise + wrap each paragraph into sub-lines ──────────────────
    struct SubLine {
        atoms: Vec<LayoutAtom>,
        align: Option<ParagraphAlign>,
        indent_first: f32,
        indent_left: f32,
        indent_right: f32,
        tab_stops: Vec<TabStop>,
        is_first_in_paragraph: bool,
    }

    let mut sub_lines: Vec<SubLine> = Vec::new();
    for para in &paragraphs {
        let mut atoms: Vec<LayoutAtom> = Vec::new();
        for run in &para.runs {
            match &run.kind {
                MTextRunKind::Glyphs(text) => {
                    let mut word = String::new();
                    for ch in text.chars() {
                        if ch == ' ' || ch == '\u{00A0}' {
                            if !word.is_empty() {
                                atoms.push(LayoutAtom {
                                    kind: AtomKind::Word(std::mem::take(&mut word)),
                                    state: run.state.clone(),
                                });
                            }
                            atoms.push(LayoutAtom {
                                kind: AtomKind::Space,
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
            }
        }

        // Trim leading + trailing Space atoms so line_w / cursor_start agree
        // on the paragraph's visible content. Without this a stray trailing
        // space measures wider than it draws and centring / right-alignment
        // is off by half a space-width.
        let first_word = atoms
            .iter()
            .position(|a| !matches!(a.kind, AtomKind::Space))
            .unwrap_or(atoms.len());
        atoms.drain(..first_word);
        while matches!(atoms.last().map(|a| &a.kind), Some(AtomKind::Space)) {
            atoms.pop();
        }

        let wrapped = wrap_paragraph(
            atoms,
            rect_w,
            para.indent_first,
            para.indent_left,
            para.indent_right,
            &para.tab_stops,
            entity_h,
            base_wf,
            &base_font_name,
        );
        for (idx, atoms) in wrapped.into_iter().enumerate() {
            sub_lines.push(SubLine {
                atoms,
                align: para.align,
                indent_first: para.indent_first,
                indent_left: para.indent_left,
                indent_right: para.indent_right,
                tab_stops: para.tab_stops.clone(),
                is_first_in_paragraph: idx == 0,
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
        });
    }

    // ── 3. Block geometry (line spacing, attachment, rotation) ───────────
    let n_lines = sub_lines.len().max(1) as f32;
    let ls_factor = if opts.line_spacing_factor > 0.0 {
        opts.line_spacing_factor
    } else {
        1.0
    };
    // DXF code 44 — multiplier on the default 5/3-em baseline-to-baseline gap.
    let line_h = entity_h * ls_factor * (5.0 / 3.0) * base_font.line_spacing;
    let h = entity_h;
    let v_offset = match opts.v_anchor {
        MTextVAnchor::Top => -h,
        MTextVAnchor::Middle => ((n_lines - 1.0) * line_h - h) * 0.5,
        MTextVAnchor::Bottom => (n_lines - 1.0) * line_h,
        MTextVAnchor::MiddleOfTopLine => -h * 0.5,
        MTextVAnchor::MiddleOfBottomLine => (n_lines - 1.0) * line_h - h * 0.5,
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
        let li = i as f32;
        let (line_base_x, line_base_y) = if opts.vertical_text {
            let col_offset = li * entity_h * 1.2;
            (
                col_offset * cos_r + v_offset * (-sin_r),
                col_offset * sin_r + v_offset * cos_r,
            )
        } else {
            let ly = -(li * line_h) + v_offset;
            (ly * (-sin_r), ly * cos_r)
        };

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

        let mut cursor_x = cursor_start;
        for atom in &sub.atoms {
            match &atom.kind {
                AtomKind::Word(text) => {
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
                    let color = atom.state.color.as_ref().and_then(resolve_inline_color);
                    let body = decorated(text, &atom.state);

                    let lx = cursor_x;
                    let ly = valign_dy;
                    let world_dx = lx * cos_r - ly * sin_r;
                    let world_dy = lx * sin_r + ly * cos_r;
                    let origin: [f64; 2] = [
                        ins_x + (line_base_x + world_dx) as f64,
                        ins_y + (line_base_y + world_dy) as f64,
                    ];
                    let strokes = lff::tessellate_text_run(
                        [0.0, 0.0],
                        run_h,
                        rot,
                        signed_wf,
                        oblique,
                        tracking,
                        font_name,
                        &body,
                    );
                    all_strokes.push(TextStroke {
                        strokes,
                        origin,
                        color,
                    });
                    if opts.want_glyph_boxes {
                        // Per-character boxes, advancing exactly as
                        // `measure_word` does so they track the glyphs.
                        let scale = run_scale(&atom.state, entity_h, base_wf);
                        let font = lff::get_font(font_name);
                        let mut cx = cursor_x;
                        for ch in text.chars() {
                            let adv = match font.glyph(ch) {
                                Some(g) => {
                                    (g.advance + font.letter_spacing * tracking) * scale
                                }
                                None => (6.0 + font.letter_spacing * tracking) * scale,
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
                    cursor_x +=
                        measure_word(text, &atom.state, entity_h, base_wf, &base_font_name);
                }
                AtomKind::Space => {
                    let adv = measure_space(&atom.state, entity_h, base_wf, &base_font_name);
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
                    cursor_x = next_tab_position(
                        cursor_x,
                        &sub.tab_stops,
                        sub.indent_left,
                        entity_h,
                    );
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
    }
}
pub(crate) fn text_obb_corners_native(
    e: &EntityType,
    anno_scale: f32,
    mtext_lines_override: Option<usize>,
) -> Option<[[f64; 3]; 4]> {
    use acadrust::entities::attribute_definition::{
        HorizontalAlignment as AttrHA, VerticalAlignment as AttrVA,
    };
    use acadrust::entities::{AttachmentPoint, TextHorizontalAlignment, TextVerticalAlignment};

    let anno = anno_scale as f64;

    // Map Attribute alignment enums to the same h/v anchor fractions used by
    // Text. Kept local because the Attribute enum is distinct from Text's.
    let attr_h_anchor = |ha: AttrHA| -> f64 {
        match ha {
            AttrHA::Left => 0.0,
            AttrHA::Center | AttrHA::Middle | AttrHA::Aligned | AttrHA::Fit => 0.5,
            AttrHA::Right => 1.0,
        }
    };
    let attr_v_anchor = |va: AttrVA| -> f64 {
        match va {
            AttrVA::Baseline | AttrVA::Bottom => 0.0,
            AttrVA::Middle => 0.5,
            AttrVA::Top => 1.0,
        }
    };
    let attr_use_align_pt = |ha: AttrHA, va: AttrVA| -> bool {
        !matches!((ha, va), (AttrHA::Left, AttrVA::Baseline))
    };

    let (ix, iy, iz, w, h, rot, h_anchor, v_anchor) = match e {
        EntityType::Text(t) => {
            let h_world = t.height * anno;
            let w_factor = if t.width_factor > 0.0 { t.width_factor } else { 1.0 };
            let n = t.value.chars().count().max(1) as f64;
            // Approximate glyph width: AutoCAD's stroke fonts average ~0.6 em.
            let w_world = n * h_world * w_factor * 0.6;
            let h_anchor = match t.horizontal_alignment {
                TextHorizontalAlignment::Left => 0.0,
                TextHorizontalAlignment::Center
                | TextHorizontalAlignment::Middle
                | TextHorizontalAlignment::Aligned
                | TextHorizontalAlignment::Fit => 0.5,
                TextHorizontalAlignment::Right => 1.0,
            };
            let v_anchor = match t.vertical_alignment {
                TextVerticalAlignment::Baseline | TextVerticalAlignment::Bottom => 0.0,
                TextVerticalAlignment::Middle => 0.5,
                TextVerticalAlignment::Top => 1.0,
            };
            // AutoCAD writes `alignment_point` (DXF 11) whenever the text
            // isn't simple Left+Baseline; in that case it — not the
            // insertion_point — is the anchor the alignment fractions map to.
            let use_alignment_pt = !matches!(
                (t.horizontal_alignment, t.vertical_alignment),
                (
                    TextHorizontalAlignment::Left,
                    TextVerticalAlignment::Baseline,
                )
            );
            let anchor = match (use_alignment_pt, t.alignment_point) {
                (true, Some(p)) => p,
                _ => t.insertion_point,
            };
            (
                anchor.x,
                anchor.y,
                anchor.z,
                w_world,
                h_world,
                t.rotation,
                h_anchor,
                v_anchor,
            )
        }
        EntityType::AttributeDefinition(a) => {
            let h_world = a.height * anno;
            let w_factor = if a.width_factor > 0.0 { a.width_factor } else { 1.0 };
            // Render the tag in preview when no default; matches `attribute.rs`.
            let display = if a.default_value.is_empty() { &a.tag } else { &a.default_value };
            let n = display.chars().count().max(1) as f64;
            let w_world = n * h_world * w_factor * 0.6;
            let h_anchor = attr_h_anchor(a.horizontal_alignment);
            let v_anchor = attr_v_anchor(a.vertical_alignment);
            let anchor = if attr_use_align_pt(a.horizontal_alignment, a.vertical_alignment) {
                a.alignment_point
            } else {
                a.insertion_point
            };
            (
                anchor.x, anchor.y, anchor.z, w_world, h_world, a.rotation, h_anchor, v_anchor,
            )
        }
        EntityType::AttributeEntity(a) => {
            let h_world = a.height * anno;
            let w_factor = if a.width_factor > 0.0 { a.width_factor } else { 1.0 };
            let n = a.value.chars().count().max(1) as f64;
            let w_world = n * h_world * w_factor * 0.6;
            let h_anchor = attr_h_anchor(a.horizontal_alignment);
            let v_anchor = attr_v_anchor(a.vertical_alignment);
            let anchor = if attr_use_align_pt(a.horizontal_alignment, a.vertical_alignment) {
                a.alignment_point
            } else {
                a.insertion_point
            };
            (
                anchor.x, anchor.y, anchor.z, w_world, h_world, a.rotation, h_anchor, v_anchor,
            )
        }
        EntityType::Tolerance(t) => {
            // Approximate: a feature control frame is roughly 1 line tall;
            // width comes from char-count of the (already-stripped) text.
            // GD&T symbols all advance one cell, so plain char count is close.
            let raw_h = if t.text_height > 0.0 { t.text_height } else { 2.5 };
            let h_world = raw_h * anno;
            // Each cell ≈ 1.4 × height (matches `tolerance.rs` min_cell_w).
            let n = t.text.chars().filter(|c| *c != '\n').count().max(1) as f64;
            let w_world = h_world * 1.4 * n * 0.5; // rough — fine for LOD greek
            // direction encodes rotation as a vector.
            let rot = (t.direction.y).atan2(t.direction.x);
            (
                t.insertion_point.x,
                t.insertion_point.y,
                t.insertion_point.z,
                w_world,
                h_world * 1.5, // frame is taller than glyph cap by ~0.5 h
                rot,
                0.0, // anchored at insertion point (bottom-left)
                0.0,
            )
        }
        EntityType::MText(m) => {
            let h_world = m.height * anno;
            let raw_lines = (m.value.matches('\n').count() + 1) as f64;
            let effective_lines = match mtext_lines_override {
                Some(n) => n.max(1) as f64,
                None => raw_lines,
            };
            let w_world = if m.rectangle_width > 0.0 {
                m.rectangle_width
            } else {
                h_world * 8.0 * effective_lines.max(1.0)
            };
            // Wrap-aware override beats `rectangle_height` — the stored
            // height can be stale on DWGs that were re-saved without
            // updating the bounds.
            let total_h = if mtext_lines_override.is_some() {
                h_world * effective_lines.max(1.0) * m.line_spacing_factor.max(0.5)
            } else {
                m.rectangle_height.unwrap_or(
                    h_world * raw_lines.max(1.0) * m.line_spacing_factor.max(0.5),
                )
            };
            // MText `attachment_point` puts `insertion_point` at one of the
            // 9 corners/midpoints of the text bbox. h_anchor / v_anchor are
            // fractions from (left, bottom) of the bbox.
            let (h_anchor, v_anchor) = match m.attachment_point {
                AttachmentPoint::TopLeft => (0.0, 1.0),
                AttachmentPoint::TopCenter => (0.5, 1.0),
                AttachmentPoint::TopRight => (1.0, 1.0),
                AttachmentPoint::MiddleLeft => (0.0, 0.5),
                AttachmentPoint::MiddleCenter => (0.5, 0.5),
                AttachmentPoint::MiddleRight => (1.0, 0.5),
                AttachmentPoint::BottomLeft => (0.0, 0.0),
                AttachmentPoint::BottomCenter => (0.5, 0.0),
                AttachmentPoint::BottomRight => (1.0, 0.0),
            };
            (
                m.insertion_point.x,
                m.insertion_point.y,
                m.insertion_point.z,
                w_world,
                total_h,
                m.rotation,
                h_anchor,
                v_anchor,
            )
        }
        _ => return None,
    };

    let x0 = -h_anchor * w;
    let x1 = (1.0 - h_anchor) * w;
    let y0 = -v_anchor * h;
    let y1 = (1.0 - v_anchor) * h;

    let (s, c) = (rot.sin(), rot.cos());
    let rot_pt = |lx: f64, ly: f64| -> [f64; 3] {
        let rx = lx * c - ly * s;
        let ry = lx * s + ly * c;
        [ix + rx, iy + ry, iz]
    };

    Some([
        rot_pt(x0, y0),
        rot_pt(x1, y0),
        rot_pt(x1, y1),
        rot_pt(x0, y1),
    ])
}
pub(crate) fn text_baseline_points(
    e: &EntityType,
    anno_scale: f32,
    world_offset: [f64; 3],
    n_lines: usize,
) -> Vec<[f32; 3]> {
    let Some(corners) = text_obb_corners_native(e, anno_scale, Some(n_lines)) else {
        return vec![];
    };
    let line_h = match e {
        EntityType::Text(t) => (t.height * anno_scale as f64) as f32,
        EntityType::MText(m) => (m.height * anno_scale as f64) as f32,
        EntityType::AttributeDefinition(a) => (a.height * anno_scale as f64) as f32,
        EntityType::AttributeEntity(a) => (a.height * anno_scale as f64) as f32,
        EntityType::Tolerance(t) => {
            let raw = if t.text_height > 0.0 { t.text_height } else { 2.5 };
            (raw * anno_scale as f64) as f32
        }
        _ => return vec![],
    };
    if line_h <= 0.0 {
        return vec![];
    }
    let n_lines = n_lines.max(1);
    let [ox, oy, oz] = world_offset;
    let cast = |p: [f64; 3]| -> [f32; 3] {
        [(p[0] - ox) as f32, (p[1] - oy) as f32, (p[2] - oz) as f32]
    };
    let bl = cast(corners[0]);
    let br = cast(corners[1]);
    let full_tl = cast(corners[3]);

    let (ux, uy, uz) = (full_tl[0] - bl[0], full_tl[1] - bl[1], full_tl[2] - bl[2]);
    let ulen = (ux * ux + uy * uy + uz * uz).sqrt();
    if ulen < 1e-9 {
        return vec![bl, br];
    }
    let (nx, ny, nz) = (ux / ulen, uy / ulen, uz / ulen);

    let mut pts = Vec::with_capacity(n_lines * 3);
    for i in 0..n_lines {
        // i = 0 is the topmost line — its bottom sits one line_h below
        // the OBB top (≈ `ulen`). For n_lines > ulen/line_h the deepest
        // baselines clamp to the OBB bottom.
        let bot_off = ((i + 1) as f32) * line_h;
        let along = (ulen - bot_off).max(0.0);
        let p0 = [bl[0] + nx * along, bl[1] + ny * along, bl[2] + nz * along];
        let p1 = [br[0] + nx * along, br[1] + ny * along, br[2] + nz * along];
        if !pts.is_empty() {
            pts.push([f32::NAN; 3]);
        }
        pts.extend_from_slice(&[p0, p1]);
    }
    pts
}

/// Filled tris for a greeked top-level Text / MText. One 2-triangle rect
/// per visible line — `n_lines` is the actual rendered line count from
/// `mtext_line_count` (1 for Text). Stacked top → bottom along the OBB's
/// up direction. The face3d pipeline skips its 0.45 dim for wires with
/// empty `points`, so these tris render at the literal text color.
pub(crate) fn text_greek_obb_tris(
    e: &EntityType,
    anno_scale: f32,
    world_offset: [f64; 3],
    n_lines: usize,
) -> Vec<[f32; 3]> {
    let Some(corners) = text_obb_corners_native(e, anno_scale, Some(n_lines)) else {
        return vec![];
    };
    let line_h = match e {
        EntityType::Text(t) => (t.height * anno_scale as f64) as f32,
        EntityType::MText(m) => (m.height * anno_scale as f64) as f32,
        EntityType::AttributeDefinition(a) => (a.height * anno_scale as f64) as f32,
        EntityType::AttributeEntity(a) => (a.height * anno_scale as f64) as f32,
        EntityType::Tolerance(t) => {
            let raw = if t.text_height > 0.0 { t.text_height } else { 2.5 };
            (raw * anno_scale as f64) as f32
        }
        _ => return vec![],
    };
    if line_h <= 0.0 {
        return vec![];
    }
    let n_lines = n_lines.max(1);
    let [ox, oy, oz] = world_offset;
    let cast = |p: [f64; 3]| -> [f32; 3] {
        [(p[0] - ox) as f32, (p[1] - oy) as f32, (p[2] - oz) as f32]
    };
    let bl = cast(corners[0]);
    let br = cast(corners[1]);
    let full_tl = cast(corners[3]);

    let (ux, uy, uz) = (full_tl[0] - bl[0], full_tl[1] - bl[1], full_tl[2] - bl[2]);
    let ulen = (ux * ux + uy * uy + uz * uz).sqrt();
    if ulen < 1e-9 {
        return vec![];
    }
    let (nx, ny, nz) = (ux / ulen, uy / ulen, uz / ulen);

    let mut tris = Vec::with_capacity(n_lines * 6);
    for i in 0..n_lines {
        let top_along = (ulen - (i as f32) * line_h).max(0.0);
        let bot_along = (ulen - ((i + 1) as f32) * line_h).max(0.0);
        let tl = [
            bl[0] + nx * top_along,
            bl[1] + ny * top_along,
            bl[2] + nz * top_along,
        ];
        let tr = [
            br[0] + nx * top_along,
            br[1] + ny * top_along,
            br[2] + nz * top_along,
        ];
        let lbl = [
            bl[0] + nx * bot_along,
            bl[1] + ny * bot_along,
            bl[2] + nz * bot_along,
        ];
        let lbr = [
            br[0] + nx * bot_along,
            br[1] + ny * bot_along,
            br[2] + nz * bot_along,
        ];
        tris.extend_from_slice(&[lbl, lbr, tr, lbl, tr, tl]);
    }
    tris
}

