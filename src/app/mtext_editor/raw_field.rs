//! Text around native FIELDs. The editor edits the displayed value, where the
//! codec's `field_spans` locates each field's text; those spans are atomic.
//! Every FIELD object stays as it is: on save only the host's `_text`
//! container gets the new template (each span back to its `%<\_FldIdx n>%`)
//! and the checksum of the new displayed value. A field inserted in the
//! editor has no FIELD object yet; a save with one rebuilds the host's field
//! tree from the codes of its fields.
use super::raw_adapter::EditError;
use super::*;
use document::fields::NewField;
use document::objects::ObjectType;
use std::ops::Range;

/// A FIELD host's field texts in the edited value, and their placeholders.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldHost {
    /// The host's `_text` container; none for a text with no field yet.
    container: Option<Handle>,
    /// Byte ranges in the editor's value, in template order.
    pub spans: Vec<Range<usize>>,
    placeholders: Vec<String>,
    /// Per span: the field inserted in this session, still without an object.
    inserted: Vec<Option<NewField>>,
    /// A field was deleted in this session, so its FIELD objects must go.
    removed: bool,
}

/// The host's `_text` container: xdictionary → ACAD_FIELD → TEXT.
fn container(doc: &document::Document, host: Handle) -> Option<Handle> {
    let xdict = doc.get_entity(host)?.common().xdictionary_handle?;
    let ObjectType::Dictionary(x) = doc.objects.get(&xdict)? else {
        return None;
    };
    let ObjectType::Dictionary(fields) = doc.objects.get(&x.get("ACAD_FIELD")?)? else {
        return None;
    };
    let text = fields.get("TEXT")?;
    matches!(doc.objects.get(&text), Some(ObjectType::Field(f)) if f.evaluator_id == "_text")
        .then_some(text)
}

fn placeholders(template: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(p) = rest.find("%<\\_FldIdx ") {
        let end = rest[p..].find(">%")? + p + 2;
        out.push(rest[p..end].to_owned());
        rest = &rest[end..];
    }
    Some(out)
}

impl FieldHost {
    /// The host's fields located in `value`, when the template and the
    /// cached field values reproduce it exactly.
    pub(super) fn locate(doc: &document::Document, host: Handle, value: &str) -> Option<Self> {
        let container = container(doc, host)?;
        let Some(ObjectType::Field(field)) = doc.objects.get(&container) else {
            return None;
        };
        let placeholders = placeholders(&field.code)?;
        let spans = document::fields::field_spans(doc, host, value)?;
        (spans.len() == placeholders.len()).then(|| Self {
            container: Some(container),
            inserted: vec![None; spans.len()],
            removed: false,
            spans,
            placeholders,
        })
    }

    pub(in crate::app) fn has_changes(&self) -> bool {
        self.removed || self.inserted.iter().any(Option::is_some)
    }

    fn locate_table_cell(
        doc: &document::Document,
        table: Handle,
        row: usize,
        column: usize,
        value: &str,
    ) -> Option<Self> {
        let EntityType::Table(table) = doc.get_entity(table)? else {
            return None;
        };
        let container = table.cell(row, column)?.contents.first()?.field_handle?;
        let ObjectType::Field(field) = doc.objects.get(&container)? else {
            return None;
        };
        if field.evaluator_id != "_text" {
            return None;
        }
        let placeholders = placeholders(&field.code)?;
        let mut children: Vec<_> = doc
            .fields
            .iter()
            .filter(|(_, f)| f.owner == container)
            .collect();
        children.sort_by_key(|(h, _)| h.value());
        let (mut spans, mut out, mut at) = (Vec::new(), String::new(), 0);
        for placeholder in &placeholders {
            let offset = field.code[at..].find(placeholder)? + at;
            out.push_str(&field.code[at..offset]);
            let index = placeholder
                .strip_prefix("%<\\_FldIdx ")?
                .strip_suffix(">%")?
                .trim()
                .parse::<usize>()
                .ok()?;
            let (handle, _) = children.get(index)?;
            let ObjectType::Field(child) = doc.objects.get(handle)? else {
                return None;
            };
            let shown = if child.value.display().is_empty() {
                &child.value_string
            } else {
                child.value.display()
            };
            let start = out.len();
            out.push_str(shown);
            spans.push(start..out.len());
            at = offset + placeholder.len();
        }
        out.push_str(&field.code[at..]);
        (out == value).then_some(Self {
            container: Some(container),
            inserted: vec![None; spans.len()],
            spans,
            placeholders,
            removed: false,
        })
    }

    /// The fields that stay after an edit from `old` to `new` (each with its
    /// index in this host and its new span), or `None` when the edit splits
    /// a field's text or cannot be placed. A field the edit covers whole
    /// is deleted. An insertion or deletion
    /// beside equal characters reads the same at several places; when those
    /// places put a field on different sides of it, `caret` (raw byte
    /// positions of the operation's caret in `new`) decides, else `None`.
    pub(super) fn track(
        &self,
        old: &str,
        new: &str,
        caret: Option<&[usize]>,
    ) -> Option<Vec<(usize, Range<usize>)>> {
        let (o, n) = (old.as_bytes(), new.as_bytes());
        let p = o.iter().zip(n).take_while(|(a, b)| a == b).count();
        let room = o.len().min(n.len()) - p;
        let s = o
            .iter()
            .rev()
            .zip(n.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let (removed, added) = (o.len() - s - p, n.len() - s - p);
        let overlaps = |a: usize, b: usize| self.spans.iter().any(|r| a < r.end && r.start < b);
        let inside = |at: usize| self.spans.iter().any(|r| r.start < at && at < r.end);
        // Every place the change can be read at: one for a replacement,
        // a slide to the left for a pure insertion or deletion.
        let mut places = vec![p];
        if removed == 0 || added == 0 {
            let (longer, k) = if removed > 0 {
                (o, removed)
            } else {
                (n, added)
            };
            let mut q = p;
            while q > 0 && longer[q - 1] == longer[q - 1 + k] {
                q -= 1;
                places.push(q);
            }
        }
        let fits = |q: usize| {
            old.is_char_boundary(q)
                && !inside(q)
                && !inside(q + removed)
                // A deletion may take a field whole; a replacement may not.
                && (added == 0 || !overlaps(q, q + removed))
        };
        let moved = |q: usize| -> Vec<(usize, Range<usize>)> {
            let shift = |x: usize| x - removed + added;
            self.spans
                .iter()
                .cloned()
                .enumerate()
                .filter_map(|(i, r)| {
                    if r.end <= q {
                        Some((i, r))
                    } else if r.start >= q + removed {
                        Some((i, shift(r.start)..shift(r.end)))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let mut readings: Vec<(usize, Vec<(usize, Range<usize>)>)> = places
            .into_iter()
            .filter(|q| fits(*q))
            .map(|q| (q, moved(q)))
            .collect();
        readings.dedup_by(|a, b| a.1 == b.1);
        if readings.len() > 1 {
            // The operation's caret ends an insertion / starts a deletion.
            let caret = caret?;
            readings.retain(|(q, _)| caret.contains(&(q + added)) || caret.contains(q));
            readings.dedup_by(|a, b| a.1 == b.1);
        }
        match readings.as_slice() {
            [(_, spans)] => Some(spans.clone()),
            _ => None,
        }
    }

    /// The template of `value`: each field's text back to its placeholder.
    fn template(&self, value: &str) -> String {
        let mut out = String::new();
        let mut at = 0;
        for (span, placeholder) in self.spans.iter().zip(&self.placeholders) {
            out.push_str(&value[at..span.start]);
            out.push_str(placeholder);
            at = span.end;
        }
        out.push_str(&value[at..]);
        out
    }

    /// Write the template and display checksum for `value` into the
    /// container, in both of the document's FIELD stores. Nothing else in
    /// any FIELD object changes.
    pub(super) fn save(&self, doc: &mut document::Document, host: Handle, value: &str) {
        let Some(container) = self
            .container
            .filter(|_| !self.removed && self.inserted.iter().all(Option::is_none))
        else {
            return self.rebuild(doc, host, value);
        };
        let template = self.template(value);
        if let Some(def) = doc.fields.get_mut(&container) {
            def.code = template.clone();
        }
        if let Some(ObjectType::Field(field)) = doc.objects.get_mut(&container) {
            field.code = template;
            for child in &mut field.child_values {
                if child.key == "ACFD_FIELDTEXT_CHECKSUM" {
                    let mut checksum = document::entities::CellValue::number(checksum(value));
                    checksum.flags = child.value.flags;
                    checksum.formatted_value.clear();
                    child.value = checksum;
                }
            }
        }
    }
}

impl FieldHost {
    /// This host with a field of `display` inserted at byte `at` of the
    /// value, which must lie between the spans.
    pub(super) fn with_inserted(&self, at: usize, display: &str, field: NewField) -> Self {
        let i = self.spans.iter().filter(|r| r.end <= at).count();
        let mut host = self.clone();
        host.spans.insert(i, at..at + display.len());
        host.placeholders.insert(i, String::new());
        host.inserted.insert(i, Some(field));
        host
    }

    /// Store the field tree for a table cell, as `rebuild` does for a text.
    pub(in crate::app) fn save_table_cell(
        &self,
        doc: &mut document::Document,
        table: Handle,
        row: usize,
        column: usize,
        value: &str,
    ) {
        if let Some((template, children)) = self.field_parts(doc, value) {
            doc.set_table_cell_field(table, row, column, &template, children);
        }
    }

    /// Replace the host's field tree by one with a FIELD for every span: the
    /// existing fields from their codes, the inserted ones as made.
    fn rebuild(&self, doc: &mut document::Document, host: Handle, value: &str) {
        if let Some((template, children)) = self.field_parts(doc, value) {
            doc.set_text_field(host, &template, children);
        }
    }

    /// The container's fields, in `%<\_FldIdx n>%` order.
    fn old_fields(&self, doc: &document::Document) -> Vec<Handle> {
        let mut old: Vec<Handle> = self
            .container
            .map(|c| {
                doc.fields
                    .iter()
                    .filter(|(_, f)| f.owner == c)
                    .map(|(h, _)| *h)
                    .collect()
            })
            .unwrap_or_default();
        old.sort_by_key(|h| h.value());
        old
    }

    /// The code and objects of an existing field from its placeholder.
    fn placeholder_code(
        doc: &document::Document,
        old: &[Handle],
        placeholder: &str,
    ) -> Option<(String, Vec<Handle>)> {
        let n = placeholder
            .strip_prefix("%<\\_FldIdx ")?
            .strip_suffix(">%")?
            .trim();
        document::fields::inline_code(doc, *old.get(n.parse::<usize>().ok()?)?)
    }

    /// The code and objects of the field at span `k`.
    fn span_code(&self, doc: &document::Document, k: usize) -> Option<(String, Vec<Handle>)> {
        match self.inserted.get(k)? {
            Some(field) => Some((field.code.clone(), field.objects.clone())),
            None => Self::placeholder_code(doc, &self.old_fields(doc), &self.placeholders[k]),
        }
    }

    fn field_parts(
        &self,
        doc: &document::Document,
        value: &str,
    ) -> Option<(String, Vec<NewField>)> {
        let old = self.old_fields(doc);
        let (mut template, mut at, mut children) = (String::new(), 0, Vec::new());
        for ((span, placeholder), inserted) in self
            .spans
            .iter()
            .zip(&self.placeholders)
            .zip(&self.inserted)
        {
            let field = inserted.clone().or_else(|| {
                let (code, objects) = Self::placeholder_code(doc, &old, placeholder)?;
                let mut field = NewField::new(code, &value[span.clone()]);
                field.objects = objects;
                Some(field)
            });
            let Some(field) = field else {
                return None;
            };
            template.push_str(&value[at..span.start]);
            template.push_str(&format!("%<\\_FldIdx {}>%", children.len()));
            children.push(field);
            at = span.end;
        }
        template.push_str(&value[at..]);
        Some((template, children))
    }
}

/// The FIELD container's checksum of its displayed text (as stored: each
/// UTF-16 unit weighted by its 1-based position).
fn checksum(text: &str) -> f64 {
    text.encode_utf16()
        .enumerate()
        .map(|(i, u)| (i as f64 + 1.0) * f64::from(u))
        .sum()
}

impl MTextEditorState {
    /// Re-locate the fields after an edit from `old` to `new`; refuse one
    /// that touches them.
    pub(super) fn track_fields(
        &self,
        old: &str,
        new: &str,
        caret: Option<&[usize]>,
    ) -> Result<Option<FieldHost>, EditError> {
        let Some(host) = &self.field_host else {
            return Ok(None);
        };
        let kept = host.track(old, new, caret).ok_or(EditError::FieldText)?;
        Ok(Some(FieldHost {
            spans: kept.iter().map(|(_, r)| r.clone()).collect(),
            placeholders: kept
                .iter()
                .map(|(k, _)| host.placeholders[*k].clone())
                .collect(),
            inserted: kept
                .iter()
                .map(|(k, _)| host.inserted[*k].clone())
                .collect(),
            removed: host.removed || kept.len() < host.spans.len(),
            ..host.clone()
        }))
    }
}

impl crate::app::OpenCADStudio {
    pub(in crate::app) fn mtext_table_cell_fields(
        &mut self,
        table: Handle,
        row: usize,
        column: usize,
    ) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        ed.field_host = FieldHost::locate_table_cell(
            &self.tabs[self.active_tab].scene.document,
            table,
            row,
            column,
            &ed.content.text(),
        );
    }

    /// Ctrl+F in the text editor: choose a field to put at the caret. Only an
    /// existing MTEXT can host one, as the field belongs to its entity.
    pub(in crate::app) fn mtext_insert_field_open(&mut self) {
        if self
            .mtext_editor
            .as_ref()
            .is_some_and(|e| e.editing.is_some())
        {
            self.open_field_dialog(crate::ui::window::field_dialog::FieldTarget::EditorInsert);
        } else if self.mtext_editor.is_some() {
            self.command_line.push_error(
                crate::t!("MTEXT: a field can be inserted only into existing text.").as_ref(),
            );
        }
    }

    /// Put the field `code` showing `display` at the caret, replacing the
    /// selection. Its FIELD objects are made when the text is saved.
    pub(in crate::app) fn mtext_insert_field(
        &mut self,
        code: String,
        objects: Vec<Handle>,
        display: &str,
    ) {
        if display.is_empty() || display.contains(['\\', '{', '}', '\n', '\r']) {
            self.command_line.push_error(
                crate::t!("MTEXT: this field's value cannot be placed in the text.").as_ref(),
            );
            return;
        }
        let mut field = NewField::new(code, display);
        field.objects = objects;
        // A selection goes first, as a deletion: one that takes a field whole
        // is allowed, a replacement over it is not.
        let selected = |app: &Self| {
            app.mtext_editor
                .as_ref()
                .is_some_and(|e| !e.selected_range().is_empty())
        };
        if selected(self) {
            self.raw_delete(false);
            if selected(self) {
                return;
            }
        }
        let before = self.mtext_editor.as_ref().map(|e| e.content.text());
        let at = std::cell::Cell::new(None);
        self.edit_raw(|ed, index| {
            let range = ed.selected_range();
            at.set(Some(index.insertion_byte(range.start)?));
            index.replace(range, display)
        });
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let (Some(at), Some(before)) = (at.get(), before) else {
            return;
        };
        if ed.content.text() == before {
            return;
        }
        let base = ed.field_host.take().unwrap_or(FieldHost {
            container: None,
            spans: Vec::new(),
            placeholders: Vec::new(),
            inserted: Vec::new(),
            removed: false,
        });
        ed.field_host = Some(base.with_inserted(at, display, field));
    }

    /// Double-click on a field's text in the editor: select the field whole
    /// and open the Field dialog on it; OK puts the chosen field in its place.
    /// False when `cell` is not in a field.
    pub(super) fn mtext_edit_field_at(&mut self, cell: usize) -> bool {
        let doc = &self.tabs[self.active_tab].scene.document;
        let Some(ed) = self.mtext_editor.as_mut() else {
            return false;
        };
        let Some(host) = ed.field_host.as_ref() else {
            return false;
        };
        let raw = ed.content.text();
        let index = super::raw_adapter::SourceIndex::new(&raw);
        let Some((k, cells)) = host.field_at_cell(&index, cell) else {
            return false;
        };
        let Some((code, objects)) = host.span_code(doc, k) else {
            return false;
        };
        ed.typing_format = Default::default();
        ed.sel_anchor = cells.start;
        ed.sel = Some((cells.start, cells.end));
        ed.caret = cells.end;
        ed.caret_blink_on = true;
        self.open_field_dialog_on(
            crate::ui::window::field_dialog::FieldTarget::EditorInsert,
            &code,
            &objects,
        );
        true
    }
}

impl super::MTextEditorState {
    /// The editor cells of each field, shaded gray as FIELDDISPLAY shades
    /// them on the canvas.
    pub(in crate::app) fn field_shading(&self) -> Vec<(usize, usize)> {
        Vec::new()
    }
}

/// The editor can place the fields of `host` in its text `value`, so the
/// text is edited there with its fields atomic.
pub(in crate::app) fn locates_fields(doc: &document::Document, host: Handle, value: &str) -> bool {
    FieldHost::locate(doc, host, value).is_some()
}

impl FieldHost {
    /// The editor cells `range` covers when it is `cell` of one field's
    /// text: that whole field, so a key deletes a field in one step.
    pub(super) fn whole_field(
        &self,
        index: &super::raw_adapter::SourceIndex,
        cell: std::ops::Range<usize>,
    ) -> Option<std::ops::Range<usize>> {
        self.field_cells(index)
            .find(|(_, r)| r.start <= cell.start && cell.end <= r.end && r.start < r.end)
            .map(|(_, r)| r)
    }

    /// The index and editor cells of the field whose text holds `cell`.
    fn field_at_cell(
        &self,
        index: &super::raw_adapter::SourceIndex,
        cell: usize,
    ) -> Option<(usize, std::ops::Range<usize>)> {
        self.field_cells(index).find(|(_, r)| r.contains(&cell))
    }

    /// Each field's editor cells, with its index.
    fn field_cells<'a>(
        &'a self,
        index: &'a super::raw_adapter::SourceIndex,
    ) -> impl Iterator<Item = (usize, std::ops::Range<usize>)> + 'a {
        let count = index.visible_len;
        let at =
            move |byte: usize| (0..=count?).find(|c| index.insertion_byte(*c).ok() == Some(byte));
        self.spans
            .iter()
            .enumerate()
            .filter_map(move |(k, r)| Some((k, at(r.start)?..at(r.end)?)))
    }
}
