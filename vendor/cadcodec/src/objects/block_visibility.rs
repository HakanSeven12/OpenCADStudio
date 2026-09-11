//! Dynamic-block visibility parameter (AcDbBlockVisibilityParameter).
//!
//! A dynamic block with a visibility parameter keeps the geometry for every
//! visibility state in a single (anonymous) block definition. The parameter
//! object lists, per state, which member entities are visible. The currently
//! shown state is baked into the anonymous block by marking the other states'
//! entities invisible, so a plain reader still renders the right subset — but
//! switching states needs the full per-state membership recorded here.
//!
//! These objects are still preserved verbatim as `ObjectType::Unknown` for
//! DWG round-trip; this is a parsed *side* view keyed by the parameter handle.

use crate::types::{Handle, Vector3};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlockEvalValue {
    None,
    Real(f64),
    Point([f64; 2]),
    Text(String),
    Long(i32),
    Handle(Handle),
    Short(i16),
}

impl Default for BlockEvalValue {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameterConnection {
    pub code: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockParameterPropertyInfo {
    pub connections: Vec<BlockParameterConnection>,
}

/// One visibility state: a named choice (e.g. "120") and the member entities
/// that are visible while it is active.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockVisibilityState {
    /// State name as shown in the lookup list (e.g. "80", "120", "600").
    pub name: String,
    /// Handles of member entities visible while this state is active.
    pub visible_blocks: Vec<Handle>,
    /// Handles of member parameters active while this state is active.
    pub visible_params: Vec<Handle>,
}

/// Parsed AcDbBlockVisibilityParameter: the visibility grip location, the full
/// member list, and every selectable state with its visible-entity set.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockVisibilityParameter {
    /// Handle of the parameter object itself.
    pub handle: Handle,
    /// Owner handle (the block record's extension dictionary chain).
    pub owner: Handle,
    pub eval_parent_id: i32,
    pub eval_major: i32,
    pub eval_minor: i32,
    pub eval_value_code: i16,
    pub eval_value: BlockEvalValue,
    pub eval_node_id: i32,
    pub element_name: String,
    pub element_major: i32,
    pub element_minor: i32,
    pub element_eed_1071: i32,
    pub show_properties: bool,
    pub chain_actions: bool,
    /// Parameter display name (group code 301).
    pub name: String,
    /// Parameter description (group code 302).
    pub description: String,
    /// Grip / definition point in block-definition coordinates.
    pub def_point: Vector3,
    pub property_info: [BlockParameterPropertyInfo; 2],
    pub property_info_count: i32,
    pub is_initialized: bool,
    pub unknown_bool: bool,
    /// All member entity handles the parameter governs (the union of states).
    pub all_blocks: Vec<Handle>,
    /// Selectable visibility states, in list order.
    pub states: Vec<BlockVisibilityState>,
}

impl BlockVisibilityParameter {
    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        visit(&mut self.handle);
        visit(&mut self.owner);
        if let BlockEvalValue::Handle(handle) = &mut self.eval_value {
            visit(handle);
        }
        for handle in &mut self.all_blocks {
            visit(handle);
        }
        for state in &mut self.states {
            for handle in &mut state.visible_blocks {
                visit(handle);
            }
            for handle in &mut state.visible_params {
                visit(handle);
            }
        }
    }
}
