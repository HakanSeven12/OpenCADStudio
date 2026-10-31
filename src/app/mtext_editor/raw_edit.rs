//! Editor transactions over the authoritative raw source. Derived documents and
//! geometry never become a second independently editable document model.
use super::raw_adapter::{EditError, SourceIndex};
use super::raw_operations::{apply_patches, TextEdit};
use super::*;

/// The codec drops a final empty paragraph. Force its derived parse to flush
/// with a disposable sentinel, then remove the sentinel from the VIEW only.
pub(super) fn editor_document(raw: &str) -> MTextDocument {
    let mut doc = parse_mtext(raw, true);
    let index = SourceIndex::new(raw);
    if index.visible_len.is_some() {
        let expected = 1 + index
            .tokens
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    super::raw_adapter::Kind::Break | super::raw_adapter::Kind::Column
                )
            })
            .count();
        if doc.paragraphs.len() < expected {
            doc = parse_mtext(&format!("{raw}{{\u{e000}}}"), true);
            if let Some(span) = doc.paragraphs.last_mut().and_then(|p| p.spans.last_mut()) {
                debug_assert!(span.text.ends_with('\u{e000}'));
                span.text.pop();
            }
        }
    }
    doc
}

impl MTextEditorState {
    /// The selection as a visible range, or the empty range at the caret.
    pub(super) fn selected_range(&self) -> std::ops::Range<usize> {
        match self.sel {
            Some((a, b)) if a != b => a.min(b)..a.max(b),
            _ => self.caret..self.caret,
        }
    }

    /// `position`, moved out of a fraction to its near edge in `direction`
    /// (a fraction is one atom: a key steps over it or deletes it whole).
    fn safe_caret(index: &SourceIndex, position: usize, direction: i8) -> usize {
        index
            .units
            .iter()
            .find(|u| {
                u.kind == super::raw_adapter::Kind::Stack
                    && u.visible.start < position
                    && position < u.visible.end
            })
            .map_or(position, |u| {
                if direction < 0 {
                    u.visible.start
                } else {
                    u.visible.end
                }
            })
    }
}

impl crate::app::OpenCADStudio {
    /// The edited MTEXT hosts native FIELDs the editor could not locate.
    pub(super) fn unlocated_field_host(&self) -> bool {
        let doc = &self.tabs[self.active_tab].scene.document;
        self.mtext_editor.as_ref().is_some_and(|ed| {
            ed.field_host.is_none()
                && ed
                    .editing
                    .and_then(|h| doc.get_entity(h))
                    .is_some_and(|entity| crate::entities::field::hosts_field(doc, entity))
        })
    }

    /// Apply one text edit to the raw source. `edit` patches the source it is
    /// given; nothing else is rewritten. A refused edit leaves the text, the
    /// caret and the undo history as they were and says why.
    pub(super) fn edit_raw(
        &mut self,
        edit: impl FnOnce(&MTextEditorState, &SourceIndex) -> Result<TextEdit, EditError>,
    ) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let raw = ed.content.text();
        let index = SourceIndex::new(&raw);
        // A FIELD host whose fields could not be located stays read-only:
        // its template must not drift from its text.
        let result = index
            .validate(self.unlocated_field_host())
            .and_then(|count| {
                // The caret counts the cells the preview shows; both must agree.
                if doc_to_cells(&ed.doc).len() != count {
                    return Err(EditError::ProjectionMismatch);
                }
                let edit = edit(ed, &index)?;
                let caret = SourceIndex::new(&edit.raw).raw_positions(edit.caret);
                let fields = ed.track_fields(&raw, &edit.raw, Some(&caret))?;
                Ok((edit, fields))
            });
        match result {
            Ok((edit, fields)) => {
                let ed = self.mtext_editor.as_mut().unwrap();
                if edit.raw != raw {
                    ed.record_undo();
                    ed.content = text_editor::Content::with_text(&edit.raw);
                    if fields.is_some() {
                        ed.field_host = fields;
                    }
                }
                ed.caret = edit.caret;
                ed.sel = Some((edit.caret, edit.caret));
                ed.sel_anchor = edit.caret;
                ed.caret_blink_on = true;
            }
            Err(error) => {
                self.command_line
                    .push_error(crate::t!(error.to_string()).as_ref());
            }
        }
        self.rebuild_mtext_preview();
    }

    /// Insert `text` at the caret, replacing the selection. A line break goes
    /// in as `\P`. Typing `d`, `p` or `c` right after `%%` makes the symbol.
    pub(in crate::app) fn raw_type(&mut self, text: &str, typing: bool) {
        let face = self.format_face();
        self.edit_raw(|ed, index| {
            let mut range = ed.selected_range();
            let mut text = text.to_string();
            if typing && range.is_empty() && range.start >= 2 && text.chars().count() == 1 {
                let cells = doc_to_cells(&ed.doc);
                if matches!(
                    cells.get(range.start - 2..range.start),
                    Some([Cell::Char('%', _), Cell::Char('%', _)])
                ) {
                    let symbol = match text.as_str() {
                        "d" | "D" => Some("°"),
                        "p" | "P" => Some("±"),
                        "c" | "C" => Some("Ø"),
                        _ => None,
                    };
                    if let Some(symbol) = symbol {
                        range.start -= 2;
                        text = symbol.into();
                    }
                }
            }
            let start = range.start;
            let mut edit = index.replace(range, &text)?;
            // B, I, U, overline or S switched with nothing selected format
            // what is typed next, as the toolbar shows.
            for &(kind, on) in &ed.typing_format.0 {
                edit.raw = format_range(&edit.raw, start..edit.caret, kind, on, &face)?;
            }
            Ok(edit)
        });
    }

    /// Paste MTEXT copied in this editor: `fragment` keeps the characters'
    /// formatting (superscript, fractions, fonts, colours). It replaces the
    /// selection inside a group of its own, so nothing around it changes;
    /// the characters must come out as the fragment holds them, or the paste
    /// is refused.
    pub(in crate::app) fn raw_paste_fragment(&mut self, fragment: &str) {
        // The visible slots of a cell run: characters, breaks, fraction glyphs.
        let glyphs = |cells: &[Cell]| -> String {
            cells
                .iter()
                .map(|c| match c {
                    Cell::Char(ch, _) => *ch,
                    Cell::Break(..) => '\n',
                    Cell::Stack { .. } => '\u{fffc}',
                })
                .collect()
        };
        self.edit_raw(|ed, index| {
            let fragment_index = SourceIndex::new(fragment);
            let added = fragment_index.validate(false)?;
            let cleared = index.replace(ed.selected_range(), "")?;
            let base = SourceIndex::new(&cleared.raw);
            let count = base.validate(false)?;
            let at = base.insertion_byte(cleared.caret)?;
            let raw = apply_patches(&cleared.raw, vec![(at..at, format!("{{{fragment}}}"))])?;
            base.certify(&raw, count + added)?;
            SourceIndex::new(&raw).validate(false)?;
            let before = doc_to_cells(&editor_document(&cleared.raw));
            let after = doc_to_cells(&editor_document(&raw));
            let caret = cleared.caret + added;
            let inserted = after
                .get(cleared.caret..caret)
                .ok_or(EditError::ProjectionMismatch)?;
            let kept = after.get(..cleared.caret) == before.get(..cleared.caret)
                && after.get(caret..) == before.get(cleared.caret..);
            // The text around stays; the pasted characters are the fragment's.
            if !kept || glyphs(inserted) != glyphs(&doc_to_cells(&editor_document(fragment))) {
                return Err(EditError::ProjectionMismatch);
            }
            Ok(TextEdit { raw, caret })
        });
    }

    /// Delete the selection, or one visible character (a fraction whole)
    /// before (`backspace`) or after the caret.
    pub(in crate::app) fn raw_delete(&mut self, backspace: bool) {
        self.edit_raw(|ed, index| {
            let mut range = ed.selected_range();
            if range.is_empty() {
                let count = index.visible_len.unwrap_or(0);
                if backspace {
                    range.start =
                        MTextEditorState::safe_caret(index, range.start.saturating_sub(1), -1);
                } else {
                    range.end = MTextEditorState::safe_caret(index, (range.end + 1).min(count), 1);
                }
            }
            index.replace(range, "")
        });
    }
}

/// A font control for `font`; refused when a field would not survive the
/// control's own syntax. A stroke font file is written as `\F…;`.
pub(super) fn font_code(font: &MTextFont) -> Result<String, EditError> {
    if font.name.trim().is_empty()
        || font.name.contains(['|', ','])
        || std::iter::once(&font.name)
            .chain(std::iter::once(&font.style))
            .chain(font.extra.iter())
            .any(|v| {
                v.contains(['\\', '{', '}', ';'])
                    || v.contains("%<")
                    || v.chars().any(char::is_control)
            })
    {
        return Err(EditError::UnsupportedInput);
    }
    if crate::scene::text::lff::is_shx_name(&font.name) {
        return Ok(format!("\\F{};", font.name));
    }
    let mut code = format!("\\f{}", font.name);
    if font.bold_explicit {
        code.push_str(if font.bold { "|b1" } else { "|b0" });
    }
    if font.italic_explicit {
        code.push_str(if font.italic { "|i1" } else { "|i0" });
    }
    if let Some(v) = font.charset {
        code.push_str(&format!("|c{v}"));
    }
    if let Some(v) = font.pitch {
        code.push_str(&format!("|p{v}"));
    }
    if !font.style.is_empty() {
        code.push('|');
        code.push_str(&font.style);
    }
    for flag in &font.extra {
        code.push('|');
        code.push_str(flag);
    }
    code.push(';');
    Ok(code)
}

/// Whether a character already has `kind`, by the editor's rules: italic
/// is the 15° oblique (or an imported italic face), bold the face's flag.
fn format_active(kind: MTextFmt, p: &SpanProperties) -> bool {
    match kind {
        MTextFmt::Underline => p.underline(),
        MTextFmt::Overline => p.overline(),
        MTextFmt::Strike => p.strikethrough(),
        MTextFmt::Italic => {
            p.oblique_angle == Some(15.0) || p.font.as_ref().is_some_and(|f| f.italic)
        }
        MTextFmt::Bold => p.font.as_ref().is_some_and(|f| f.bold),
        MTextFmt::Uppercase | MTextFmt::Lowercase => false,
    }
}

/// The codes that switch `kind` to `on` for a character with `p`; empty
/// when it already is. `face` names the font a bold run without one gets.
fn format_prefix(
    kind: MTextFmt,
    on: bool,
    p: &SpanProperties,
    face: &str,
) -> Result<String, EditError> {
    if format_active(kind, p) == on {
        return Ok(String::new());
    }
    let face_of = |p: &SpanProperties| {
        p.font
            .clone()
            .filter(|f| !f.name.trim().is_empty())
            .unwrap_or_else(|| MTextFont::from_name(face))
    };
    Ok(match kind {
        MTextFmt::Underline => if on { "\\L" } else { "\\l" }.into(),
        MTextFmt::Overline => if on { "\\O" } else { "\\o" }.into(),
        MTextFmt::Strike => if on { "\\K" } else { "\\k" }.into(),
        MTextFmt::Italic => {
            // The oblique carries italic; an imported italic face is cleared
            // so the one form stays the oblique.
            let mut code = String::new();
            if p.font.as_ref().is_some_and(|f| f.italic) {
                let mut font = face_of(p);
                font.italic = false;
                font.italic_explicit = true;
                code.push_str(&font_code(&font)?);
            }
            let oblique = p.oblique_angle.unwrap_or(0.0);
            if on && oblique != 15.0 {
                code.push_str("\\Q15;");
            } else if !on && oblique != 0.0 {
                code.push_str("\\Q0;");
            }
            code
        }
        MTextFmt::Bold => {
            let mut font = face_of(p);
            font.bold = on;
            font.bold_explicit = true;
            font_code(&font)?
        }
        MTextFmt::Uppercase | MTextFmt::Lowercase => return Err(EditError::UnsupportedInput),
    })
}

/// `kind` switched to `on` over the visible `range` of `raw`. Only those
/// characters change, and only in that one property; anything else refuses.
fn format_range(
    raw: &str,
    range: std::ops::Range<usize>,
    kind: MTextFmt,
    on: bool,
    face: &str,
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let cells = doc_to_cells(&editor_document(raw));
    let selected = cells.get(range.clone()).ok_or(EditError::InvalidRange)?;
    if selected.iter().any(|c| matches!(c, Cell::Stack { .. })) {
        return Err(EditError::AtomicStructure);
    }
    let raw = index.format(range.clone(), |n| match cells.get(n) {
        Some(Cell::Char(_, p)) => format_prefix(kind, on, p, face),
        _ => Err(EditError::ProjectionMismatch),
    })?;
    let after = doc_to_cells(&editor_document(&raw));
    let certified = after.len() == cells.len()
        && cells
            .iter()
            .zip(&after)
            .enumerate()
            .all(|(n, pair)| match pair {
                (Cell::Char(a, p), Cell::Char(b, q)) if range.contains(&n) => {
                    a == b && format_active(kind, q) == on && only_format_changed(kind, p, q)
                }
                (old, new) => old == new,
            });
    if !certified {
        return Err(EditError::ProjectionMismatch);
    }
    Ok(raw)
}

/// B, I, U, overline or S switched with nothing selected: the format the
/// next typed text takes, in the order they were switched.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct TypingFormat(Vec<(MTextFmt, bool)>);

impl TypingFormat {
    fn value(&self, kind: MTextFmt) -> Option<bool> {
        self.0.iter().find(|(k, _)| *k == kind).map(|(_, on)| *on)
    }

    fn set(&mut self, kind: MTextFmt, on: bool) {
        self.0.retain(|(k, _)| *k != kind);
        self.0.push((kind, on));
    }
}

/// What a format button shows for the selection, or for the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum FormatState {
    Off,
    On,
    /// Part of the selection has it.
    Mixed,
    /// The text or the selection cannot be formatted (a protected code, a
    /// fraction in the selection).
    Unavailable,
}

impl MTextEditorState {
    /// The properties a character typed at `caret` takes.
    fn insertion_properties(
        index: &SourceIndex,
        caret: usize,
    ) -> Result<SpanProperties, EditError> {
        let probe = index.replace(caret..caret, "\u{e000}")?;
        match doc_to_cells(&editor_document(&probe.raw)).get(caret) {
            Some(Cell::Char(_, props)) => Ok(props.clone()),
            _ => Err(EditError::ProjectionMismatch),
        }
    }

    /// Whether `kind` is on: over the selection, or at the caret including
    /// what was switched for the next typed text.
    pub(in crate::app) fn format_state(&self, kind: MTextFmt) -> FormatState {
        if matches!(kind, MTextFmt::Uppercase | MTextFmt::Lowercase) {
            return FormatState::Off;
        }
        let raw = self.content.text();
        let index = SourceIndex::new(&raw);
        let state = (|| -> Result<FormatState, EditError> {
            index.validate(false)?;
            let range = self.selected_range();
            index.validate_range(&range)?;
            if range.is_empty() {
                let active = match self.typing_format.value(kind) {
                    Some(on) => on,
                    None => format_active(kind, &Self::insertion_properties(&index, range.start)?),
                };
                return Ok(if active {
                    FormatState::On
                } else {
                    FormatState::Off
                });
            }
            let mut value = None;
            let mut mixed = false;
            for cell in doc_to_cells(&self.doc)
                .get(range)
                .ok_or(EditError::InvalidRange)?
            {
                let active = match cell {
                    Cell::Char(_, props) => format_active(kind, props),
                    Cell::Break(..) => continue,
                    Cell::Stack { .. } => return Err(EditError::AtomicStructure),
                };
                mixed |= value.is_some_and(|previous| previous != active);
                value = Some(active);
            }
            Ok(if mixed {
                FormatState::Mixed
            } else if value == Some(true) {
                FormatState::On
            } else {
                FormatState::Off
            })
        })();
        state.unwrap_or(FormatState::Unavailable)
    }
}

/// Toggling `kind` changed nothing else about the character: the same
/// face name, the other flags and every other property.
fn only_format_changed(kind: MTextFmt, before: &SpanProperties, after: &SpanProperties) -> bool {
    let mut normalized = after.clone();
    match kind {
        MTextFmt::Underline => normalized.set_underline(before.underline()),
        MTextFmt::Overline => normalized.set_overline(before.overline()),
        MTextFmt::Strike => normalized.set_strikethrough(before.strikethrough()),
        MTextFmt::Italic => {
            normalized.oblique_angle = before.oblique_angle;
            match (normalized.font.as_mut(), before.font.as_ref()) {
                (Some(new), Some(old)) => {
                    if new.name != old.name || new.bold != old.bold {
                        return false;
                    }
                    *new = old.clone();
                }
                (None, None) => {}
                _ => return false,
            }
        }
        MTextFmt::Bold => {
            let Some(new) = normalized.font.as_ref() else {
                return false;
            };
            let same_face = before.font.as_ref().map_or(true, |old| {
                old.name.trim().is_empty() || (old.name == new.name && old.italic == new.italic)
            });
            if !same_face {
                return false;
            }
            normalized.font = before.font.clone();
        }
        MTextFmt::Uppercase | MTextFmt::Lowercase => return false,
    }
    normalized == *before
}

impl crate::app::OpenCADStudio {
    /// B, I, U, overline or S on the selection: on everywhere unless every
    /// character already has it, then off. Only the selection changes, and
    /// only in that one property; anything else refuses the toggle.
    pub(in crate::app) fn raw_format(&mut self, kind: MTextFmt, face: &str) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let range = ed.selected_range();
        if range.is_empty() {
            // Nothing selected: the next typed text takes it, as the button
            // then shows; the text itself does not change.
            match ed.format_state(kind) {
                FormatState::Unavailable => {}
                state => {
                    ed.record_undo();
                    ed.typing_format.set(kind, state != FormatState::On);
                }
            }
            return;
        }
        self.edit_raw(|ed, index| {
            let cells = doc_to_cells(&ed.doc);
            let selected = cells.get(range.clone()).ok_or(EditError::InvalidRange)?;
            let mut all = true;
            for cell in selected {
                match cell {
                    Cell::Char(_, p) => all &= format_active(kind, p),
                    Cell::Stack { .. } => return Err(EditError::AtomicStructure),
                    Cell::Break(..) => {}
                }
            }
            Ok(TextEdit {
                raw: format_range(index.raw, range.clone(), kind, !all, face)?,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }

    /// Upper- or lower-case the selected letters in place: each changed
    /// character's own bytes are replaced; codes and formatting stay.
    pub(in crate::app) fn raw_change_case(&mut self, upper: bool) {
        let Some(range) = self
            .mtext_editor
            .as_ref()
            .map(MTextEditorState::selected_range)
            .filter(|r| !r.is_empty())
        else {
            return;
        };
        self.edit_raw(|_, index| {
            let mut patches = Vec::new();
            for unit in &index.units {
                if unit.visible.start < range.start
                    || unit.visible.end > range.end
                    || unit.kind != super::raw_adapter::Kind::Text
                {
                    continue;
                }
                let text = &index.raw[unit.raw.clone()];
                let mapped: String = if upper {
                    text.to_uppercase()
                } else {
                    text.to_lowercase()
                };
                // A letter whose case form is longer (ß → SS) is left alone.
                if mapped != text && mapped.chars().count() == text.chars().count() {
                    patches.push((unit.raw.clone(), mapped));
                }
            }
            let raw = apply_patches(index.raw, patches)?;
            index.certify(&raw, index.visible_len.unwrap_or(0))?;
            Ok(TextEdit {
                raw,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }

    /// Clear the character formatting of the selection, or of the whole
    /// text when nothing is selected, removing or splitting only the codes
    /// that format it. Refused when that cannot be done exactly.
    pub(in crate::app) fn raw_clear_formatting(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = Some(ed.selected_range())
            .filter(|r| !r.is_empty())
            .unwrap_or(0..doc_to_cells(&ed.doc).len());
        if range.is_empty() {
            return;
        }
        self.edit_raw(|_, index| {
            // The object font and width stay: they are not the selection's.
            let keep = super::raw_style::object_controls(index.raw);
            Ok(TextEdit {
                raw: index.cleared_selection(range.clone(), &keep)?,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }

    /// A formatting command keeps its selection, as before.
    pub(super) fn keep_selection(&mut self, range: std::ops::Range<usize>) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.sel = Some((range.start, range.end));
            ed.sel_anchor = range.start;
            ed.caret = range.end;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use glam::DVec3;

    fn app(raw: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(1));
        app.active_tab = 1;
        app.open_mtext_editor(DVec3::ZERO, None, raw, 2.5, None);
        app
    }
    fn raw(app: &OpenCADStudio) -> String {
        app.mtext_editor.as_ref().unwrap().content.text()
    }
    fn caret(app: &mut OpenCADStudio, at: usize) {
        let ed = app.mtext_editor.as_mut().unwrap();
        ed.caret = at;
        ed.sel = Some((at, at));
        ed.sel_anchor = at;
    }
    fn select(app: &mut OpenCADStudio, a: usize, b: usize) {
        let ed = app.mtext_editor.as_mut().unwrap();
        ed.sel = Some((a, b));
        ed.sel_anchor = a;
        ed.caret = b;
    }

    #[test]
    fn typing_patches_only_the_typed_place_and_keeps_every_other_code() {
        // Tab stops, a nested group and an extended `\p` stay byte for byte.
        let mut app = app(r"\pxqc,t4;é{\Labc\l}");
        caret(&mut app, 2);
        app.mtext_type("ő😀");
        assert_eq!(raw(&app), r"\pxqc,t4;é{\Laő😀bc\l}");
        assert_eq!(app.mtext_editor.as_ref().unwrap().caret, 4);
        // The value saved is the edited source itself.
        assert_eq!(
            app.mtext_editor.as_ref().unwrap().build_mtext().value,
            raw(&app)
        );
    }

    #[test]
    fn backspace_and_delete_merge_paragraphs_and_remove_ranges() {
        let mut app = app(r"ab\Pcd");
        caret(&mut app, 3);
        app.mtext_backspace();
        assert_eq!(raw(&app), "abcd");
        let mut app = self::app(r"abc\Pdef");
        select(&mut app, 1, 5);
        app.mtext_delete();
        assert_eq!(raw(&app), "aef");
        assert_eq!(app.mtext_editor.as_ref().unwrap().caret, 1);
    }

    #[test]
    fn a_fraction_is_deleted_whole_from_either_side() {
        let mut app = app(r"a\S1/2;b");
        caret(&mut app, 4);
        app.mtext_backspace();
        assert_eq!(raw(&app), "ab");
        let mut app = self::app(r"a\S1/2;b");
        caret(&mut app, 1);
        app.mtext_delete();
        assert_eq!(raw(&app), "ab");
    }

    #[test]
    fn percent_codes_typed_one_by_one_become_symbols() {
        let mut app = app("x");
        caret(&mut app, 1);
        for ch in ["%", "%", "c"] {
            app.mtext_type(ch);
        }
        assert_eq!(app.mtext_editor.as_ref().unwrap().doc.to_plain_text(), "xØ");
    }

    #[test]
    fn enter_and_a_multi_line_paste_write_paragraph_breaks() {
        let mut app = app("a");
        caret(&mut app, 1);
        app.mtext_type("\n");
        assert_eq!(raw(&app), r"a\P");
        assert_eq!(app.mtext_editor.as_ref().unwrap().caret, 2);
        app.mtext_type("x\r\ny");
        assert_eq!(raw(&app), r"a\Px\Py");
        // Clipboard text with MTEXT-like codes stays literal.
        app.mtext_type(r"\C1;{}");
        assert_eq!(
            app.mtext_editor.as_ref().unwrap().doc.to_plain_text(),
            "a\nx\ny\\C1;{}"
        );
    }

    #[test]
    fn a_refused_edit_changes_nothing() {
        // An unknown control: the source cannot be edited safely.
        let mut app = app(r"é\Zpayload");
        let before = raw(&app);
        caret(&mut app, 1);
        app.mtext_type("x");
        app.mtext_backspace();
        assert_eq!(raw(&app), before);
        // Nothing was recorded either: undo has nothing to undo.
        app.mtext_undo();
        assert_eq!(raw(&app), before);
    }

    fn fmt(app: &mut OpenCADStudio, kind: crate::app::mtext_editor::MTextFmt) {
        app.mtext_apply_fmt(kind);
    }
    fn cells(app: &OpenCADStudio) -> Vec<super::Cell> {
        super::doc_to_cells(&app.mtext_editor.as_ref().unwrap().doc)
    }
    fn props(app: &OpenCADStudio, n: usize) -> codec::entities::mtext_format::SpanProperties {
        match &cells(app)[n] {
            super::Cell::Char(_, p) => p.clone(),
            _ => panic!("char"),
        }
    }

    #[test]
    fn underline_overline_and_strike_toggle_only_the_selection() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"\pxqc,t4;abc");
        select(&mut app, 0, 2);
        fmt(&mut app, Underline);
        assert_eq!(raw(&app), r"\pxqc,t4;{\Lab}c");
        assert!(props(&app, 1).underline() && !props(&app, 2).underline());
        assert_eq!(app.mtext_editor.as_ref().unwrap().sel, Some((0, 2)));
        fmt(&mut app, Underline);
        assert!(!props(&app, 0).underline() && !props(&app, 1).underline());
        fmt(&mut app, Overline);
        assert!(props(&app, 0).overline());
        fmt(&mut app, Strike);
        assert!(props(&app, 1).strikethrough() && !props(&app, 2).strikethrough());
        assert!(raw(&app).starts_with(r"\pxqc,t4;"), "{}", raw(&app));
    }

    #[test]
    fn a_mixed_selection_is_switched_on_throughout() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"{\Lab}cd");
        select(&mut app, 1, 3);
        fmt(&mut app, Underline);
        assert!((0..4)
            .map(|n| props(&app, n).underline())
            .eq([true, true, true, false]));
    }

    #[test]
    fn italic_is_the_15_degree_oblique_and_bold_names_the_face() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app("abc");
        select(&mut app, 0, 2);
        fmt(&mut app, Italic);
        assert_eq!(props(&app, 0).oblique_angle, Some(15.0));
        assert_eq!(props(&app, 2).oblique_angle, None);
        fmt(&mut app, Italic);
        assert!(props(&app, 0).oblique_angle.unwrap_or(0.0) == 0.0);
        fmt(&mut app, Bold);
        let font = props(&app, 1).font.unwrap();
        assert!(font.bold && !font.name.is_empty(), "{font:?}");
        assert!(props(&app, 2).font.is_none());
    }

    #[test]
    fn repeated_toggling_never_nests_deeper() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app("abcd");
        select(&mut app, 1, 3);
        for _ in 0..10 {
            fmt(&mut app, Bold);
            fmt(&mut app, Underline);
        }
        let depth = super::super::raw_adapter::SourceIndex::new(&raw(&app)).max_group_depth();
        assert!(depth <= 1, "{}", raw(&app));
        assert_eq!(
            app.mtext_editor.as_ref().unwrap().doc.to_plain_text(),
            "abcd"
        );
    }

    #[test]
    fn a_selection_over_a_fraction_is_not_formatted() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"a\S1/2;b");
        let before = raw(&app);
        select(&mut app, 0, 5);
        fmt(&mut app, Underline);
        assert_eq!(raw(&app), before);
    }

    #[test]
    fn changing_case_replaces_only_the_letters() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"{\Labc}Dé\Pß");
        select(&mut app, 0, 7);
        fmt(&mut app, Uppercase);
        assert_eq!(raw(&app), r"{\LABC}DÉ\Pß");
        fmt(&mut app, Lowercase);
        assert_eq!(raw(&app), r"{\Labc}dé\Pß");
        assert!(props(&app, 0).underline());
    }

    #[test]
    fn clear_formatting_patches_the_selection_or_the_whole_text() {
        let source = r"{\Léa}b\pxqc,t4;\P\S1/2;\C3;xyz";
        let mut app = app(source);
        select(&mut app, 0, 2);
        app.mtext_clear_formatting();
        let first = r"{éa}b\pxqc,t4;\P\S1/2;\C3;xyz";
        assert_eq!(raw(&app), first);
        assert_eq!(app.mtext_editor.as_ref().unwrap().sel, Some((0, 2)));
        select(&mut app, 8, 9);
        app.mtext_clear_formatting();
        let second = r"{éa}b\pxqc,t4;\P\S1/2;{\C3;x}y\C3;z";
        assert_eq!(raw(&app), second);
        app.mtext_undo();
        assert_eq!(raw(&app), first);
        app.mtext_redo();
        assert_eq!(raw(&app), second);
        // Nothing selected: the whole text, with the paragraph settings and
        // the fraction kept.
        caret(&mut app, 0);
        app.mtext_clear_formatting();
        // Only the colour codes go; an empty group is left as harmless.
        assert_eq!(raw(&app), r"{éa}b\pxqc,t4;\P\S1/2;{x}yz");
        let cells = cells(&app);
        assert!(cells.iter().all(|c| match c {
            super::Cell::Char(_, p) => p.color.is_none() && !p.underline(),
            _ => true,
        }));
    }

    #[test]
    fn a_cleared_text_saves_and_reads_back_byte_for_byte() {
        let mut app = app(r"{\Léa}b\C3;xyz");
        select(&mut app, 3, 4);
        app.mtext_clear_formatting();
        let value = raw(&app);
        assert_eq!(value, r"{\Léa}bx\C3;yz");
        assert!(app.mtext_commit());
        let (h, saved) = app.tabs[1]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                codec::EntityType::MText(t) => Some((t.common.handle, t.value.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(saved, value);
        for ext in ["dwg", "dxf"] {
            let bytes = crate::io::save_to_bytes(
                &app.tabs[1].scene.document,
                ext,
                codec::DxfVersion::AC1032,
            )
            .unwrap();
            let doc = crate::io::load_bytes(&format!("clear.{ext}"), bytes).unwrap();
            let Some(codec::EntityType::MText(t)) = doc.get_entity(h) else {
                panic!()
            };
            assert_eq!(t.value, value, "{ext}");
        }
    }

    fn state(app: &OpenCADStudio, kind: crate::app::mtext_editor::MTextFmt) -> super::FormatState {
        app.mtext_editor.as_ref().unwrap().format_state(kind)
    }

    #[test]
    fn a_format_switched_with_nothing_selected_takes_the_next_typed_text() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"\pxqc,t4;ab");
        caret(&mut app, 2);
        fmt(&mut app, Underline);
        fmt(&mut app, Italic);
        // Nothing is written until something is typed; the buttons say so.
        assert_eq!(raw(&app), r"\pxqc,t4;ab");
        assert_eq!(state(&app, Underline), super::FormatState::On);
        assert_eq!(state(&app, Italic), super::FormatState::On);
        for ch in ["c", "d", "e"] {
            app.mtext_type(ch);
        }
        // One group for the typed run, not one per character.
        assert_eq!(raw(&app), r"\pxqc,t4;ab{\L\Q15;cde}");
        assert!(!props(&app, 1).underline());
        assert!(props(&app, 2).underline() && props(&app, 4).underline());
        assert_eq!(props(&app, 4).oblique_angle, Some(15.0));
    }

    #[test]
    fn a_caret_move_drops_the_pending_format_and_undo_restores_it() {
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app("ab");
        caret(&mut app, 2);
        fmt(&mut app, Strike);
        app.mtext_caret_move(-1, false);
        assert_eq!(state(&app, Strike), super::FormatState::Off);
        app.mtext_type("x");
        assert_eq!(raw(&app), "axb");
        app.mtext_undo();
        assert_eq!(raw(&app), "ab");
        // Undo of the switch itself puts the button back.
        let mut app = self::app("ab");
        caret(&mut app, 2);
        fmt(&mut app, Strike);
        app.mtext_undo();
        assert_eq!(state(&app, Strike), super::FormatState::Off);
        app.mtext_redo();
        assert_eq!(state(&app, Strike), super::FormatState::On);
    }

    #[test]
    fn the_buttons_show_on_off_mixed_and_unavailable() {
        use super::FormatState::*;
        use crate::app::mtext_editor::MTextFmt::*;
        let mut app = app(r"{\Lab}c\S1/2;");
        select(&mut app, 0, 2);
        assert_eq!(state(&app, Underline), On);
        select(&mut app, 1, 3);
        assert_eq!(state(&app, Underline), Mixed);
        select(&mut app, 2, 3);
        assert_eq!(state(&app, Underline), Off);
        // A fraction in the selection cannot be formatted.
        select(&mut app, 2, 6);
        assert_eq!(state(&app, Bold), Unavailable);
        // The caret takes the format of the text it continues.
        caret(&mut app, 1);
        assert_eq!(state(&app, Underline), On);
        // A protected code: nothing can be formatted, and a press changes nothing.
        let mut app = self::app(r"a\Zopaque");
        caret(&mut app, 1);
        assert_eq!(state(&app, Bold), Unavailable);
        fmt(&mut app, Bold);
        assert_eq!(state(&app, Bold), Unavailable);
        app.mtext_undo();
        assert_eq!(raw(&app), r"a\Zopaque");
    }

    #[test]
    fn copy_and_paste_in_the_editor_keep_superscript_fractions_and_formatting() {
        use super::Cell;
        let mut app = app(r"x\S2^;{\C1;\Lab}\S1/2;");
        // Copy the superscript, the formatted letters and the fraction.
        select(&mut app, 1, 7);
        let plain = app.mtext_selected_text().unwrap();
        let end = cells(&app).len();
        caret(&mut app, end);
        let _ = app.update(crate::app::Message::MTextPasteClip(Some(plain)));
        let after = cells(&app);
        assert_eq!(after.len(), 2 * end - 1);
        // The pasted copy has the same stacks and character formatting.
        for n in 1..7 {
            match (&after[n], &after[n + 6]) {
                (Cell::Char(a, p), Cell::Char(b, q)) => assert!(a == b && p == q, "{n}"),
                (Cell::Stack { data: a, .. }, Cell::Stack { data: b, .. }) => {
                    assert_eq!(a, b, "{n}")
                }
                other => panic!("{n}: {other:?}"),
            }
        }
        // The text before the paste is byte for byte as it was.
        assert!(raw(&app).starts_with(r"x\S2^;{\C1;\Lab}\S1/2;"));
        // One undo step takes the paste back.
        app.mtext_undo();
        assert_eq!(raw(&app), r"x\S2^;{\C1;\Lab}\S1/2;");
        // Text from elsewhere is pasted as plain text, as before.
        let _ = app.update(crate::app::Message::MTextPasteClip(Some("2/3".into())));
        assert!(raw(&app).ends_with("2/3"));
    }
}
