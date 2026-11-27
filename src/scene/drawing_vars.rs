//! System variables kept in the drawing's variable dictionary
//! (`DICTIONARYVAR` records), read and written by name.

use document::Document;

/// A system variable stored in the drawing's variable dictionary.
pub(crate) fn drawing_variable(doc: &Document, name: &str) -> Option<String> {
    doc.drawing_variable(name).map(str::to_string)
}

/// Write a drawing variable, creating the variable dictionary and record when
/// needed so new drawings preserve the value too.
pub(crate) fn set_drawing_variable(doc: &mut Document, name: &str, value: &str) {
    // The codec follows the header's root pointer; repair a stale one first.
    crate::scene::annotative::root_named_dict_handle(doc);
    doc.set_drawing_variable(name, value);
}

/// The layout tab that was active when the drawing was saved — the `CTAB`
/// system variable (stored in the variable dictionary as a `DICTIONARYVAR`).
/// `None` when the file recorded no current tab.
pub fn saved_active_layout(doc: &Document) -> Option<String> {
    drawing_variable(doc, "CTAB").filter(|s| !s.is_empty())
}

/// Record `name` as the active layout tab (`CTAB`) so the next save round-trips
/// which space was open. Updates the existing `CTAB` variable in place, or
/// creates one under the root named-object dictionary when the drawing never
/// carried it (e.g. a document authored here from scratch) — otherwise the exact
/// paper layout would be lost and reopening fell back to the first paper tab.
pub fn set_saved_active_layout(doc: &mut Document, name: &str) {
    set_drawing_variable(doc, "CTAB", name);
}
