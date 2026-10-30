//! Editor transactions over the authoritative raw source. Derived documents and
//! geometry never become a second independently editable document model.
use super::raw_adapter::SourceIndex;
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
