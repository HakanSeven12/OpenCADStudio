//! Editor transactions over the authoritative raw source. Derived documents and
//! geometry never become a second independently editable document model.
use super::raw_adapter::{EditError, SourceIndex};
use super::raw_operations::TextEdit;
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
    /// Apply one text edit to the raw source. `edit` patches the source it is
    /// given; nothing else is rewritten. A refused edit leaves the text, the
    /// caret and the undo history as they were and says why.
    fn edit_raw(
        &mut self,
        edit: impl FnOnce(&MTextEditorState, &SourceIndex) -> Result<TextEdit, EditError>,
    ) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let raw = ed.content.text();
        let index = SourceIndex::new(&raw);
        let result = index.validate(false).and_then(|count| {
            // The caret counts the cells the preview shows; both must agree.
            if doc_to_cells(&ed.doc).len() != count {
                return Err(EditError::ProjectionMismatch);
            }
            edit(ed, &index)
        });
        match result {
            Ok(edit) => {
                let ed = self.mtext_editor.as_mut().unwrap();
                if edit.raw != raw {
                    ed.record_undo();
                    ed.content = text_editor::Content::with_text(&edit.raw);
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
            index.replace(range, &text)
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
}
