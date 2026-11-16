//! Text Editor tools that patch the source: removing paragraph or all
//! formatting, Match (copy one character's formatting onto selected text),
//! the column break, the character set, a text style change, AutoStack and
//! the stack properties.
use super::prefs::{prefs, StackPosition, StackStyle};
use super::raw_adapter::{EditError, Kind, SourceIndex};
use super::raw_edit::{editor_document, font_code};
use super::raw_operations::{apply_patches, TextEdit};
use super::raw_paragraph_codes::{rewrite, InForce};
use super::*;
use crate::app::OpenCADStudio;
use document::entities::mtext_format::{MTextColor, MTextFont, MTextScalar};
use std::ops::Range;

/// What Clear removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearMode {
    Character,
    Paragraph,
    All,
}

/// The raw byte range of the visible characters `range`, when each is plain
/// text with nothing but plain text between them.
fn plain_raw_range(index: &SourceIndex, range: &Range<usize>) -> Result<Range<usize>, EditError> {
    let units: Vec<_> = index
        .units
        .iter()
        .filter(|u| u.visible.start >= range.start && u.visible.end <= range.end)
        .collect();
    let count: usize = units.iter().map(|u| u.visible.len()).sum();
    if count != range.len()
        || units
            .iter()
            .any(|u| !matches!(u.kind, Kind::Text | Kind::Escape | Kind::Unicode))
    {
        return Err(EditError::StackText);
    }
    let (Some(first), Some(last)) = (units.first(), units.last()) else {
        return Err(EditError::InvalidRange);
    };
    let raw = first.raw.start..last.raw.end;
    // Plain text only: no control or group between the characters.
    if index
        .tokens
        .iter()
        .any(|t| t.raw.start >= raw.start && t.raw.end <= raw.end && t.visible.is_none())
    {
        return Err(EditError::StackText);
    }
    Ok(raw)
}

/// The code a span's formatting is written with (what Match copies).
fn span_code(p: &SpanProperties) -> Result<String, EditError> {
    let mut code = String::new();
    if let Some(font) = &p.font {
        code.push_str(&font_code(font)?);
    }
    if let Some((r, g, b)) = p.color_rgb {
        code.push_str(&format!("\\c{};", u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)));
    } else {
        match p.color {
            Some(MTextColor::Index(n)) => code.push_str(&format!("\\C{n};")),
            Some(MTextColor::ByLayer) => code.push_str("\\C256;"),
            Some(MTextColor::ByBlock) => code.push_str("\\C0;"),
            _ => {}
        }
    }
    match p.height {
        Some(MTextScalar::Factor(f)) => code.push_str(&format!("\\H{}x;", raw_span::number(f))),
        Some(MTextScalar::Absolute(a)) => code.push_str(&format!("\\H{};", raw_span::number(a))),
        None => {}
    }
    if let Some(w) = p.width_factor {
        code.push_str(&format!("\\W{};", raw_span::number(w)));
    }
    if let Some(q) = p.oblique_angle {
        code.push_str(&format!("\\Q{};", raw_span::number(q)));
    }
    if let Some(t) = p.tracking {
        code.push_str(&format!("\\T{};", raw_span::number(t)));
    }
    if p.stroke.underline() {
        code.push_str("\\L");
    }
    if p.stroke.overline() {
        code.push_str("\\O");
    }
    if p.stroke.strikethrough() {
        code.push_str("\\K");
    }
    Ok(code)
}

/// The pitch and family byte of a TrueType family as its font code names it
/// (`|p34` Arial, `|p18` Times New Roman, `|p49` Consolas), read from the
/// installed face. A family that is not installed falls back to the swiss
/// family, roman for a serif name.
pub(super) fn pitch_of(family: &str) -> u8 {
    crate::scene::text::sysfont::with_face_data(family, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let os2 = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"OS/2"))?;
        Some(pitch_family(os2.get(32..42)?, face.is_monospaced()))
    })
    .flatten()
    .unwrap_or_else(|| {
        let lower = family.to_lowercase();
        let serif = ["times", "roman", "georgia", "garamond", "cambria", "book", "serif"];
        if serif.iter().any(|s| lower.contains(s)) && !lower.contains("sans") {
            18
        } else {
            34
        }
    })
}

/// The system's pitch and family byte for a face's PANOSE bytes: script
/// and decorative faces by family kind, monospaced faces modern, and the
/// rest by serif style — none given, swiss for the sans styles, else roman.
/// Pitch is fixed (1) or variable (2).
fn pitch_family(panose: &[u8], fixed: bool) -> u8 {
    let pitch = if fixed { 1 } else { 2 };
    let family = match (panose[0], panose[1], panose[3]) {
        (3, ..) => 0x40,
        (4, ..) => 0x50,
        (_, _, 9) => 0x30,
        _ if fixed => 0x30,
        (_, 0 | 1, _) => 0,
        (_, 11.., _) => 0x20,
        _ => 0x10,
    };
    family | pitch
}

#[cfg(test)]
#[test]
fn pitch_family_follows_the_panose_bytes() {
    // Arial, Times New Roman, Calibri (rounded sans), Courier New, Comic
    // Sans MS, Gabriola, Wingdings, Symbol, OCR A Extended (monospaced).
    let cases: [([u8; 4], bool, u8); 9] = [
        ([2, 11, 6, 4], false, 34),
        ([2, 2, 6, 3], false, 18),
        ([2, 15, 5, 2], false, 34),
        ([2, 7, 3, 9], true, 49),
        ([3, 15, 7, 2], false, 66),
        ([4, 4, 6, 5], false, 82),
        ([5, 0, 0, 0], false, 2),
        ([5, 5, 1, 2], false, 18),
        ([2, 1, 5, 9], false, 50),
    ];
    for (panose, fixed, code) in cases {
        assert_eq!(pitch_family(&panose, fixed), code, "{panose:?}");
    }
}

impl MTextEditorState {
    /// The formatting of the character before the caret (the one after it
    /// at the start of a paragraph).
    fn caret_props(&self) -> SpanProperties {
        let cells = doc_to_cells(&self.doc);
        let range = self.selected_range();
        let at = if range.is_empty() {
            range.start.checked_sub(1).unwrap_or(0)
        } else {
            range.start
        };
        [at, range.start]
            .iter()
            .find_map(|&n| match cells.get(n) {
                Some(Cell::Char(_, p)) => Some(p.clone()),
                Some(Cell::Stack { props, .. }) => Some(props.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The visible range of the stack the caret is beside or the selection
    /// covers, with its data.
    pub(in crate::app) fn caret_stack(&self) -> Option<(Range<usize>, StackingData, SpanProperties)> {
        let cells = doc_to_cells(&self.doc);
        let range = self.selected_range();
        let probe = [range.start, range.start.saturating_sub(1)];
        let at = probe
            .iter()
            .copied()
            .find(|&n| matches!(cells.get(n), Some(Cell::Stack { .. })))?;
        let mut start = at;
        while start > 0 && !matches!(cells.get(start), Some(Cell::Stack { head: true, .. })) {
            start -= 1;
        }
        let Some(Cell::Stack { data, props, .. }) = cells.get(start).cloned() else {
            return None;
        };
        let mut end = start + 1;
        while matches!(cells.get(end), Some(Cell::Stack { head: false, .. })) {
            end += 1;
        }
        Some((start..end, data, props))
    }
}

impl OpenCADStudio {
    /// Clear: the selection's character formatting, the touched paragraphs'
    /// paragraph formatting, or both.
    pub(in crate::app) fn mtext_clear(&mut self, mode: ClearMode) {
        if mode == ClearMode::Character {
            self.raw_clear_formatting();
            return;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        let whole = 0..doc_to_cells(&ed.doc).len();
        let chars = Some(range.clone())
            .filter(|r| !r.is_empty())
            .unwrap_or(whole.clone());
        let touched = ed.selected_paragraphs();
        self.edit_raw(|_, index| {
            let mut raw = index.raw.to_string();
            if mode == ClearMode::All && !chars.is_empty() {
                let keep = super::raw_style::object_controls(&raw);
                raw = SourceIndex::new(&raw).cleared_selection(chars.clone(), &keep)?;
            }
            let raw = rewrite(&raw, |n, state| {
                if touched.contains(&n) {
                    *state = InForce::default();
                }
            })?;
            Ok(TextEdit {
                raw,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }

    /// Match: on, it remembers the formatting at the caret; each selection
    /// made with the pointer then takes it, until Match is pressed again or
    /// Esc.
    pub(in crate::app) fn mtext_match_toggle(&mut self) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        ed.matching = match ed.matching.take() {
            Some(_) => None,
            None => Some(ed.caret_props()),
        };
    }

    /// A pointer selection was made: with Match on, it takes the remembered
    /// formatting (the character formatting only).
    pub(in crate::app) fn mtext_match_apply(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let Some(props) = ed.matching.clone() else {
            return;
        };
        let range = ed.selected_range();
        if range.is_empty() {
            return;
        }
        let Ok(code) = span_code(&props) else {
            return;
        };
        self.edit_raw(|_, index| {
            let keep = super::raw_style::object_controls(index.raw);
            let cleared = index.cleared_selection(range.clone(), &keep)?;
            let raw = if code.is_empty() {
                cleared
            } else {
                raw_span::patch_span(
                    &cleared,
                    range.clone(),
                    |_| Ok(code.clone()),
                    |want, t| {
                        if t.font.is_some() {
                            want.font = t.font.clone();
                        }
                        if t.color.is_some() || t.color_rgb.is_some() {
                            want.color = t.color;
                            want.color_rgb = t.color_rgb;
                        }
                        if t.height.is_some() {
                            want.height = t.height;
                        }
                        if t.width_factor.is_some() {
                            want.width_factor = t.width_factor;
                        }
                        if t.oblique_angle.is_some() {
                            want.oblique_angle = t.oblique_angle;
                        }
                        if t.tracking.is_some() {
                            want.tracking = t.tracking;
                        }
                        want.stroke = t.stroke.clone();
                    },
                )?
            };
            Ok(TextEdit {
                raw,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }

    /// Insert Column Break (`\N`) at the caret, replacing the selection; only
    /// in a text with columns.
    pub(in crate::app) fn mtext_column_break(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        if ed.column_type == 0 {
            return;
        }
        self.edit_raw(|ed, index| {
            let cleared = index.replace(ed.selected_range(), "")?;
            let base = SourceIndex::new(&cleared.raw);
            let count = base.validate(false)?;
            let at = base.insertion_byte(cleared.caret)?;
            let raw = apply_patches(&cleared.raw, vec![(at..at, "\\N".to_string())])?;
            base.certify(&raw, count + 1)?;
            Ok(TextEdit {
                raw,
                caret: cleared.caret + 1,
            })
        });
    }

    /// The character set of the selection: its TrueType font written with
    /// the code page `charset` (`\fArial|b0|i0|c204|p34;`).
    pub(in crate::app) fn mtext_charset(&mut self, charset: u8) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        if range.is_empty() {
            return;
        }
        let style_face = ed.object_style.face.clone();
        self.raw_selection_patch(range, move |raw, range| {
            raw_span::patch_span(
                raw,
                range,
                |p| {
                    let mut font = p.font.clone().unwrap_or_else(|| MTextFont {
                        name: style_face.clone(),
                        ..Default::default()
                    });
                    if font.name.trim().is_empty()
                        || crate::scene::text::lff::is_shx_name(&font.name)
                    {
                        return Err(EditError::UnsupportedInput);
                    }
                    font.bold_explicit = true;
                    font.italic_explicit = true;
                    font.charset = Some(u16::from(charset));
                    if font.pitch.is_none() {
                        font.pitch = Some(pitch_of(&font.name));
                    }
                    font_code(&font)
                },
                |p, t| p.font = t.font.clone(),
            )
        });
    }

    /// The character set of the selection's font, for the window to start
    /// from (the system's code page when none is written).
    pub(in crate::app) fn mtext_selection_charset(&self) -> u8 {
        self.mtext_editor
            .as_ref()
            .and_then(|ed| ed.caret_props().font.and_then(|f| f.charset))
            .and_then(|c| u8::try_from(c).ok())
            .unwrap_or_else(super::import_text::system_charset)
    }

    /// A style picked for the text: the formatting the style itself sets —
    /// font, height, width and oblique codes — goes from the whole text,
    /// then the text takes the style.
    pub(in crate::app) fn mtext_change_style(&mut self, name: String) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        self.edit_raw(|_, index| {
            let count = index.validate(false)?;
            let patches = index
                .tokens
                .iter()
                .filter(|t| matches!(t.kind, Kind::Control('f' | 'F' | 'H' | 'Q' | 'W')))
                .map(|t| (t.raw.clone(), String::new()))
                .collect();
            let raw = apply_patches(index.raw, patches)?;
            index.certify(&raw, count)?;
            Ok(TextEdit {
                raw,
                caret: range.end,
            })
        });
        self.mtext_object_style(name);
        self.keep_selection(range);
    }

    /// AutoStack, after a typed space: the digits around `/`, `#` or `^`
    /// just before it become a stack, `{\H0.7x;\S1/2;}`, and the text sits
    /// its stacks centred (`\A1;`) unless it says otherwise.
    pub(in crate::app) fn mtext_autostack(&mut self) {
        let p = prefs();
        if !p.auto_stack {
            return;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        if !range.is_empty() || range.start < 4 {
            return;
        }
        let cells = doc_to_cells(&ed.doc);
        let ch = |n: usize| match cells.get(n) {
            Some(Cell::Char(c, _)) => Some(*c),
            _ => None,
        };
        if ch(range.start - 1) != Some(' ') {
            return;
        }
        let end = range.start - 1;
        let mut k = end;
        while k > 0 && ch(k - 1).is_some_and(|c| c.is_ascii_digit()) {
            k -= 1;
        }
        let denominator: String = (k..end).filter_map(ch).collect();
        if denominator.is_empty() || k == 0 {
            return;
        }
        let Some(separator) = ch(k - 1).filter(|c| matches!(c, '/' | '#' | '^')) else {
            return;
        };
        let sep_at = k - 1;
        let mut start = sep_at;
        while start > 0 && ch(start - 1).is_some_and(|c| c.is_ascii_digit()) {
            start -= 1;
        }
        let numerator: String = (start..sep_at).filter_map(ch).collect();
        // A decimal point makes no fraction.
        if numerator.is_empty() || (start > 0 && ch(start - 1) == Some('.')) {
            return;
        }
        // Remove leading blank: `1 x/y` becomes `1x/y`.
        let trim = p.auto_stack_trim_blank
            && start >= 2
            && ch(start - 1) == Some(' ')
            && ch(start - 2).is_some_and(|c| c.is_ascii_digit());
        let from = if trim { start - 1 } else { start };
        let sep = match separator {
            '/' if p.auto_stack_diagonal => '#',
            other => other,
        };
        let stack = format!("\\S{numerator}{sep}{denominator};");
        let code = if p.stack_size >= 100 {
            stack
        } else {
            format!(
                "{{\\H{}x;{stack}}}",
                raw_span::number(f64::from(p.stack_size) / 100.0)
            )
        };
        let align = match p.stack_position {
            StackPosition::Top => Some("\\A2;"),
            StackPosition::Center => Some("\\A1;"),
            StackPosition::Bottom => None,
        };
        let width = numerator.chars().count() + 1 + denominator.chars().count();
        self.edit_raw(|_, index| {
            let count = index.validate(false)?;
            let raw_range = plain_raw_range(index, &(from..end))?;
            let mut patches = vec![(raw_range, code.clone())];
            let leading_align = index
                .tokens
                .iter()
                .any(|t| t.kind == Kind::Control('A'));
            if let (Some(align), false) = (align, leading_align) {
                patches.push((0..0, align.to_string()));
            }
            let raw = apply_patches(index.raw, patches)?;
            let removed = usize::from(trim);
            index.certify(&raw, count - removed)?;
            Ok(TextEdit {
                raw,
                caret: from + width + 1,
            })
        });
        // The stack it made gets the AutoStack tag.
        if let Some(ed) = self.mtext_editor.as_mut() {
            if matches!(doc_to_cells(&ed.doc).get(from), Some(Cell::Stack { .. })) {
                ed.smart_tag = Some(super::context_menu::SmartTag::Stack(from));
            }
        }
    }

    /// Write the Stack Properties window's stack over the one at `range`.
    pub(in crate::app) fn mtext_set_stack(
        &mut self,
        range: Range<usize>,
        upper: String,
        lower: String,
        style: StackStyle,
        position: StackPosition,
        percent: u8,
    ) {
        let kind = match style {
            StackStyle::Horizontal => StackingType::Horizontal,
            StackStyle::Diagonal => StackingType::Diagonal,
            StackStyle::Tolerance => StackingType::Limit,
            StackStyle::Decimal => StackingType::Decimal,
        };
        let data = StackingData {
            numerator: upper,
            denominator: lower,
            stacking_type: kind,
        };
        let align = match position {
            StackPosition::Top => Some("\\A2;"),
            StackPosition::Center => Some("\\A1;"),
            StackPosition::Bottom => None,
        };
        self.edit_raw(|_, index| {
            let edit = index.stack(range.clone(), &data)?;
            let width = edit.caret - range.start;
            // The stack's size: a `{\H…x;` group of its own, replaced, or one
            // put round it.
            let after = SourceIndex::new(&edit.raw);
            let count = after.validate(false)?;
            let unit = after
                .units
                .iter()
                .find(|u| u.kind == Kind::Stack && u.visible.start == range.start)
                .ok_or(EditError::ProjectionMismatch)?
                .raw
                .clone();
            let before_unit = &edit.raw[..unit.start];
            let own_group = before_unit
                .rfind('{')
                .filter(|open| {
                    let head = &before_unit[open + 1..];
                    head.starts_with("\\H")
                        && head.ends_with("x;")
                        && head.matches(';').count() == 1
                        && edit.raw[unit.end..].starts_with('}')
                });
            let stack_code = &edit.raw[unit.clone()];
            let sized = if percent >= 100 {
                stack_code.to_string()
            } else {
                format!(
                    "{{\\H{}x;{stack_code}}}",
                    raw_span::number(f64::from(percent) / 100.0)
                )
            };
            let mut patches = match own_group {
                Some(open) => vec![(open..unit.end + 1, sized)],
                None => vec![(unit.clone(), sized)],
            };
            // Where stacks sit: the text's leading `\A` code.
            for t in after.tokens.iter().filter(|t| t.kind == Kind::Control('A')) {
                patches.push((t.raw.clone(), String::new()));
            }
            if let Some(align) = align {
                patches.push((0..0, align.to_string()));
            }
            let raw = apply_patches(&edit.raw, patches)?;
            after.certify(&raw, count)?;
            Ok(TextEdit {
                raw,
                caret: range.start + width,
            })
        });
    }
}

/// The halves, style and text size of a stack, for the Stack Properties
/// window.
pub(in crate::app) fn stack_draft_values(
    data: &StackingData,
    props: &SpanProperties,
) -> (String, String, StackStyle, u8) {
    let style = match data.stacking_type {
        StackingType::Horizontal => StackStyle::Horizontal,
        StackingType::Diagonal => StackStyle::Diagonal,
        StackingType::Limit => StackStyle::Tolerance,
        StackingType::Decimal => StackStyle::Decimal,
    };
    let percent = match props.height {
        Some(MTextScalar::Factor(f)) => (f * 100.0).round().clamp(1.0, 100.0) as u8,
        _ => 100,
    };
    (data.numerator.clone(), data.denominator.clone(), style, percent)
}

/// Where the text's stacks sit, from its `\A` code.
pub(in crate::app) fn stack_position_of(raw: &str) -> StackPosition {
    let doc = editor_document(raw);
    let first = doc
        .paragraphs
        .iter()
        .flat_map(|p| p.spans.iter())
        .find_map(|s| s.properties.line_align);
    match first {
        Some(document::entities::mtext_format::MTextLineAlignment::Top) => StackPosition::Top,
        Some(document::entities::mtext_format::MTextLineAlignment::Middle) => StackPosition::Center,
        _ => StackPosition::Bottom,
    }
}
