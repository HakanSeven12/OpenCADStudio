//! Bullets and numbering as the format stores them: a hanging-indent paragraph
//! (`i-3,l3,t3`, written once where the list begins: the items after it keep
//! the values in force) whose text starts with its marker and a tab:
//! `1.\tText`, `a)\tText`, and a bullet as the Symbol font's `·`
//! (`{\fSymbol|b0|i0|c2|p18;·\t}Text`). A list item is recognised by both,
//! never by marker text alone.
//!
//! Typing a marker (`1.`, `1)`, `a.`, `A)`, or one sign such as `*` or `-`)
//! and a tab at the start of a paragraph makes it a list item (Allow Auto
//! Bullets and Numbering); with Allow Bullets and Lists off, no paragraph is
//! a list item: markers are plain text and Enter goes on with no marker.
use super::raw_adapter::{EditError, SourceIndex};
use super::raw_operations::{apply_patches, TextEdit};
use super::raw_paragraph_codes::{paragraphs_in_force, rewrite, InForce};
use super::*;
use crate::app::OpenCADStudio;
use document::entities::mtext_format::{ParagraphProperties, TabStop};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// Hanging indent of a list item made from the menu, in text heights.
const INDENT: f64 = 3.0;
/// Hanging indent of a list item made by typing its marker and a tab: the
/// marker stays one height in, the text goes to the next default tab stop.
const AUTO_INDENT: f64 = 4.0;
/// The bullet the menu writes: the Symbol font's `·`.
const BULLET: char = '\u{b7}';
const BULLET_CODE: &str = "{\\fSymbol|b0|i0|c2|p18;\u{b7}\t}";
const EMPTY_BULLET_GROUP: &str = "{\\fSymbol|b0|i0|c2|p18;}";

/// Allow Bullets and Lists (the user's setting).
static ALLOW: AtomicBool = AtomicBool::new(true);
/// Allow Auto Bullets and Numbering (the user's setting).
static ALLOW_AUTO: AtomicBool = AtomicBool::new(true);

pub(crate) fn lists_allowed() -> bool {
    ALLOW.load(Ordering::Relaxed)
}
pub(crate) fn set_lists_allowed(on: bool) {
    ALLOW.store(on, Ordering::Relaxed);
}
pub(crate) fn auto_lists_allowed() -> bool {
    ALLOW_AUTO.load(Ordering::Relaxed)
}
pub(crate) fn set_auto_lists_allowed(on: bool) {
    ALLOW_AUTO.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Number,
    /// a. b. c. …
    LowerLetter,
    /// A. B. C. …
    UpperLetter,
}

impl ListKind {
    /// Items of the kind count up (1., a., A.).
    fn counted(self) -> bool {
        self != ListKind::Bullet
    }
}

/// The letters of item `number` (1 = a), past z as aa, ab, ….
fn letters(number: usize, upper: bool) -> String {
    let mut n = number.max(1);
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    let text: String = out.into_iter().rev().collect();
    if upper {
        text.to_uppercase()
    } else {
        text
    }
}

/// The number of a letter marker (a = 1, aa = 27).
fn letter_number(text: &str) -> Option<usize> {
    text.chars().try_fold(0usize, |n, c| {
        let c = c.to_ascii_lowercase();
        c.is_ascii_lowercase()
            .then(|| n * 26 + (c as usize - 'a' as usize + 1))
    })
}

/// How an item's marker is written: what closes a counted marker (`.` or
/// `)`) and the sign of a bullet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Style {
    close: char,
    bullet: char,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            close: '.',
            bullet: BULLET,
        }
    }
}

/// The visible marker of item `number` (marker and tab).
fn marker_text(kind: ListKind, number: usize, style: Style) -> String {
    match kind {
        ListKind::Bullet => format!("{}\t", style.bullet),
        ListKind::Number => format!("{number}{}\t", style.close),
        ListKind::LowerLetter => format!("{}{}\t", letters(number, false), style.close),
        ListKind::UpperLetter => format!("{}{}\t", letters(number, true), style.close),
    }
}

/// One paragraph: its visible range (without the break), its text with any
/// non-character cell (a stack) as U+FFFC, and the values in force in it.
struct Paragraph {
    range: Range<usize>,
    text: Vec<char>,
    state: InForce,
}

/// A recognised list item: kind, number, visible length of marker + tab,
/// and how the marker is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    kind: ListKind,
    number: usize,
    marker: usize,
    style: Style,
}

/// The kind, number and style of a marker's text (without its tab).
fn parse_marker(head: &str) -> Option<(ListKind, usize, Style)> {
    let mut chars = head.chars();
    if let (Some(sign), None) = (chars.next(), chars.next()) {
        if !sign.is_alphanumeric() && !sign.is_whitespace() {
            let style = Style {
                close: '.',
                bullet: sign,
            };
            return Some((ListKind::Bullet, 0, style));
        }
    }
    let close = head.chars().last().filter(|c| matches!(c, '.' | ')'))?;
    let body = &head[..head.len() - 1];
    let style = Style {
        close,
        bullet: BULLET,
    };
    if body.is_empty() {
        return None;
    }
    if body.chars().all(|c| c.is_ascii_digit()) {
        return Some((ListKind::Number, body.parse().ok()?, style));
    }
    let upper = body.chars().all(|c| c.is_ascii_uppercase());
    let lower = body.chars().all(|c| c.is_ascii_lowercase());
    if upper || lower {
        let kind = if upper {
            ListKind::UpperLetter
        } else {
            ListKind::LowerLetter
        };
        return Some((kind, letter_number(body)?, style));
    }
    None
}

impl Paragraph {
    fn item(&self) -> Option<Item> {
        if !lists_allowed() || !(self.state.first < 0.0 && self.state.left > 0.0) {
            return None;
        }
        let tab = self.text.iter().position(|c| *c == '\t')?;
        let head: String = self.text[..tab].iter().collect();
        let (kind, number, style) = parse_marker(&head)?;
        Some(Item {
            kind,
            number,
            marker: tab + 1,
            style,
        })
    }

    /// The item text after the marker.
    fn body(&self, item: Option<Item>) -> &[char] {
        &self.text[item.map_or(0, |i| i.marker)..]
    }
}

fn paragraphs(raw: &str) -> Vec<Paragraph> {
    let doc = editor_document(raw);
    let cells = doc_to_cells(&doc);
    let mut states = paragraphs_in_force(raw).into_iter().map(|(state, _)| state);
    let mut out = Vec::new();
    let mut start = 0;
    let mut text = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        match cell {
            Cell::Break(..) => {
                out.push(Paragraph {
                    range: start..i,
                    text: std::mem::take(&mut text),
                    state: states.next().unwrap_or_default(),
                });
                start = i + 1;
            }
            Cell::Char(c, _) => text.push(*c),
            Cell::Stack { .. } => text.push('\u{fffc}'),
        }
    }
    out.push(Paragraph {
        range: start..cells.len(),
        text,
        state: states.next().unwrap_or_default(),
    });
    out
}

use super::raw_edit::editor_document;

/// Replace a visible range with plain text, certified by `replace`.
fn put(raw: &str, range: Range<usize>, text: &str) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    index.validate(false)?;
    Ok(index.replace(range, text)?.raw)
}

/// Put the source `code` (`visible` characters) at visible position `at`.
fn put_code(raw: &str, at: usize, code: &str, visible: usize) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let byte = index.insertion_byte(at)?;
    let out = apply_patches(raw, vec![(byte..byte, code.to_string())])?;
    index.certify(&out, count + visible)?;
    Ok(out)
}

/// Remove item `n`'s marker, and the empty Symbol group a bullet leaves.
fn remove_marker(raw: &str, n: usize, item: Item) -> Result<String, EditError> {
    let at = paragraphs(raw)[n].range.start;
    let out = put(raw, at..at + item.marker, "")?;
    if item.kind != ListKind::Bullet {
        return Ok(out);
    }
    let index = SourceIndex::new(&out);
    let count = index.validate(false)?;
    let byte = index.insertion_byte(at)?;
    // The paragraph's insertion point may sit before, inside or after the
    // group.
    let found = out
        .match_indices(EMPTY_BULLET_GROUP)
        .map(|(start, _)| start)
        .find(|start| (*start..=start + EMPTY_BULLET_GROUP.len()).contains(&byte));
    let Some(start) = found else {
        return Ok(out);
    };
    let cleaned = apply_patches(
        &out,
        vec![(start..start + EMPTY_BULLET_GROUP.len(), String::new())],
    )?;
    index.certify(&cleaned, count)?;
    Ok(cleaned)
}

/// Set the hanging indent of the paragraphs `n` (`indent` heights; 0 ends
/// the list), keeping every other paragraph's values.
fn set_indent(raw: &str, targets: &[usize], indent: f64) -> Result<String, EditError> {
    rewrite(raw, |n, state| {
        if targets.contains(&n) {
            state.first = -indent;
            state.left = indent;
            state.tabs = if indent > 0.0 {
                vec![TabStop::Left(indent)]
            } else {
                Vec::new()
            };
        }
    })
}

/// Make paragraph `n` a list item (marker; the hanging indent where none is
/// in force) or change its marker.
fn make_item(
    raw: &str,
    n: usize,
    kind: ListKind,
    number: usize,
    style: Style,
) -> Result<String, EditError> {
    let paragraph = &paragraphs(raw)[n];
    let marker = marker_text(kind, number, style);
    if let Some(item) = paragraph.item() {
        if item.kind != ListKind::Bullet && kind != ListKind::Bullet {
            let at = paragraph.range.start;
            return put(raw, at..at + item.marker, &marker);
        }
        let raw = remove_marker(raw, n, item)?;
        return insert_marker(&raw, n, kind, &marker, style);
    }
    let hanging = paragraph.state.first < 0.0 && paragraph.state.left > 0.0;
    let raw = insert_marker(raw, n, kind, &marker, style)?;
    if hanging {
        return Ok(raw);
    }
    set_indent(&raw, &[n], INDENT)
}

fn insert_marker(
    raw: &str,
    n: usize,
    kind: ListKind,
    marker: &str,
    style: Style,
) -> Result<String, EditError> {
    let at = paragraphs(raw)[n].range.start;
    if kind == ListKind::Bullet && style.bullet == BULLET {
        return put_code(raw, at, BULLET_CODE, 2);
    }
    put(raw, at..at, marker)
}

/// Turn list item `n` back into a plain paragraph: no marker, no hanging
/// indent.
fn clear_item(raw: &str, n: usize) -> Result<String, EditError> {
    let Some(item) = paragraphs(raw)[n].item() else {
        return Ok(raw.to_owned());
    };
    let raw = remove_marker(raw, n, item)?;
    set_indent(&raw, &[n], 0.0)
}

/// Number every run of consecutive items of a kind: the run's first item
/// keeps its number, an item numbered 1 starts again, the others follow on.
fn renumber(mut raw: String) -> Result<String, EditError> {
    let count = paragraphs(&raw).len();
    let mut next: Option<(ListKind, usize)> = None;
    for n in 0..count {
        let paragraph = &paragraphs(&raw)[n];
        match paragraph.item().filter(|i| i.kind.counted()) {
            Some(item) => {
                let want = match next {
                    Some((kind, number)) if kind == item.kind && item.number != 1 => number,
                    _ => item.number,
                };
                if item.number != want {
                    raw = make_item(&raw, n, item.kind, want, item.style)?;
                }
                next = Some((item.kind, want + 1));
            }
            None => next = None,
        }
    }
    Ok(raw)
}

/// Every paragraph's body and values outside `changed` must be as before;
/// inside it, the body must be unchanged and the item as asked.
fn certify(
    before: &str,
    after: &str,
    changed: &Range<usize>,
    want: Option<ListKind>,
) -> Result<(), EditError> {
    let (old, new) = (paragraphs(before), paragraphs(after));
    if old.len() != new.len() {
        return Err(EditError::ProjectionMismatch);
    }
    for (n, (o, a)) in old.iter().zip(&new).enumerate() {
        let (oi, ai) = (o.item(), a.item());
        if o.body(oi) != a.body(ai) {
            return Err(EditError::ProjectionMismatch);
        }
        let ok = if changed.contains(&n) {
            ai.map(|i| i.kind) == want
        } else {
            o.state == a.state && oi.map(|i| i.kind) == ai.map(|i| i.kind)
        };
        if !ok {
            return Err(EditError::ParagraphBoundary);
        }
    }
    Ok(())
}

impl MTextEditorState {
    /// Indexes of the paragraphs the selection (or caret) touches.
    pub(super) fn selected_paragraphs(&self) -> Range<usize> {
        let range = self.selected_range();
        let paragraphs = paragraphs(&self.content.text());
        let at = |pos: usize| {
            paragraphs
                .iter()
                .position(|p| pos <= p.range.end)
                .unwrap_or(paragraphs.len() - 1)
        };
        let end = if range.is_empty() {
            range.end
        } else {
            range.end - 1
        };
        at(range.start)..at(end.max(range.start)) + 1
    }

    /// The paragraph values in force in the caret's paragraph.
    pub(in crate::app) fn caret_paragraph_props(&self) -> ParagraphProperties {
        let n = self.selected_paragraphs().start;
        let state = paragraphs(&self.content.text())
            .into_iter()
            .nth(n)
            .map(|p| p.state)
            .unwrap_or_default();
        let set = |v: f64| (v != 0.0).then_some(v);
        ParagraphProperties {
            alignment: Some(state.alignment)
                .filter(|a| *a != document::entities::mtext_format::MTextParagraphAlignment::Default),
            first_line_indent: set(state.first),
            left_margin: set(state.left),
            right_margin: set(state.right),
            spacing_before: set(state.before),
            spacing_after: set(state.after),
            line_spacing: Some(state.spacing).filter(|s| {
                *s != document::entities::mtext_format::MTextLineSpacing::Default
            }),
            tab_stops: state.tabs,
        }
    }

    /// The alignment in force in the caret's paragraph (None: the
    /// attachment's own).
    pub(in crate::app) fn caret_alignment(
        &self,
    ) -> Option<document::entities::mtext_format::MTextParagraphAlignment> {
        self.caret_paragraph_props().alignment
    }

    /// The list kind of the caret's paragraph: the menus' check marks.
    pub(in crate::app) fn list_kind(&self) -> Option<ListKind> {
        let n = self.selected_paragraphs().start;
        paragraphs(&self.content.text())
            .get(n)
            .and_then(Paragraph::item)
            .map(|i| i.kind)
    }
}

impl OpenCADStudio {
    /// Make the selected paragraphs bullets or a list numbered from 1, or
    /// (`None`) plain paragraphs again. One undo step.
    pub(in crate::app) fn mtext_list(&mut self, kind: Option<ListKind>) {
        if !lists_allowed() {
            return;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let (before, sel) = (ed.content.text(), ed.selected_range());
        self.edit_raw(|ed, index| {
            let targets = ed.selected_paragraphs();
            let mut raw = index.raw.to_string();
            for n in targets.clone().rev() {
                raw = match kind {
                    Some(kind) => {
                        make_item(&raw, n, kind, n - targets.start + 1, Style::default())?
                    }
                    None => clear_item(&raw, n)?,
                };
            }
            let raw = renumber(raw)?;
            certify(index.raw, &raw, &targets, kind)?;
            // The caret keeps its place in the text it was in.
            let caret = caret_in(index.raw, &raw, ed.caret);
            Ok(TextEdit { raw, caret })
        });
        self.keep_list_selection(&before, sel);
    }

    /// The selection keeps covering the same text, so another list command
    /// acts on the same paragraphs.
    fn keep_list_selection(&mut self, before: &str, sel: Range<usize>) {
        if let Some(ed) = self
            .mtext_editor
            .as_mut()
            .filter(|ed| ed.content.text() != before)
        {
            let after = ed.content.text();
            let (a, b) = (
                caret_in(before, &after, sel.start),
                caret_in(before, &after, sel.end),
            );
            ed.sel = Some((a, b));
            ed.sel_anchor = a;
            ed.caret = b;
        }
    }

    /// Start: the caret's item numbers from 1 (a, A), the items after it
    /// follow on. Continue: it numbers on from the list before it.
    pub(in crate::app) fn mtext_list_restart(&mut self, continue_list: bool) {
        if !lists_allowed() {
            return;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let (before, sel) = (ed.content.text(), ed.selected_range());
        self.edit_raw(|ed, index| {
            let all = paragraphs(index.raw);
            let n = ed.selected_paragraphs().start;
            let item = all
                .get(n)
                .and_then(Paragraph::item)
                .filter(|i| i.kind.counted())
                .ok_or(EditError::ParagraphBoundary)?;
            let number = if continue_list {
                // The last item of the kind before this run of items.
                let run_start = (0..n)
                    .rev()
                    .take_while(|k| all[*k].item().is_some_and(|i| i.kind == item.kind))
                    .last()
                    .unwrap_or(n);
                (0..run_start)
                    .rev()
                    .find_map(|k| all[k].item().filter(|i| i.kind == item.kind))
                    .map_or(1, |i| i.number + 1)
            } else {
                1
            };
            let raw = make_item(index.raw, n, item.kind, number, item.style)?;
            let raw = if continue_list {
                // The items of this run before the caret's follow on too.
                renumber_from(raw, n)?
            } else {
                renumber(raw)?
            };
            let caret = caret_in(index.raw, &raw, ed.caret);
            Ok(TextEdit { raw, caret })
        });
        self.keep_list_selection(&before, sel);
    }

    /// Enter in a list item: a new item after it (numbers follow on), or,
    /// on an empty item, leave the list. `false` when not in a list item.
    pub(in crate::app) fn mtext_list_enter(&mut self) -> bool {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return false;
        };
        let range = ed.selected_range();
        let all = paragraphs(&ed.content.text());
        let n = ed.selected_paragraphs().start;
        let Some(item) = all.get(n).and_then(Paragraph::item) else {
            return false;
        };
        let paragraph = &all[n];
        // Before or inside the marker: an ordinary line break.
        if range.start < paragraph.range.start + item.marker || ed.selected_paragraphs().len() > 1 {
            return false;
        }
        let empty = paragraph.text.len() == item.marker;
        self.edit_raw(|_, index| {
            if empty {
                let raw = renumber(clear_item(index.raw, n)?)?;
                let caret = paragraphs(&raw)[n].range.start;
                return Ok(TextEdit { raw, caret });
            }
            let split = index.replace(range.clone(), "\n")?;
            let next = n + 1;
            let number = item.number + 1;
            let raw = make_item(&split.raw, next, item.kind, number, item.style)?;
            let raw = renumber(raw)?;
            let caret = paragraphs(&raw)[next].range.start
                + marker_text(item.kind, number, item.style).chars().count();
            Ok(TextEdit { raw, caret })
        });
        true
    }

    /// A tab typed right after a marker (`1.`, `a)`, `*` …) that opens a
    /// paragraph makes the paragraph a list item, when automatic lists are
    /// allowed. `false` when the tab is an ordinary tab.
    pub(in crate::app) fn mtext_list_auto(&mut self) -> bool {
        if !lists_allowed() || !auto_lists_allowed() {
            return false;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return false;
        };
        let range = ed.selected_range();
        if !range.is_empty() {
            return false;
        }
        let all = paragraphs(&ed.content.text());
        let n = ed.selected_paragraphs().start;
        let Some(paragraph) = all.get(n) else {
            return false;
        };
        if paragraph.item().is_some() || range.start != paragraph.range.end {
            return false;
        }
        let head: String = paragraph.text.iter().collect();
        let Some((kind, number, _)) = parse_marker(&head) else {
            return false;
        };
        // A counted list typed by hand starts at 1 (a, A).
        if kind.counted() && number != 1 {
            return false;
        }
        let caret = range.start + 1;
        self.edit_raw(|_, index| {
            let raw = index.replace(range.clone(), "\t")?.raw;
            let raw = set_indent(&raw, &[n], AUTO_INDENT)?;
            Ok(TextEdit { raw, caret })
        });
        true
    }
}

/// Renumber the run holding item `n` from `n` on, then every other run.
fn renumber_from(raw: String, n: usize) -> Result<String, EditError> {
    let all = paragraphs(&raw);
    let Some(item) = all[n].item() else {
        return renumber(raw);
    };
    let mut raw = raw;
    let mut number = item.number + 1;
    for k in n + 1..all.len() {
        let Some(next) = paragraphs(&raw)[k].item().filter(|i| i.kind == item.kind) else {
            break;
        };
        if next.number != number {
            raw = make_item(&raw, k, next.kind, number, next.style)?;
        }
        number += 1;
    }
    Ok(raw)
}

/// The caret position in `after` that keeps it on the same text character
/// as in `before`, when markers were added or removed before it.
fn caret_in(before: &str, after: &str, caret: usize) -> usize {
    let (old, new) = (paragraphs(before), paragraphs(after));
    let Some(n) = old.iter().position(|p| caret <= p.range.end) else {
        return caret;
    };
    let (o, a) = (&old[n], &new[n]);
    let (om, am) = (
        o.item().map_or(0, |i| i.marker),
        a.item().map_or(0, |i| i.marker),
    );
    let offset = (caret - o.range.start).saturating_sub(om);
    (a.range.start + am + offset).min(a.range.end)
}
