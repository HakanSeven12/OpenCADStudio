// Selection sets saved by name in the drawing (NAMEDSEL).

use super::*;
use document::objects::{Dictionary, XRecord, XRecordValue};

/// The dictionary under the root dictionary that holds the sets: one XRecord
/// for each set, keyed by its name, with a handle entry for each member.
pub(crate) const NAMED_SELECTIONS_DICTIONARY: &str = "OCS_NAMED_SELECTIONS";
/// The group code of a member: a soft pointer, which neither owns the
/// object nor keeps it from being erased.
const MEMBER_CODE: i32 = 330;

/// What a recall of a set can select now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedSelectionRecall {
    /// The name as it was saved.
    pub name: String,
    /// The members that exist, are in the current space, and a pick can
    /// reach.
    pub selectable: Vec<Handle>,
    pub erased: usize,
    pub other_space: usize,
    /// Members that are hidden (layer off or frozen, object invisible or
    /// hidden), or on a locked layer while PICKLOCKED is off.
    pub unpickable: usize,
}

impl Scene {
    fn named_selections_dictionary_handle(&self) -> Option<Handle> {
        let root = self.document.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = self.document.objects.get(&root) else {
            return None;
        };
        let handle = root.get(NAMED_SELECTIONS_DICTIONARY)?;
        matches!(
            self.document.objects.get(&handle),
            Some(ObjectType::Dictionary(_))
        )
        .then_some(handle)
    }

    fn named_selections_dictionary(&self) -> Option<&Dictionary> {
        match self
            .document
            .objects
            .get(&self.named_selections_dictionary_handle()?)
        {
            Some(ObjectType::Dictionary(dictionary)) => Some(dictionary),
            _ => None,
        }
    }

    /// The saved name and the record of a set, found by name without case.
    fn named_selection_entry(&self, name: &str) -> Option<(String, Handle)> {
        self.named_selections_dictionary()?
            .iter()
            .find(|(key, handle)| {
                key.eq_ignore_ascii_case(name)
                    && matches!(
                        self.document.objects.get(handle),
                        Some(ObjectType::XRecord(_))
                    )
            })
            .map(|(key, handle)| (key.to_string(), handle))
    }

    fn named_selection_members(&self, record: Handle) -> Vec<Handle> {
        match self.document.objects.get(&record) {
            Some(ObjectType::XRecord(record)) => record
                .entries
                .iter()
                .filter_map(|entry| match entry.value {
                    XRecordValue::Handle(handle) => Some(handle),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Record an object for undo before its first change.
    fn record_named_selection_object(&mut self, handle: Handle) {
        if self.is_recording_undo() {
            let before = self.document.objects.get(&handle).cloned();
            self.record_undo_object_before(handle, before);
        }
    }

    /// Every set, sorted by name without case, with its saved members.
    pub fn named_selections(&self) -> Vec<(String, Vec<Handle>)> {
        let Some(dictionary) = self.named_selections_dictionary() else {
            return Vec::new();
        };
        let mut sets: Vec<(String, Vec<Handle>)> = dictionary
            .iter()
            .filter(|(_, handle)| {
                matches!(
                    self.document.objects.get(handle),
                    Some(ObjectType::XRecord(_))
                )
            })
            .map(|(name, handle)| (name.to_string(), self.named_selection_members(handle)))
            .collect();
        sets.sort_by_key(|(name, _)| name.to_lowercase());
        sets
    }

    /// One set by name without case: its saved name and members.
    pub fn named_selection(&self, name: &str) -> Option<(String, Vec<Handle>)> {
        let (name, record) = self.named_selection_entry(name)?;
        Some((name, self.named_selection_members(record)))
    }

    fn ensure_named_selections_dictionary(&mut self) -> Handle {
        if let Some(handle) = self.named_selections_dictionary_handle() {
            return handle;
        }
        let root = self.document.header.named_objects_dict_handle;
        let handle = self.document.allocate_handle();
        self.record_named_selection_object(root);
        self.record_named_selection_object(handle);
        let mut dictionary = Dictionary::new();
        dictionary.handle = handle;
        dictionary.owner = root;
        self.document
            .objects
            .insert(handle, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(root)) = self.document.objects.get_mut(&root) {
            root.add_entry(NAMED_SELECTIONS_DICTIONARY, handle);
        }
        handle
    }

    /// Save a set under `name`, keeping the handles of objects that are in
    /// the drawing now, once each and in order. A set saved under the same
    /// name (without case) is replaced and keeps its saved name. Returns
    /// whether a set was replaced.
    pub fn save_named_selection(&mut self, name: &str, handles: &[Handle]) -> bool {
        let mut members: Vec<Handle> = Vec::with_capacity(handles.len());
        for &handle in handles {
            if self.document.get_entity(handle).is_some() && !members.contains(&handle) {
                members.push(handle);
            }
        }
        let dictionary = self.ensure_named_selections_dictionary();
        let existing = self.named_selection_entry(name);
        let replaced = existing.is_some();
        let (name, handle) = existing.unwrap_or_else(|| {
            let handle = self.document.allocate_handle();
            self.record_named_selection_object(dictionary);
            if let Some(ObjectType::Dictionary(dictionary)) =
                self.document.objects.get_mut(&dictionary)
            {
                dictionary.add_entry(name, handle);
            }
            (name.to_string(), handle)
        });
        self.record_named_selection_object(handle);
        let mut record = XRecord::named(name);
        record.handle = handle;
        record.owner = dictionary;
        for member in members {
            record.add_handle(MEMBER_CODE, member);
        }
        self.document
            .objects
            .insert(handle, ObjectType::XRecord(record));
        replaced
    }

    /// Rename a set. The caller checks that `new` names no other set.
    /// Returns `false` when no set is named `old`.
    pub fn rename_named_selection(&mut self, old: &str, new: &str) -> bool {
        let (Some((key, record)), Some(dictionary)) = (
            self.named_selection_entry(old),
            self.named_selections_dictionary_handle(),
        ) else {
            return false;
        };
        self.record_named_selection_object(dictionary);
        self.record_named_selection_object(record);
        if let Some(ObjectType::Dictionary(dictionary)) = self.document.objects.get_mut(&dictionary)
        {
            dictionary.remove(&key);
            dictionary.add_entry(new, record);
        }
        if let Some(ObjectType::XRecord(record)) = self.document.objects.get_mut(&record) {
            record.name = new.to_string();
        }
        true
    }

    /// Delete a set; the dictionary goes with its last set. Returns `false`
    /// when no set is named `name`.
    pub fn delete_named_selection(&mut self, name: &str) -> bool {
        let (Some((key, record)), Some(dictionary)) = (
            self.named_selection_entry(name),
            self.named_selections_dictionary_handle(),
        ) else {
            return false;
        };
        self.record_named_selection_object(dictionary);
        self.record_named_selection_object(record);
        self.document.objects.remove(&record);
        let empty = match self.document.objects.get_mut(&dictionary) {
            Some(ObjectType::Dictionary(dictionary)) => {
                dictionary.remove(&key);
                dictionary.is_empty()
            }
            _ => false,
        };
        if empty {
            let root = self.document.header.named_objects_dict_handle;
            self.record_named_selection_object(root);
            if let Some(ObjectType::Dictionary(root)) = self.document.objects.get_mut(&root) {
                root.remove(NAMED_SELECTIONS_DICTIONARY);
            }
            self.document.objects.remove(&dictionary);
        }
        true
    }

    /// The members of a set that a recall selects in the current space, and
    /// how many it leaves out because they are erased, in another space, or
    /// out of reach of a pick.
    pub(crate) fn named_selection_recall(&self, name: &str) -> Option<NamedSelectionRecall> {
        let (name, members) = self.named_selection(name)?;
        let space = self.interaction_block_handle();
        let mut recall = NamedSelectionRecall {
            name,
            selectable: Vec::new(),
            erased: 0,
            other_space: 0,
            unpickable: 0,
        };
        for handle in members {
            match self.document.get_entity(handle) {
                None => recall.erased += 1,
                Some(entity) => {
                    let common = entity.common();
                    if common.owner_handle != space && !common.owner_handle.is_null() {
                        recall.other_space += 1;
                    } else if self.layer_hidden(&common.layer)
                        || self.display_entity_invisible(common)
                        || self.entity_temporarily_hidden(handle)
                        || (!crate::scene::pick_locked() && self.is_layer_locked(handle))
                    {
                        recall.unpickable += 1;
                    } else {
                        recall.selectable.push(handle);
                    }
                }
            }
        }
        Some(recall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::Line;
    use document::types::Vector3;

    fn lines(count: usize) -> (Scene, Vec<Handle>) {
        let mut scene = Scene::new();
        let handles = (0..count)
            .map(|row| {
                let y = row as f64 * 5.0;
                scene.add_entity(EntityType::Line(Line::from_points(
                    Vector3::new(0.0, y, 0.0),
                    Vector3::new(10.0, y, 0.0),
                )))
            })
            .collect();
        (scene, handles)
    }

    fn sets_dictionary(scene: &Scene) -> Option<Handle> {
        let root = scene.document.header.named_objects_dict_handle;
        match scene.document.objects.get(&root) {
            Some(ObjectType::Dictionary(root)) => root.get(NAMED_SELECTIONS_DICTIONARY),
            _ => None,
        }
    }

    #[test]
    fn a_saved_set_is_read_back_by_name_in_any_case() {
        let (mut scene, handles) = lines(2);
        scene.save_named_selection("FRAME", &handles);
        assert_eq!(
            scene.named_selection("frame"),
            Some(("FRAME".to_string(), handles.clone()))
        );
        assert!(sets_dictionary(&scene).is_some());
    }

    #[test]
    fn saving_under_a_used_name_replaces_the_set() {
        let (mut scene, handles) = lines(2);
        assert!(!scene.save_named_selection("A", &handles[..1]));
        assert!(scene.save_named_selection("a", &handles[1..]));
        assert_eq!(
            scene.named_selections(),
            vec![("A".to_string(), vec![handles[1]])]
        );
        let dictionary = sets_dictionary(&scene).unwrap();
        let Some(ObjectType::Dictionary(dictionary)) = scene.document.objects.get(&dictionary)
        else {
            panic!("the sets dictionary is a dictionary");
        };
        assert_eq!(dictionary.len(), 1);
        let xrecords = scene
            .document
            .objects
            .values()
            .filter(|object| matches!(object, ObjectType::XRecord(_)))
            .count();
        assert_eq!(xrecords, 1, "the replaced set's record is gone");
    }

    #[test]
    fn a_set_keeps_only_handles_of_objects_in_the_drawing() {
        let (mut scene, handles) = lines(1);
        let missing = scene.document.allocate_handle();
        scene.save_named_selection("A", &[handles[0], missing]);
        assert_eq!(scene.named_selection("A").unwrap().1, vec![handles[0]]);
    }

    #[test]
    fn rename_and_delete_change_only_that_set() {
        let (mut scene, handles) = lines(2);
        scene.save_named_selection("A", &handles[..1]);
        scene.save_named_selection("B", &handles[1..]);
        assert!(scene.rename_named_selection("a", "C"));
        assert!(scene.delete_named_selection("b"));
        assert!(!scene.delete_named_selection("ghost"));
        assert!(!scene.rename_named_selection("ghost", "D"));
        assert_eq!(
            scene.named_selections(),
            vec![("C".to_string(), vec![handles[0]])]
        );

        let dictionary = sets_dictionary(&scene).unwrap();
        assert!(scene.delete_named_selection("C"));
        assert_eq!(sets_dictionary(&scene), None);
        assert!(!scene.document.objects.contains_key(&dictionary));
    }

    #[test]
    fn the_sets_are_listed_by_name_without_case() {
        let (mut scene, handles) = lines(1);
        for name in ["beta", "Alpha", "GAMMA"] {
            scene.save_named_selection(name, &handles);
        }
        let names: Vec<String> = scene
            .named_selections()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["Alpha", "beta", "GAMMA"]);
    }

    #[test]
    fn recall_leaves_out_erased_and_other_space_members() {
        let (mut scene, handles) = lines(3);
        let paper = scene
            .document
            .block_records
            .get("*Paper_Space")
            .expect("a new drawing has a paper space")
            .handle;
        scene
            .document
            .get_entity_mut(handles[1])
            .unwrap()
            .common_mut()
            .owner_handle = paper;
        scene.save_named_selection("A", &handles);
        scene.erase_entities(&[handles[2]]);

        let recall = scene.named_selection_recall("a").unwrap();
        assert_eq!(recall.name, "A");
        assert_eq!(recall.selectable, vec![handles[0]]);
        assert_eq!(recall.erased, 1);
        assert_eq!(recall.other_space, 1);
        assert!(scene.named_selection_recall("ghost").is_none());
    }

    #[test]
    fn recall_leaves_out_hidden_and_locked_members() {
        let (mut scene, handles) = lines(5);
        let mut frozen = document::tables::Layer::new("FROZEN");
        frozen.flags.frozen = true;
        let _ = scene.document.layers.add(frozen);
        let mut locked = document::tables::Layer::new("LOCKED");
        locked.flags.locked = true;
        let _ = scene.document.layers.add(locked);
        for (index, layer) in [(1, "FROZEN"), (3, "LOCKED")] {
            let entity = scene.document.get_entity_mut(handles[index]).unwrap();
            entity.common_mut().layer = layer.to_string();
        }
        let entity = scene.document.get_entity_mut(handles[2]).unwrap();
        entity.common_mut().invisible = true;
        scene.select_entity(handles[4], false);
        scene.hide_selected();
        scene.deselect_all();
        scene.save_named_selection("A", &handles);

        let recall = scene.named_selection_recall("A").unwrap();
        assert_eq!(recall.selectable, vec![handles[0], handles[3]]);
        assert_eq!(recall.unpickable, 3);

        crate::scene::set_pick_locked(false);
        let recall = scene.named_selection_recall("A").unwrap();
        crate::scene::set_pick_locked(true);
        assert_eq!(recall.selectable, vec![handles[0]]);
        assert_eq!(recall.unpickable, 4);
    }
}
