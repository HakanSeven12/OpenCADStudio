use acadrust::CadDocument;

use crate::scene::cxf;

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

    let font = cxf::get_font(font_name);
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

pub fn strip_mtext_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek().copied() {
                // Paragraph / line breaks
                Some('P') | Some('n') | Some('N') => {
                    chars.next();
                    out.push('\n');
                }
                // Non-breaking space → regular space (preserves spacing, simpler for word-wrap)
                Some('~') => {
                    chars.next();
                    out.push(' ');
                }
                // Tab
                Some('t') => {
                    chars.next();
                    out.push_str("    ");
                }
                // \U+XXXX or \u+XXXX — Unicode character by hex code point.
                // Up to 6 hex digits; optional trailing semicolon.
                // NOTE: 'U'/'u' were previously in the "strip until ;" list — that was wrong.
                Some('U') | Some('u') => {
                    chars.next();
                    if chars.peek() == Some(&'+') {
                        chars.next(); // consume '+'
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
                            chars.next(); // consume optional trailing semicolon
                        }
                        if let Ok(n) = u32::from_str_radix(&hex, 16) {
                            if let Some(decoded) = char::from_u32(n) {
                                out.push(decoded);
                                continue;
                            }
                        }
                        // Undecodable — silently drop
                    } else {
                        // \U or \u without '+' — strip until semicolon
                        for c in chars.by_ref() {
                            if c == ';' {
                                break;
                            }
                        }
                    }
                }
                // \S<upper><sep><lower>; — stacked text (fraction / tolerance).
                // sep is '/' (diagonal fraction), '^' (stacked with bar), '#' (horizontal bar).
                // Render as "upper/lower" or "upper^lower" since stroke fonts can't stack.
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
                    out.push_str(&upper);
                    if !lower.is_empty() {
                        out.push(if sep == '#' { '/' } else { sep });
                        out.push_str(&lower);
                    }
                }
                // Decoration toggles — keep as \X markers so the tessellator can
                // emit underline / overline / strikethrough strokes.
                Some('L') | Some('l') | Some('O') | Some('o') | Some('K') | Some('k') => {
                    out.push('\\');
                    out.push(chars.next().unwrap());
                }
                // Literal backslash
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                // Literal braces
                Some('{') | Some('}') => {
                    out.push(chars.next().unwrap());
                }
                // In-line codes with semicolon-terminated arguments — strip entirely.
                //   p = paragraph format   H = height       W = width factor
                //   Q = oblique angle      T = tracking     A = alignment
                //   C = ACI color          c = true color   f/F = font change
                //   M = DBCS multibyte     X = paragraph-align end (arg-less but safe to strip)
                Some(c) if "pHWQTACcfFMX".contains(c) => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                // Unknown escape — consume and silently discard the code character so
                // it does not appear as a literal in the output.
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            // Strip brace grouping markers (scope delimiters for in-line formatting)
            '{' | '}' => {}
            '\r' => {}
            other => out.push(other),
        }
    }

    resolve_dxf_special_chars(&out)
}

pub fn split_mtext_lines(s: &str) -> Vec<String> {
    let lines: Vec<String> = s.split('\n').map(|l| l.trim().to_string()).collect();
    // Drop leading and trailing blank lines, but preserve blank lines in the
    // middle — they are intentional paragraph separators (\\P\\P in MTEXT).
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    lines[start..end].to_vec()
}

/// Measure the advance width of an MText line (after strip_mtext_codes), correctly
/// skipping decoration toggle markers (`\L`, `\l`, `\O`, `\o`, `\K`, `\k`).
pub fn measure_mtext_chars(text: &str, scale: f32, font: &cxf::CxfFile) -> f32 {
    let mut width = 0.0_f32;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && matches!(chars.peek(), Some('L' | 'l' | 'O' | 'o' | 'K' | 'k')) {
            chars.next();
            continue;
        }
        if c == ' ' {
            width += font.word_spacing * scale;
        } else {
            width += font
                .glyph(c)
                .map(|g| (g.advance + font.letter_spacing) * scale)
                .unwrap_or(scale * 6.0);
        }
    }
    width
}

/// Total number of visible lines an MText renders to — explicit `\P` /
/// `\n` / `\N` breaks plus word-wrap induced sublines when
/// `rectangle_width > 0`. Mirrors the line-splitting `entities/mtext.rs`
/// and `tessellate_multileader` perform; lets renderers split the OBB
/// into per-row LOD primitives without re-running the wrap measurement.
pub fn mtext_line_count(
    m: &acadrust::entities::MText,
    document: &CadDocument,
    anno_scale: f32,
) -> usize {
    let resolved = resolve_text_style(&m.style, document);
    let font = cxf::get_font(&resolved.font_name);
    let width_factor = resolved.width_factor.max(0.01);
    let height = m.height as f32 * anno_scale;
    let plain = strip_mtext_codes(&m.value);
    let explicit = split_mtext_lines(&plain);
    let total: usize = if m.rectangle_width > 0.0 && height > 0.0 {
        let scale = height / 9.0 * width_factor;
        let max_w = m.rectangle_width as f32 * anno_scale;
        explicit
            .iter()
            .map(|l| word_wrap(l, max_w, scale, font).len())
            .sum()
    } else {
        explicit.len()
    };
    total.max(1)
}

pub fn word_wrap(text: &str, max_w: f32, scale: f32, font: &'static cxf::CxfFile) -> Vec<String> {
    if max_w <= 0.0 || text.is_empty() {
        return vec![text.to_string()];
    }

    let space_w = font.word_spacing * scale;
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_w = 0.0_f32;

    for word in text.split(' ') {
        let word_w = measure_mtext_chars(word, scale, font);
        let gap = if current.is_empty() { 0.0 } else { space_w };
        if !current.is_empty() && current_w + gap + word_w > max_w {
            lines.push(std::mem::take(&mut current));
            current_w = 0.0;
        }
        if !current.is_empty() {
            current.push(' ');
            current_w += space_w;
        }
        current.push_str(word);
        current_w += word_w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
