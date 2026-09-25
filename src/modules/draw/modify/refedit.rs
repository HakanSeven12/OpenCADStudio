// REFEDIT — in-place block reference editing.
//
// Workflow:
//   1. REFEDIT: user picks an INSERT entity.
//   2. The block's entities are copied into model space with the INSERT
//      transform applied (translate + rotate + uniform scale).
//   3. The user edits them with normal commands (MOVE, COPY, DELETE, …).
//   4. REFCLOSE SAVE: temp entities are inverse-transformed back into the
//      block definition; all INSERT references auto-update.
//      REFCLOSE DISCARD: temp entities are removed, block unchanged.
//
// Limitation: non-uniform scale inserts (x_scale ≠ y_scale) are rejected
// with an error message — full matrix inversion for those cases would
// require per-entity matrix transforms not yet in EntityTransform.

use codec::{EntityType, Handle};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// Right-edge side-toolbar buttons shown while a REFEDIT session is active:
/// save the in-place block edit, or discard it. (#136)
pub fn refedit_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            id: "REFCLOSE_SAVE",
            label: "Save Block Edit",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/mt_ok.svg")),
            event: ModuleEvent::Command("REFCLOSE_SAVE".to_string()),
        },
        ToolDef {
            id: "REFCLOSE_DISCARD",
            label: "Discard Block Edit",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/mt_cancel.svg")),
            event: ModuleEvent::Command("REFCLOSE_DISCARD".to_string()),
        },
    ]
}

// ── Session state (held in DocumentTab) ───────────────────────────────────

/// Active REFEDIT session.  Lives in `DocumentTab::refedit_session`.
#[derive(Debug, Clone)]
pub struct RefEditSession {
    /// Name of the block being edited.
    pub block_name: String,
    /// Handle of the block record (owns the block entities).
    pub br_handle: Handle,
    /// Handles of the temporary model-space entities added for editing.
    pub temp_handles: Vec<Handle>,
    // ── INSERT placement, as full affine transforms ───────────────────
    /// Block-local → world (the INSERT's `get_transform`). Handles OCS,
    /// rotation and non-uniform / mirrored scale in one matrix.
    pub forward: codec::types::Transform,
    /// World → block-local, applied on SAVE to bring edits back.
    pub inverse: codec::types::Transform,
    /// Handle watermark captured before the temp copies were created: every
    /// entity allocated at or above it during the session (drawn, pasted,
    /// offset, …) belongs to the working set, so Save folds it into the block
    /// and Discard removes it (#423). Catches every creation path without
    /// per-command hooks.
    pub handle_watermark: u64,
}

// ── REFEDIT pick command ───────────────────────────────────────────────────

/// Step 1: wait for the user to pick a single INSERT entity.
pub struct RefEditPickCommand;

impl RefEditPickCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for RefEditPickCommand {
    fn name(&self) -> &'static str {
        "REFEDIT"
    }
    fn prompt(&self) -> String {
        crate::t!("REFEDIT  Select block reference to edit:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Signal the host to enter the editing session for this handle.
        // We reuse Relaunch("REFEDIT_BEGIN:<handle>") as a convention.
        CmdResult::Relaunch(format!("REFEDIT_BEGIN:{}", handle.value()), vec![handle])
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── REFCLOSE command ───────────────────────────────────────────────────────

/// Step 4: prompt for SAVE or DISCARD.
pub struct RefCloseCommand;

impl RefCloseCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for RefCloseCommand {
    fn name(&self) -> &'static str {
        "REFCLOSE"
    }
    fn prompt(&self) -> String {
        t!("REFCLOSE  [Save/Discard] <Save>:").into_owned()
    }
    fn wants_text_input(&self) -> bool {
        true
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().to_uppercase();
        let save = t.is_empty() || t == "S" || t == "SAVE";
        let discard = t == "D" || t == "DISCARD";
        if save {
            Some(CmdResult::Relaunch("REFCLOSE_SAVE".into(), vec![]))
        } else if discard {
            Some(CmdResult::Relaunch("REFCLOSE_DISCARD".into(), vec![]))
        } else {
            Some(CmdResult::NeedPoint) // re-prompt
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        // Default: SAVE
        CmdResult::Relaunch("REFCLOSE_SAVE".into(), vec![])
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
}

// ── Geometry helpers ───────────────────────────────────────────────────────

/// Apply the INSERT's forward transform to a block-local entity so it
/// appears at its correct world-space position. The full affine handles
/// OCS, rotation and non-uniform / mirrored scale at once.
pub fn apply_insert_transform(entity: &mut EntityType, session: &RefEditSession) {
    entity.as_entity_mut().apply_transform(&session.forward);
}

/// Apply the INSERT's inverse transform to a world-space entity to bring it
/// back to block-local coordinates.
pub fn apply_insert_inverse_transform(entity: &mut EntityType, session: &RefEditSession) {
    entity.as_entity_mut().apply_transform(&session.inverse);
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["REFCLOSE"] });  // RefCloseCommand
inventory::submit!(crate::command::CommandRegistration { names: &["REFEDIT"] });  // RefEditPickCommand
