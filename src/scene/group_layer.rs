// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    // ── Group helpers ──────────────────────────────────────────────────────

    pub fn groups(&self) -> impl Iterator<Item = &acadrust::objects::Group> {
        self.document.objects.values().filter_map(|obj| match obj {
            ObjectType::Group(g) => Some(g),
            _ => None,
        })
    }

    /// Returns the names of all groups that contain `handle`.
    pub fn group_names_for_entity(&self, handle: Handle) -> Vec<String> {
        self.groups()
            .filter(|g| g.contains(handle))
            .map(|g| g.name.clone())
            .collect()
    }

    /// Creates a named group from the given handles and registers it in the group dictionary.
    pub fn create_group(&mut self, name: String, handles: Vec<Handle>) -> Handle {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let mut group = acadrust::objects::Group::new(&name);
        group.handle = self.document.allocate_handle();
        group.owner = group_dict_handle;
        group.add_entities(handles);
        let gh = group.handle;
        self.document.objects.insert(gh, ObjectType::Group(group));
        if let Some(ObjectType::Dictionary(dict)) =
            self.document.objects.get_mut(&group_dict_handle)
        {
            dict.add_entry(&name, gh);
        }
        gh
    }

    /// Recreate every *live* group whose full membership was copied.
    ///
    /// Partial group copies intentionally remain ungrouped: copying one member
    /// out of a group should not create a new one-member fragment. When the
    /// whole source group is in `handle_map`, the new handles get their own
    /// Group object so later selection/editing treats the copy as a group too.
    ///
    /// The in-drawing COPY / ARRAY path: the source groups still live in this
    /// document, so gather them here and hand them to [`Scene::recreate_groups`].
    /// The clipboard paste path snapshots its groups instead (they may come from
    /// another drawing) and calls `recreate_groups` directly — one shared body
    /// so all copy routes preserve groups identically.
    pub fn copy_complete_groups(
        &mut self,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) -> usize {
        if handle_map.is_empty() {
            return 0;
        }
        let sources: Vec<_> = self
            .document
            .objects
            .values()
            .filter_map(|obj| match obj {
                ObjectType::Group(g)
                    if !g.entities.is_empty()
                        && g.entities.iter().all(|h| handle_map.contains_key(h)) =>
                {
                    Some(g.clone())
                }
                _ => None,
            })
            .collect();
        self.recreate_groups(sources, handle_map)
    }

    /// Recreate each group in `sources` in this document, remapping its member
    /// handles through `handle_map` (source → new). Each recreated group gets a
    /// fresh handle, a unique `NAME_COPYn`, and a group-dictionary entry.
    /// Members absent from the map are dropped; a group left with no members is
    /// skipped.
    ///
    /// Shared by the in-drawing COPY/ARRAY path ([`Scene::copy_complete_groups`],
    /// live source groups) and the clipboard paste path (groups snapshotted into
    /// the clipboard at copy time), so a fully-copied group stays grouped whether
    /// the copy lands in the same drawing or a different file.
    pub fn recreate_groups(
        &mut self,
        sources: Vec<acadrust::objects::Group>,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) -> usize {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let mut copied = 0;
        let mut dictionary_recorded = false;
        for source in sources {
            let entities: Vec<Handle> = source
                .entities
                .iter()
                .filter_map(|h| handle_map.get(h).copied())
                .collect();
            if entities.is_empty() {
                continue;
            }
            let name = self.unique_group_copy_name(&source.name);
            let mut group = source;
            group.handle = self.document.allocate_handle();
            group.owner = group_dict_handle;
            group.name = name.clone();
            group.entities = entities;
            let gh = group.handle;
            if self.is_recording_undo() {
                if !dictionary_recorded {
                    let before = self.document.objects.get(&group_dict_handle).cloned();
                    self.record_undo_object_before(group_dict_handle, before);
                    dictionary_recorded = true;
                }
                self.record_undo_object_before(gh, None);
            }
            self.document.objects.insert(gh, ObjectType::Group(group));
            if let Some(ObjectType::Dictionary(dict)) =
                self.document.objects.get_mut(&group_dict_handle)
            {
                dict.add_entry(&name, gh);
            }
            copied += 1;
        }
        copied
    }

    fn unique_group_copy_name(&self, source: &str) -> String {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let exists = |name: &str| {
            self.document
                .objects
                .get(&group_dict_handle)
                .and_then(|obj| match obj {
                    ObjectType::Dictionary(dict) => Some(
                        dict.entries
                            .iter()
                            .any(|(entry, _)| entry.eq_ignore_ascii_case(name)),
                    ),
                    _ => None,
                })
                .unwrap_or(false)
        };
        for n in 1.. {
            let candidate = format!("{source}_COPY{n}");
            if !exists(&candidate) {
                return candidate;
            }
        }
        unreachable!()
    }

    /// Dissolves all groups that contain any of the given handles.
    /// Returns the number of groups removed.
    pub fn delete_groups_containing(&mut self, handles: &[Handle]) -> usize {
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let to_delete: Vec<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|obj| match obj {
                ObjectType::Group(g) if handles.iter().any(|h| g.contains(*h)) => Some(g.handle),
                _ => None,
            })
            .collect();
        let count = to_delete.len();
        for gh in &to_delete {
            if let Some(ObjectType::Dictionary(dict)) =
                self.document.objects.get_mut(&group_dict_handle)
            {
                dict.entries.retain(|(_, h)| h != gh);
            }
            self.document.objects.remove(gh);
        }
        count
    }

    /// Return `handles` plus every member of a selectable group containing one
    /// of them. Selection and rollover highlighting share this expansion so the
    /// preview matches what a click will select.
    pub fn handles_expanded_for_selectable_groups(
        &self,
        handles: &[Handle],
    ) -> HashSet<Handle> {
        let mut expanded: HashSet<Handle> = handles.iter().copied().collect();
        expanded.extend(
            self.document
                .objects
                .values()
                .filter_map(|obj| match obj {
                    ObjectType::Group(g)
                        if g.selectable
                            && handles.iter().any(|handle| g.contains(*handle)) =>
                    {
                        Some(g.entities.clone())
                    }
                    _ => None,
                })
                .flatten(),
        );
        expanded
    }

    /// If any handle belongs to a selectable group, also select every member.
    pub fn expand_selection_for_groups(&mut self, handles: &[Handle]) {
        let previous_len = self.selected.len();
        self.selected
            .extend(self.handles_expanded_for_selectable_groups(handles));
        if self.selected.len() != previous_len {
            self.bump_selection_set();
        }
    }
}
