//! The in-place editor's right-click menu, and the small tags AutoStack and
//! an automatic list leave under the text they changed. Every row runs a
//! `_MTED` tool, so the menu, the ribbon and a script do the same thing.
use super::*;
use crate::app::{Message, OpenCADStudio};
use iced::Task;

/// A tag under text the editor just changed by itself: the stack AutoStack
/// made, or the marker of the list item a typed marker made. The index is
/// the visible character the tag stands under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartTag {
    Stack(usize),
    List(usize),
}

/// The submenus of the right-click menu; one is open at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextGroup {
    Suggestions,
    PasteSpecial,
    Symbol,
    Alignment,
    Lists,
    Columns,
    Case,
    Charset,
    RemoveFormatting,
    Settings,
}

/// The right-click menu while it is open: where, the open submenu, and the
/// misspelled word under the pointer with its suggestions.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorContextMenu {
    pub at: iced::Point,
    pub open: Option<ContextGroup>,
    pub word: Option<(usize, usize)>,
    pub suggestions: Vec<String>,
}

impl MTextEditorState {
    /// The kind of the stack at visible character `at`.
    pub fn stack_kind_at(&self, at: usize) -> Option<document::entities::mtext_format::StackingType> {
        match doc_to_cells(&self.doc).get(at) {
            Some(Cell::Stack { data, .. }) => Some(data.stacking_type),
            _ => None,
        }
    }

    /// The list tag's Remove ran; its Redo is offered instead.
    pub fn list_tag_removed(&self) -> bool {
        self.list_tag_removed
    }
}

impl OpenCADStudio {
    /// Right-click on the text at visible offset `offset`: the caret goes
    /// there unless it is inside the selection, and the menu opens.
    pub(in crate::app) fn mtext_context_open(&mut self, at: iced::Point, offset: usize) {
        let language = self.spell_prefs.language.clone();
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let range = ed.selected_range();
        if !(range.start <= offset && offset <= range.end && !range.is_empty()) {
            ed.sel = Some((offset, offset));
            ed.sel_anchor = offset;
            ed.caret = offset;
        }
        let word = ed
            .spell_marks
            .iter()
            .copied()
            .find(|&(s, e)| s <= offset && offset <= e);
        let suggestions = word
            .map(|(s, e)| {
                let text: String = doc_to_cells(&ed.doc)[s..e]
                    .iter()
                    .filter_map(|c| match c {
                        Cell::Char(ch, _) => Some(*ch),
                        _ => None,
                    })
                    .collect();
                crate::app::commands::spelling_suggestions(&language, &text)
            })
            .unwrap_or_default();
        ed.smart_tag_open = false;
        ed.menu = None;
        ed.spell_word = word;
        ed.context_menu = Some(EditorContextMenu {
            at,
            open: None,
            word,
            suggestions,
        });
    }

    /// A row of the right-click menu or of a tag: both close, then the tool
    /// runs.
    pub(in crate::app) fn mtext_context_run(&mut self, command: &str) -> Task<Message> {
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.context_menu = None;
            ed.smart_tag_open = false;
        }
        self.dispatch_command(command)
    }

    /// Paste Special: the clipboard text pasted as copied, then its
    /// character (`NOCHAR`), paragraph (`NOPARA`) or all (`PLAIN`) formatting
    /// cleared again; the paragraphs it lands in lose theirs with it.
    pub(in crate::app) fn mtext_paste_as(&mut self, mode: &str, text: Option<String>) {
        let Some(text) = text.filter(|t| !t.is_empty()).map(|t| t.replace('\r', "")) else {
            return;
        };
        let start = match self.mtext_editor.as_ref() {
            Some(ed) => ed.selected_range().start,
            None => return,
        };
        match self.mtext_clipboard.clone() {
            Some((plain, fragment)) if plain.replace('\r', "") == text => {
                self.raw_paste_fragment(&fragment)
            }
            _ => self.mtext_type(&text),
        }
        let Some(end) = self.mtext_editor.as_ref().map(|ed| ed.caret) else {
            return;
        };
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.sel = Some((start, end));
            ed.sel_anchor = start;
        }
        self.mtext_clear(match mode {
            "NOCHAR" => ClearMode::Character,
            "NOPARA" => ClearMode::Paragraph,
            _ => ClearMode::All,
        });
        if let Some(ed) = self.mtext_editor.as_mut() {
            let end = ed.caret.max(ed.selected_range().end);
            ed.sel = Some((end, end));
            ed.sel_anchor = end;
            ed.caret = end;
        }
        self.rebuild_mtext_preview();
    }

    /// The misspelled word under the menu, replaced by `word`.
    pub(in crate::app) fn mtext_spell_replace(&mut self, word: &str) {
        let Some((s, e)) = self.mtext_editor.as_ref().and_then(|ed| ed.spell_word) else {
            return;
        };
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.sel = Some((s, e));
            ed.sel_anchor = s;
            ed.caret = e;
        }
        self.mtext_type(word);
    }

    /// The misspelled word under the menu: added to the custom dictionary in
    /// use (`add`), or not marked again while this text is edited.
    pub(in crate::app) fn mtext_spell_keep(&mut self, add: bool) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let Some((s, e)) = ed.spell_word else {
            return;
        };
        let word: String = doc_to_cells(&ed.doc)[s..e]
            .iter()
            .filter_map(|c| match c {
                Cell::Char(ch, _) => Some(*ch),
                _ => None,
            })
            .collect();
        if add {
            let source = self.spell_prefs.current_custom;
            let mut words = match source.and_then(|n| self.spell_prefs.custom_files.get(n)) {
                Some(path) => crate::app::commands::read_word_file(path),
                None => self.spell_prefs.custom.clone(),
            };
            if !words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                words.push(word);
                words.sort_by_key(|w| w.to_lowercase());
                self.store_dictionary_words(source, &words);
            }
        } else if let Some(ed) = self.mtext_editor.as_mut() {
            ed.spell_ignored.push(word);
            ed.spell_key.clear();
        }
        self.refresh_mtext_spelling();
    }

    /// The paragraph a list tag stands under.
    fn list_tag_paragraph(&self) -> Option<usize> {
        let ed = self.mtext_editor.as_ref()?;
        let Some(SmartTag::List(at)) = ed.smart_tag else {
            return None;
        };
        Some(
            doc_to_cells(&ed.doc)[..at.min(doc_to_cells(&ed.doc).len())]
                .iter()
                .filter(|c| matches!(c, Cell::Break(..)))
                .count(),
        )
    }

    /// The list tag: number the item from 1 or on from the list before,
    /// leave the typed marker as text (and put the list back), or stop
    /// making lists from typed markers.
    pub(in crate::app) fn mtext_list_tag(&mut self, pick: &str) {
        let Some(at) = self.mtext_editor.as_ref().and_then(|ed| match ed.smart_tag {
            Some(SmartTag::List(at)) => Some(at),
            _ => None,
        }) else {
            return;
        };
        match pick {
            "START" | "CONTINUE" => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.sel = Some((at, at));
                    ed.sel_anchor = at;
                    ed.caret = at;
                }
                self.mtext_list_restart(pick == "CONTINUE");
            }
            "REMOVE" => {
                if let Some(n) = self.list_tag_paragraph() {
                    self.mtext_list_plain(n);
                    if let Some(ed) = self.mtext_editor.as_mut() {
                        ed.list_tag_removed = true;
                    }
                }
            }
            "REDO" => {
                let _ = self.update(Message::MTextUndo);
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.list_tag_removed = false;
                    ed.smart_tag = Some(SmartTag::List(at));
                }
            }
            "STOP" => {
                super::prefs::update_prefs(|p| p.allow_auto_lists = false);
                self.save_config();
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.smart_tag = None;
                }
            }
            _ => {}
        }
    }
}
