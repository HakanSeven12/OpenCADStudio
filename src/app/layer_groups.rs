//! Layer group filters: named sets of layers for the Layer Manager, kept the
//! way DWG keeps them so other programs read them too — one
//! `AcLyLayerGroup` XRECORD per group (`300` name, a `330` reference per
//! layer) under anonymous `*A<n>` keys in the `ACLYDICTIONARY` of the layer
//! table's extension dictionary, beside the property filters stored there.

use document::objects::{
    Dictionary, DictionaryCloningFlags, ObjectType, XRecord, XRecordEntry, XRecordValue,
};
use document::{Document, Handle};

const DICTIONARY: &str = "ACLYDICTIONARY";
const GROUP: &str = "AcLyLayerGroup";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerGroup {
    pub name: String,
    pub layers: Vec<Handle>,
}

pub(crate) fn filter_dictionary(document: &Document) -> Option<Handle> {
    let extension = document.extension_dictionary_handle(document.layers.handle())?;
    let Some(ObjectType::Dictionary(extension)) = document.objects.get(&extension) else {
        return None;
    };
    extension
        .entries
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(DICTIONARY))
        .map(|(_, handle)| *handle)
        .filter(|handle| matches!(document.objects.get(handle), Some(ObjectType::Dictionary(_))))
}

fn parse(xrecord: &XRecord) -> Option<LayerGroup> {
    let mut entries = xrecord.entries.iter();
    let kind = entries.next()?;
    if kind.code != 1 || kind.value.as_string() != Some(GROUP) {
        return None;
    }
    let mut group = LayerGroup {
        name: String::new(),
        layers: Vec::new(),
    };
    for entry in entries {
        match (entry.code, &entry.value) {
            (300, XRecordValue::String(name)) if group.name.is_empty() => group.name = name.clone(),
            (330, XRecordValue::Handle(handle)) if !handle.is_null() => group.layers.push(*handle),
            _ => {}
        }
    }
    (!group.name.is_empty()).then_some(group)
}

/// The drawing's layer groups, in dictionary order.
pub(crate) fn layer_groups(document: &Document) -> Vec<LayerGroup> {
    let Some(dictionary) = filter_dictionary(document) else {
        return Vec::new();
    };
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&dictionary) else {
        return Vec::new();
    };
    dictionary
        .entries
        .iter()
        .filter_map(|(_, handle)| match document.objects.get(handle) {
            Some(ObjectType::XRecord(xrecord)) => parse(xrecord),
            _ => None,
        })
        .collect()
}

/// The XRECORD holding group `name`, with its dictionary.
fn find(document: &Document, name: &str) -> Option<(Handle, Handle)> {
    let dictionary = filter_dictionary(document)?;
    let Some(ObjectType::Dictionary(entries)) = document.objects.get(&dictionary) else {
        return None;
    };
    entries.entries.iter().find_map(|(_, handle)| match document.objects.get(handle) {
        Some(ObjectType::XRecord(xrecord))
            if parse(xrecord).is_some_and(|group| group.name.eq_ignore_ascii_case(name)) =>
        {
            Some((dictionary, *handle))
        }
        _ => None,
    })
}

pub(crate) fn ensure_filter_dictionary(document: &mut Document) -> Handle {
    if let Some(handle) = filter_dictionary(document) {
        return handle;
    }
    let table = document.layers.handle();
    let extension = document.extension_dictionary_handle(table).unwrap_or_else(|| {
        let handle = document.allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = handle;
        dictionary.owner = table;
        dictionary.hard_owner = true;
        document.objects.insert(handle, ObjectType::Dictionary(dictionary));
        handle
    });
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = extension;
    dictionary.reactors = vec![extension];
    dictionary.duplicate_cloning = 1;
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(extension)) = document.objects.get_mut(&extension) {
        extension.entries.push((DICTIONARY.to_string(), handle));
    }
    handle
}

/// Create group `name`, or replace its layers when it exists.
pub(crate) fn store_layer_group(document: &mut Document, name: &str, layers: &[Handle]) {
    let mut entries = vec![
        XRecordEntry::string(1, GROUP),
        XRecordEntry::new(90, XRecordValue::Int32(1)),
        XRecordEntry::string(300, name),
    ];
    entries.extend(layers.iter().map(|handle| XRecordEntry::new(330, XRecordValue::Handle(*handle))));
    if let Some((_, handle)) = find(document, name) {
        if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&handle) {
            xrecord.entries = entries;
            xrecord.raw_data.clear();
            xrecord.raw_dwg_data = None;
        }
        return;
    }
    let dictionary = ensure_filter_dictionary(document);
    let handle = document.allocate_handle();
    let mut xrecord = XRecord::new();
    xrecord.handle = handle;
    xrecord.owner = dictionary;
    xrecord.reactors = vec![dictionary];
    xrecord.cloning_flags = DictionaryCloningFlags::KeepExisting;
    xrecord.entries = entries;
    document.objects.insert(handle, ObjectType::XRecord(xrecord));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        // Filters sit under anonymous keys: the next free *A<n>.
        let next = (1..)
            .map(|n| format!("*A{n}"))
            .find(|key| !dictionary.entries.iter().any(|(taken, _)| taken.eq_ignore_ascii_case(key)))
            .unwrap_or_default();
        dictionary.entries.push((next, handle));
    }
}

/// Remove group `name`; its layers stay.
pub(crate) fn delete_layer_group(document: &mut Document, name: &str) -> bool {
    let Some((dictionary, handle)) = find(document, name) else {
        return false;
    };
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.entries.retain(|(_, entry)| *entry != handle);
    }
    document.objects.remove(&handle);
    true
}

/// Rename group `old` to `new`.
pub(crate) fn rename_layer_group(document: &mut Document, old: &str, new: &str) -> bool {
    let Some((_, handle)) = find(document, old) else {
        return false;
    };
    if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&handle) {
        for entry in &mut xrecord.entries {
            if entry.code == 300 {
                entry.value = XRecordValue::String(new.to_string());
                break;
            }
        }
        xrecord.raw_data.clear();
        xrecord.raw_dwg_data = None;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_round_trip_through_the_layer_dictionary() {
        let mut document = Document::new();
        let zero = document.layers.get("0").unwrap().handle;
        assert!(layer_groups(&document).is_empty());
        store_layer_group(&mut document, "Walls", &[zero]);
        store_layer_group(&mut document, "Doors", &[]);
        let groups = layer_groups(&document);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], LayerGroup { name: "Walls".into(), layers: vec![zero] });
        // The same name updates in place under its key.
        store_layer_group(&mut document, "walls", &[]);
        assert!(layer_groups(&document)[0].layers.is_empty());
        assert!(rename_layer_group(&mut document, "Doors", "Openings"));
        assert!(delete_layer_group(&mut document, "Walls"));
        let names: Vec<_> = layer_groups(&document).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Openings"]);
    }

    #[test]
    fn groups_survive_dwg_and_dxf() {
        let mut document = Document::new();
        let zero = document.layers.get("0").unwrap().handle;
        store_layer_group(&mut document, "Walls", &[zero]);
        let dwg = document::DwgWriter::write_to_vec(&document).unwrap();
        let from_dwg = document::DwgReader::from_stream(std::io::Cursor::new(dwg)).read().unwrap();
        let dxf = document::DxfWriter::new(&document).write_to_vec().unwrap();
        let from_dxf = document::DxfReader::from_reader(std::io::Cursor::new(dxf))
            .unwrap()
            .read()
            .unwrap();
        for reopened in [from_dwg, from_dxf] {
            assert_eq!(
                layer_groups(&reopened),
                [LayerGroup { name: "Walls".into(), layers: vec![zero] }]
            );
        }
    }
}

use super::{Message, OpenCADStudio};
use iced::Task;

impl OpenCADStudio {
    /// The drawing's groups by layer name, for the Layer Manager.
    pub(in crate::app) fn layer_group_names(&self, i: usize) -> Vec<(String, Vec<String>)> {
        let document = &self.tabs[i].scene.document;
        let name_of = |handle: &Handle| {
            document
                .layers
                .iter()
                .find(|layer| layer.handle == *handle)
                .map(|layer| layer.name.clone())
        };
        layer_groups(document)
            .into_iter()
            .map(|group| (group.name, group.layers.iter().filter_map(name_of).collect()))
            .collect()
    }

    fn layer_handles(&self, i: usize, names: &[String]) -> Vec<Handle> {
        let layers = &self.tabs[i].scene.document.layers;
        names
            .iter()
            .filter_map(|name| layers.get(name).map(|layer| layer.handle))
            .collect()
    }

    /// Change the groups as one undo step, then show the result.
    fn edit_layer_groups(&mut self, i: usize, label: &str, edit: impl FnOnce(&mut Document)) {
        self.push_undo_snapshot(i, label);
        edit(&mut self.tabs[i].scene.document);
        self.tabs[i].dirty = true;
        self.refresh_layer_panel();
    }

    /// Group `name` holding `layers`, or with them added (`add`) / taken out.
    pub(in crate::app) fn set_layer_group(&mut self, i: usize, name: &str, layers: &[String], add: bool) {
        let mut members = layer_groups(&self.tabs[i].scene.document)
            .into_iter()
            .find(|group| group.name.eq_ignore_ascii_case(name))
            .map(|group| group.layers)
            .unwrap_or_default();
        for handle in self.layer_handles(i, layers) {
            if add && !members.contains(&handle) {
                members.push(handle);
            } else if !add {
                members.retain(|member| *member != handle);
            }
        }
        let name = name.to_string();
        self.edit_layer_groups(i, "LAYERGROUP", |document| {
            store_layer_group(document, &name, &members)
        });
    }

    /// Delete group `name` (its layers stay), straight, not through the
    /// command line where a name with a space would split.
    pub(in crate::app) fn delete_layer_group_named(&mut self, i: usize, name: &str) {
        let name = name.to_string();
        self.edit_layer_groups(i, "LAYERGROUP", |document| {
            delete_layer_group(document, &name);
        });
        self.command_line
            .push_output(&crate::tr!("layer-groups", "deleted", name = name));
    }

    /// Rename group `old` to `new`.
    pub(in crate::app) fn rename_layer_group_named(&mut self, i: usize, old: &str, new: &str) {
        let (old, new) = (old.to_string(), new.to_string());
        self.edit_layer_groups(i, "LAYERGROUP", |document| {
            rename_layer_group(document, &old, &new);
        });
        self.command_line
            .push_output(&crate::tr!("layer-groups", "updated", name = new));
    }

    /// A free "Group <n>" name.
    pub(in crate::app) fn next_layer_group_name(&self, i: usize) -> String {
        let taken = self.layer_group_names(i);
        (1..)
            .map(|n| crate::tr!("layer-groups", "default-name", n = n))
            .find(|name| !taken.iter().any(|(group, _)| group.eq_ignore_ascii_case(name)))
            .unwrap_or_default()
    }

    /// LAYERGROUP NEW <name> [layers] | ADD <name> <layers> |
    /// REMOVE <name> <layers> | RENAME <old> <new> | DELETE <name> | LIST.
    pub(in crate::app) fn dispatch_layer_group(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let mut tokens = cmd.split_whitespace();
        if !tokens.next()?.eq_ignore_ascii_case("LAYERGROUP") {
            return None;
        }
        let verb = tokens.next().unwrap_or("").to_ascii_uppercase();
        let name = tokens.next().unwrap_or("").to_string();
        let rest: Vec<String> = tokens.map(str::to_string).collect();
        let exists = self
            .layer_group_names(i)
            .iter()
            .any(|(group, _)| group.eq_ignore_ascii_case(&name));
        let report = match verb.as_str() {
            "LIST" | "L" | "" => {
                let groups = self.layer_group_names(i);
                if groups.is_empty() {
                    crate::tr!("layer-groups", "none")
                } else {
                    groups
                        .iter()
                        .map(|(group, layers)| format!("{group}: {}", layers.join(", ")))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            _ if name.is_empty() => crate::tr!("layer-groups", "usage"),
            "NEW" | "N" if exists => crate::tr!("layer-groups", "exists", name = name.clone()),
            "NEW" | "N" => {
                self.set_layer_group(i, &name, &rest, true);
                crate::tr!("layer-groups", "created", name = name.clone())
            }
            _ if !exists => crate::tr!("layer-groups", "not-found", name = name.clone()),
            "ADD" | "A" | "REMOVE" | "R" => {
                self.set_layer_group(i, &name, &rest, verb.starts_with('A'));
                crate::tr!("layer-groups", "updated", name = name.clone())
            }
            "RENAME" | "RE" => match rest.first() {
                Some(new) => {
                    let new = new.clone();
                    self.edit_layer_groups(i, "LAYERGROUP", |document| {
                        rename_layer_group(document, &name, &new);
                    });
                    crate::tr!("layer-groups", "updated", name = new)
                }
                None => crate::tr!("layer-groups", "usage"),
            },
            "DELETE" | "D" => {
                self.edit_layer_groups(i, "LAYERGROUP", |document| {
                    delete_layer_group(document, &name);
                });
                crate::tr!("layer-groups", "deleted", name = name.clone())
            }
            _ => crate::tr!("layer-groups", "usage"),
        };
        self.command_line.push_output(&report);
        Some(Task::none())
    }
}
