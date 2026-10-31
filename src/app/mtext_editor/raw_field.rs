//! Text around native FIELDs. The editor edits the displayed value, where the
//! codec's `field_spans` locates each field's text; those spans are atomic.
//! Every FIELD object stays as it is: on save only the host's `_text`
//! container gets the new template (each span back to its `%<\_FldIdx n>%`)
//! and the checksum of the new displayed value.
use super::raw_adapter::EditError;
use super::*;
use codec::objects::ObjectType;
use std::ops::Range;

/// A FIELD host's field texts in the edited value, and their placeholders.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldHost {
    container: Handle,
    /// Byte ranges in the editor's value, in template order.
    pub spans: Vec<Range<usize>>,
    placeholders: Vec<String>,
}

/// The host's `_text` container: xdictionary → ACAD_FIELD → TEXT.
fn container(doc: &codec::Document, host: Handle) -> Option<Handle> {
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
    pub(super) fn locate(doc: &codec::Document, host: Handle, value: &str) -> Option<Self> {
        let container = container(doc, host)?;
        let Some(ObjectType::Field(field)) = doc.objects.get(&container) else {
            return None;
        };
        let placeholders = placeholders(&field.code)?;
        let spans = codec::fields::field_spans(doc, host, value)?;
        (spans.len() == placeholders.len()).then_some(Self {
            container,
            spans,
            placeholders,
        })
    }

    /// The spans after an edit from `old` to `new`, or `None` when the edit
    /// touches a field's text or cannot be placed. An insertion or deletion
    /// beside equal characters reads the same at several places; when those
    /// places put a field on different sides of it, `caret` (raw byte
    /// positions of the operation's caret in `new`) decides, else `None`.
    pub(super) fn track(
        &self,
        old: &str,
        new: &str,
        caret: Option<&[usize]>,
    ) -> Option<Vec<Range<usize>>> {
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
        let inside = |at: usize| self.spans.iter().any(|r| r.start < at && at < r.end);
        let overlaps = |a: usize, b: usize| self.spans.iter().any(|r| a < r.end && r.start < b);
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
                && !overlaps(q, q + removed)
        };
        let moved = |q: usize| -> Vec<Range<usize>> {
            let shift = |x: usize| x - removed + added;
            self.spans
                .iter()
                .map(|r| {
                    if r.end <= q {
                        r.clone()
                    } else {
                        shift(r.start)..shift(r.end)
                    }
                })
                .collect()
        };
        let mut readings: Vec<(usize, Vec<Range<usize>>)> = places
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
    pub(super) fn save(&self, doc: &mut codec::Document, value: &str) {
        let template = self.template(value);
        if let Some(def) = doc.fields.get_mut(&self.container) {
            def.code = template.clone();
        }
        if let Some(ObjectType::Field(field)) = doc.objects.get_mut(&self.container) {
            field.code = template;
            for child in &mut field.child_values {
                if child.key == "ACFD_FIELDTEXT_CHECKSUM" {
                    let mut checksum = codec::entities::CellValue::number(checksum(value));
                    checksum.flags = child.value.flags;
                    checksum.formatted_value.clear();
                    child.value = checksum;
                }
            }
        }
    }
}

/// The FIELD container's checksum of its displayed text (AutoCAD's: each
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
        let spans = host.track(old, new, caret).ok_or(EditError::FieldText)?;
        Ok(Some(FieldHost {
            spans,
            ..host.clone()
        }))
    }
}
