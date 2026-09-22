// BEDIT — block editor as a dedicated "space" tab (issue #261).
//
// Workflow:
//   1. BEDIT: user picks an INSERT (or has one pre-selected).
//   2. A block-editor space opens: rendering, picking and new geometry are
//      scoped to the block's record, so ONLY the block's own (block-local)
//      entities are drawn — no INSERT transform, they show exactly as stored.
//   3. The user edits them with normal commands. Because they ARE the block
//      definition's entities, edits are live.
//   4. Save Block (BEDIT_SAVE): keep the edits and leave the block space.
//      Discard (BEDIT_DISCARD): restore the block to its on-entry snapshot.
//
// Unlike REFEDIT (which copies entities into model space with the INSERT
// transform and inverse-transforms them back on save), a BEDIT space edits the
// block record in place, so there is no copy-back and no transform — the space
// render filter (`current_layout_block_handle`) already draws only the block.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// Right-edge side-toolbar buttons shown while a BEDIT block-editor space is
/// active: save the block edits and leave, or discard them.
pub fn block_edit_tools() -> Vec<ToolDef> {
    vec![
        crate::modules::ribbon_command("BEDIT_SAVE"),
        crate::modules::ribbon_command("BEDIT_DISCARD"),
    ]
}

// ── Session state (held in DocumentTab) ───────────────────────────────────

/// One open BEDIT block-editor tab. Lives in `DocumentTab::block_edits`.
#[derive(Clone)]
pub struct BlockEditSession {
    /// Name of the block being edited (also the space-tab label).
    pub block_name: String,
    /// Handle of the block record whose entities are being edited.
    pub br_handle: Handle,
    /// Space to return to when the block editor closes (the layout that was
    /// active when BEDIT began).
    pub return_layout: String,
    /// Parent block tab to return to for a nested BEDIT session. `None` returns
    /// to `return_layout`.
    pub return_block: Option<String>,
    /// The complete block definition captured on entry, so Discard restores
    /// structural markers and content with their original handles.
    pub snapshot: Vec<EntityType>,
    /// Baked anonymous dimension-block entities transformed alongside
    /// dimensions in this definition. They remain in place, so Discard can
    /// restore their shared before-images directly.
    pub dependent_snapshot: Vec<std::sync::Arc<EntityType>>,
    /// Inline ATTRIB values/positions on every existing INSERT of this block.
    /// Rebasing the origin moves them with the definition; Discard restores
    /// their exact on-entry state.
    pub reference_attributes:
        Vec<(Handle, Vec<acadrust::entities::AttributeEntity>)>,
    /// Camera state of the space that was active when BEDIT began, restored
    /// on Save/Discard so the view returns exactly where it was (#425).
    pub return_camera: crate::scene::view::camera::Camera,
    /// Camera owned by this block tab. Updated whenever another space tab is
    /// activated, then restored when the user returns to this block.
    pub editor_camera: crate::scene::view::camera::Camera,
    /// Transient UCS being manipulated in this block tab. On commit its full
    /// frame is baked into the definition and this returns to `None`.
    pub editor_ucs: Option<acadrust::tables::Ucs>,
}

// ── BEDIT pick command ─────────────────────────────────────────────────────

/// Step 1: wait for the user to pick a single INSERT entity to edit.
pub struct BlockEditPickCommand;

impl BlockEditPickCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for BlockEditPickCommand {
    fn name(&self) -> &'static str {
        "BEDIT"
    }
    fn prompt(&self) -> String {
        crate::t!("BEDIT  Select block reference to edit:").into_owned()
    }
    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Signal the host to open the block-editor space for this INSERT.
        CmdResult::Relaunch(format!("BEDIT_BEGIN:{}", handle.value()), vec![handle])
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["BEDIT"] }); // BlockEditPickCommand
inventory::submit!(crate::command::CommandRegistration { names: &["BEDIT_SAVE"] });
inventory::submit!(crate::command::CommandRegistration { names: &["BEDIT_DISCARD"] });
