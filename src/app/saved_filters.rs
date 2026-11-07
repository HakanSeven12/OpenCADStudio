//! Saved selection filters, kept in the drawing so they travel with it
//! (#2611): a dictionary `OCS_SELECTION_FILTERS` in the named objects
//! dictionary, one XRECORD per filter (`1` kind, `90` version, `300` name,
//! then the filter as JSON in `3` chunks). Records of an unknown version are
//! left as they are and not offered.

use super::{Message, OpenCADStudio, QSelectResult, QSelectScope};
use crate::filter::Filter;
use document::objects::{Dictionary, ObjectType, XRecord, XRecordEntry, XRecordValue};
use document::{Document, Handle};
use iced::Task;

const DICTIONARY: &str = "OCS_SELECTION_FILTERS";
const KIND: &str = "OcsSelectionFilter";
const VERSION: i32 = 1;
/// Text entries stay short so every DWG and DXF reader takes them.
const CHUNK: usize = 240;

fn dictionary(document: &Document) -> Option<Handle> {
    let root = document.named_objects_dictionary()?;
    let handle = root.get(DICTIONARY)?;
    matches!(
        document.objects.get(&handle),
        Some(ObjectType::Dictionary(_))
    )
    .then_some(handle)
}

fn parse(xrecord: &XRecord) -> Option<(String, Filter)> {
    let mut entries = xrecord.entries.iter();
    let kind = entries.next()?;
    if kind.code != 1 || kind.value.as_string() != Some(KIND) {
        return None;
    }
    let mut version = None;
    let mut name = None;
    let mut json = String::new();
    for entry in entries {
        match (entry.code, &entry.value) {
            (90, XRecordValue::Int32(v)) => version = Some(*v),
            (300, XRecordValue::String(n)) => name = Some(n.clone()),
            (3, XRecordValue::String(part)) => json.push_str(part),
            _ => {}
        }
    }
    if version != Some(VERSION) {
        return None;
    }
    let filter = serde_json::from_str(&json).ok()?;
    Some((name?, filter))
}

/// The drawing's saved filters by name, in the order they were saved.
pub(crate) fn saved_filters(document: &Document) -> Vec<(String, Filter)> {
    let Some(handle) = dictionary(document) else {
        return Vec::new();
    };
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&handle) else {
        return Vec::new();
    };
    dictionary
        .entries
        .iter()
        .filter_map(|(_, record)| match document.objects.get(record) {
            Some(ObjectType::XRecord(xrecord)) => parse(xrecord),
            _ => None,
        })
        .collect()
}

pub(crate) fn find_saved_filter(document: &Document, name: &str) -> Option<(String, Filter)> {
    saved_filters(document)
        .into_iter()
        .find(|(saved, _)| same_name(saved, name))
}

fn entries(name: &str, filter: &Filter) -> Vec<XRecordEntry> {
    let json = serde_json::to_string(filter).unwrap_or_default();
    let mut entries = vec![
        XRecordEntry::string(1, KIND),
        XRecordEntry::new(90, XRecordValue::Int32(VERSION)),
        XRecordEntry::string(300, name),
    ];
    // Chunks are measured as an older DXF writes them, where a non-ASCII
    // character becomes a `\U+XXXX` escape.
    let mut part = String::new();
    let mut length = 0;
    for c in json.chars() {
        let width = match c as u32 {
            0..=0x7F => 1,
            0x80..=0xFFFF => 7,
            _ => 14,
        };
        if length + width > CHUNK {
            entries.push(XRecordEntry::string(3, &part));
            part.clear();
            length = 0;
        }
        part.push(c);
        length += width;
    }
    if !part.is_empty() {
        entries.push(XRecordEntry::string(3, &part));
    }
    entries
}

/// Saved filter names compare without case, in any script.
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn ensure_dictionary(document: &mut Document) -> Handle {
    if let Some(handle) = dictionary(document) {
        return handle;
    }
    let root = document.ensure_named_objects_dictionary();
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    dictionary.hard_owner = true;
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(root) = document.named_objects_dictionary_mut() {
        root.add_entry(DICTIONARY, handle);
    }
    handle
}

/// Save `filter` as `name`, replacing a filter of that name (any case).
pub(crate) fn store_saved_filter(document: &mut Document, name: &str, filter: &Filter) {
    let dictionary = ensure_dictionary(document);
    let existing = match document.objects.get(&dictionary) {
        Some(ObjectType::Dictionary(d)) => d
            .entries
            .iter()
            .find(|(key, _)| same_name(key, name))
            .map(|(_, handle)| *handle),
        _ => None,
    };
    if let Some(handle) = existing {
        if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&handle) {
            xrecord.entries = entries(name, filter);
            xrecord.raw_data.clear();
            xrecord.raw_dwg_data = None;
        }
        if let Some(ObjectType::Dictionary(d)) = document.objects.get_mut(&dictionary) {
            if let Some(entry) = d.entries.iter_mut().find(|(_, h)| *h == handle) {
                entry.0 = name.to_string();
            }
        }
        return;
    }
    let handle = document.allocate_handle();
    let mut xrecord = XRecord::new();
    xrecord.handle = handle;
    xrecord.owner = dictionary;
    xrecord.entries = entries(name, filter);
    document
        .objects
        .insert(handle, ObjectType::XRecord(xrecord));
    if let Some(ObjectType::Dictionary(d)) = document.objects.get_mut(&dictionary) {
        d.add_entry(name, handle);
    }
}

pub(crate) fn delete_saved_filter(document: &mut Document, name: &str) -> bool {
    let Some(dictionary) = dictionary(document) else {
        return false;
    };
    let Some(ObjectType::Dictionary(d)) = document.objects.get_mut(&dictionary) else {
        return false;
    };
    let Some(at) = d
        .entries
        .iter()
        .position(|(key, _)| same_name(key, name))
    else {
        return false;
    };
    let (_, handle) = d.entries.remove(at);
    document.objects.remove(&handle);
    true
}

impl OpenCADStudio {
    pub(in crate::app) fn saved_filter_names(&self, i: usize) -> Vec<String> {
        saved_filters(&self.tabs[i].scene.document)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// Save or delete as one undo step.
    pub(in crate::app) fn edit_saved_filters(
        &mut self,
        i: usize,
        label: &str,
        edit: impl FnOnce(&mut Document),
    ) {
        self.push_undo_snapshot(i, label);
        edit(&mut self.tabs[i].scene.document);
        self.tabs[i].dirty = true;
    }

    /// `FILTER <name>`: apply a saved filter to the current space without
    /// the dialog; an unknown name lists the saved ones.
    pub(in crate::app) fn apply_saved_filter(&mut self, name: &str) -> Task<Message> {
        let i = self.active_tab;
        let name = name.trim().trim_matches('"');
        let Some((saved, filter)) = find_saved_filter(&self.tabs[i].scene.document, name) else {
            let names = self.saved_filter_names(i);
            self.command_line
                .push_error(crate::tf!("FILTER: no saved filter named \"{name}\".").as_ref());
            if !names.is_empty() {
                self.command_line.push_info(&names.join(", "));
            }
            return Task::none();
        };
        let found = self.tabs[i]
            .scene
            .filter_matches(QSelectScope::CurrentSpace, &filter, false);
        let selected = self.tabs[i].scene.apply_found(found, QSelectResult::New);
        self.command_line.push_output(
            crate::tf!("FILTER: {} object(s) selected ({}).", selected, saved).as_ref(),
        );
        self.refresh_properties();
        self.unfocus_widgets()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{Condition, Item, Match, Op};

    fn walls() -> Filter {
        Filter {
            root: Match::Any,
            items: vec![Item::Condition(Condition {
                negate: true,
                type_filter: Some("Line".into()),
                property: Some("layer".into()),
                op: Op::Eq,
                value: "FAL*".repeat(80),
            })],
        }
    }

    #[test]
    fn saved_filters_round_trip_through_the_drawing() {
        let mut document = Document::new();
        assert!(saved_filters(&document).is_empty());
        store_saved_filter(&mut document, "Walls", &walls());
        store_saved_filter(&mut document, "Doors", &Filter::default());
        let saved = saved_filters(&document);
        assert_eq!(saved.len(), 2);
        assert_eq!(
            saved[0],
            ("Walls".to_string(), walls()),
            "a long filter survives the chunks"
        );
        // The same name, any case, replaces it.
        store_saved_filter(&mut document, "WALLS", &Filter::default());
        assert_eq!(saved_filters(&document).len(), 2);
        assert_eq!(
            find_saved_filter(&document, "walls").unwrap().1,
            Filter::default()
        );
        assert!(delete_saved_filter(&mut document, "doors"));
        assert_eq!(saved_filters(&document).len(), 1);
    }

    #[test]
    fn filter_name_applies_without_the_dialog_and_saving_is_undone() {
        use document::entities::{Circle, Line};
        use document::types::Vector3;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let _ = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Circle(Circle::new()));
        let circles = Filter {
            root: Match::All,
            items: vec![Item::Condition(Condition {
                type_filter: Some("Circle".into()),
                ..Default::default()
            })],
        };
        app.edit_saved_filters(i, "FILTER SAVE", |document| {
            store_saved_filter(document, "Circles", &circles)
        });
        let _ = app.apply_saved_filter("circles");
        assert_eq!(app.tabs[i].scene.selected.len(), 1);
        app.command_line.last_error = None;
        let _ = app.apply_saved_filter("nothing");
        assert!(app.command_line.last_error.is_some());
        app.undo_active_tab();
        assert!(
            app.saved_filter_names(i).is_empty(),
            "saving is one undo step"
        );
    }
}
