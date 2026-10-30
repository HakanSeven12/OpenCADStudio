//! Construction membership stored in a project-owned XDATA record.
//!
//! Ordinary entity invisibility also hides these members in external readers.
//! The active definition editor supplies the only display exception. This is
//! application metadata, not a claim about another application's native format.
use codec::{entities::EntityCommon, xdata::XDataValue, Document, EntityType, Handle};

const APP: &str = "OCS_BLOCK_CONSTRUCTION";

pub(crate) fn display_style() -> super::render_graph::ResolvedStyle {
    (
        [0.55, 0.55, 0.55, 1.0],
        8.0,
        [4.0, -4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        1.0,
        8,
    )
}

pub(crate) fn style_wires(wires: &mut [super::WireModel]) {
    let (color, length, pattern, _, _) = display_style();
    for wire in wires {
        wire.color = color;
        wire.pattern_length = length;
        wire.pattern = pattern;
        wire.bg_adapt = None;
        wire.render_instance = None;
        for vertex in &mut wire.text_verts {
            vertex.color = color;
        }
    }
}

pub(crate) fn is_construction(common: &EntityCommon) -> bool {
    common.extended_data.get_record(APP).is_some()
}

fn original_visibility(common: &EntityCommon) -> Result<bool, &'static str> {
    match common
        .extended_data
        .get_record(APP)
        .map(|record| record.values.as_slice())
    {
        Some([XDataValue::Integer16(1), XDataValue::Integer16(hidden @ (0 | 1))]) => {
            Ok(*hidden != 0)
        }
        _ => Err("This construction record has an unsupported version or value."),
    }
}

pub(crate) fn set(
    document: &mut Document,
    definition: Handle,
    handles: &[Handle],
    convert: bool,
    layer: &str,
) -> Result<usize, &'static str> {
    let record = document
        .block_records
        .iter()
        .find(|r| r.handle == definition)
        .ok_or("Open a block definition before editing construction geometry.")?;
    if record.is_model_space() || record.is_paper_space() {
        return Err("Open a block definition before editing construction geometry.");
    }
    let mut edits = std::collections::BTreeMap::new();
    for handle in handles {
        let Some(entity) = document.get_entity(*handle) else {
            if document.objects.contains_key(handle) {
                continue;
            }
            return Err("A selected construction object is no longer available.");
        };
        let common = entity.common();
        if common.owner_handle != definition || !record.entity_handles.contains(handle) {
            return Err("Select geometry in the active block definition.");
        }
        if matches!(
            entity,
            EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::Viewport(_)
        ) {
            continue;
        }
        if is_construction(common) == convert {
            continue;
        }
        if document
            .layers
            .get(&common.layer)
            .is_some_and(|layer| layer.flags.locked)
        {
            return Err("Construction geometry cannot be changed on a locked layer.");
        }
        let hidden = if convert {
            common.invisible
        } else {
            original_visibility(common)?
        };
        edits.insert(*handle, hidden);
    }
    if edits.is_empty() {
        return Ok(0);
    }
    if !convert
        && document
            .layers
            .get(layer)
            .is_none_or(|layer| layer.flags.locked)
    {
        return Err("Choose an existing unlocked layer for reverted geometry.");
    }
    for (handle, hidden) in &edits {
        super::view::dispatch::set_entity_xdata(
            document,
            *handle,
            APP,
            convert.then(|| {
                vec![
                    XDataValue::Integer16(1),
                    XDataValue::Integer16(i16::from(*hidden)),
                ]
            }),
        );
        let common = document.get_entity_mut(*handle).unwrap().common_mut();
        common.invisible = convert || *hidden;
        if !convert {
            common.layer = layer.to_owned();
            common.layer_handle = None;
        }
    }
    Ok(edits.len())
}

impl super::Scene {
    pub(crate) fn construction_revealed(&self, handle: Handle) -> bool {
        self.block_edit_block.is_some_and(|definition| {
            !self.block_construction_hidden.contains(&definition)
                && self.document.get_entity(handle).is_some_and(|entity| {
                    entity.common().owner_handle == definition && is_construction(entity.common())
                })
        })
    }

    pub(crate) fn set_construction_shown(&mut self, definition: Handle, shown: bool) {
        let changed = if shown {
            self.block_construction_hidden.remove(&definition)
        } else {
            self.block_construction_hidden.insert(definition)
        };
        if changed {
            self.deselect_all();
            self.recolor_meshes();
            self.bump_geometry();
        }
    }
}

#[cfg(test)]
mod tests;
