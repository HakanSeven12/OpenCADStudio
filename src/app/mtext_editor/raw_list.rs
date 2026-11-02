//! Bullets and numbering as the format stores them: a hanging-indent paragraph
//! (`\pxi-1.5,l1.5,t1.5;`, appended so it wins in its paragraph) whose text
//! starts with its marker and a tab: `•\tText`, `1.\tText`. A list item is
//! recognised by both, never by marker text alone.
use super::raw_adapter::{EditError, Kind, SourceIndex};
use super::raw_edit::editor_document;
use super::raw_operations::{apply_patches, TextEdit};
use super::*;
use crate::app::OpenCADStudio;
use codec::entities::mtext_format::ParagraphProperties;
use std::ops::Range;

/// Hanging indent of a list item, in text heights.
const INDENT: f64 = 1.5;
const BULLET: char = '•';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Number,
}

fn settings() -> String {
    format!("\\pxi-{INDENT},l{INDENT},t{INDENT};")
}

fn marker_text(kind: ListKind, number: usize) -> String {
    match kind {
        ListKind::Bullet => format!("{BULLET}\t"),
        ListKind::Number => format!("{number}.\t"),
    }
}

/// One paragraph: its visible range (without the break) and its text, with
/// any non-character cell (a stack) as U+FFFC.
struct Paragraph {
    range: Range<usize>,
    text: Vec<char>,
    props: ParagraphProperties,
}

/// A recognised list item: kind, number, and visible length of marker + tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    kind: ListKind,
    number: usize,
    marker: usize,
}

impl Paragraph {
    fn item(&self) -> Option<Item> {
        let hanging = self.props.first_line_indent.is_some_and(|i| i < 0.0)
            && self.props.left_margin.is_some_and(|l| l > 0.0);
        if !hanging {
            return None;
        }
        let tab = self.text.iter().position(|c| *c == '\t')?;
        let head: String = self.text[..tab].iter().collect();
        if head == BULLET.to_string() {
            return Some(Item {
                kind: ListKind::Bullet,
                number: 0,
                marker: tab + 1,
            });
        }
        let digits = head.strip_suffix('.')?;
        let number = digits
            .parse()
            .ok()
            .filter(|_| digits.chars().all(|c| c.is_ascii_digit()))?;
        Some(Item {
            kind: ListKind::Number,
            number,
            marker: tab + 1,
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
    let mut out = Vec::new();
    let mut start = 0;
    let mut text = Vec::new();
    let mut props = doc.paragraphs.iter().map(|p| p.properties.clone());
    for (i, cell) in cells.iter().enumerate() {
        match cell {
            Cell::Break(..) => {
                out.push(Paragraph {
                    range: start..i,
                    text: std::mem::take(&mut text),
                    props: props.next().unwrap_or_default(),
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
        props: props.next().unwrap_or_default(),
    });
    out
}

/// Raw byte at which paragraph `n`'s own settings go: its end, before the
/// break that closes it (or the tail of the text).
fn paragraph_end(index: &SourceIndex, n: usize) -> usize {
    index
        .units
        .iter()
        .filter(|u| matches!(u.kind, Kind::Break | Kind::Column))
        .nth(n)
        .map_or_else(|| index.tail_boundary(), |u| u.raw.start)
}

/// Raw ranges of list-settings controls this editor wrote in paragraph `n`.
fn own_settings(index: &SourceIndex, n: usize) -> Vec<Range<usize>> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(
            index
                .units
                .iter()
                .filter(|u| matches!(u.kind, Kind::Break | Kind::Column))
                .map(|u| u.raw.end),
        )
        .collect();
    let (from, to) = (starts[n], paragraph_end(index, n));
    let code = settings();
    index
        .tokens
        .iter()
        .filter(|t| t.kind == Kind::Control('p') && t.raw.start >= from && t.raw.end <= to)
        .filter(|t| index.raw[t.raw.clone()] == code)
        .map(|t| t.raw.clone())
        .collect()
}

/// Replace a visible range with plain text, certified by `replace`.
fn put(raw: &str, range: Range<usize>, text: &str) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    index.validate(false)?;
    Ok(index.replace(range, text)?.raw)
}

/// Make paragraph `n` a list item (marker and settings) or change its marker.
fn make_item(raw: &str, n: usize, kind: ListKind, number: usize) -> Result<String, EditError> {
    let paragraph = &paragraphs(raw)[n];
    let marker = marker_text(kind, number);
    match paragraph.item() {
        Some(item) => {
            let at = paragraph.range.start;
            put(raw, at..at + item.marker, &marker)
        }
        None => {
            let raw = put(raw, paragraph.range.start..paragraph.range.start, &marker)?;
            let index = SourceIndex::new(&raw);
            let count = index.validate(false)?;
            // A paragraph split off a list item already carries the settings.
            if !own_settings(&index, n).is_empty() {
                return Ok(raw);
            }
            let end = paragraph_end(&index, n);
            let out = apply_patches(&raw, vec![(end..end, settings())])?;
            index.certify(&out, count)?;
            Ok(out)
        }
    }
}

/// Turn list item `n` back into a plain paragraph. Only settings this editor
/// wrote can be removed; any other hanging indent refuses the change.
fn clear_item(raw: &str, n: usize) -> Result<String, EditError> {
    let paragraph = &paragraphs(raw)[n];
    let Some(item) = paragraph.item() else {
        return Ok(raw.to_owned());
    };
    let index = SourceIndex::new(raw);
    let own = own_settings(&index, n);
    if own.is_empty() {
        return Err(EditError::ParagraphBoundary);
    }
    let count = index.validate(false)?;
    let out = apply_patches(raw, own.into_iter().map(|r| (r, String::new())).collect())?;
    index.certify(&out, count)?;
    let at = paragraphs(&out)[n].range.start;
    put(&out, at..at + item.marker, "")
}

/// Number every run of consecutive numbered items from its first number.
fn renumber(mut raw: String) -> Result<String, EditError> {
    let count = paragraphs(&raw).len();
    let mut next: Option<usize> = None;
    for n in 0..count {
        let paragraph = &paragraphs(&raw)[n];
        match paragraph.item().filter(|i| i.kind == ListKind::Number) {
            Some(item) => {
                let want = next.unwrap_or(item.number);
                if item.number != want {
                    raw = make_item(&raw, n, ListKind::Number, want)?;
                }
                next = Some(want + 1);
            }
            None => next = None,
        }
    }
    Ok(raw)
}

/// Every paragraph's body and settings outside `changed` must be as before;
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
            o.props == a.props && oi.map(|i| i.kind) == ai.map(|i| i.kind)
        };
        if !ok {
            return Err(EditError::ParagraphBoundary);
        }
    }
    Ok(())
}

impl MTextEditorState {
    /// Indexes of the paragraphs the selection (or caret) touches.
    fn selected_paragraphs(&self) -> Range<usize> {
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
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let (before, sel) = (ed.content.text(), ed.selected_range());
        self.edit_raw(|ed, index| {
            let targets = ed.selected_paragraphs();
            let mut raw = index.raw.to_string();
            for n in targets.clone().rev() {
                raw = match kind {
                    Some(kind) => make_item(&raw, n, kind, n - targets.start + 1)?,
                    None => clear_item(&raw, n)?,
                };
            }
            let raw = renumber(raw)?;
            certify(index.raw, &raw, &targets, kind)?;
            // The caret keeps its place in the text it was in.
            let caret = caret_in(index.raw, &raw, ed.caret);
            Ok(TextEdit { raw, caret })
        });
        // The selection keeps covering the same text, so another list
        // command acts on the same paragraphs.
        if let Some(ed) = self
            .mtext_editor
            .as_mut()
            .filter(|ed| ed.content.text() != before)
        {
            let after = ed.content.text();
            let (a, b) = (
                caret_in(&before, &after, sel.start),
                caret_in(&before, &after, sel.end),
            );
            ed.sel = Some((a, b));
            ed.sel_anchor = a;
            ed.caret = b;
        }
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
            let raw = make_item(&split.raw, next, item.kind, number)?;
            let raw = renumber(raw)?;
            let caret =
                paragraphs(&raw)[next].range.start + marker_text(item.kind, number).chars().count();
            Ok(TextEdit { raw, caret })
        });
        true
    }
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
