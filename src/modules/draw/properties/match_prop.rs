// Match Properties tool — ribbon definition + CadCommand implementation.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MATCHPROP",
        label: "Match",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/match_prop.svg")),
        event: ModuleEvent::Command("MATCHPROP".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, SelectionHoverPreview};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub struct MatchPropCommand {
    /// Source handle; NULL until phase 1 is complete.
    src: Handle,
    /// The previous run's source, still in the drawing; Enter at the source
    /// prompt takes it.
    last: Handle,
}

impl MatchPropCommand {
    /// A single preselected object can supply the source; NULL asks for a pick.
    pub fn new(src: Handle) -> Self {
        Self {
            src,
            last: Handle::NULL,
        }
    }

    pub fn with_last(mut self, last: Handle) -> Self {
        self.last = last;
        self
    }

    fn phase1_done(&self) -> bool {
        !self.src.is_null()
    }

    /// The source is set: the active settings line comes before the
    /// destination prompt.
    fn source_taken() -> CmdResult {
        CmdResult::ReportMeasurement(crate::entities::match_props::active_settings_line(
            crate::entities::match_props::match_mode(),
        ))
    }
}

impl CadCommand for MatchPropCommand {
    fn name(&self) -> &'static str {
        "MATCHPROP"
    }

    fn prompt(&self) -> String {
        if !self.phase1_done() {
            crate::t!("MATCHPROP  Select source object:").into_owned()
        } else {
            crate::t!("MATCHPROP  Select destination object(s) or [Settings]:").into_owned()
        }
    }

    // ── Phase 1: single entity pick ───────────────────────────────────────

    fn needs_entity_pick(&self) -> bool {
        !self.phase1_done()
    }

    /// A typed coordinate at the source prompt picks the object there.
    fn typed_point_picks_entity(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_includes_fills(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_acquire_previews(&self, scene: &Scene, handle: Handle) -> Vec<WireModel> {
        if self.phase1_done() || handle.is_null() {
            return vec![];
        }
        scene.matchprop_source_highlight_wires(handle)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint; // nothing hit, keep waiting
        }
        self.src = handle;
        Self::source_taken() // move to phase 2 (selection gathering)
    }

    // ── Phase 2: destination selection gathering ──────────────────────────

    fn is_selection_gathering(&self) -> bool {
        self.phase1_done()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::MatchProperties {
            dest: handles,
            src: self.src,
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.phase1_done() {
            vec![CmdOption::new(crate::t!("Settings").as_ref(), "S")]
        } else {
            Vec::new()
        }
    }

    fn wants_text_input(&self) -> bool {
        self.phase1_done()
    }

    /// S / Settings opens the Property Settings window over the command.
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        (self.phase1_done() && !word.is_empty() && "SETTINGS".starts_with(word.as_str()))
            .then_some(CmdResult::OpenMatchPropSettings(self.src))
    }

    fn selection_hover_preview_enabled(&self) -> bool {
        self.phase1_done()
    }

    fn selection_hover_preview(
        &self,
        scene: &Scene,
        hovered: Option<Handle>,
    ) -> Option<SelectionHoverPreview> {
        if !self.phase1_done() {
            return None;
        }

        let mut preview = SelectionHoverPreview::default();
        // The source is the property reference for the command's entire
        // lifetime, so keep its glow through every destination application.
        preview
            .wires
            .extend(scene.matchprop_source_highlight_wires(self.src));
        if let Some(target) = hovered.filter(|target| *target != self.src) {
            if let Some((mut wires, hatches)) = scene.matchprop_target_preview(self.src, target) {
                preview
                    .wires
                    .extend(scene.matchprop_target_glow_wires(&wires));
                preview.wires.append(&mut wires);
                preview.hatches = hatches;
            }
        }
        Some(preview)
    }

    fn on_enter(&mut self) -> CmdResult {
        if !self.phase1_done() && !self.last.is_null() {
            self.src = self.last;
            return Self::source_taken();
        }
        CmdResult::Cancel
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["MATCHPROP"]
}); // MatchPropCommand
