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
use codec::{EntityType, Handle};

use crate::scene::model::object::{GripDef, GripShape};

use super::OpenCADStudio;

/// Sentinel grip id for the visibility (lookup) grip, distinct from any
/// entity's own grip ids so the click handler routes it to the dropdown.
pub(super) const VIS_GRIP_ID: usize = usize::MAX;
pub(super) const TABLE_GRIP_ID: usize = usize::MAX - 1;

/// The visibility and lookup grips open a list, so their triangle points down
/// as a list arrow does; it does not turn with the block.
pub(super) const LIST_GRIP_DIR: glam::DVec3 = glam::DVec3::NEG_Y;

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

#[derive(Clone, Debug)]
pub(super) struct TableGrip {
    pub insert_handle: Handle,
    pub current: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct TablePopup {
    pub insert_handle: Handle,
    pub anchor: iced::Point,
    pub header: String,
    pub rows: Vec<String>,
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
    pub(super) fn apply_table_row(&mut self, insert: Handle, row: usize) {
        let i = self.active_tab;
        if self.reject_locked_edit(i, insert) {
            return;
        }
        let result = {
            let doc = &self.tabs[i].scene.document;
            let Some(def) = doc.dynamic_definition_for_insert(insert) else {
                return;
            };
            let Ok(Some(view)) = crate::scene::block_properties_table::table(doc, def) else {
                return;
            };
            if let Some(test) = self.tabs[i].block_test.as_ref().filter(|test| test.insert == insert) {
                crate::scene::block_actions::instance::evaluate_test_table_row(doc, &test.baseline, insert, &view.columns, row)
            } else {
                crate::scene::block_actions::instance::evaluate_table_row(doc, insert, &view.columns, row)
            }
        };
        match result {
            Ok(Some(staged)) => {
                self.push_undo_snapshot(i, "Block Properties Table row");
                self.apply_action_document(i, staged);
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.finish_pending_history(i);
                self.refresh_properties();
            }
            Ok(None) => {}
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
    }
    pub(super) fn open_table_popup(&mut self, anchor: iced::Point) {
        if let Some(grip) = &self.tabs[self.active_tab].table_grip {
            let tab = &self.tabs[self.active_tab];
            let doc = &tab.scene.document;
            let Some(def) = doc.dynamic_definition_for_insert(grip.insert_handle) else {
                return;
            };
            let Ok(Some(view)) = crate::scene::block_properties_table::table(doc, def) else {
                return;
            };
            let parameters = crate::scene::block_actions::all_parameters(doc, def);
            let header = view
                .columns
                .iter()
                .map(|c| {
                    let display = if c.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
                        crate::scene::block_orientation_parameter::parameters(doc, def)
                            .into_iter()
                            .find(|p| p.handle == c.handle)
                            .map(|p| p.label)
                    } else {
                        crate::scene::block_distance_parameter::parameters(doc, def)
                            .into_iter()
                            .find(|p| p.handle == c.handle)
                            .and_then(|p| p.labels.first().cloned())
                    };
                    display
                        .filter(|n| !n.trim().is_empty())
                        .or_else(|| {
                            parameters
                                .iter()
                                .find(|p| p.handle == c.handle)
                                .map(|p| p.name.clone())
                        })
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| c.connection_name.clone())
                })
                .collect::<Vec<_>>()
                .join(", ");
            let rows = view
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|v| format!("{v:.4}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect();
            self.table_popup = Some(TablePopup {
                insert_handle: grip.insert_handle,
                anchor,
                header,
                rows,
                current: grip.current,
            });
        }
    }
    pub(super) fn refresh_table_grip(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        self.tabs[i].table_grip = None;
        let Some(handle) = self.tabs[i].selected_handle else {
            return;
        };
        if self.tabs[i].active_block_edit_session().is_some()
            || self.tabs[i].scene.selected.len() != 1
        {
            return;
        }
        let doc = &self.tabs[i].scene.document;
        let Some(def) = doc.dynamic_definition_for_insert(handle) else {
            return;
        };
        let Ok(Some(view)) = crate::scene::block_properties_table::table(doc, def) else {
            return;
        };
        if view.table.parameter.property_count <= 0 { return; }
        let Some(EntityType::Insert(ins)) = doc.get_entity(handle) else {
            return;
        };
        let graph =
            crate::scene::block_properties_table::graph_handle(doc, def).unwrap_or(Handle::NULL);
        let point = doc
            .objects
            .values()
            .find_map(|value| match value {
                codec::objects::ObjectType::DynamicBlock(obj) if obj.owner == graph => match &obj
                    .data
                {
                    codec::objects::DynamicBlockData::PropertiesTableGrip(g) => Some(g.location),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or(view.table.parameter.definition_point);
        let wp = crate::scene::render_graph::insert_transform(doc, ins).apply(point);
        let baseline = self.tabs[i].block_test.as_ref().filter(|test| test.insert == handle)
            .map(|test| test.baseline.as_ref());
        let current = crate::scene::block_parameter_properties::current_table_row(doc, baseline, handle);
        self.tabs[i].selected_grips.push(GripDef {
            id: TABLE_GRIP_ID,
            world: glam::DVec3::new(wp.x - wo[0], wp.y - wo[1], wp.z - wo[2]),
            is_midpoint: false,
            shape: GripShape::Triangle,
            dir: Some(LIST_GRIP_DIR),
            axis: None,
        });
        self.tabs[i].selected_grip_handles.push(handle);
        self.tabs[i].table_grip = Some(TableGrip {
            insert_handle: handle,
            current,
        });
    }

    /// Compute the index sets needed to apply `state` of `param` (governing the
    /// dynamic definition `def_block`) to the anonymous block `anon_name`.
    ///
    /// The anonymous block is an evaluated clone of the definition with a
    /// parallel member order, so a member visible in the definition maps to the
    /// same position in the anonymous block.
    fn state_mapping(
        doc: &codec::Document,
        def_block: Handle,
        anon_name: &str,
        state_idx: usize,
        param: &BlockVisibilityParameter,
    ) -> Option<StateMapping> {
        let def_handles = crate::scene::block_authoring::members(doc, def_block);
        let anon_handles: Vec<Handle> = doc
            .block_records
            .iter()
            .find(|b| b.name == anon_name)?
            .entity_handles
            .clone();
        if def_handles.len() != anon_handles.len() {
            return None;
        }
        let shown = crate::scene::block_visibility::state_visibility(doc, param.handle, state_idx);
        if shown.len() != def_handles.len() {
            return None;
        }
        let visible_idx: HashSet<usize> = shown
            .iter()
            .enumerate()
            .filter(|(_, visible)| **visible)
            .map(|(index, _)| index)
            .collect();
        Some(StateMapping {
            anon_handles,
            visible_idx,
        })
    }

    /// Which state (if any) matches the anonymous block's current per-member
    /// invisibility flags.
    fn current_visibility_state(
        doc: &codec::Document,
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

    /// The visibility parameter of a dynamic-block reference as a Properties
    /// row: the parameter's name, its states, and the one shown now.
    pub(super) fn visibility_property(
        doc: &codec::Document,
        handle: Handle,
    ) -> Option<crate::scene::model::object::Property> {
        let Some(EntityType::Insert(ins)) = doc.get_entity(handle) else {
            return None;
        };
        let (def_block, param) = doc.dynamic_visibility_for_insert(handle)?;
        if !param.show_properties { return None; }
        let options: Vec<String> = param.states.iter().map(|s| s.name.clone()).collect();
        let current = Self::current_visibility_state(doc, def_block, &ins.block_name, param);
        // An unnamed parameter takes the name a new one gets in the file.
        let label = if param.name.trim().is_empty() {
            "Visibility1".to_string()
        } else {
            param.name.clone()
        };
        Some(crate::scene::model::object::Property {
            label,
            field: "dyn_visibility",
            value: crate::scene::model::object::PropValue::ChoiceWithTooltip {
                selected: current
                    .and_then(|c| options.get(c).cloned())
                    .unwrap_or_default(),
                options,
                tooltip: param.description.clone(),
            },
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

        if param.property_info_count <= 0 && crate::scene::block_grips::config::count(doc, def_block, param.handle) == Some(0) { return; }
        // `def_point` is a block-definition coordinate (see the codec's
        // BlockVisibilityParameter), so the grip follows the same base-point
        // convention as the rendered reference.
        let point = crate::scene::block_grips::config::locations(doc, def_block, param.handle).ok()
            .and_then(|grips| grips.into_iter().next()).map(|grip| grip.point).unwrap_or(param.def_point);
        let wp = crate::scene::render_graph::insert_transform(doc, ins).apply(point);
        let state_names: Vec<String> = param.states.iter().map(|s| s.name.clone()).collect();
        let anon_name = ins.block_name.clone();
        let current = Self::current_visibility_state(doc, def_block, &anon_name, param);

        self.tabs[i].selected_grips.push(GripDef {
            id: VIS_GRIP_ID,
            world: glam::DVec3::new(wp.x - wo[0], wp.y - wo[1], wp.z - wo[2]),
            is_midpoint: false,
            shape: GripShape::Triangle,
            dir: Some(LIST_GRIP_DIR),
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

        // A reference to the definition itself, or to a copy other
        // references share, gets its own evaluated copy first, so the state
        // picked for it leaves the definition and the others alone.
        self.push_undo_snapshot(i, "Visibility State");
        self.tabs[i].scene.visibility_representation(insert_handle);

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
