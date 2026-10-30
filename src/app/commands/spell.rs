//! SPELL: the Check Spelling window.
//!
//! The objects in the chosen place are read in drawing order, each word
//! checked with the system spell checker of the main dictionary. A word not
//! in it stops the check: it can be changed (here or everywhere), ignored
//! (here or everywhere) or added to the user's dictionary, and each change
//! can be taken back with Undo while the window is open.

use std::cell::RefCell;

use crate::app::control::spellcheck::SystemSpeller;
use crate::app::control::text_ops::{decode_dxf_escapes, replace_mtext_safe, TextMatcher};
use crate::app::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::ui::window::spell_dialog::{SpellFind, SpellMsg, SpellScope, SpellState};
use codec::{EntityType, Handle};
use glam::DVec3;
use iced::Task;

/// The main dictionaries offered, those the system can check shown.
const LANGUAGES: &[(&str, &str)] = &[
    ("en-US", "American English"),
    ("en-GB", "British English"),
    ("en-CA", "Canadian English"),
    ("ca-ES", "Catalan"),
    ("cs-CZ", "Czech"),
    ("da-DK", "Danish"),
    ("nl-NL", "Dutch"),
    ("fi-FI", "Finnish"),
    ("fr-FR", "French"),
    ("de-DE", "German"),
    ("it-IT", "Italian"),
    ("nb-NO", "Norwegian (Bokmal)"),
    ("pt-BR", "Portuguese (Brazilian)"),
    ("pt-PT", "Portuguese (Iberian)"),
    ("ru-RU", "Russian"),
    ("es-ES", "Spanish"),
    ("sv-SE", "Swedish"),
    ("tr-TR", "Turkish"),
];

thread_local! {
    static SPELLER: RefCell<Option<SystemSpeller>> = const { RefCell::new(None) };
}

/// Run `f` with the speller of `language` (made once per language).
fn with_speller<T>(language: &str, f: impl FnOnce(&SystemSpeller) -> T) -> T {
    SPELLER.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().is_none_or(|s| s.language() != language) {
            *cell = Some(SystemSpeller::new((!language.is_empty()).then_some(language)));
        }
        f(cell.as_ref().expect("speller just made"))
    })
}

fn available_languages() -> Vec<(String, String)> {
    LANGUAGES
        .iter()
        .filter(|(tag, _)| {
            let s = SystemSpeller::new(Some(tag));
            s.is_available() && s.language().eq_ignore_ascii_case(tag)
        })
        .map(|(tag, name)| (tag.to_string(), name.to_string()))
        .collect()
}

fn hex(handles: &[Handle]) -> String {
    handles
        .iter()
        .map(|h| format!("{:X}", h.value()))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_handles(text: &str) -> Vec<Handle> {
    text.split(',')
        .filter_map(|s| u64::from_str_radix(s.trim(), 16).ok().map(Handle::new))
        .collect()
}

/// The text the check reads in an object, `None` when it holds none.
fn checked_text(entity: &EntityType, options: &crate::ui::window::spell_dialog::SpellOptions) -> Option<String> {
    match entity {
        EntityType::Text(t) => Some(decode_dxf_escapes(&t.value)),
        EntityType::MText(m) => Some(decode_dxf_escapes(
            &codec::entities::mtext_format::parse_mtext(&m.value, true).to_plain_text(),
        )),
        EntityType::AttributeEntity(a) if options.attributes => {
            Some(decode_dxf_escapes(a.get_value()))
        }
        EntityType::AttributeDefinition(a) if options.attributes => {
            Some(decode_dxf_escapes(&a.default_value))
        }
        EntityType::Dimension(d) if options.dimensions => {
            let text = d.base().text.clone();
            (!text.is_empty() && text != "<>").then(|| decode_dxf_escapes(&text.replace("<>", " ")))
        }
        _ => None,
    }
}

/// The words of `text` from byte `from`: (start, end) of each.
fn words(text: &str, from: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-';
    for (i, c) in text.char_indices().skip_while(|(i, _)| *i < from) {
        match (is_word(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    // Leading and trailing apostrophes and hyphens are punctuation.
    out.into_iter()
        .filter_map(|(s, e)| {
            let word = &text[s..e];
            let trimmed_start = word.len() - word.trim_start_matches(['\'', '’', '-']).len();
            let trimmed = word.trim_matches(['\'', '’', '-']);
            (!trimmed.is_empty()).then(|| (s + trimmed_start, s + trimmed_start + trimmed.len()))
        })
        .collect()
}

/// Whether the options let `word` pass without a check.
fn skipped(word: &str, options: &crate::ui::window::spell_dialog::SpellOptions) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return true;
    }
    let has_digit = word.chars().any(|c| c.is_numeric());
    let has_punct = word.chars().any(|c| !c.is_alphanumeric());
    let upper = letters.iter().all(|c| c.is_uppercase());
    let first_upper = letters.first().is_some_and(|c| c.is_uppercase());
    let rest_lower = letters.iter().skip(1).all(|c| c.is_lowercase());
    let mixed = !upper && letters.iter().skip(1).any(|c| c.is_uppercase());
    (options.ignore_numbers && has_digit)
        || (options.ignore_punctuation && has_punct)
        || (options.ignore_upper && upper && letters.len() > 1)
        || (options.ignore_capitalized && first_upper && rest_lower)
        || (options.ignore_mixed && mixed)
}

/// Selection for SPELL's Selected objects: `Select objects:`, then the
/// window comes back with them.
pub struct SpellSelectCommand;

impl CadCommand for SpellSelectCommand {
    fn name(&self) -> &'static str {
        "SPELL"
    }

    fn prompt(&self) -> String {
        crate::t!("Select objects:").into_owned()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("_SPELLSELECTED {}", hex(&handles)))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("_SPELLSELECTED".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("_SPELLSELECTED".to_string())
    }
}

impl OpenCADStudio {
    pub(super) fn dispatch_spell(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        match verb.trim_start_matches(['_', '.']).to_ascii_uppercase().as_str() {
            // A pickfirst selection is not used: the window opens on the
            // place chosen last (Entire drawing at first), as measured.
            "SPELL" if rest.trim().is_empty() => {
                self.open_spell(i, None);
                Some(Task::none())
            }
            "SPELLSELECTED" if verb.starts_with('_') => {
                let handles = parse_handles(rest.trim());
                let kept = self.spell.take();
                self.spell = kept;
                self.open_spell(i, (!handles.is_empty()).then_some(handles));
                Some(Task::none())
            }
            _ => None,
        }
    }

    fn open_spell(&mut self, i: usize, selection: Option<Vec<Handle>>) {
        let mut state = self.spell.take().unwrap_or_else(|| {
            let languages = available_languages();
            // American English first, as the reference offers; else the
            // system's own language.
            let language = if !self.spell_prefs.language.is_empty() {
                self.spell_prefs.language.clone()
            } else if languages.iter().any(|(tag, _)| tag == "en-US") {
                "en-US".to_string()
            } else {
                with_speller("", |s| s.language().to_string())
            };
            SpellState {
                language,
                languages,
                options: self.spell_prefs.options,
                custom: self.spell_prefs.custom.clone(),
                ..Default::default()
            }
        });
        if let Some(selection) = selection {
            state.scope = SpellScope::Selection;
            state.selection = selection;
        }
        state.running = false;
        state.current = None;
        state.status.clear();
        self.tabs[i].scene.deselect_all();
        self.spell = Some(state);
        self.active_modal = Some(crate::app::ModalKind::Spell);
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
    }

    /// The objects the check reads, in drawing order.
    fn spell_targets(&self, i: usize, state: &SpellState) -> Vec<Handle> {
        let scene = &self.tabs[i].scene;
        let wanted: Option<std::collections::HashSet<Handle>> = match state.scope {
            SpellScope::Selection => Some(state.selection.iter().copied().collect()),
            _ => None,
        };
        scene
            .document
            .entities()
            .filter(|e| checked_text(e, &state.options).is_some())
            .map(|e| e.common().handle)
            .filter(|h| match (&wanted, state.scope) {
                (Some(set), _) => set.contains(h),
                (None, SpellScope::Space) => scene.entity_belongs_to_active_space(*h),
                _ => true,
            })
            .collect()
    }

    /// Go on from the cursor to the next word not in the dictionary; at the
    /// end the check is complete.
    fn spell_advance(&mut self, i: usize) {
        let Some(mut state) = self.spell.take() else {
            return;
        };
        state.current = None;
        state.suggestions.clear();
        state.replacement.clear();
        let language = state.language.clone();
        while state.cursor.0 < state.targets.len() {
            let handle = state.targets[state.cursor.0];
            let text = self.tabs[i]
                .scene
                .document
                .get_entity(handle)
                .and_then(|e| checked_text(e, &state.options))
                .unwrap_or_default();
            let found = words(&text, state.cursor.1).into_iter().find(|&(s, e)| {
                let word = &text[s..e];
                if skipped(word, &state.options)
                    || state.ignored.iter().any(|w| w == word)
                    || state.custom.iter().any(|w| w.eq_ignore_ascii_case(word))
                {
                    return false;
                }
                !with_speller(&language, |sp| sp.check_word(word, false).0)
            });
            match found {
                Some((s, e)) => {
                    let word = text[s..e].to_string();
                    let suggestions = with_speller(&language, |sp| sp.check_word(&word, true).1);
                    state.replacement = suggestions.first().cloned().unwrap_or_else(|| word.clone());
                    state.suggestions = suggestions;
                    state.cursor.1 = e;
                    state.current = Some(SpellFind {
                        handle,
                        word,
                        start: s,
                        end: e,
                        text,
                    });
                    // The object with the word shows selected.
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.select_entity(handle, true);
                    self.spell = Some(state);
                    return;
                }
                None => state.cursor = (state.cursor.0 + 1, 0),
            }
        }
        state.running = false;
        state.status = crate::tr!("spell", "complete");
        self.tabs[i].scene.deselect_all();
        self.spell = Some(state);
    }

    /// `word` → `replacement` in `handle`: the occurrence the check stopped
    /// at (`nth`, counted in its text), or all of them.
    fn spell_replace_in(
        &mut self,
        i: usize,
        handle: Handle,
        word: &str,
        replacement: &str,
        nth: Option<usize>,
    ) -> Option<(Handle, EntityType)> {
        let before = self.tabs[i].scene.document.get_entity(handle)?.clone();
        let matcher = TextMatcher::new(word.to_string(), true, true, false);
        let replace_plain = |value: &str| -> Option<String> {
            let hits = matcher.find_all_in(value);
            if hits.is_empty() {
                return None;
            }
            let mut out = value.to_string();
            let picked: Vec<(usize, usize)> = match nth {
                Some(n) => hits.get(n).or_else(|| hits.first()).copied().into_iter().collect(),
                None => hits,
            };
            for (s, e) in picked.into_iter().rev() {
                out.replace_range(s..e, replacement);
            }
            Some(out)
        };
        let mut after = before.clone();
        let changed = match &mut after {
            EntityType::Text(t) => replace_plain(&t.value).map(|v| t.value = v).is_some(),
            EntityType::MText(m) => {
                // ponytail: the first (or every) MTEXT occurrence; an earlier
                // copy ignored once is changed instead of the one shown.
                let (value, n) = replace_mtext_safe(&m.value, &matcher, replacement, nth.is_none());
                m.value = value;
                n > 0
            }
            EntityType::AttributeEntity(a) => match replace_plain(a.get_value()) {
                Some(v) => {
                    a.set_value(&v);
                    true
                }
                None => false,
            },
            EntityType::AttributeDefinition(a) => {
                replace_plain(&a.default_value).map(|v| a.default_value = v).is_some()
            }
            EntityType::Dimension(d) => {
                let text = d.base().text.clone();
                replace_plain(&text).map(|v| d.base_mut().text = v).is_some()
            }
            _ => false,
        };
        if !changed {
            return None;
        }
        let entity = after;
        self.apply_property_op(i, "SPELL", &[handle], |app, h| {
            if let Some(e) = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|e| e.common().handle == h)
            {
                *e = entity.clone();
            }
        });
        Some((handle, before))
    }

    pub(in crate::app) fn update_spell(&mut self, message: SpellMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.spell.as_mut() else {
            return Task::none();
        };
        match message {
            SpellMsg::Scope(scope) => {
                state.scope = scope;
                state.running = false;
                state.current = None;
                state.status.clear();
            }
            SpellMsg::SelectObjects => {
                self.active_modal = None;
                return self.start_count_command(i, Box::new(SpellSelectCommand));
            }
            SpellMsg::Start => {
                let targets = self.spell_targets(i, self.spell.as_ref().expect("open"));
                let state = self.spell.as_mut().expect("open");
                state.targets = targets;
                state.cursor = (0, 0);
                state.ignored.clear();
                state.undo.clear();
                state.running = true;
                state.status.clear();
                self.spell_advance(i);
            }
            SpellMsg::Replacement(v) | SpellMsg::Suggestion(v) => state.replacement = v,
            SpellMsg::Change | SpellMsg::ChangeAll => {
                let Some(found) = state.current.clone() else {
                    return Task::none();
                };
                let replacement = state.replacement.clone();
                let cursor = (state.cursor.0, found.start);
                let all = matches!(message, SpellMsg::ChangeAll);
                let targets = if all { state.targets.clone() } else { vec![found.handle] };
                // Which occurrence of the word in its text this one is.
                let nth = TextMatcher::new(found.word.clone(), true, true, false)
                    .find_all_in(&found.text)
                    .iter()
                    .position(|&(s, _)| s == found.start);
                let mut undo = Vec::new();
                for h in targets {
                    let nth = if all { None } else { nth };
                    if let Some(entry) = self.spell_replace_in(i, h, &found.word, &replacement, nth) {
                        undo.push(entry);
                    }
                }
                let state = self.spell.as_mut().expect("open");
                if !undo.is_empty() {
                    state.undo.push((undo, cursor));
                }
                // Go on after the replaced word.
                state.cursor = (cursor.0, cursor.1 + replacement.len());
                if all {
                    state.ignored.push(replacement);
                }
                self.spell_advance(i);
            }
            SpellMsg::Ignore => self.spell_advance(i),
            SpellMsg::IgnoreAll => {
                if let Some(found) = &state.current {
                    state.ignored.push(found.word.clone());
                }
                self.spell_advance(i);
            }
            SpellMsg::AddWord => {
                if let Some(found) = &state.current {
                    if !state.custom.iter().any(|w| w.eq_ignore_ascii_case(&found.word)) {
                        state.custom.push(found.word.clone());
                    }
                    self.spell_prefs.custom = state.custom.clone();
                    self.save_config();
                }
                self.spell_advance(i);
            }
            SpellMsg::Undo => {
                let Some((entries, cursor)) = state.undo.pop() else {
                    return Task::none();
                };
                let handles: Vec<Handle> = entries.iter().map(|(h, _)| *h).collect();
                let mut entries = entries.into_iter();
                self.apply_property_op(i, "SPELL", &handles, |app, h| {
                    let Some((_, old)) = entries.next() else {
                        return;
                    };
                    if let Some(e) = app.tabs[i]
                        .scene
                        .document
                        .entities_mut()
                        .find(|e| e.common().handle == h)
                    {
                        *e = old;
                    }
                });
                let state = self.spell.as_mut().expect("open");
                state.cursor = cursor;
                state.running = true;
                state.status.clear();
                self.spell_advance(i);
            }
            SpellMsg::Language(name) => {
                if let Some((tag, _)) = state.languages.iter().find(|(_, n)| *n == name) {
                    state.language = tag.clone();
                    self.spell_prefs.language = tag.clone();
                    self.save_config();
                }
            }
            SpellMsg::ToggleSettings => state.show_settings = !state.show_settings,
            SpellMsg::Setting(setting, on) => {
                state.options.set(setting, on);
                self.spell_prefs.options = state.options;
                self.save_config();
            }
            SpellMsg::RemoveWord(word) => {
                state.custom.retain(|w| *w != word);
                self.spell_prefs.custom = state.custom.clone();
                self.save_config();
            }
            SpellMsg::Help => {
                self.command_line.push_info(&crate::tr!("spell", "help"));
            }
        }
        Task::none()
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["SPELL"] });
