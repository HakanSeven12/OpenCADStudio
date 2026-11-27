//! Construction membership stored in a project-owned XDATA record.
//!
//! Ordinary entity invisibility also hides these members in external readers.
//! The active definition editor supplies the only display exception. This is
//! application metadata, not a claim about another application's native format.
use document::Handle;

pub(crate) use crate::entities::construction::{is_construction, set};

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

impl super::Scene {
    pub(crate) fn construction_revealed(&self, handle: Handle) -> bool {
        self.block_edit_block.is_some_and(|definition| {
            !self.block_construction_hidden.contains(&definition)
                && self.document.get_entity(handle).is_some_and(|entity| {
                    entity.common().owner_handle == definition && is_construction(entity.common())
                })
        })
    }

    fn construction_member_handles(&self, definition: Handle) -> Vec<Handle> {
        if let Some(record) = self
            .document
            .block_records
            .iter()
            .find(|r| r.handle == definition && !r.entity_handles.is_empty())
        {
            record
                .entity_handles
                .iter()
                .copied()
                .filter(|&h| {
                    self.document.get_entity(h).is_some_and(|e| {
                        let common = e.common();
                        common.owner_handle == definition && is_construction(common)
                    })
                })
                .collect()
        } else {
            self.document
                .entities()
                .filter_map(|e| {
                    let common = e.common();
                    (common.owner_handle == definition && is_construction(common))
                        .then_some(common.handle)
                })
                .collect()
        }
    }

    pub(crate) fn set_construction_shown(&mut self, definition: Handle, shown: bool) {
        let changed = if shown {
            self.block_construction_hidden.remove(&definition)
        } else {
            self.block_construction_hidden.insert(definition)
        };
        if changed {
            self.deselect_all();
            let affected = self.construction_member_handles(definition);
            if !affected.is_empty() {
                self.recolor_meshes_for_handles(&affected);
                let changes: Vec<(Handle, super::ChangeKind)> = affected
                    .into_iter()
                    .map(|h| (h, super::ChangeKind::Modified))
                    .collect();
                self.bump_entities_restyled(&changes);
            }
        }
    }
}

#[cfg(test)]
mod tests;
