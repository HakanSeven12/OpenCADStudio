// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    // ── Group helpers ──────────────────────────────────────────────────────

    pub fn groups(&self) -> impl Iterator<Item = &codec::objects::Group> {
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
        let mut group = codec::objects::Group::new(&name);
        group.handle = self.document.allocate_handle();
        group.owner = group_dict_handle;
        group.add_entities(handles);
        let gh = group.handle;
        if self.is_recording_undo() {
            let before = self.document.objects.get(&group_dict_handle).cloned();
            self.record_undo_object_before(group_dict_handle, before);
            self.record_undo_object_before(gh, None);
        }
        self.set_group_member_reactors(gh, &group.entities, true);
        self.document.objects.insert(gh, ObjectType::Group(group));
        if let Some(ObjectType::Dictionary(dict)) =
            self.document.objects.get_mut(&group_dict_handle)
        {
            dict.add_entry(&name, gh);
        }
        gh
    }

    /// Keep member back-links and their first-touch undo images together.
    fn set_group_member_reactors(&mut self, group: Handle, members: &[Handle], add: bool) {
        for &member in members {
            let Some(entity) = self.document.get_entity(member) else {
                continue;
            };
            if entity.common().reactors.contains(&group) == add {
                continue;
            }
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(member);
                self.record_undo_before(member, before);
            }
            if let Some(entity) = self.document.get_entity_mut(member) {
                let reactors = &mut entity.common_mut().reactors;
                if add {
                    reactors.push(group);
                } else {
                    reactors.retain(|reactor| *reactor != group);
                }
            }
        }
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
        sources: Vec<codec::objects::Group>,
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
            self.set_group_member_reactors(gh, &group.entities, true);
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
        if count > 0 && self.is_recording_undo() {
            let before = self.document.objects.get(&group_dict_handle).cloned();
            self.record_undo_object_before(group_dict_handle, before);
        }
        for gh in &to_delete {
            if let Some(ObjectType::Group(group)) = self.document.objects.get(gh) {
                let members = group.entities.clone();
                if self.is_recording_undo() {
                    let before = self.document.objects.get(gh).cloned();
                    self.record_undo_object_before(*gh, before);
                }
                self.set_group_member_reactors(*gh, &members, false);
            }
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
    pub fn handles_expanded_for_selectable_groups(&self, handles: &[Handle]) -> HashSet<Handle> {
        let mut expanded: HashSet<Handle> = handles.iter().copied().collect();
        let wanted: HashSet<Handle> = handles.iter().copied().collect();
        for obj in self.document.objects.values() {
            if let ObjectType::Group(g) = obj {
                if g.selectable && g.entities.iter().any(|e| wanted.contains(e)) {
                    expanded.extend(g.entities.iter().copied());
                }
            }
        }
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

    /// Partition selected entity handles into visual objects for operations
    /// such as edge alignment. A selectable CAD group is one object and must
    /// receive one shared transform; treating its member lines independently
    /// can collapse a rectangle or triangle onto its own edge.
    pub fn selected_object_units(&self, handles: &[Handle]) -> Vec<Vec<Handle>> {
        let selected: HashSet<Handle> = handles.iter().copied().collect();
        let mut claimed = HashSet::default();
        let mut units = Vec::new();
        for group in self.groups().filter(|group| group.selectable) {
            let members: Vec<_> = group
                .entities
                .iter()
                .copied()
                .filter(|handle| selected.contains(handle))
                .collect();
            if !members.is_empty()
                && members.iter().all(|handle| !claimed.contains(handle))
                && group
                    .entities
                    .iter()
                    .all(|handle| selected.contains(handle))
            {
                claimed.extend(members.iter().copied());
                units.push(members);
            }
        }
        units.extend(
            handles
                .iter()
                .copied()
                .filter(|handle| !claimed.contains(handle))
                .map(|handle| vec![handle]),
        );
        units
    }
}

#[cfg(test)]
mod group_reactor_tests {
    use super::*;
    use codec::entities::{EntityType, Line};
    use codec::types::Vector3;

    fn two_lines() -> (Scene, [Handle; 2]) {
        let mut scene = Scene::new();
        let members = [0.0, 5.0].map(|y| {
            scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(0.0, y, 0.0),
                Vector3::new(10.0, y, 0.0),
            )))
        });
        (scene, members)
    }

    #[test]
    fn create_group_links_members_back() {
        let (mut scene, members) = two_lines();
        let group = scene.create_group("G1".into(), vec![members[0], members[1], members[0]]);
        for member in members {
            let reactors = &scene.document.get_entity(member).unwrap().common().reactors;
            assert_eq!(reactors.iter().filter(|&&h| h == group).count(), 1);
        }
    }

    #[test]
    fn deleting_group_clears_member_reactors() {
        let (mut scene, members) = two_lines();
        let unrelated = scene.document.allocate_handle();
        scene
            .document
            .get_entity_mut(members[0])
            .unwrap()
            .common_mut()
            .reactors
            .push(unrelated);
        let group = scene.create_group("G1".into(), members.to_vec());
        // Seed the back-links independently of create, as in an imported drawing.
        // Duplicates must also be removed without touching other reactors.
        for member in members {
            scene
                .document
                .get_entity_mut(member)
                .unwrap()
                .common_mut()
                .reactors
                .extend([group, group]);
        }
        let before: Vec<_> = members
            .iter()
            .map(|&h| {
                scene
                    .document
                    .get_entity(h)
                    .unwrap()
                    .common()
                    .reactors
                    .clone()
            })
            .collect();
        scene.begin_undo_recording();
        assert_eq!(scene.delete_groups_containing(&[members[1]]), 1);
        assert!(!scene.document.objects.contains_key(&group));
        for member in members {
            assert!(!scene
                .document
                .get_entity(member)
                .unwrap()
                .common()
                .reactors
                .contains(&group));
        }
        assert_eq!(
            scene
                .document
                .get_entity(members[0])
                .unwrap()
                .common()
                .reactors,
            vec![unrelated]
        );
        let recorded = scene.take_undo_recording().unwrap().into_before_images();
        assert_eq!(recorded.len(), 2);
        let delta: Vec<_> = recorded
            .into_iter()
            .map(|(h, before)| (h, before, scene.document.get_entity_arc(h)))
            .collect();
        scene.apply_entity_delta(&delta, true);
        for (member, expected) in members.into_iter().zip(before) {
            assert_eq!(
                scene.document.get_entity(member).unwrap().common().reactors,
                expected
            );
        }
    }

    #[test]
    fn undo_create_restores_member_reactors() {
        let (mut scene, members) = two_lines();
        scene.begin_undo_recording();
        let group = scene.create_group("G1".into(), members.to_vec());
        let recorded = scene.take_undo_recording().unwrap().into_before_images();
        assert_eq!(
            recorded.len(),
            2,
            "both member before-images must be recorded"
        );
        let delta: Vec<_> = recorded
            .into_iter()
            .map(|(h, before)| (h, before, scene.document.get_entity_arc(h)))
            .collect();
        scene.apply_entity_delta(&delta, true);
        for member in members {
            assert!(scene
                .document
                .get_entity(member)
                .unwrap()
                .common()
                .reactors
                .is_empty());
        }
        scene.apply_entity_delta(&delta, false);
        for member in members {
            assert_eq!(
                scene.document.get_entity(member).unwrap().common().reactors,
                vec![group]
            );
        }
    }
}

#[cfg(test)]
mod group_expansion_tests {
    use super::*;
    use crate::scene::Scene;

    #[test]
    fn group_expansion_pulls_in_siblings_and_nothing_else() {
        use codec::entities::{EntityType, Line};
        use codec::types::Vector3;

        let mut scene = Scene::new();
        let mut line = |x: f64| {
            scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x + 1.0, 0.0, 0.0),
            )))
        };
        let (a, b, c) = (line(0.0), line(1.0), line(2.0));
        let (d, e) = (line(3.0), line(4.0));
        let lonely = line(9.0);
        scene.create_group("GRUPPO".to_string(), vec![a, b, c]);
        scene.create_group("ALTRO".to_string(), vec![d, e]);

        // One member pulls in its siblings, and only its own group's.
        let from_a = scene.handles_expanded_for_selectable_groups(&[a]);
        assert_eq!(
            from_a,
            [a, b, c].into_iter().collect::<HashSet<_>>(),
            "selecting a member must select that group and no other",
        );

        // An entity in no group expands to itself.
        assert_eq!(
            scene.handles_expanded_for_selectable_groups(&[lonely]),
            [lonely].into_iter().collect::<HashSet<_>>(),
        );

        // Touching both groups pulls in both, and nothing outside them.
        let both = scene.handles_expanded_for_selectable_groups(&[a, e]);
        assert_eq!(both, [a, b, c, d, e].into_iter().collect::<HashSet<_>>());
        assert!(!both.contains(&lonely));
    }

    /// `selected_object_units` is `Scene::selected_object_units`'s
    /// counterpart for operations like edge alignment (`ALIGNTOP`, etc.):
    /// a complete, fully-selected group collapses to one unit sharing a
    /// single transform, so aligning a triangle's edge to another object
    /// moves the whole triangle rather than collapsing its own lines onto
    /// each other.
    #[test]
    fn selected_object_units_keep_a_complete_group_together() {
        use codec::entities::{EntityType, Line};
        use codec::types::Vector3;

        let mut scene = Scene::new();
        let mut line = |x: f64| {
            scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x + 1.0, 0.0, 0.0),
            )))
        };
        let (a, b, c, standalone) = (line(0.0), line(1.0), line(2.0), line(10.0));
        scene.create_group("triangle".into(), vec![a, b, c]);

        let units = scene.selected_object_units(&[a, b, c, standalone]);
        assert_eq!(units.len(), 2);
        assert!(units.iter().any(|unit| unit == &vec![a, b, c]));
        assert!(units.iter().any(|unit| unit == &vec![standalone]));
    }
}
