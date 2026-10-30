//! Read-only groups of native actions for parameter action bars.
use super::block_actions::{self as actions, Kind};
use codec::{Document, Handle};
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub(crate) enum EntryKind {
    Action(Kind),
    Orphan(Kind),
    Lookup,
    PropertiesTable,
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub handle: Handle,
    pub label: String,
    pub kind: EntryKind,
}

#[derive(Clone)]
pub(crate) struct Group {
    pub parameter: Handle,
    pub label: String,
    pub anchor: DVec3,
    pub entries: Vec<Entry>,
}

pub(crate) fn groups(doc: &Document, definition: Handle) -> Vec<Group> {
    let parameters = actions::all_parameters(doc, definition);
    let mut groups: Vec<Group> = vec![];
    for (handle, action, kind, id) in actions::actions(doc, definition) {
        let parameter = parameters
            .iter()
            .find(|p| p.id == id && actions::compatible(p, kind));
        let (key, label, anchor, entry_kind) = parameter.map_or_else(
            || {
                (
                    handle,
                    crate::t!("Unassociated action").into_owned(),
                    actions::v(action.display_location),
                    EntryKind::Orphan(kind),
                )
            },
            |p| (p.handle, p.name.clone(), p.end, EntryKind::Action(kind)),
        );
        let entry = Entry {
            handle,
            label: action.element.name.clone(),
            kind: entry_kind,
        };
        if let Some(group) = groups.iter_mut().find(|group| group.parameter == key) {
            group.entries.push(entry);
        } else {
            groups.push(Group {
                parameter: key,
                label,
                anchor,
                entries: vec![entry],
            });
        }
    }
    for (handle, parameter) in super::block_lookup::parameters(doc, definition) {
        groups.push(Group {
            parameter: handle,
            label: parameter.lookup_name.clone(),
            anchor: actions::v(parameter.parameter.definition_point),
            entries: vec![Entry {
                handle,
                label: crate::t!("Lookup table").into_owned(),
                kind: EntryKind::Lookup,
            }],
        });
    }
    if let Ok(Some(table)) = super::block_properties_table::table(doc, definition) {
        groups.push(Group {
            parameter: table.handle,
            label: crate::t!("Block Properties Table").into_owned(),
            anchor: actions::v(table.table.parameter.definition_point),
            entries: vec![Entry {
                handle: table.handle,
                label: crate::t!("Edit table").into_owned(),
                kind: EntryKind::PropertiesTable,
            }],
        });
    }
    groups.sort_by_key(|group| group.parameter.value());
    for group in &mut groups {
        group.entries.sort_by_key(|entry| entry.handle.value());
    }
    groups
}

pub(crate) fn is_action(doc: &Document, handle: Handle) -> bool {
    use codec::objects::{DynamicBlockData as D, ObjectType};
    matches!(doc.objects.get(&handle),Some(ObjectType::DynamicBlock(object)) if matches!(object.data,
        D::MoveAction(_)|D::StretchAction(_)|D::PolarStretchAction(_)|D::RotateAction(_)|D::ScaleAction(_)|D::FlipAction(_)|D::ArrayAction(_)))
}
