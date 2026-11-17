//! The Text Editor windows of `ui::window::mtext_tool_dialogs`: what each
//! starts from, what its controls change and what OK writes; Import Text;
//! the style question; and the misspelled words the editor underlines.

use super::prefs::{prefs, update_prefs};
use super::*;
use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::mtext_dialogs::{MTextDialog, MTextDialogMsg as M};
use crate::ui::window::mtext_tool_dialogs::{
    AutoStackDraft, CharsetDraft, DictionariesDraft, HighlightDraft, SpellSettingsDraft,
    StackDraft, StyleChangeDraft,
};
use iced::Task;

impl OpenCADStudio {
    /// The window `which` of this file, or None for another one.
    pub(super) fn mtext_tool_dialog(&self, which: &str) -> Option<MTextDialog> {
        let ed = self.mtext_editor.as_ref()?;
        Some(match which {
            "CHARSET" => MTextDialog::Charset(CharsetDraft {
                charset: self.mtext_selection_charset(),
            }),
            "DICTIONARIES" => MTextDialog::Dictionaries(self.dictionaries_draft()),
            "SPELLSETTINGS" => MTextDialog::SpellSettings(SpellSettingsDraft {
                options: self.spell_prefs.options,
            }),
            "HIGHLIGHT" => {
                let [r, g, b] = prefs().highlight.unwrap_or([16, 62, 112]);
                MTextDialog::Highlight(HighlightDraft {
                    r: r.to_string(),
                    g: g.to_string(),
                    b: b.to_string(),
                })
            }
            "STACKPROPS" => MTextDialog::Stack(self.stack_draft()?),
            "AUTOSTACK" => {
                let p = prefs();
                MTextDialog::AutoStack(AutoStackDraft {
                    enabled: p.auto_stack,
                    diagonal: p.auto_stack_diagonal,
                    trim: p.auto_stack_trim_blank,
                    back: None,
                })
            }
            _ => {
                let _ = ed;
                return None;
            }
        })
    }

    fn stack_draft(&self) -> Option<StackDraft> {
        let ed = self.mtext_editor.as_ref()?;
        let (range, data, props) = ed.caret_stack()?;
        let (upper, lower, style, percent) = raw_tools::stack_draft_values(&data, &props);
        Some(StackDraft {
            upper,
            lower,
            style,
            position: raw_tools::stack_position_of(&ed.content.text()),
            size: format!("{percent}%"),
            range: (range.start, range.end),
        })
    }

    fn dictionaries_draft(&self) -> DictionariesDraft {
        let p = &self.spell_prefs;
        let languages = crate::app::commands::dictionary_languages();
        let language = if p.language.is_empty() {
            languages.first().map(|(tag, _)| tag.clone()).unwrap_or_default()
        } else {
            p.language.clone()
        };
        let current = p.current_custom.filter(|n| *n < p.custom_files.len());
        DictionariesDraft {
            languages,
            language,
            files: p.custom_files.clone(),
            current,
            words: self.dictionary_words(current),
            ..Default::default()
        }
    }

    /// The words of a custom dictionary (None: the profile's word list).
    fn dictionary_words(&self, source: Option<usize>) -> Vec<String> {
        match source.and_then(|n| self.spell_prefs.custom_files.get(n)) {
            Some(path) => crate::app::commands::read_word_file(path),
            None => self.spell_prefs.custom.clone(),
        }
    }

    /// Keep a custom dictionary's words: in its file, or in the profile.
    pub(super) fn store_dictionary_words(&mut self, source: Option<usize>, words: &[String]) {
        match source.and_then(|n| self.spell_prefs.custom_files.get(n).cloned()) {
            Some(path) => {
                if let Err(error) = crate::app::commands::write_word_file(&path, words) {
                    self.command_line.push_error(&error.to_string());
                }
            }
            None => self.spell_prefs.custom = words.to_vec(),
        }
        self.save_config();
    }

    /// A message of one of this file's windows.
    pub(super) fn on_mtext_tool_dialog(&mut self, message: M) -> Task<Message> {
        let Some(dialog) = self.mtext_dialog.as_mut() else {
            return Task::none();
        };
        match (dialog, message) {
            (MTextDialog::Charset(d), M::Charset(code)) => d.charset = code,
            (MTextDialog::StyleChange(d), M::StyleAlways(on)) => d.always = on,
            (MTextDialog::StyleChange(_), M::StyleYes) => return self.answer_style_change(true),
            (MTextDialog::StyleChange(_), M::StyleNo) => return self.answer_style_change(false),
            (MTextDialog::ListsToggle(d), M::ListsAlways(on)) => d.always = on,
            (MTextDialog::ListsToggle(d), M::ListsContinue) => {
                let always = d.always;
                self.mtext_dialog = None;
                self.close_active_modal();
                update_prefs(|p| {
                    p.allow_lists = !p.allow_lists;
                    p.always_toggle_lists |= always;
                });
                self.save_config();
            }
            (MTextDialog::Dictionaries(d), message) => {
                let source = d.current;
                match message {
                    M::DictLanguage(tag) => {
                        d.language = tag.clone();
                        self.spell_prefs.language = tag;
                        self.save_config();
                    }
                    M::DictSource(n) => {
                        d.current = n;
                        d.selected = None;
                        self.spell_prefs.current_custom = n;
                        let words = self.dictionary_words(n);
                        if let Some(MTextDialog::Dictionaries(d)) = self.mtext_dialog.as_mut() {
                            d.words = words;
                        }
                        self.save_config();
                    }
                    M::DictWord(word) => d.word = word,
                    M::DictAdd => {
                        let word = d.word.trim().to_string();
                        if !word.is_empty() && !d.words.iter().any(|w| *w == word) {
                            d.words.push(word);
                            d.words.sort_by_key(|w| w.to_lowercase());
                        }
                        d.word.clear();
                        let words = d.words.clone();
                        self.store_dictionary_words(source, &words);
                    }
                    M::DictDelete => {
                        if let Some(n) = d.selected.take().filter(|n| *n < d.words.len()) {
                            d.words.remove(n);
                            let words = d.words.clone();
                            self.store_dictionary_words(source, &words);
                        }
                    }
                    M::DictSelect(n) => d.selected = Some(n),
                    M::DictImport => return pick_word_file(false, false),
                    M::DictImported(words) => {
                        for word in words {
                            if !d.words.contains(&word) {
                                d.words.push(word);
                            }
                        }
                        d.words.sort_by_key(|w| w.to_lowercase());
                        let words = d.words.clone();
                        self.store_dictionary_words(source, &words);
                    }
                    M::DictManage(on) => d.manage = on,
                    M::DictManageSelect(n) => d.manage_selected = Some(n),
                    M::DictNewFile => return pick_word_file(true, true),
                    M::DictAddFile => return pick_word_file(false, true),
                    M::DictFilePicked(path, new) => {
                        if new {
                            if let Err(error) = crate::app::commands::write_word_file(&path, &[]) {
                                self.command_line.push_error(&error.to_string());
                                return Task::none();
                            }
                        }
                        if !d.files.contains(&path) {
                            d.files.push(path);
                        }
                        d.manage_selected = Some(d.files.len() - 1);
                        self.spell_prefs.custom_files = d.files.clone();
                        self.save_config();
                    }
                    M::DictRemoveFile => {
                        if let Some(n) = d.manage_selected.take().filter(|n| *n < d.files.len()) {
                            d.files.remove(n);
                            if d.current == Some(n) {
                                d.current = None;
                                d.words = self.spell_prefs.custom.clone();
                            } else if let Some(c) = d.current.filter(|c| *c > n) {
                                d.current = Some(c - 1);
                            }
                            self.spell_prefs.custom_files = d.files.clone();
                            self.spell_prefs.current_custom = d.current;
                            self.save_config();
                        }
                    }
                    M::DictHelp => return self.dispatch_command("HELP SPELL"),
                    _ => {}
                }
            }
            (MTextDialog::SpellSettings(d), M::SpellOption(setting, on)) => {
                use crate::ui::window::spell_dialog::SpellSetting as S;
                let o = &mut d.options;
                match setting {
                    S::IgnoreCapitalized => o.ignore_capitalized = on,
                    S::IgnoreMixed => o.ignore_mixed = on,
                    S::IgnoreUpper => o.ignore_upper = on,
                    S::IgnoreNumbers => o.ignore_numbers = on,
                    S::IgnorePunctuation => o.ignore_punctuation = on,
                    S::Dimensions => o.dimensions = on,
                    S::Attributes => o.attributes = on,
                    S::Xrefs => o.xrefs = on,
                }
            }
            (MTextDialog::Highlight(d), message) => match message {
                M::HighlightR(v) => d.r = v,
                M::HighlightG(v) => d.g = v,
                M::HighlightB(v) => d.b = v,
                M::HighlightTheme => {
                    update_prefs(|p| p.highlight = None);
                    self.save_config();
                    self.mtext_dialog = None;
                    self.close_active_modal();
                }
                _ => {}
            },
            (MTextDialog::Stack(d), message) => match message {
                M::StackUpper(v) => d.upper = v,
                M::StackLower(v) => d.lower = v,
                M::StackStyleSet(s) => d.style = s,
                M::StackPositionSet(p) => d.position = p,
                M::StackSize(v) => d.size = v,
                M::StackDefaults(save) => {
                    if save {
                        let (style, position, size) = (d.style, d.position, d.percent());
                        update_prefs(|p| {
                            p.stack_style = style;
                            p.stack_position = position;
                            if let Some(size) = size {
                                p.stack_size = size;
                            }
                        });
                        self.save_config();
                    } else {
                        let p = prefs();
                        d.style = p.stack_style;
                        d.position = p.stack_position;
                        d.size = format!("{}%", p.stack_size);
                    }
                }
                M::AutoStackOpen => {
                    let p = prefs();
                    let back = d.clone();
                    self.mtext_dialog = Some(MTextDialog::AutoStack(AutoStackDraft {
                        enabled: p.auto_stack,
                        diagonal: p.auto_stack_diagonal,
                        trim: p.auto_stack_trim_blank,
                        back: Some(back),
                    }));
                }
                _ => {}
            },
            (MTextDialog::AutoStack(d), message) => match message {
                M::AutoEnabled(on) => d.enabled = on,
                M::AutoDiagonal(on) => d.diagonal = on,
                M::AutoTrim(on) => d.trim = on,
                M::AutoBack => {
                    if let Some(back) = d.back.take() {
                        self.mtext_dialog = Some(MTextDialog::Stack(back));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Task::none()
    }

    /// OK of one of this file's windows (the window is already closed).
    pub(super) fn apply_mtext_tool_dialog(&mut self, dialog: MTextDialog) -> Task<Message> {
        match dialog {
            MTextDialog::Charset(d) => self.mtext_charset(d.charset),
            MTextDialog::SpellSettings(d) => {
                self.spell_prefs.options = d.options;
                self.save_config();
            }
            MTextDialog::Highlight(d) => {
                if let Some(rgb) = d.rgb() {
                    update_prefs(|p| p.highlight = Some(rgb));
                    self.save_config();
                }
            }
            MTextDialog::Stack(d) => {
                if let Some(percent) = d.percent() {
                    self.mtext_set_stack(
                        d.range.0..d.range.1,
                        d.upper,
                        d.lower,
                        d.style,
                        d.position,
                        percent,
                    );
                }
            }
            MTextDialog::AutoStack(d) => {
                update_prefs(|p| {
                    p.auto_stack = d.enabled;
                    p.auto_stack_diagonal = d.diagonal;
                    p.auto_stack_trim_blank = d.trim;
                });
                self.save_config();
                // Back to the Stack Properties window it was opened from.
                if let Some(back) = d.back {
                    self.mtext_dialog = Some(MTextDialog::Stack(back));
                    self.active_modal = Some(ModalKind::MTextDialog);
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// A style picked for the text being edited: with nothing selected the
    /// change is asked about first (unless "Always change the text style").
    pub(in crate::app) fn mtext_pick_style(&mut self, name: String) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        if ed.selected_range().is_empty() && !prefs().always_change_style {
            self.mtext_dialog = Some(MTextDialog::StyleChange(StyleChangeDraft {
                style: name,
                always: false,
            }));
            self.active_modal = Some(ModalKind::MTextDialog);
            return;
        }
        self.mtext_change_style(name);
    }

    /// Allow Bullets and Lists: the lists already in the text stay as they
    /// are; it is asked about first unless "Always turn Bullets and Lists on
    /// or off".
    pub(in crate::app) fn mtext_lists_toggle(&mut self) {
        if prefs().always_toggle_lists {
            update_prefs(|p| p.allow_lists = !p.allow_lists);
            self.save_config();
            return;
        }
        self.mtext_dialog = Some(MTextDialog::ListsToggle(Default::default()));
        self.active_modal = Some(ModalKind::MTextDialog);
    }

    /// The style question answered: Yes (and Esc) changes the style.
    fn answer_style_change(&mut self, yes: bool) -> Task<Message> {
        let Some(MTextDialog::StyleChange(d)) = self.mtext_dialog.take() else {
            return Task::none();
        };
        self.close_active_modal();
        if yes {
            if d.always {
                update_prefs(|p| p.always_change_style = true);
                self.save_config();
            }
            self.mtext_change_style(d.style);
        }
        Task::none()
    }

    /// Esc or ✕ on the style question: the style changes, as Yes.
    pub(in crate::app) fn mtext_style_question_closed(&mut self, dialog: MTextDialog) {
        if let MTextDialog::StyleChange(d) = dialog {
            self.mtext_change_style(d.style);
        }
    }

    /// Import Text: pick a text or Rich Text file.
    pub(in crate::app) fn mtext_import_pick(&mut self) -> Task<Message> {
        Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title(crate::t!("Select File").into_owned())
                    .add_filter(crate::t!("Text/Template/Extract File (*.txt)").into_owned(), &["txt"])
                    .add_filter(crate::t!("Rich Text files (*.rtf)").into_owned(), &["rtf"])
                    .pick_file()
                    .await
                    .map(|h| crate::sys::handle_path(&h))
            },
            |path| match path {
                Some(path) => Message::MTextImport(path.to_string_lossy().into_owned()),
                None => Message::Noop,
            },
        )
    }

    /// Put the text of the file `path` in at the caret.
    pub(in crate::app) fn mtext_import(&mut self, path: &str) {
        let path = std::path::Path::new(path);
        match std::fs::read(path) {
            Ok(bytes) => {
                let source = import_text::file_source(path, &bytes);
                if !source.is_empty() {
                    self.raw_paste_fragment(&source);
                }
            }
            Err(error) => self.command_line.push_error(&error.to_string()),
        }
    }

    /// Find the misspelled words again when the text or the dictionary
    /// changed (spell checking on), for the editor to underline.
    pub(in crate::app) fn refresh_mtext_spelling(&mut self) {
        let on = prefs().spell_check;
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let text: String = doc_to_cells(&ed.doc)
            .iter()
            .map(|c| match c {
                Cell::Char(ch, _) => *ch,
                Cell::Break(..) => '\n',
                Cell::Stack { .. } => '\u{fffc}',
            })
            .collect();
        let key = format!(
            "{on}\u{1}{}\u{1}{:?}\u{1}{:?}\u{1}{}",
            self.spell_prefs.language,
            self.spell_prefs.current_custom,
            self.spell_prefs.options,
            self.spell_prefs.custom.len()
        ) + "\u{2}"
            + &text;
        if ed.spell_key == key {
            return;
        }
        let marks = if on {
            let mut custom = crate::app::commands::custom_words(&self.spell_prefs);
            custom.extend(ed.spell_ignored.iter().cloned());
            crate::app::commands::misspelled(&text, &self.spell_prefs, &custom)
        } else {
            Vec::new()
        };
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.spell_marks = marks;
            ed.spell_key = key;
        }
    }
}

/// Pick a custom dictionary file: a new one to make (`new`), one to add, or
/// (`file` false) one to import the words of.
fn pick_word_file(new: bool, file: bool) -> Task<Message> {
    Task::perform(
        async move {
            let dialog = rfd::AsyncFileDialog::new().add_filter(
                crate::t!("Custom dictionary file (*.cus)").into_owned(),
                &["cus"],
            );
            let dialog = if file {
                dialog
            } else {
                dialog.add_filter(crate::t!("Text file (*.txt)").into_owned(), &["txt"])
            };
            let handle = if new {
                dialog.set_file_name("custom.cus").save_file().await
            } else {
                dialog.pick_file().await
            };
            handle.map(|h| crate::sys::handle_path(&h).to_string_lossy().into_owned())
        },
        move |path| match path {
            Some(path) if file => Message::MTextDialog(M::DictFilePicked(path, new)),
            Some(path) => Message::MTextDialog(M::DictImported(
                crate::app::commands::read_word_file(&path),
            )),
            None => Message::Noop,
        },
    )
}
