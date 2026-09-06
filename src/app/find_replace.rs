use super::{FindMatchKey, Message, OpenCADStudio};
use crate::entities::traits::EntityTypeOps;
use acadrust::EntityType;
use iced::Task;

impl OpenCADStudio {
    pub(super) fn open_find_replace(&mut self) -> Task<Message> {
        self.find_replace.current_match = None;
        self.find_replace.status.clear();
        self.active_modal = Some(super::ModalKind::FindReplace);
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        iced::widget::operation::focus(iced::widget::Id::new(
            crate::ui::window::find_replace::FIND_INPUT_ID,
        ))
    }

    pub(super) fn find_replace_search_changed(&mut self, value: String) {
        self.find_replace.search = value;
        self.find_replace.current_match = None;
        self.find_replace.status.clear();
    }

    pub(super) fn find_replace_replacement_changed(&mut self, value: String) {
        self.find_replace.replacement = value;
    }

    pub(super) fn find_replace_next(&mut self) {
        let i = self.active_tab;
        let matches = self.find_navigation_matches(i);
        if matches.is_empty() {
            self.find_replace.current_match = None;
            self.find_replace.status = no_match_status(&self.find_replace.search);
            return;
        }

        let start = self
            .find_replace
            .current_match
            .and_then(|current| matches.iter().position(|candidate| *candidate == current))
            .map_or(0, |current| (current + 1) % matches.len());
        for offset in 0..matches.len() {
            let index = (start + offset) % matches.len();
            let target = matches[index];
            let centered = match target {
                FindMatchKey::Entity(handle) => {
                    self.tabs[i].scene.center_camera_on_entity(handle)
                }
                FindMatchKey::BlockEntityInInsert { entity, insert } => self.tabs[i]
                    .scene
                    .center_camera_on_block_entity(insert, entity),
                FindMatchKey::InsertAttribute { insert, index } => {
                    self.tabs[i]
                        .scene
                        .center_camera_on_insert_attribute(insert, index)
                }
            };
            if !centered {
                continue;
            }

            self.find_replace.current_match = Some(target);
            self.find_replace.status = crate::tf!(
                "{} of {} — {}",
                index + 1,
                matches.len(),
                match_label(target)
            ).into_owned();
            self.tabs[i].scene.deselect_all();
            self.tabs[i]
                .scene
                .select_entity(match_owner_handle(target), false);
            self.refresh_properties();
            return;
        }

        self.find_replace.current_match = None;
        self.find_replace.status = no_match_status(&self.find_replace.search);
    }

    pub(super) fn find_replace_one(&mut self) {
        let i = self.active_tab;
        let matches = self.find_text_matches(i);
        let Some(target) = self
            .find_replace
            .current_match
            .filter(|current| {
                matches.iter().any(|candidate| {
                    match_document_handle(*candidate) == match_document_handle(*current)
                })
            })
            .or_else(|| matches.first().copied())
        else {
            self.find_replace.current_match = None;
            self.find_replace.status = no_match_status(&self.find_replace.search);
            return;
        };

        let search = self.find_replace.search.clone();
        let replacement = self.find_replace.replacement.clone();
        if match_is_locked(&self.tabs[i].scene, target) {
            self.find_replace.status = crate::t!("The matching object is on a locked layer.").into_owned();
            return;
        }
        self.push_undo_snapshot(i, "FIND/REPLACE");
        let replaced = replace_match_text(
            &mut self.tabs[i].scene.document,
            target,
            &search,
            &replacement,
            false,
        );
        if replaced == 0 {
            self.discard_last_undo_entry(i);
            self.find_replace.status = no_match_status(&search);
            return;
        }

        let handle = match_document_handle(target);
        if self.tabs[i]
            .scene
            .entity_belongs_to_active_space(handle)
        {
            self.invalidate_property_targets(i, &[handle]);
        } else {
            // A block-definition edit must rebuild the definition cache; an
            // entity-only delta would leave every visible INSERT showing the
            // old text until some unrelated full rebuild.
            self.tabs[i].scene.bump_geometry();
        }
        self.tabs[i].dirty = true;
        self.find_replace.current_match = None;
        let remaining = self.find_text_matches(i).len();
        self.find_replace.status = crate::tf!(
            "Replaced 1 occurrence in {}; {remaining} matching object(s) remain.",
            match_label(target)
        ).into_owned();
        self.command_line.push_output(crate::tf!(
            "FIND/REPLACE: replaced 1 occurrence of \"{search}\"."
        ).as_ref());
        self.refresh_properties();
    }

    pub(super) fn find_replace_all(&mut self) {
        let i = self.active_tab;
        let matches = self.find_text_matches(i);
        if matches.is_empty() {
            self.find_replace.current_match = None;
            self.find_replace.status = no_match_status(&self.find_replace.search);
            return;
        }

        let search = self.find_replace.search.clone();
        let replacement = self.find_replace.replacement.clone();
        self.push_undo_snapshot(i, "FIND/REPLACE ALL");
        let mut replaced = 0usize;
        let mut changed = Vec::new();
        let mut changed_outside_active_space = false;
        let editable: Vec<_> = matches
            .into_iter()
            .filter(|target| !match_is_locked(&self.tabs[i].scene, *target))
            .collect();
        for target in editable {
            let count = replace_match_text(
                &mut self.tabs[i].scene.document,
                target,
                &search,
                &replacement,
                true,
            );
            if count > 0 {
                replaced += count;
                let handle = match_document_handle(target);
                changed_outside_active_space |= !self.tabs[i]
                    .scene
                    .entity_belongs_to_active_space(handle);
                if !changed.contains(&handle) {
                    changed.push(handle);
                }
            }
        }
        if changed.is_empty() {
            self.discard_last_undo_entry(i);
            self.find_replace.status = no_match_status(&search);
            return;
        }

        if changed_outside_active_space {
            self.tabs[i].scene.bump_geometry();
        } else {
            self.invalidate_property_targets(i, &changed);
        }
        self.tabs[i].dirty = true;
        self.find_replace.current_match = None;
        self.find_replace.status = crate::tf!(
            "Replaced {replaced} occurrence(s) in {} object(s).",
            changed.len()
        ).into_owned();
        self.command_line.push_output(crate::tf!(
            "FIND/REPLACE: replaced {replaced} occurrence(s) of \"{search}\"."
        ).as_ref());
        self.refresh_properties();
    }

    fn find_text_matches(&self, i: usize) -> Vec<FindMatchKey> {
        let search = self.find_replace.search.trim();
        if search.is_empty() {
            return Vec::new();
        }
        let mut visible = Vec::new();
        let mut definitions = Vec::new();
        for entity in self.tabs[i].scene.document.entities() {
            if let Some(text) = entity.text_content() {
                if find_case_insensitive_range(&text, search, 0).is_some() {
                    let target = FindMatchKey::Entity(entity.common().handle);
                    if self.tabs[i]
                        .scene
                        .entity_belongs_to_active_space(entity.common().handle)
                    {
                        visible.push(target);
                    } else {
                        definitions.push(target);
                    }
                }
            }
            if let EntityType::Insert(insert) = entity {
                for (index, attribute) in insert.attributes.iter().enumerate() {
                    if find_case_insensitive_range(attribute.get_value(), search, 0).is_some() {
                        let target = FindMatchKey::InsertAttribute {
                            insert: insert.common.handle,
                            index,
                        };
                        if self.tabs[i]
                            .scene
                            .entity_belongs_to_active_space(insert.common.handle)
                        {
                            visible.push(target);
                        } else {
                            definitions.push(target);
                        }
                    }
                }
            }
        }
        visible.extend(definitions);
        visible
    }

    fn find_navigation_matches(&self, i: usize) -> Vec<FindMatchKey> {
        let scene = &self.tabs[i].scene;
        let mut matches = Vec::new();
        for target in self.find_text_matches(i) {
            let owner = match_owner_handle(target);
            if scene.entity_belongs_to_active_space(owner) {
                matches.push(target);
                continue;
            }

            let FindMatchKey::Entity(entity) = target else {
                continue;
            };
            for candidate in scene.document.entities() {
                let EntityType::Insert(insert) = candidate else {
                    continue;
                };
                if !scene.entity_belongs_to_active_space(insert.common.handle) {
                    continue;
                }
                let mut visited = Vec::new();
                if block_contains_entity(
                    &scene.document,
                    &insert.block_name,
                    entity,
                    &mut visited,
                ) {
                    matches.push(FindMatchKey::BlockEntityInInsert {
                        entity,
                        insert: insert.common.handle,
                    });
                }
            }
        }
        matches
    }
}

fn no_match_status(search: &str) -> String {
    if search.trim().is_empty() {
        crate::t!("Enter text to find.").into_owned()
    } else {
        crate::tf!("\"{search}\" was not found.").into_owned()
    }
}

fn replace_entity_text(
    entity: &mut EntityType,
    search: &str,
    replacement: &str,
    replace_all: bool,
) -> usize {
    let Some(value) = entity.text_content() else {
        return 0;
    };
    let (value, count) = replace_case_insensitive(&value, search, replacement, replace_all);
    if count == 0 {
        return 0;
    }
    match entity {
        EntityType::Text(text) => text.value = value,
        EntityType::MText(text) => text.value = value,
        EntityType::AttributeDefinition(attribute) => attribute.default_value = value,
        EntityType::AttributeEntity(attribute) => attribute.set_value(value),
        _ => return 0,
    }
    count
}

fn replace_match_text(
    document: &mut acadrust::CadDocument,
    target: FindMatchKey,
    search: &str,
    replacement: &str,
    replace_all: bool,
) -> usize {
    match target {
        FindMatchKey::Entity(handle) | FindMatchKey::BlockEntityInInsert { entity: handle, .. } => {
            document.get_entity_mut(handle).map_or(0, |entity| {
                replace_entity_text(entity, search, replacement, replace_all)
            })
        }
        FindMatchKey::InsertAttribute { insert, index } => {
            let Some(EntityType::Insert(entity)) = document.get_entity_mut(insert) else {
                return 0;
            };
            let Some(attribute) = entity.attributes.get_mut(index) else {
                return 0;
            };
            let (value, count) = replace_case_insensitive(
                attribute.get_value(),
                search,
                replacement,
                replace_all,
            );
            if count > 0 {
                attribute.set_value(value);
            }
            count
        }
    }
}

fn match_owner_handle(target: FindMatchKey) -> acadrust::Handle {
    match target {
        FindMatchKey::Entity(handle) => handle,
        FindMatchKey::BlockEntityInInsert { insert, .. } => insert,
        FindMatchKey::InsertAttribute { insert, .. } => insert,
    }
}

fn match_document_handle(target: FindMatchKey) -> acadrust::Handle {
    match target {
        FindMatchKey::Entity(handle) => handle,
        FindMatchKey::BlockEntityInInsert { entity, .. } => entity,
        FindMatchKey::InsertAttribute { insert, .. } => insert,
    }
}

fn match_is_locked(scene: &crate::scene::Scene, target: FindMatchKey) -> bool {
    scene.is_layer_locked(match_owner_handle(target))
        || scene.is_layer_locked(match_document_handle(target))
}

fn match_label(target: FindMatchKey) -> String {
    match target {
        FindMatchKey::Entity(handle) => crate::tf!("handle {:X}", handle.value()).into_owned(),
        FindMatchKey::BlockEntityInInsert { entity, insert } => {
            crate::tf!(
                "block {:X}, text {:X}",
                insert.value(),
                entity.value()
            ).into_owned()
        }
        FindMatchKey::InsertAttribute { insert, index } => {
            crate::tf!("block {:X}, attribute {}", insert.value(), index + 1).into_owned()
        }
    }
}

fn block_contains_entity(
    document: &acadrust::CadDocument,
    block_name: &str,
    target: acadrust::Handle,
    visited: &mut Vec<String>,
) -> bool {
    if visited
        .iter()
        .any(|name| name.eq_ignore_ascii_case(block_name))
    {
        return false;
    }
    let Some(record) = document
        .block_records
        .iter()
        .find(|record| record.name.eq_ignore_ascii_case(block_name))
    else {
        return false;
    };
    visited.push(record.name.clone());
    let found = record.entity_handles.iter().any(|handle| {
        if *handle == target {
            return true;
        }
        matches!(
            document.get_entity(*handle),
            Some(EntityType::Insert(insert))
                if block_contains_entity(document, &insert.block_name, target, visited)
        )
    });
    visited.pop();
    found
}

fn replace_case_insensitive(
    value: &str,
    search: &str,
    replacement: &str,
    replace_all: bool,
) -> (String, usize) {
    if search.is_empty() {
        return (value.to_string(), 0);
    }

    let mut result = String::with_capacity(value.len());
    let mut cursor = 0usize;
    let mut count = 0usize;
    while let Some((start, end)) = find_case_insensitive_range(value, search, cursor) {
        result.push_str(&value[cursor..start]);
        result.push_str(replacement);
        cursor = end;
        count += 1;
        if !replace_all {
            break;
        }
    }
    if count == 0 {
        return (value.to_string(), 0);
    }
    result.push_str(&value[cursor..]);
    (result, count)
}

fn find_case_insensitive_range(value: &str, search: &str, from: usize) -> Option<(usize, usize)> {
    let needle = search.to_lowercase();
    if needle.is_empty() {
        return None;
    }

    for (start, _) in value.char_indices().filter(|(index, _)| *index >= from) {
        let ends = value[start..]
            .char_indices()
            .skip(1)
            .map(|(offset, _)| start + offset)
            .chain(std::iter::once(value.len()));
        for end in ends {
            let candidate = value[start..end].to_lowercase();
            if candidate == needle {
                return Some((start, end));
            }
            if candidate.len() > needle.len() {
                break;
            }
        }
    }
    None
}
