//! Dynamic-block visibility-state UI.
//!
//! A dynamic block with a visibility parameter shows a small "lookup" grip;
//! clicking it opens a dropdown of named states. Picking one toggles which of
//! the inserted (anonymous) block's member entities are visible.
//!
//! This is kept separate from the static `GripMenuAction` grip-menu system:
//! the state list is data-driven (owned strings from the file) and the apply
//! step mutates *other* entities (the anonymous block's members), which the
//! per-entity `apply_grip_menu` path can't reach.

use std::collections::HashSet;

use codec::objects::BlockVisibilityParameter;
use codec::types::Vector3;
use codec::{EntityType, Handle};

use crate::scene::model::object::{GripDef, GripShape};

use super::OpenCADStudio;

/// Sentinel grip id for the visibility (lookup) grip, distinct from any
/// entity's own grip ids so the click handler routes it to the dropdown.
pub(super) const VIS_GRIP_ID: usize = usize::MAX;

/// Resolved visibility info for the active tab's selected dynamic-block INSERT.
#[derive(Clone, Debug)]
pub(super) struct VisibilityGrip {
    pub insert_handle: Handle,
    pub state_names: Vec<String>,
    pub current: Option<usize>,
}

/// The open visibility-state dropdown.
#[derive(Clone, Debug)]
pub struct VisibilityPopup {
    pub insert_handle: Handle,
    pub anchor: iced::Point,
    pub items: Vec<String>,
    pub current: Option<usize>,
}

/// Block-local entity-handle lists for a dynamic definition and the anonymous
/// block actually inserted, plus the state's visible-by-index set.
struct StateMapping {
    /// Anonymous (inserted) block member handles, in definition order.
    anon_handles: Vec<Handle>,
    /// Indices (into `anon_handles`) that the state makes visible.
    visible_idx: HashSet<usize>,
}

impl OpenCADStudio {
    /// Compute the index sets needed to apply `state` of `param` (governing the
    /// dynamic definition `def_block`) to the anonymous block `anon_name`.
    ///
    /// The anonymous block is an evaluated clone of the definition with a
    /// parallel member order, so a member visible in the definition maps to the
    /// same position in the anonymous block.
    fn state_mapping(
        doc: &codec::CadDocument,
        def_block: Handle,
        anon_name: &str,
        state_idx: usize,
        param: &BlockVisibilityParameter,
    ) -> Option<StateMapping> {
        let def_handles: Vec<Handle> = doc
            .block_records
            .iter()
            .find(|b| b.handle == def_block)?
            .entity_handles
            .clone();
        let anon_handles: Vec<Handle> = doc
            .block_records
            .iter()
            .find(|b| b.name == anon_name)?
            .entity_handles
            .clone();
        let state = param.states.get(state_idx)?;
        let visible: HashSet<u64> = state.visible_blocks.iter().map(|h| h.value()).collect();
        let visible_idx: HashSet<usize> = def_handles
            .iter()
            .enumerate()
            .filter(|(_, h)| visible.contains(&h.value()))
            .map(|(i, _)| i)
            .collect();
        Some(StateMapping {
            anon_handles,
            visible_idx,
        })
    }

    /// Which state (if any) matches the anonymous block's current per-member
    /// invisibility flags.
    fn current_visibility_state(
        doc: &codec::CadDocument,
        def_block: Handle,
        anon_name: &str,
        param: &BlockVisibilityParameter,
    ) -> Option<usize> {
        // Currently-visible member positions in the anonymous block.
        let anon = doc.block_records.iter().find(|b| b.name == anon_name)?;
        let cur_visible: HashSet<usize> = anon
            .entity_handles
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                doc.get_entity(**h)
                    .map(|e| !e.common().invisible)
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();
        (0..param.states.len()).find(|&si| {
            Self::state_mapping(doc, def_block, anon_name, si, param)
                .map(|m| m.visible_idx == cur_visible)
                .unwrap_or(false)
        })
    }

    /// Recompute the visibility grip for the active tab's single selection and
    /// append it (as a Triangle grip) to `selected_grips`. Clears it when the
    /// selection is not a dynamic-block reference. `wo` is the world offset
    /// already subtracted from the other grips.
    pub(super) fn refresh_visibility_grip(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        self.tabs[i].visibility_grip = None;

        let Some(handle) = self.tabs[i].selected_handle else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let Some(EntityType::Insert(ins)) = doc.get_entity(handle) else {
            return;
        };
        let Some((def_block, param)) = doc.dynamic_visibility_for_insert(handle) else {
            return;
        };

        let wp = ins.get_transform().apply(Vector3::new(
            param.def_point.x,
            param.def_point.y,
            param.def_point.z,
        ));
        let state_names: Vec<String> = param.states.iter().map(|s| s.name.clone()).collect();
        let anon_name = ins.block_name.clone();
        let current = Self::current_visibility_state(doc, def_block, &anon_name, param);

        self.tabs[i].selected_grips.push(GripDef {
            id: VIS_GRIP_ID,
            world: glam::DVec3::new(wp.x - wo[0], wp.y - wo[1], wp.z - wo[2]),
            is_midpoint: false,
            shape: GripShape::Triangle,
            dir: None,
            axis: None,
        });
        self.tabs[i].selected_grip_handles.push(handle);
        self.tabs[i].visibility_grip = Some(VisibilityGrip {
            insert_handle: handle,
            state_names,
            current,
        });
    }

    /// Open the visibility dropdown at `anchor` for the active tab's grip.
    pub(super) fn open_visibility_popup(&mut self, anchor: iced::Point) {
        let i = self.active_tab;
        if let Some(vg) = &self.tabs[i].visibility_grip {
            self.visibility_popup = Some(VisibilityPopup {
                insert_handle: vg.insert_handle,
                anchor,
                items: vg.state_names.clone(),
                current: vg.current,
            });
        }
    }

    /// Apply visibility `state_idx` to the dynamic-block reference: set each
    /// anonymous-block member visible/invisible per the state, then rebuild.
    pub(super) fn apply_visibility_state(&mut self, insert_handle: Handle, state_idx: usize) {
        let i = self.active_tab;
        if self.reject_locked_edit(i, insert_handle) {
            self.visibility_popup = None;
            return;
        }

        // Resolve everything against an immutable borrow first.
        let mapping = {
            let doc = &self.tabs[i].scene.document;
            let Some(EntityType::Insert(ins)) = doc.get_entity(insert_handle) else {
                return;
            };
            let anon_name = ins.block_name.clone();
            let Some((def_block, param)) = doc.dynamic_visibility_for_insert(insert_handle) else {
                return;
            };
            Self::state_mapping(doc, def_block, &anon_name, state_idx, param)
        };
        let Some(mapping) = mapping else {
            return;
        };

        self.push_undo_snapshot(i, "Visibility State");

        // Apply invisibility flags to the anonymous block's members.
        {
            let doc = &mut self.tabs[i].scene.document;
            for (idx, h) in mapping.anon_handles.iter().enumerate() {
                let visible = mapping.visible_idx.contains(&idx);
                if let Some(e) = doc.get_entity_mut(*h) {
                    e.common_mut().invisible = !visible;
                }
            }
        }

        self.visibility_popup = None;
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.refresh_selected_grips();
    }
}
