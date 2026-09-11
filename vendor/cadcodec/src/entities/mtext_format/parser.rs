//! MTEXT format string parser.
//!
//! Parses AutoCAD MTEXT formatted strings into structured [`MTextDocument`]
//! containing paragraphs and styled spans.
//!
//! For the full specification of supported control codes, see the [`mtext_format`] module.
//!
//! [`mtext_format`]: crate::entities::mtext_format

use super::types::*;

/// Parse an MTEXT format string into a structured document.
///
/// Recognizes all control codes whether or not the content is wrapped in
/// `{...}`. Matches the behavior of AutoCAD MTEXT and MLEADER entities.
///
/// # Arguments
///
/// * `input` - The MTEXT format string to parse.
/// * `merge_spans` - If `true`, consecutive spans with identical properties
///   are merged into a single span.
///
/// # Returns
///
/// An [`MTextDocument`] containing the parsed paragraphs and spans.
pub fn parse_mtext(input: &str, merge_spans: bool) -> MTextDocument {
    let parser = MTextParser::new(input);
    parser.parse_formatted(merge_spans)
}

/// Parse a plain text string (e.g. TEXT entity) with minimal processing.
///
/// Only `%%` special characters and `\P` paragraph breaks are handled.
/// All other characters — including backslash control codes like `\C`, `\H`,
/// etc. — are treated as literal text. This matches the behavior of TEXT,
/// ATTRIB, and ATTDEF entities.
///
/// # Returns
///
/// An [`MTextDocument`] containing the parsed paragraphs and spans.
pub fn parse_plain_text(input: &str) -> MTextDocument {
    let mut parser = MTextParser::new(input);
    parser.parse_plain();
    parser.document
}

/// Internal parser state with context stack support.
struct MTextParser {
    /// Input characters as a vector for indexed access.
    chars: Vec<char>,
    /// Current position in the character vector.
    pos: usize,
    /// Stack of style contexts. `{` pushes, `}` pops.
    ctx_stack: Vec<SpanProperties>,
    /// Text buffer for the current span.
    text_buf: String,
    /// Output document.
    document: MTextDocument,
    /// Current paragraph.
    current_paragraph: MTextParagraph,
}

impl MTextParser {
    fn new(input: &str) -> Self {
        MTextParser {
            chars: input.chars().collect(),
            pos: 0,
            ctx_stack: vec![SpanProperties::default()],
            text_buf: String::new(),
            document: MTextDocument::new(),
            current_paragraph: MTextParagraph::new(),
        }
    }

    /// Current active properties (top of stack).
    fn current_props(&self) -> &SpanProperties {
        self.ctx_stack.last().unwrap()
    }

    /// Mutable current active properties.
    fn current_props_mut(&mut self) -> &mut SpanProperties {
        self.ctx_stack.last_mut().unwrap()
    }

    /// Parse with full MTEXT formatting: all control codes are recognized
    /// whether or not the content is wrapped in `{...}`.
    fn parse_formatted(mut self, merge_spans: bool) -> MTextDocument {
        if self.chars.is_empty() {
            return self.document;
        }

        // If content starts with `{`, treat it as an outer group.
        // Otherwise, still parse control codes directly.
        if self.chars[0] == '{' {
            self.pos = 1;
            self.push_ctx(); // push default context for the outer group
        }
        self.parse_formatted_mode(merge_spans);
        self.document
    }

    /// Parse in plain mode: only `%%` special chars and `\P` paragraph breaks
    /// are handled. All other characters (including `\C`, `\H`, etc.) are
    /// treated as literal text.
    ///
    /// This matches the behavior of TEXT entities which do not support
    /// backslash-based formatting codes.
    fn parse_plain(&mut self) {
        if self.chars.is_empty() {
            return;
        }
        // TEXT entities: backslash is literal (no MTEXT `\` codes); only `%%`
        // control codes apply. `%%u`/`%%o` toggle underline/overline (each toggle
        // starts a new span), `%%d`/`%%p`/`%%c` and `%%%%` resolve to symbols,
        // `%%nnn` is a decimal character code, and `\P`/`\p` break paragraphs.
        // Underscoring/overscoring turn off automatically at the string end.
        let flush = |para: &mut MTextParagraph, text: &mut String, u: bool, o: bool| {
            if text.is_empty() {
                return;
            }
            let mut props = SpanProperties::default();
            props.set_underline(u);
            props.set_overline(o);
            para.push_span(MTextSpan::new(std::mem::take(text), props));
        };

        let n = self.chars.len();
        let mut para = MTextParagraph::new();
        let mut text = String::new();
        let mut underline = false;
        let mut overline = false;

        while self.pos < n {
            let ch = self.chars[self.pos];

            if ch == '%' && self.pos + 1 < n && self.chars[self.pos + 1] == '%' {
                // `%%%%` → literal `%`.
                if self.pos + 3 < n
                    && self.chars[self.pos + 2] == '%'
                    && self.chars[self.pos + 3] == '%'
                {
                    self.pos += 4;
                    text.push('%');
                    continue;
                }
                if self.pos + 2 < n {
                    let code = self.chars[self.pos + 2];
                    match code.to_ascii_lowercase() {
                        'u' => {
                            self.pos += 3;
                            flush(&mut para, &mut text, underline, overline);
                            underline = !underline;
                            continue;
                        }
                        'o' => {
                            self.pos += 3;
                            flush(&mut para, &mut text, underline, overline);
                            overline = !overline;
                            continue;
                        }
                        lc @ ('d' | 'p' | 'c') => {
                            if let Some(sp) = SpecialChar::from_char(lc) {
                                self.pos += 3;
                                text.push(sp.to_char());
                                continue;
                            }
                        }
                        _ if code.is_ascii_digit() => {
                            // `%%nnn` — up to three decimal digits → character.
                            self.pos += 2;
                            let mut digits = String::new();
                            while digits.len() < 3
                                && self.pos < n
                                && self.chars[self.pos].is_ascii_digit()
                            {
                                digits.push(self.chars[self.pos]);
                                self.pos += 1;
                            }
                            if let Some(c) = digits.parse::<u32>().ok().and_then(char::from_u32) {
                                text.push(c);
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                // Unknown `%%` code — pass the pair through literally.
                self.pos += 2;
                text.push('%');
                text.push('%');
                continue;
            }

            if ch == '\\'
                && self.pos + 1 < n
                && (self.chars[self.pos + 1] == 'P' || self.chars[self.pos + 1] == 'p')
            {
                flush(&mut para, &mut text, underline, overline);
                self.document.push_paragraph(std::mem::take(&mut para));
                self.pos += 2;
                continue;
            }

            text.push(ch);
            self.pos += 1;
        }

        flush(&mut para, &mut text, underline, overline);
        self.document.push_paragraph(para);
    }

    /// Push a copy of the current context onto the stack.
    fn push_ctx(&mut self) {
        if let Some(props) = self.ctx_stack.last() {
            self.ctx_stack.push(props.clone());
        }
    }

    /// Pop the current context, restoring the previous one.
    fn pop_ctx(&mut self) {
        if self.ctx_stack.len() > 1 {
            self.ctx_stack.pop();
        }
    }

    /// Parse in formatted mode (inside `{...}`).
    fn parse_formatted_mode(&mut self, merge_spans: bool) {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];

            match ch {
                '\\' => {
                    self.flush_current_span(merge_spans);
                    self.parse_control_code();
                }
                '{' => {
                    // Push new context (inherit from current)
                    self.push_ctx();
                    self.pos += 1;
                }
                '}' => {
                    // Pop context — this is a span boundary
                    self.flush_current_span(merge_spans);
                    self.pop_ctx();
                    // After popping, flush again so that text following
                    // a group uses the restored (different) style.
                    if !self.text_buf.is_empty() {
                        self.flush_current_span(merge_spans);
                    }
                    self.pos += 1;
                }
                '%' => {
                    self.handle_special_char();
                }
                '^' => {
                    // Caret-encoded controls: `^I` tab, `^J` line break,
                    // `^M` carriage return (ignored). Anything else is a
                    // literal caret.
                    match self.chars.get(self.pos + 1).copied() {
                        Some('I') => {
                            self.text_buf.push('\t');
                            self.pos += 2;
                        }
                        Some('J') => {
                            self.pos += 2;
                            self.flush_current_span(merge_spans);
                            self.document
                                .push_paragraph(std::mem::take(&mut self.current_paragraph));
                        }
                        Some('M') => {
                            self.pos += 2;
                        }
                        _ => {
                            self.text_buf.push('^');
                            self.pos += 1;
                        }
                    }
                }
                '\n' => {
                    // A raw line feed is a hard paragraph break, same as `\P`
                    // or the caret form `^J`.
                    self.pos += 1;
                    self.flush_current_span(merge_spans);
                    self.document
                        .push_paragraph(std::mem::take(&mut self.current_paragraph));
                }
                '\t' => {
                    // Keep the tab character; the layout engine resolves it to
                    // the paragraph's next tab stop.
                    self.text_buf.push('\t');
                    self.pos += 1;
                }
                c if (c as u32) < 0x20 => {
                    // Any other control character renders as a space.
                    self.text_buf.push(' ');
                    self.pos += 1;
                }
                _ => {
                    self.text_buf.push(ch);
                    self.pos += 1;
                }
            }
        }

        // End of input — flush remaining
        self.flush_current_span(merge_spans);
        if !self.current_paragraph.is_empty() {
            self.document
                .push_paragraph(std::mem::take(&mut self.current_paragraph));
        }
    }

    /// Handle `%%` special character codes.
    fn handle_special_char(&mut self) {
        if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '%' {
            self.pos += 2; // skip "%%"

            if self.pos < self.chars.len() {
                // %%%% → literal %
                if self.chars[self.pos] == '%'
                    && self.pos + 1 < self.chars.len()
                    && self.chars[self.pos + 1] == '%'
                {
                    self.pos += 2;
                    self.text_buf.push('%');
                    return;
                }

                let code_char = self.chars[self.pos];

                if let Some(special) = SpecialChar::from_char(code_char) {
                    self.pos += 1;
                    match special {
                        SpecialChar::Percent => {
                            self.text_buf.push('%');
                        }
                        SpecialChar::Degree | SpecialChar::PlusMinus | SpecialChar::Diameter => {
                            self.text_buf.push(special.to_char());
                        }
                    }
                } else {
                    // Unknown %% code — emit literally and advance past the code char
                    self.text_buf.push('%');
                    self.text_buf.push('%');
                    self.text_buf.push(code_char);
                    self.pos += 1;
                }
            }
        } else {
            // Not a %% code, just a regular '%' character
            self.text_buf.push('%');
            self.pos += 1;
        }
    }

    /// Flush the current text buffer as a span with current properties.
    fn flush_current_span(&mut self, merge_spans: bool) {
        if self.text_buf.is_empty() {
            return;
        }

        let props = self.current_props().clone();
        let span = MTextSpan::new(self.text_buf.clone(), props);
        self.text_buf.clear();

        if merge_spans {
            self.current_paragraph.push_span_merged(span);
        } else {
            self.current_paragraph.push_span(span);
        }
    }

    /// Parse a control code starting at self.pos (which is the `\`).
    fn parse_control_code(&mut self) {
        if self.pos >= self.chars.len() {
            return;
        }
        self.pos += 1; // skip '\'

        if self.pos >= self.chars.len() {
            self.text_buf.push('\\');
            return;
        }

        let code = self.chars[self.pos];

        match code {
            // Escaped characters
            '\\' => {
                self.text_buf.push('\\');
                self.pos += 1;
            }
            '{' => {
                self.text_buf.push('{');
                self.pos += 1;
            }
            '}' => {
                self.text_buf.push('}');
                self.pos += 1;
            }
            // Escaped semicolon → literal `;` (so a `;` can appear in text
            // without terminating a control code).
            ';' => {
                self.text_buf.push(';');
                self.pos += 1;
            }

            // Non-breaking space
            '~' => {
                self.text_buf.push('\u{00A0}');
                self.pos += 1;
            }

            // Unicode escape: \U+XXXX
            'U' if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '+' => {
                self.pos += 2; // skip \U+
                let start = self.pos;
                while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_hexdigit() {
                    self.pos += 1;
                }
                let hex: String = self.chars[start..self.pos].iter().collect();
                if let Ok(code_point) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(code_point) {
                        self.text_buf.push(ch);
                    }
                }
            }

            // Paragraph break: \P (uppercase) is always a paragraph break.
            // \p followed by content is paragraph properties; \p alone is a paragraph break.
            'P' => {
                self.pos += 1;
                self.flush_current_span(false);
                self.document
                    .push_paragraph(std::mem::take(&mut self.current_paragraph));
                // Span properties (color, font, bold, height, etc.) carry over
                // to the next paragraph. Only paragraph-level properties are
                // per-paragraph (set via \p...; control codes).
            }

            // \p...; → paragraph properties, or \p alone → paragraph break
            'p' => {
                self.pos += 1;
                self.parse_paragraph_properties();
            }

            // New column break
            'N' => {
                self.pos += 1;
                self.flush_current_span(false);
                self.document
                    .push_paragraph(std::mem::take(&mut self.current_paragraph));
                // Span properties carry over, same as \P — the one difference
                // is that what follows opens a new column, so mark it. Drop the
                // mark and `\N` is indistinguishable from `\P` downstream.
                self.current_paragraph.starts_column = true;
            }

            // Wrap at dimension line (skip, no output)
            'X' => {
                self.pos += 1;
            }

            // ── Stroke decorations ──

            // Underline on
            'L' => {
                self.pos += 1;
                self.current_props_mut().set_underline(true);
            }

            // Underline off
            'l' => {
                self.pos += 1;
                self.current_props_mut().set_underline(false);
            }

            // Overline on
            'O' => {
                self.pos += 1;
                self.current_props_mut().set_overline(true);
            }

            // Overline off
            'o' => {
                self.pos += 1;
                self.current_props_mut().set_overline(false);
            }

            // Strikethrough on
            'K' => {
                self.pos += 1;
                self.current_props_mut().set_strikethrough(true);
            }

            // Strikethrough off
            'k' => {
                self.pos += 1;
                self.current_props_mut().set_strikethrough(false);
            }

            // ── Color codes ──

            // ACI color: \C{start};{end};
            'C' => {
                self.pos += 1;
                self.parse_aci_color();
            }

            // True-color RGB: \c{bgr_packed};
            'c' => {
                self.pos += 1;
                self.parse_rgb_color();
            }

            // ── Font codes ──

            // Font with pipe flags: \f{name}|b0/b1|i0/i1|...;
            'f' => {
                self.pos += 1;
                self.parse_font_pipe();
            }

            // \F{font}; — Font selection (alias for \f, supports pipe flags too).
            // \FN{name}.shx; — Font by SHX name.
            'F' => {
                self.pos += 1;
                if self.pos < self.chars.len() && self.chars[self.pos] == 'N' {
                    self.pos += 1;
                    self.parse_font_shx();
                } else {
                    // \F can also be used like \f with pipe-separated flags
                    self.parse_font_pipe();
                }
            }

            // ── Height ──

            // Height: \H{value}; (absolute) or \H{value}x; (relative)
            'H' => {
                self.pos += 1;
                self.parse_height_code();
            }

            // ── Width ──

            // Width factor: \W{value}; (absolute) or \W{value}x; (relative)
            'W' | 'w' => {
                self.pos += 1;
                self.parse_width_code();
            }

            // ── Tracking ──

            // Tracking: \T{value}; (absolute) or \T{value}x; (relative)
            'T' => {
                self.pos += 1;
                self.parse_tracking_code();
            }

            // ── Oblique ──

            // Oblique angle: \Q{angle};
            'Q' => {
                self.pos += 1;
                self.parse_oblique_code();
            }

            // ── Line alignment ──

            // Line alignment: \A{code};
            'A' => {
                self.pos += 1;
                self.parse_alignment_code();
            }

            // ── Stacking ──

            // Stacking: \S{numerator}/{denominator};
            'S' | 's' => {
                self.pos += 1;
                self.parse_stacking_code();
            }

            // ── Legacy strikethrough ──

            // Legacy strikethrough: \b{n};
            'b' => {
                self.pos += 1;
                self.parse_legacy_strikethrough();
            }

            // ── Tab stop (skip) ──
            't' => {
                self.pos += 1;
                self.skip_semicolon_value();
            }

            // ── Background mask (skip) ──
            'B' => {
                self.pos += 1;
                self.skip_semicolon_value();
            }

            // Unknown control code — emit literally
            _ => {
                self.text_buf.push('\\');
                self.text_buf.push(code);
                self.pos += 1;
            }
        }
    }

    // ========================================================================
    // Individual code parsers
    // ========================================================================

    /// Parse ACI color: \C{color1};{color2};
    fn parse_aci_color(&mut self) {
        self.current_props_mut().color = None;
        self.current_props_mut().second_color = None;
        self.current_props_mut().color_rgb = None;

        // Read color1. ACI runs 0..=256 where 0 = ByBlock and 256 = ByLayer
        // (both are explicit inheritance overrides, distinct from "no code");
        // an out-of-range value is invalid and ignored.
        if let Some(c1) = self.parse_numeric_semicolon_value() {
            if (0..=256).contains(&c1) {
                self.current_props_mut().color = Some(MTextColor::from_index(c1));
            }

            // Read color2 (ending color for gradient)
            if let Some(c2) = self.parse_numeric_semicolon_value() {
                if (0..=256).contains(&c2) {
                    self.current_props_mut().second_color = Some(MTextColor::from_index(c2));
                }
            }
        }
    }

    /// Parse true-color RGB: \c{packed_bgr};
    fn parse_rgb_color(&mut self) {
        let value_str = self.parse_semicolon_value();
        if let Ok(packed) = value_str.trim().parse::<u32>() {
            // The MTEXT inline `\c` true-color value is byte-reversed relative
            // to the usual 0xRRGGBB: the low byte is red and the high byte is
            // blue (so `\c255` is pure red, not blue).
            let r_val = (packed & 0xFF) as u8;
            let g_val = ((packed >> 8) & 0xFF) as u8;
            let b_val = ((packed >> 16) & 0xFF) as u8;
            self.current_props_mut().color_rgb = Some((r_val, g_val, b_val));
            self.current_props_mut().color = None; // RGB overrides ACI
        }
    }

    /// Parse font with pipe flags: \f{name}|b0/b1|i0/i1|c<n>|p<n>|...;
    ///
    /// Flags the model knows (`b`, `i`, `c` = charset, `p` = pitch) map onto
    /// typed fields; everything else is preserved verbatim in
    /// [`MTextFont::extra`] so a later serialize restores the exact spec.
    fn parse_font_pipe(&mut self) {
        let spec = self.parse_semicolon_value();
        let parts: Vec<&str> = spec.split([',', '|']).collect();

        let name = parts.first().map(|s| s.trim()).unwrap_or("");
        if name.is_empty() {
            return;
        }

        let mut bold = false;
        let mut bold_explicit = false;
        let mut italic = false;
        let mut italic_explicit = false;
        let mut charset = None;
        let mut pitch = None;
        let mut extra = Vec::new();

        for part in parts.iter().skip(1) {
            let part = part.trim();
            match part {
                "b0" => {
                    bold = false;
                    bold_explicit = true;
                }
                "b1" => {
                    bold = true;
                    bold_explicit = true;
                }
                "i0" => {
                    italic = false;
                    italic_explicit = true;
                }
                "i1" => {
                    italic = true;
                    italic_explicit = true;
                }
                "" => {}
                other => {
                    let parsed = match (other.strip_prefix('c'), other.strip_prefix('p')) {
                        (Some(value), _) => value.trim().parse::<u16>().ok().map(|v| {
                            charset = Some(v);
                        }),
                        (_, Some(value)) => value.trim().parse::<u8>().ok().map(|v| {
                            pitch = Some(v);
                        }),
                        _ => None,
                    };
                    if parsed.is_none() {
                        extra.push(other.to_string());
                    }
                }
            }
        }

        self.current_props_mut().font = Some(MTextFont {
            name: name.to_string(),
            bold,
            italic,
            bold_explicit,
            italic_explicit,
            charset,
            pitch,
            extra,
            ..Default::default()
        });
    }

    /// Parse font by SHX name: \FN{name}.shx;
    fn parse_font_shx(&mut self) {
        let name = self.parse_semicolon_value();
        if !name.trim().is_empty() {
            self.current_props_mut().font = Some(MTextFont::from_name(name.trim()));
        }
    }

    /// Parse height: \H{value}; (absolute) or \H{value}x; (relative)
    fn parse_height_code(&mut self) {
        let value_str = self.parse_semicolon_value_or_x();
        let is_relative = value_str.ends_with('x') || value_str.ends_with('X');
        let num_str = if is_relative {
            &value_str[..value_str.len() - 1]
        } else {
            &value_str
        };

        if let Ok(f) = num_str.trim().parse::<f64>() {
            // Relative multiplies the current height, preserving whether that
            // current height is a factor or an absolute value; a fresh relative
            // is a factor over the implicit 1.0. Absolute always resets.
            let scalar = if is_relative {
                match self.current_props().height {
                    Some(MTextScalar::Absolute(c)) => MTextScalar::Absolute(c * f.abs()),
                    Some(MTextScalar::Factor(c)) => MTextScalar::Factor(c * f.abs()),
                    None => MTextScalar::Factor(f.abs()),
                }
            } else {
                MTextScalar::Absolute(f.abs())
            };
            self.current_props_mut().height = Some(scalar);
        }
    }

    /// Parse width factor: \W{value}; (absolute) or \W{value}x; (relative)
    fn parse_width_code(&mut self) {
        let value_str = self.parse_semicolon_value_or_x();
        let is_relative = value_str.ends_with('x') || value_str.ends_with('X');
        let num_str = if is_relative {
            &value_str[..value_str.len() - 1]
        } else {
            &value_str
        };

        if let Ok(f) = num_str.trim().parse::<f64>() {
            if is_relative {
                let cur = self.current_props().width_factor.unwrap_or(1.0);
                self.current_props_mut().width_factor = Some(cur * f.abs());
            } else {
                self.current_props_mut().width_factor = Some(f.abs());
            }
        }
    }

    /// Parse tracking: \T{value}; (absolute) or \T{value}x; (relative)
    fn parse_tracking_code(&mut self) {
        let value_str = self.parse_semicolon_value_or_x();
        let is_relative = value_str.ends_with('x') || value_str.ends_with('X');
        let num_str = if is_relative {
            &value_str[..value_str.len() - 1]
        } else {
            &value_str
        };

        if let Ok(f) = num_str.trim().parse::<f64>() {
            if is_relative {
                let cur = self.current_props().tracking.unwrap_or(1.0);
                self.current_props_mut().tracking = Some(cur * f.abs());
            } else {
                self.current_props_mut().tracking = Some(f);
            }
        }
    }

    /// Parse oblique angle: \Q{angle};
    fn parse_oblique_code(&mut self) {
        let value_str = self.parse_semicolon_value();
        if let Ok(f) = value_str.trim().parse::<f64>() {
            self.current_props_mut().oblique_angle = Some(f);
        }
    }

    /// Parse line alignment: \A{code};
    fn parse_alignment_code(&mut self) {
        let value_str = self.parse_semicolon_value();
        let code: u8 = value_str.parse().unwrap_or(0);
        self.current_props_mut().line_align = Some(MTextLineAlignment::from_code(code));
    }

    /// Parse stacking: \S{numerator}/{denominator};
    /// Separators: / (horizontal), # (diagonal), ^ (limit)
    fn parse_stacking_code(&mut self) {
        let expr = self.parse_semicolon_value();

        // Find separator
        let mut numerator = String::new();
        let mut denominator = String::new();
        let mut stacking_type = StackingType::Horizontal;

        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;
        let mut found_sep = false;

        while i < chars.len() {
            let ch = chars[i];
            if !found_sep && matches!(ch, '/' | '#' | '^') {
                found_sep = true;
                stacking_type = StackingType::from_char(ch);
                if ch == '^' && i + 1 < chars.len() && chars[i + 1] == ' ' {
                    i += 1; // ^ may be followed by space
                }
            } else if found_sep {
                denominator.push(ch);
            } else {
                numerator.push(ch);
            }
            i += 1;
        }

        // For stacking, we emit the text inline with a slash
        let stacking = StackingData {
            numerator,
            denominator,
            stacking_type,
        };

        // Stacking spans have empty text — the plain text is derived from stacking data
        let span = MTextSpan::stacking("", self.current_props().clone(), stacking);
        self.current_paragraph.push_span(span);
    }

    /// Parse paragraph properties: \p...q<c/l/r/j/d>...;
    /// If the value is empty (no semicolon content), treat as paragraph break.
    fn parse_paragraph_properties(&mut self) {
        // Peek ahead to see if this looks like paragraph properties
        // If the next non-whitespace char is ';' or end of string, it's just a paragraph break
        let start_pos = self.pos;
        let expr = self.parse_semicolon_value();

        // If expr is empty or only whitespace, treat as paragraph break
        if expr.trim().is_empty() {
            self.pos = start_pos;
            self.flush_current_span(false);
            self.document
                .push_paragraph(std::mem::take(&mut self.current_paragraph));
            self.current_props_mut()
                .clone_from(&SpanProperties::default());
            return;
        }
        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];
            if ch == 'q' && i + 1 < chars.len() {
                let align = MTextParagraphAlignment::from_char(chars[i + 1]);
                self.current_paragraph.properties.alignment = Some(align);
                i += 2;
            } else if ch == 'i' && i + 1 < chars.len() {
                // First-line indent
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                {
                    k += 1;
                }
                if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                    self.current_paragraph.properties.first_line_indent = Some(v);
                }
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else if ch == 'l' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                // Left margin (only if followed by digit to distinguish from \l underline)
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                {
                    k += 1;
                }
                if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                    self.current_paragraph.properties.left_margin = Some(v);
                }
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else if ch == 'r' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                // Right margin
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                {
                    k += 1;
                }
                if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                    self.current_paragraph.properties.right_margin = Some(v);
                }
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else if ch == 'b' && i + 1 < chars.len() {
                // Spacing before paragraph
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                {
                    k += 1;
                }
                if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                    self.current_paragraph.properties.spacing_before = Some(v);
                }
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else if ch == 'a' && i + 1 < chars.len() {
                // Spacing after paragraph
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                {
                    k += 1;
                }
                if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                    self.current_paragraph.properties.spacing_after = Some(v);
                }
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else if ch == 's' && i + 1 < chars.len() {
                // Line spacing: s* (entity default), se<n> (exact), or
                // sm<n> (multiple).
                let sub = chars[i + 1];
                if sub == '*' {
                    self.current_paragraph.properties.line_spacing =
                        Some(MTextLineSpacing::Default);
                    i += 2;
                    if i < chars.len() && chars[i] == ',' {
                        i += 1;
                    }
                    continue;
                }
                if sub == 'e' || sub == 'm' {
                    let start = i + 2;
                    let mut k = start;
                    while k < chars.len()
                        && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-')
                    {
                        k += 1;
                    }
                    if let Ok(v) = chars[start..k].iter().collect::<String>().parse::<f64>() {
                        self.current_paragraph.properties.line_spacing = if sub == 'e' {
                            Some(MTextLineSpacing::Exact(v))
                        } else {
                            Some(MTextLineSpacing::Multiple(v))
                        };
                    }
                    i = k;
                    if i < chars.len() && chars[i] == ',' {
                        i += 1;
                    }
                    continue;
                }
                i += 1;
            } else if ch == 't' && i + 1 < chars.len() {
                // Tab stops: comma-separated, each optionally prefixed by
                // `c`/`r`/`D` (center/right/decimal); no prefix = left.
                let start = i + 1;
                let mut k = start;
                while k < chars.len()
                    && (chars[k].is_ascii_digit()
                        || matches!(
                            chars[k],
                            '.' | '-' | ',' | 'c' | 'C' | 'r' | 'R' | 'd' | 'D'
                        ))
                {
                    k += 1;
                }
                let segment: String = chars[start..k].iter().collect();
                let mut tab_stops = self.current_paragraph.properties.tab_stops.clone();
                for part in segment.split(',') {
                    let part = part.trim();
                    let (prefix, num) = match part.chars().next() {
                        Some('c') | Some('C') => (b'c', &part[1..]),
                        Some('r') | Some('R') => (b'r', &part[1..]),
                        // A decimal stop names its alignment character right
                        // after `D` (`.` or `,`); the position follows it, so
                        // `D.2` is a decimal stop at 2 (aligning on `.`), not 0.2.
                        Some('d') | Some('D') => {
                            let rest = &part[1..];
                            (
                                b'D',
                                rest.strip_prefix(|c: char| c == '.' || c == ',')
                                    .unwrap_or(rest),
                            )
                        }
                        _ => (0, part),
                    };
                    if let Ok(v) = num.trim().parse::<f64>() {
                        tab_stops.push(match prefix {
                            b'c' => TabStop::Center(v),
                            b'r' => TabStop::Right(v),
                            b'D' => TabStop::Decimal(v),
                            _ => TabStop::Left(v),
                        });
                    }
                }
                self.current_paragraph.properties.tab_stops = tab_stops;
                i = k;
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }

    /// Parse legacy strikethrough: \b{n};
    fn parse_legacy_strikethrough(&mut self) {
        let value_str = self.parse_semicolon_value();
        if let Ok(n) = value_str.trim().parse::<u8>() {
            self.current_props_mut().set_strikethrough(n == 1);
        }
    }

    /// Skip a value up to the next `;` without processing.
    fn skip_semicolon_value(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] != ';' {
            self.pos += 1;
        }
        if self.pos < self.chars.len() && self.chars[self.pos] == ';' {
            self.pos += 1; // skip ';'
        }
    }

    // ========================================================================
    // Helper parsers
    // ========================================================================

    /// Parse a value up to the next `;`.
    fn parse_semicolon_value(&mut self) -> String {
        let mut value = String::new();
        while self.pos < self.chars.len() && self.chars[self.pos] != ';' {
            value.push(self.chars[self.pos]);
            self.pos += 1;
        }
        if self.pos < self.chars.len() && self.chars[self.pos] == ';' {
            self.pos += 1; // skip ';'
        }
        value
    }

    /// Parse a value up to the next `;`, also capturing a trailing `x`/`X`
    /// suffix for relative values.
    fn parse_semicolon_value_or_x(&mut self) -> String {
        let mut value = String::new();
        while self.pos < self.chars.len()
            && self.chars[self.pos] != ';'
            && self.chars[self.pos] != 'x'
            && self.chars[self.pos] != 'X'
        {
            value.push(self.chars[self.pos]);
            self.pos += 1;
        }
        // Check for x/X suffix (relative)
        if self.pos < self.chars.len()
            && (self.chars[self.pos] == 'x' || self.chars[self.pos] == 'X')
        {
            value.push(self.chars[self.pos]);
            self.pos += 1;
        }
        // Skip trailing ';'
        if self.pos < self.chars.len() && self.chars[self.pos] == ';' {
            self.pos += 1;
        }
        value
    }

    /// Parse a numeric value up to the next `;`.
    /// Only consumes characters if they form a valid signed integer.
    fn parse_numeric_semicolon_value(&mut self) -> Option<i32> {
        if self.pos < self.chars.len() && self.chars[self.pos] == ';' {
            self.pos += 1;
            return None;
        }

        let start = self.pos;
        let value = self.parse_semicolon_value();

        if let Ok(n) = value.parse::<i32>() {
            Some(n)
        } else {
            self.pos = start;
            None
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Basic parsing
    // ========================================================================

    #[test]
    fn test_parse_empty() {
        let doc = parse_mtext("", false);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_parse_plain_text_no_braces() {
        let doc = parse_mtext("Hello World", false);
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
        assert_eq!(doc.paragraphs[0].spans[0].text, "Hello World");
        assert!(doc.paragraphs[0].spans[0].is_plain());
    }

    #[test]
    fn test_parse_plain_text_with_paragraph() {
        let doc = parse_mtext("Line1\\PLine2", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Line1");
        assert_eq!(doc.paragraphs[1].to_plain_text(), "Line2");
    }

    #[test]
    fn test_parse_braced_simple() {
        let doc = parse_mtext("{Hello}", false);
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Hello");
    }

    // ========================================================================
    // Color
    // ========================================================================

    #[test]
    fn test_parse_color_single() {
        let doc = parse_mtext(r"{\C1;Red}", false);
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
        assert_eq!(doc.paragraphs[0].spans[0].text, "Red");
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_parse_color_reset() {
        let doc = parse_mtext(r"{\C1;Red\C0;; normal}", false);
        assert_eq!(doc.paragraphs[0].spans.len(), 2);
        assert_eq!(doc.paragraphs[0].spans[0].text, "Red");
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(doc.paragraphs[0].spans[1].text, " normal");
        // \C0; is ACI 0 = ByBlock — an explicit inheritance override, not a
        // bare "unspecified" color.
        assert_eq!(
            doc.paragraphs[0].spans[1].properties.color,
            Some(MTextColor::ByBlock)
        );
    }

    #[test]
    fn test_parse_color_bylayer_byblock() {
        // \C256; = ByLayer (use the layer color), \C0; = ByBlock. Both must
        // stay distinguishable from "no color code" (None) — issue #56.
        let doc = parse_mtext(r"{\C256;White layer text}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::ByLayer)
        );

        let doc = parse_mtext(r"{\C0;By block text}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::ByBlock)
        );
    }

    #[test]
    fn test_parse_color_bylayer_roundtrip() {
        // \C256; must survive a parse→serialize round-trip so the text keeps
        // resolving to the layer color (issue #56).
        let input = r"\fSimSun|b1|i0|c134|p2;\C256;副斜井1820";
        let doc = parse_mtext(input, false);
        let out = doc.to_mtext_string();
        assert!(out.contains("\\C256;"), "ByLayer override dropped: {out}");
        assert!(
            out.contains("SimSun|b1|i0|c134|p2"),
            "font flags dropped: {out}"
        );

        let rt = parse_mtext(&out, false);
        assert_eq!(rt.to_mtext_string(), out, "round-trip unstable");
        assert_eq!(
            rt.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::ByLayer)
        );
    }

    #[test]
    fn test_parse_font_pipe_flags_preserved() {
        // Charset (c), pitch (p) and unknown flags survive round-trips
        // verbatim; explicit italic-off is preserved too.
        let doc = parse_mtext(r"{\fArial|b0|i0|c238|p2|e1|ş|字体|🙂|c悪|p悪;Text}", false);
        let font = doc.paragraphs[0].spans[0].properties.font.as_ref().unwrap();
        assert!(!font.bold && font.bold_explicit);
        assert!(!font.italic && font.italic_explicit);
        assert_eq!(font.charset, Some(238));
        assert_eq!(font.pitch, Some(2));
        assert_eq!(font.extra, vec!["e1", "ş", "字体", "🙂", "c悪", "p悪"]);

        let out = doc.to_mtext_string();
        assert!(
            out.contains("Arial|b0|i0|c238|p2|e1|ş|字体|🙂|c悪|p悪"),
            "flags lost: {out}"
        );
        assert_eq!(parse_mtext(&out, false).to_mtext_string(), out);
    }

    #[test]
    fn test_parse_rgb_color_red() {
        // The inline `\c` value is byte-reversed: low byte is red, so
        // `\c255` (0x0000FF) = pure RED (255,0,0).
        let doc = parse_mtext(r"{\c255;Red}", false);
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color_rgb,
            Some((255, 0, 0))
        );
    }

    #[test]
    fn test_parse_rgb_color_green() {
        // 0x00FF00 → green is the middle byte, unaffected by the R/B swap.
        let doc = parse_mtext(r"{\c65280;Green}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color_rgb,
            Some((0, 255, 0))
        );
    }

    #[test]
    fn test_parse_rgb_color_blue() {
        // High byte is blue, so `\c16711680` (0xFF0000) = pure BLUE (0,0,255).
        let doc = parse_mtext(r"{\c16711680;Blue}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color_rgb,
            Some((0, 0, 255))
        );
    }

    // ========================================================================
    // Stroke decorations
    // ========================================================================

    #[test]
    fn test_parse_underline_on() {
        let doc = parse_mtext(r"{\LUnderlined}", false);
        assert!(doc.paragraphs[0].spans[0].properties.underline());
        assert_eq!(doc.paragraphs[0].spans[0].text, "Underlined");
    }

    #[test]
    fn test_parse_underline_toggle() {
        // \L underline on, \l underline off
        let doc = parse_mtext(r"{\LUnder\lNormal}", false);
        assert!(doc.paragraphs[0].spans[0].properties.underline());
        assert_eq!(doc.paragraphs[0].spans[0].text, "Under");
        assert!(!doc.paragraphs[0].spans[1].properties.underline());
        assert_eq!(doc.paragraphs[0].spans[1].text, "Normal");
    }

    #[test]
    fn test_parse_overline_on() {
        let doc = parse_mtext(r"{\OOverlined}", false);
        assert!(doc.paragraphs[0].spans[0].properties.overline());
    }

    #[test]
    fn test_parse_overline_toggle() {
        // \O overline on, \o overline off
        let doc = parse_mtext(r"{\OOver\oNormal}", false);
        assert!(doc.paragraphs[0].spans[0].properties.overline());
        assert_eq!(doc.paragraphs[0].spans[0].text, "Over");
        assert!(!doc.paragraphs[0].spans[1].properties.overline());
    }

    #[test]
    fn test_parse_strikethrough_on() {
        let doc = parse_mtext(r"{\KStruck}", false);
        assert!(doc.paragraphs[0].spans[0].properties.strikethrough());
    }

    #[test]
    fn test_parse_strikethrough_toggle() {
        // \K strikethrough on, \k strikethrough off
        let doc = parse_mtext(r"{\KStruck\kNormal}", false);
        assert!(doc.paragraphs[0].spans[0].properties.strikethrough());
        assert_eq!(doc.paragraphs[0].spans[0].text, "Struck");
        assert!(!doc.paragraphs[0].spans[1].properties.strikethrough());
    }

    #[test]
    fn test_parse_legacy_strikethrough() {
        let doc = parse_mtext(r"{\b1;Struck}", false);
        assert!(doc.paragraphs[0].spans[0].properties.strikethrough());

        let doc = parse_mtext(r"{\b0;Normal}", false);
        assert!(!doc.paragraphs[0].spans[0].properties.strikethrough());
    }

    // ========================================================================
    // Font
    // ========================================================================

    #[test]
    fn test_parse_font_code() {
        let doc = parse_mtext(r"{\fArial|b1|i0;Bold}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .name,
            "Arial"
        );
        assert!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .bold
        );
        assert!(
            !doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .italic
        );
    }

    #[test]
    fn test_parse_font_uppercase_f() {
        // \F (uppercase) works as font alias like \f
        let doc = parse_mtext(r"{\Fkroeger|b0|i0|c238|p10;Text}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .name,
            "kroeger"
        );
        assert!(
            !doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .bold
        );
        assert!(
            !doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .italic
        );
    }

    #[test]
    fn test_parse_font_bold_italic() {
        let doc = parse_mtext(r"{\fArial|b1|i1;BI}", false);
        assert!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .bold
        );
        assert!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .italic
        );
    }

    #[test]
    fn test_parse_font_shx() {
        let doc = parse_mtext(r"{\FNromans.shx;Text}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0]
                .properties
                .font
                .as_ref()
                .unwrap()
                .name,
            "romans.shx"
        );
    }

    // ========================================================================
    // Height
    // ========================================================================

    #[test]
    fn test_parse_height_absolute() {
        let doc = parse_mtext(r"{\H2;Big}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
    }

    #[test]
    fn test_parse_height_relative() {
        let doc = parse_mtext(r"{\H2x;Bigger}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.height,
            Some(MTextScalar::Factor(2.0))
        );
    }

    #[test]
    fn test_plain_text_underline_toggle() {
        // TEXT `%%u` toggles underline; each toggle starts a new span.
        let doc = parse_plain_text("A%%uB%%uC");
        let spans = &doc.paragraphs[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "A");
        assert!(!spans[0].properties.underline());
        assert_eq!(spans[1].text, "B");
        assert!(spans[1].properties.underline());
        assert_eq!(spans[2].text, "C");
        assert!(!spans[2].properties.underline());
    }

    #[test]
    fn test_plain_text_overline_and_special() {
        let doc = parse_plain_text("%%oX%%o");
        assert!(doc.paragraphs[0]
            .spans
            .iter()
            .any(|s| s.text == "X" && s.properties.overline()));
        // `%%d` still resolves to the degree symbol in plain text.
        assert_eq!(
            parse_plain_text("90%%d").paragraphs[0].to_plain_text(),
            "90°"
        );
    }

    #[test]
    fn test_plain_text_decimal_char_code() {
        // `%%176` → decimal 176 → '°'.
        assert_eq!(parse_plain_text("%%176").paragraphs[0].to_plain_text(), "°");
    }

    #[test]
    fn test_mtext_escaped_semicolon() {
        // `\;` is a literal semicolon, not a code terminator.
        let doc = parse_mtext(r"a\;b", false);
        assert_eq!(doc.to_plain_text(), "a;b");
    }

    #[test]
    fn test_mtext_caret_codes() {
        // `^I` → tab, `^J` → line break (new paragraph), `^M` → ignored.
        assert!(parse_mtext("a^Ib", false).to_plain_text().contains('\t'));
        assert_eq!(parse_mtext("a^Jb", false).paragraphs.len(), 2);
        assert_eq!(parse_mtext("a^Mb", false).to_plain_text(), "ab");
    }

    // ========================================================================
    // Width
    // ========================================================================

    #[test]
    fn test_parse_width_factor() {
        let doc = parse_mtext(r"{\W1.5;Wide}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.width_factor,
            Some(1.5)
        );
    }

    #[test]
    fn test_parse_width_relative() {
        let doc = parse_mtext(r"{\W0.5x;Narrow}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.width_factor,
            Some(0.5)
        );
    }

    // ========================================================================
    // Tracking
    // ========================================================================

    #[test]
    fn test_parse_tracking_positive() {
        let doc = parse_mtext(r"{\T100;Spread}", false);
        assert_eq!(doc.paragraphs[0].spans[0].properties.tracking, Some(100.0));
    }

    #[test]
    fn test_parse_tracking_negative() {
        let doc = parse_mtext(r"{\T-50;Condense}", false);
        assert_eq!(doc.paragraphs[0].spans[0].properties.tracking, Some(-50.0));
    }

    // ========================================================================
    // Oblique
    // ========================================================================

    #[test]
    fn test_parse_oblique_positive() {
        let doc = parse_mtext(r"{\Q15;Slanted}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.oblique_angle,
            Some(15.0)
        );
    }

    // ========================================================================
    // Line alignment
    // ========================================================================

    #[test]
    fn test_parse_line_align_bottom() {
        let doc = parse_mtext(r"{\A0;Bottom}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.line_align,
            Some(MTextLineAlignment::Bottom)
        );
    }

    #[test]
    fn test_parse_line_align_middle() {
        let doc = parse_mtext(r"{\A1;Middle}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.line_align,
            Some(MTextLineAlignment::Middle)
        );
    }

    #[test]
    fn test_parse_line_align_top() {
        let doc = parse_mtext(r"{\A2;Top}", false);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.line_align,
            Some(MTextLineAlignment::Top)
        );
    }

    // ========================================================================
    // Stacking
    // ========================================================================

    #[test]

    fn test_parse_stacking_fraction() {
        let doc = parse_mtext(r"{\S1/4;}", false);
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
        // Stacking spans have empty text — plain text derived from stacking data
        assert_eq!(doc.paragraphs[0].spans[0].text, "");
        assert_eq!(doc.paragraphs[0].to_plain_text(), "1/4");
        assert!(doc.paragraphs[0].spans[0].stacking.is_some());
        let stack = doc.paragraphs[0].spans[0].stacking.as_ref().unwrap();
        assert_eq!(stack.numerator, "1");
        assert_eq!(stack.denominator, "4");
        assert_eq!(stack.stacking_type, StackingType::Horizontal);
    }

    #[test]
    fn test_parse_stacking_diagonal() {
        let doc = parse_mtext(r"{\S1#4;}", false);
        let stack = doc.paragraphs[0].spans[0].stacking.as_ref().unwrap();
        assert_eq!(stack.stacking_type, StackingType::Diagonal);
    }

    #[test]
    fn test_parse_stacking_limit() {
        let doc = parse_mtext(r"{\Smax^ n;}", false);
        let stack = doc.paragraphs[0].spans[0].stacking.as_ref().unwrap();
        assert_eq!(stack.stacking_type, StackingType::Limit);
        assert_eq!(stack.numerator, "max");
        // space after ^ is consumed
        assert_eq!(stack.denominator, "n");
    }

    #[test]
    fn test_stacking_roundtrip() {
        // Verify stacking parses, serializes, and re-parses correctly
        let cases = &[
            (r"{\S1/2;}", r"{\S1/2;}"),
            (r"{\S3/4;}", r"{\S3/4;}"),
            (r"{\S1#4;}", r"{\S1#4;}"),
            (r"{\Smax^ n;}", r"{\Smax^n;}"),
        ];

        for (input, expected) in cases {
            let doc = parse_mtext(input, false);
            let serialized = doc.to_mtext_string();
            assert_eq!(
                serialized, *expected,
                "Serialization mismatch for input {:?}",
                input
            );

            let reparsed = parse_mtext(&serialized, false);
            assert_eq!(
                reparsed.paragraphs.len(),
                doc.paragraphs.len(),
                "Paragraph count mismatch for input {:?}",
                input
            );
            assert_eq!(
                reparsed.paragraphs[0].to_plain_text(),
                doc.paragraphs[0].to_plain_text(),
                "Plain text mismatch for input {:?}",
                input
            );
            assert_eq!(
                reparsed.paragraphs[0].spans[0].stacking, doc.paragraphs[0].spans[0].stacking,
                "Stacking data mismatch for input {:?}",
                input
            );
        }
    }

    #[test]
    fn test_stacking_inherits_properties() {
        // Stacking spans should inherit color/font/stroke from current context
        let doc = parse_mtext(r"{\C1;\L\S1/2;}", false);
        let span = &doc.paragraphs[0].spans[0];
        assert!(span.stacking.is_some());
        assert_eq!(span.properties.color, Some(MTextColor::Index(1)));
        assert!(span.properties.stroke.underline());

        // Roundtrip must preserve both stacking and properties
        let serialized = doc.to_mtext_string();
        let reparsed = parse_mtext(&serialized, false);
        let reparsed_span = &reparsed.paragraphs[0].spans[0];
        assert_eq!(reparsed_span.properties.color, span.properties.color);
        assert_eq!(
            reparsed_span.properties.stroke, span.properties.stroke,
            "Stroke not preserved after roundtrip"
        );
        assert!(
            reparsed_span.stacking.is_some(),
            "Stacking data lost after roundtrip"
        );
    }

    // ========================================================================
    // Special characters
    // ========================================================================

    #[test]
    fn test_parse_special_degree() {
        let doc = parse_mtext("{%%d}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "°");
    }

    #[test]
    fn test_parse_special_plus_minus() {
        let doc = parse_mtext("{%%p}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "±");
    }

    #[test]
    fn test_parse_special_diameter() {
        let doc = parse_mtext("{%%c}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Ø");
    }

    #[test]
    fn test_parse_special_percent() {
        let doc = parse_mtext("{%%%%}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "%");
    }

    // ========================================================================
    // Paragraph breaks
    // ========================================================================

    #[test]
    fn test_parse_multiple_paragraphs() {
        let doc = parse_mtext("{Para1\\PPara2}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Para1");
        assert_eq!(doc.paragraphs[1].to_plain_text(), "Para2");
    }

    #[test]
    fn test_parse_new_column() {
        let doc = parse_mtext("{Col1\\NCol2}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Col1");
        assert_eq!(doc.paragraphs[1].to_plain_text(), "Col2");
    }

    // ========================================================================
    // Unicode escape
    // ========================================================================

    #[test]
    fn test_parse_unicode_escape() {
        let doc = parse_mtext(r"{\U+00B0}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "°");
    }

    // ========================================================================
    // Non-breaking space
    // ========================================================================

    #[test]
    fn test_parse_nbsp() {
        let doc = parse_mtext("{a\\~b}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "a\u{00A0}b");
    }

    // ========================================================================
    // Escaping
    // ========================================================================

    #[test]
    fn test_parse_escaped_backslash() {
        // {\\\\} = { \\ } → one escaped backslash → "\"
        let doc = parse_mtext("{\\\\}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "\\");
    }

    #[test]
    fn test_parse_escaped_braces() {
        // {\\{\\}} → { \{ \} } → literal { and }
        let doc = parse_mtext("{\\{\\}}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "{}");
    }

    // ========================================================================
    // Style groups
    // ========================================================================

    #[test]
    fn test_parse_nested_groups() {
        // Outer {C1} gets C1, Inner {C2} gets C2, after pop we get C1 again
        let doc = parse_mtext("{\\C1;Outer{\\C2;Inner}Outer}", false);
        // After pop, style changes so "Outer" after pop is separate span
        assert_eq!(doc.paragraphs[0].spans.len(), 3);
        assert_eq!(doc.paragraphs[0].spans[0].text, "Outer");
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(doc.paragraphs[0].spans[1].text, "Inner");
        assert_eq!(
            doc.paragraphs[0].spans[1].properties.color,
            Some(MTextColor::Index(2))
        );
        assert_eq!(doc.paragraphs[0].spans[2].text, "Outer");
        assert_eq!(
            doc.paragraphs[0].spans[2].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_parse_style_inheritance() {
        // Height set in outer group should be inherited
        let doc = parse_mtext("{\\H2;Normal{\\C1;Colored}Back}", false);
        // Normal has H2, Colored has H2+C1, Back has H2
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
        assert_eq!(
            doc.paragraphs[0].spans[1].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
        assert_eq!(
            doc.paragraphs[0].spans[2].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
    }

    // ========================================================================
    // Paragraph properties
    // ========================================================================

    #[test]
    fn test_parse_paragraph_align_center() {
        let doc = parse_mtext("{\\pqc;Centered text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.alignment,
            Some(MTextParagraphAlignment::Center)
        );
    }

    #[test]
    fn test_parse_paragraph_align_right() {
        let doc = parse_mtext("{\\pqr;Right text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.alignment,
            Some(MTextParagraphAlignment::Right)
        );
    }

    #[test]
    fn test_parse_paragraph_indent() {
        let doc = parse_mtext("{\\pi2;Indented}", false);
        assert_eq!(doc.paragraphs[0].properties.first_line_indent, Some(2.0));
    }

    #[test]
    fn test_parse_paragraph_spacing_before() {
        let doc = parse_mtext("{\\pb0.5;Text}", false);
        assert_eq!(doc.paragraphs[0].properties.spacing_before, Some(0.5));
    }

    #[test]
    fn test_parse_paragraph_spacing_after() {
        let doc = parse_mtext("{\\pa0.3;Text}", false);
        assert_eq!(doc.paragraphs[0].properties.spacing_after, Some(0.3));
    }

    #[test]
    fn test_parse_paragraph_line_spacing_exact() {
        let doc = parse_mtext("{\\pse0.3;Text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.line_spacing,
            Some(MTextLineSpacing::Exact(0.3))
        );
    }

    #[test]
    fn test_parse_paragraph_line_spacing_multiple() {
        let doc = parse_mtext("{\\psm1.5;Text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.line_spacing,
            Some(MTextLineSpacing::Multiple(1.5))
        );
    }

    #[test]
    fn test_parse_paragraph_tab_stops() {
        let doc = parse_mtext("{\\pt3,6,9;Text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.tab_stops,
            vec![TabStop::Left(3.0), TabStop::Left(6.0), TabStop::Left(9.0)]
        );
    }

    #[test]
    fn test_parse_paragraph_decimal_tab_stop() {
        // `D.2` = decimal stop at 2 aligning on `.` — the `.` is the alignment
        // character, not part of the position.
        let doc = parse_mtext("{\\ptD.2;Text}", false);
        assert_eq!(
            doc.paragraphs[0].properties.tab_stops,
            vec![TabStop::Decimal(2.0)]
        );
    }

    #[test]
    fn test_raw_line_feed_breaks_paragraph() {
        // A raw LF (0x0A) is a hard paragraph break, like `\P` / `^J`.
        let doc = parse_mtext("a\nb", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].spans[0].text, "a");
        assert_eq!(doc.paragraphs[1].spans[0].text, "b");
    }

    #[test]
    fn test_control_char_becomes_space() {
        // Any control char other than tab/LF renders as a space.
        let doc = parse_mtext("a\u{1}b", false);
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].spans[0].text, "a b");
    }

    #[test]
    fn test_parse_paragraph_all_properties() {
        // Real-world example: indent, margins, spacing, line spacing, tabs
        let doc = parse_mtext("{\\pi2,l0.8,r3.2,b0.4,a0.3,se0.5,t3,6;Text}", false);
        assert_eq!(doc.paragraphs[0].properties.first_line_indent, Some(2.0));
        assert_eq!(doc.paragraphs[0].properties.left_margin, Some(0.8));
        assert_eq!(doc.paragraphs[0].properties.right_margin, Some(3.2));
        assert_eq!(doc.paragraphs[0].properties.spacing_before, Some(0.4));
        assert_eq!(doc.paragraphs[0].properties.spacing_after, Some(0.3));
        assert_eq!(
            doc.paragraphs[0].properties.line_spacing,
            Some(MTextLineSpacing::Exact(0.5))
        );
        assert_eq!(
            doc.paragraphs[0].properties.tab_stops,
            vec![TabStop::Left(3.0), TabStop::Left(6.0)]
        );
    }

    #[test]
    fn test_roundtrip_paragraph_properties() {
        let doc = parse_mtext("{\\pi2,l0.8,r3.2,b0.4,a0.3,se0.5,t3,6;Text}", false);
        let output = doc.to_mtext_string();
        assert!(output.contains("\\p"));
        assert!(output.contains("i2"));
        assert!(output.contains("l0.8"));
        assert!(output.contains("b0.4"));
        assert!(output.contains("a0.3"));
        assert!(output.contains("se0.5"));
        assert!(output.contains("t3,6"));
    }

    // ========================================================================
    // Complex real-world examples
    // ========================================================================

    #[test]
    fn test_parse_complex_mtext() {
        let doc = parse_mtext(
            "{\\fArial|b1|i0;{\\H2.0;{\\C1;DIMENSION}}{\\H0.8;{\\C3;NOTE}}}",
            false,
        );
        assert!(!doc.is_empty());
        assert!(!doc.paragraphs.is_empty());
    }

    #[test]
    fn test_parse_real_world_dimension() {
        let doc = parse_mtext(r"{\S1/4;%%c 20}", false);
        // Should parse stacking + diameter symbol
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_parse_real_world_tolerance() {
        let doc = parse_mtext(r"{10.0 \S+0.05/-0.02;}", false);
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_parse_many_paragraphs() {
        // 20 paragraphs, each ending with \P
        let input: String = format!(
            "{{{}}}",
            (0..20).map(|i| format!("Para{}\\P", i)).collect::<String>()
        );
        let doc = parse_mtext(&input, false);
        assert_eq!(doc.paragraphs.len(), 20);
    }

    #[test]
    fn test_parse_trailing_paragraph_break() {
        // Trailing \P creates paragraphs for the non-empty content
        let doc = parse_mtext("{Line1\\PLine2\\P}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Line1");
        assert_eq!(doc.paragraphs[1].to_plain_text(), "Line2");
    }

    #[test]
    fn test_parse_many_color_changes() {
        let input: String = (0..10)
            .map(|i| format!("{{\\C{};Text{}}}", (i % 7) + 1, i))
            .collect();
        let doc = parse_mtext(&input, false);
        assert!(!doc.is_empty());
    }

    // ========================================================================
    // Edge cases
    // ========================================================================

    #[test]
    fn test_parse_backslash_at_end() {
        // \} is an escaped brace → literal }
        let doc = parse_mtext(r"{hello\}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "hello}");
    }

    #[test]
    fn test_parse_control_code_at_end() {
        let doc = parse_mtext(r"{hello\C1;}", false);
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_parse_unknown_control_code() {
        let doc = parse_mtext(r"{hello\Ztest}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "hello\\Ztest");
    }

    #[test]
    fn test_parse_unclosed_brace() {
        let doc = parse_mtext("{hello", false);
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_parse_empty_braces() {
        let doc = parse_mtext("{}", false);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_parse_only_special_chars() {
        let doc = parse_mtext("{%%d%%p%%c}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "°±Ø");
    }

    #[test]
    fn test_parse_single_percent() {
        let doc = parse_mtext("{%}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "%");
    }

    #[test]
    fn test_parse_deeply_nested_braces() {
        // {{\\{\\{\\}}} → nested groups with escaped braces/backslash
        let doc = parse_mtext("{{\\{\\}}", false);
        assert!(!doc.is_empty());
    }

    #[test]
    fn test_parse_consecutive_percents() {
        let doc = parse_mtext("{%%d%%d}", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "°°");
    }

    // ========================================================================
    // Merge spans
    // ========================================================================

    #[test]
    fn test_parse_with_merge_spans() {
        let doc = parse_mtext(r"{\C1;Red more}", true);
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
    }

    #[test]
    fn test_parse_without_merge_spans() {
        let doc = parse_mtext(r"{\C1;Red more}", false);
        // merge_spans=false still creates one span because there's no preceding text
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
    }

    #[test]
    fn test_roundtrip_simple_formatted() {
        let doc = parse_mtext("{\\C1;Red\\C0;; normal}", false);
        let s = doc.to_mtext_string();
        assert!(s.contains("\\C1;"));
    }

    #[test]
    fn test_roundtrip_escaped_chars() {
        // {\\} in MTEXT → one literal backslash
        let doc = parse_mtext("{\\\\}", false);
        let plain = doc.to_plain_text();
        assert_eq!(plain.len(), 1);
        assert_eq!(plain.chars().next(), Some('\\'));
    }

    // ========================================================================
    // Plain text parsing (TEXT entities)
    // ========================================================================

    #[test]
    fn test_parse_plain_special_chars() {
        let doc = parse_plain_text("%%c%%d%%p");
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Ø°±");
    }

    #[test]
    fn test_parse_plain_control_codes_literal() {
        // TEXT entities don't support backslash control codes
        let doc = parse_plain_text("\\C1;Red");
        assert_eq!(doc.paragraphs[0].to_plain_text(), "\\C1;Red");
    }

    #[test]
    fn test_parse_plain_paragraph_break() {
        let doc = parse_plain_text("Line1\\PLine2");
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Line1");
        assert_eq!(doc.paragraphs[1].to_plain_text(), "Line2");
    }

    #[test]
    fn test_parse_plain_no_brace_grouping() {
        // Braces are literal text in plain mode
        let doc = parse_plain_text("{text}");
        assert_eq!(doc.paragraphs[0].to_plain_text(), "{text}");
    }

    #[test]
    fn test_parse_mtext_unbraced_control_codes() {
        // MTEXT parser handles control codes even without braces
        let doc = parse_mtext("\\C1;Red\\C0;; normal", false);
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].spans.len(), 2);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Red normal");
    }

    #[test]
    fn test_parse_mtext_unbraced_paragraphs() {
        let doc = parse_mtext("Line1\\PLine2", false);
        assert_eq!(doc.paragraphs.len(), 2);
    }

    #[test]
    fn test_parse_mtext_unbraced_font() {
        let doc = parse_mtext("\\fArial;Hello", false);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "Hello");
        assert!(doc.paragraphs[0].spans[0].properties.font.is_some());
    }

    // ========================================================================
    // Style inheritance across paragraphs
    // ========================================================================

    #[test]
    fn test_color_carries_across_paragraphs() {
        // {\C1;First\PSecond} → both paragraphs should be red (aci=1)
        let doc = parse_mtext("{\\C1;First\\PSecond}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_font_carries_across_paragraphs() {
        let doc = parse_mtext("{\\fArial;First\\PSecond}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert!(doc.paragraphs[0].spans[0].properties.font.is_some());
        assert!(doc.paragraphs[1].spans[0].properties.font.is_some());
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.font,
            doc.paragraphs[1].spans[0].properties.font
        );
    }

    #[test]
    fn test_underline_carries_across_paragraphs() {
        let doc = parse_mtext("{\\LUnderlined\\PStill underlined}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert!(doc.paragraphs[0].spans[0].properties.stroke.underline());
        assert!(doc.paragraphs[1].spans[0].properties.stroke.underline());
    }

    #[test]
    fn test_height_carries_across_paragraphs() {
        let doc = parse_mtext("{\\H2;Tall\\PStill tall}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.height,
            Some(MTextScalar::Absolute(2.0))
        );
    }

    #[test]
    fn test_style_change_in_second_paragraph() {
        // Color changes mid-document
        let doc = parse_mtext("{\\C1;Red\\P\\C5;Blue}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(5))
        );
    }

    #[test]
    fn test_brace_group_resets_style() {
        // Inner group pops context, restoring outer style
        let doc = parse_mtext("{\\C1;Red{\\C5;Blue}Red again}", false);
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].spans.len(), 3);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            doc.paragraphs[0].spans[1].properties.color,
            Some(MTextColor::Index(5))
        );
        // After inner group pops, outer color should restore
        assert_eq!(
            doc.paragraphs[0].spans[2].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_plain_text_braces_are_literal() {
        // In plain mode, braces are literal text
        let doc = parse_plain_text("{\\C1;Red}");
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].to_plain_text(), "{\\C1;Red}");
    }

    #[test]
    fn test_new_column_carries_style() {
        let doc = parse_mtext("{\\C1;First\\NSecond}", false);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
    }

    #[test]
    fn test_complex_roundtrip_stable() {
        // Complex MTEXT with:
        // - Red color spanning first 2 paragraphs
        // - Underline on paragraph 1 only, turned off for paragraph 2
        // - Font change in paragraph 2
        // - Color reset + blue in paragraph 3
        // - Underline on/off in paragraph 3
        let input = r"{\C1;\LRed and underlined\l\P\fArial;Red Arial\P\C5;Blue \Lbold\l}";
        let doc = parse_mtext(input, false);

        // Verify structure
        assert_eq!(doc.paragraphs.len(), 3);

        // Para 0: red + underline
        assert_eq!(doc.paragraphs[0].spans.len(), 1);
        assert_eq!(
            doc.paragraphs[0].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert!(doc.paragraphs[0].spans[0].properties.stroke.underline());

        // Para 1: red + font Arial, underline OFF
        assert_eq!(doc.paragraphs[1].spans.len(), 1);
        assert_eq!(
            doc.paragraphs[1].spans[0].properties.color,
            Some(MTextColor::Index(1))
        );
        assert!(doc.paragraphs[1].spans[0].properties.font.is_some());
        assert!(!doc.paragraphs[1].spans[0].properties.stroke.underline());

        // Para 2: blue, then bold underline, then back to blue
        assert_eq!(doc.paragraphs[2].spans.len(), 2);
        assert_eq!(
            doc.paragraphs[2].spans[0].properties.color,
            Some(MTextColor::Index(5))
        );

        // Roundtrip: parse -> serialize -> parse must match
        let serialized = doc.to_mtext_string();

        let reparsed = parse_mtext(&serialized, false);

        // Same paragraph count
        assert_eq!(reparsed.paragraphs.len(), doc.paragraphs.len());
        // Same plain text content
        assert_eq!(
            reparsed.to_plain_text(),
            doc.to_plain_text(),
            "Plain text mismatch after roundtrip"
        );
        // Same color on each paragraph's first span
        for (i, (orig, reparsed)) in doc
            .paragraphs
            .iter()
            .zip(reparsed.paragraphs.iter())
            .enumerate()
        {
            assert_eq!(
                orig.spans.first().map(|s| &s.properties.color),
                reparsed.spans.first().map(|s| &s.properties.color),
                "Color mismatch in paragraph {}",
                i
            );
        }
    }

    #[test]
    fn test_corpus_roundtrip_semantic_equivalence() {
        // Each entry: (description, input_mtext_string)
        let cases: &[(&str, &str)] = &[
            // Basic features
            ("plain text", r"{Simple text}"),
            ("special chars", r"{%%c100%%d%%p2}"),
            ("paragraph break", r"{Line1\PLine2}"),
            ("color change", r"{\C1;Red\C5;Blue\C3;Green}"),
            ("font change", r"{\fArial;Arial\fTimes New Roman;Times}"),
            ("underline", r"{\LUnderlined\lNormal}"),
            ("overline", r"{\OOverlined\oNormal}"),
            ("strikethrough", r"{\KStruck\kNormal}"),
            ("height", r"{\H2;Tall\H1;Normal}"),
            ("width factor", r"{\W2;Wide\W1;Normal}"),
            ("tracking", r"{\T0.5;Spread\T0;Normal}"),
            ("oblique", r"{\Q10;Slanted\Q0;Normal}"),
            ("line alignment", r"{\A1;Middle\A0;Bottom}"),
            // Escaping
            ("escaped brace", r"{\{Literal brace\}}"),
            ("escaped backslash", r"{Two\\One}"),
            // Stacking
            ("horizontal fraction", r"{\S1/2;fraction}"),
            ("limit style", r"{\Smax/;limits}"),
            ("diagonal fraction", r"{\S1/2#;diag}"),
            // Unicode
            ("unicode escape", r"{\U+2603;Snowflake}"),
            // Paragraph properties
            ("center alignment", r"{\pqc;Centered}"),
            ("first line indent", r"{\pi0.5;Indented}"),
            ("left margin", r"{\pl1.0;Margin}"),
            ("right margin", r"{\pr2.0;Rmargin}"),
            ("spacing before", r"{\pb0.3;Before}"),
            ("spacing after", r"{\pa0.2;After}"),
            ("exact line spacing", r"{\pse0.4;Exact}"),
            ("multiple line spacing", r"{\psm1.5;Multiple}"),
            ("tab stops", r"{\pt2,4,6;Tabs}"),
            (
                "all paragraph props",
                r"{\pqc,i0.5,l0.3,r0.5,b0.1,a0.1,se0.5,t2,4;All}",
            ),
            // Multi-paragraph with style inheritance
            ("color across paras", r"{\C1;Para1\PPara2\PPara3}"),
            ("font across paras", r"{\fArial;Para1\PPara2}"),
            ("underline across paras", r"{\LPara1\PPara2\l\PPara3}"),
            // Mixed features in multiple paragraphs
            (
                "multi-para mixed styles",
                r"{\C1;\LRed underline\P\fArial;Red Arial\P\C5;Blue normal\l}",
            ),
            ("brace group restore", r"{\C1;Red{\C5;Blue}Back to red}"),
            (
                "nested brace groups",
                r"{\C1;Red{\fArial;Blue Arial{\C3;Green}}}",
            ),
            ("stacking in paragraphs", r"{\S1/2;\P\S3/4;second frac}"),
            ("special chars in color", r"{\C1;%%c100%%d\P%%p20%%d}"),
            // Edge cases
            ("empty string", r"{}"),
            ("only paragraph break", r"{\P}"),
            ("multiple paragraph breaks", r"{A\P\P\PC}"),
            ("trailing paragraph break", r"{A\PB\P}"),
            ("unicode and color", r"{\C1;\U+2603;\C5;\U+2601;}"),
            ("all strokes together", r"{\L\O\KAll three\k\o\l}"),
            ("height and width", r"{\H2;\W1.5;Tall and wide}"),
            // Real-world style MTEXT
            ("dimension-style", r"{\C1;\H1.5;Dimension\S1/2; value}"),
            (
                "titleblock-style",
                r"{\fArial,b1;\H3;Title\P\fArial;\H1.5;Subtitle}",
            ),
            (
                "notes-style",
                r"{\fArial;\H1.5;1. First note\P2. Second note\P3. Third note}",
            ),
            // New column
            ("new column", r"{Column1\P\NColumn2}"),
            // Plain text mode
            ("plain no braces", r"Simple plain text"),
            ("plain with percent", r"%%c%%d%%p"),
        ];

        for (name, input) in cases {
            // Parse
            let doc = parse_mtext(input, false);

            // Skip empty documents for further checks
            if doc.paragraphs.is_empty() {
                continue;
            }

            // Serialize
            let serialized = doc.to_mtext_string();

            // Skip cases that serialize to empty/minimal (no content to roundtrip)
            if serialized.is_empty() || serialized == "{}" || serialized == r"\P" {
                continue;
            }

            // Re-parse
            let reparsed = parse_mtext(&serialized, false);

            // Semantic equivalence checks
            assert_eq!(
                reparsed.paragraphs.len(),
                doc.paragraphs.len(),
                "[{}] Paragraph count mismatch: {} vs {}",
                name,
                reparsed.paragraphs.len(),
                doc.paragraphs.len()
            );

            assert_eq!(
                reparsed.to_plain_text(),
                doc.to_plain_text(),
                "[{}] Plain text mismatch after roundtrip:\n  original:   {:?}\n  reparsed:   {:?}",
                name,
                doc.to_plain_text(),
                reparsed.to_plain_text()
            );

            // Check each paragraph
            for (pi, (orig_para, reparsed_para)) in doc
                .paragraphs
                .iter()
                .zip(reparsed.paragraphs.iter())
                .enumerate()
            {
                assert_eq!(
                    orig_para.spans.len(),
                    reparsed_para.spans.len(),
                    "[{}] Para {} span count mismatch: {} vs {}",
                    name,
                    pi,
                    orig_para.spans.len(),
                    reparsed_para.spans.len()
                );

                // Check each span's properties
                for (si, (orig_span, reparsed_span)) in orig_para
                    .spans
                    .iter()
                    .zip(reparsed_para.spans.iter())
                    .enumerate()
                {
                    assert_eq!(
                        orig_span.text, reparsed_span.text,
                        "[{}] Para{} Span{} text mismatch: {:?} vs {:?}",
                        name, pi, si, orig_span.text, reparsed_span.text
                    );
                    assert_eq!(
                        orig_span.properties.color, reparsed_span.properties.color,
                        "[{}] Para{} Span{} color mismatch",
                        name, pi, si
                    );
                    assert_eq!(
                        orig_span.properties.font, reparsed_span.properties.font,
                        "[{}] Para{} Span{} font mismatch",
                        name, pi, si
                    );
                    assert_eq!(
                        orig_span.properties.height, reparsed_span.properties.height,
                        "[{}] Para{} Span{} height mismatch",
                        name, pi, si
                    );
                    assert_eq!(
                        orig_span.properties.stroke, reparsed_span.properties.stroke,
                        "[{}] Para{} Span{} stroke mismatch",
                        name, pi, si
                    );
                }

                // Check paragraph properties
                assert_eq!(
                    orig_para.properties.alignment, reparsed_para.properties.alignment,
                    "[{}] Para{} alignment mismatch",
                    name, pi
                );
                assert_eq!(
                    orig_para.properties.first_line_indent,
                    reparsed_para.properties.first_line_indent,
                    "[{}] Para{} indent mismatch",
                    name,
                    pi
                );
                assert_eq!(
                    orig_para.properties.left_margin, reparsed_para.properties.left_margin,
                    "[{}] Para{} left_margin mismatch",
                    name, pi
                );
            }
        }
    }
}
