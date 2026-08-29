// Paste tool — ribbon definition + CadCommand implementation.

use crate::modules::IconKind;

/// Shared icon for the Paste button and its dropdown entries.
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/paste.svg"));

/// Paste-menu entries: (command id, label, icon). The first is the default
/// fired by clicking the button body; the rest open from the ▾.
pub const MENU_ITEMS: &[(&str, &str, IconKind)] = &[
    ("PASTECLIP", "Paste", ICON),
    ("PASTEORIG", "Paste to Original Coordinates", ICON),
    ("PASTEBLOCK", "Paste as Block", ICON),
];

// ── CadCommand implementation ─────────────────────────────────────────────

use acadrust::Handle;
use glam::{DVec3, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct PasteCommand {
    /// Wire models of the clipboard entities (used for the ghost preview).
    /// Emptied when `bbox_wire` is used, so a huge clipboard isn't kept twice.
    wires: Vec<WireModel>,
    /// Lightweight bounding-box outline that replaces `wires` for very large
    /// clipboards. `on_preview_wires` runs every mouse move and translates every
    /// point of the ghost; for a whole-drawing paste that is O(hundreds of
    /// thousands) per frame and freezes placement. Above a point budget we ghost
    /// just this 5-point rectangle instead — O(1) per frame. `None` = ghost the
    /// full wires.
    bbox_wire: Option<WireModel>,
    /// Paste anchor — lower-left corner of the clipboard entities (offset origin
    /// for translation; the point that tracks the cursor).
    base: Vec3,
}

impl PasteCommand {
    /// `on_preview_wires` clones and re-uploads the whole ghost every cursor
    /// move, so its cost scales with the wire count and the total point count.
    /// Past either budget the per-move work floods the event loop, so switch to
    /// a bounding-box outline; below them the full ghost still shows.
    const MAX_PREVIEW_WIRES: usize = 20_000;
    const MAX_PREVIEW_POINTS: usize = 300_000;

    pub fn new(wires: Vec<WireModel>, base: Vec3) -> Self {
        let total_points: usize = wires.iter().map(|w| w.points.len()).sum();
        let too_heavy =
            wires.len() > Self::MAX_PREVIEW_WIRES || total_points > Self::MAX_PREVIEW_POINTS;
        if too_heavy {
            if let Some(bbox_wire) = Self::bbox_outline(&wires) {
                // Drop the full wires — the box is all the ghost needs now.
                return Self { wires: Vec::new(), bbox_wire: Some(bbox_wire), base };
            }
        }
        Self { wires, bbox_wire: None, base }
    }

    /// Build a closed rectangle outline around the XY extent of `wires`, in the
    /// same world coordinates so the standard `translated(delta)` shift applies.
    /// Uses the double-single `points + points_low` sum so the box stays precise
    /// at UTM-scale coordinates. Returns `None` when there are no points.
    fn bbox_outline(wires: &[WireModel]) -> Option<WireModel> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut any = false;
        for w in wires {
            for (i, p) in w.points.iter().enumerate() {
                let lo = w.points_low.get(i).copied().unwrap_or([0.0; 3]);
                for k in 0..3 {
                    let v = p[k] as f64 + lo[k] as f64;
                    min[k] = min[k].min(v);
                    max[k] = max[k].max(v);
                }
                any = true;
            }
        }
        if !any {
            return None;
        }
        let z = min[2];
        let pts = vec![
            [min[0], min[1], z],
            [max[0], min[1], z],
            [max[0], max[1], z],
            [min[0], max[1], z],
            [min[0], min[1], z],
        ];
        Some(WireModel::solid_f64(
            "paste_bbox".into(),
            pts,
            WireModel::CYAN,
            false,
        ))
    }
}

impl CadCommand for PasteCommand {
    fn name(&self) -> &'static str {
        "PASTECLIP"
    }

    fn prompt(&self) -> String {
        t!("PASTECLIP  Pick insertion point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::PasteClipboard { base_pt: pt }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let pt = pt.as_vec3();
        let delta = pt - self.base;
        // Large clipboard: ghost just the bounding box — O(1) per frame.
        if let Some(bbox) = &self.bbox_wire {
            return vec![bbox.translated(delta)];
        }
        self.wires.iter().map(|w| w.translated(delta)).collect()
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["PASTE", "PASTECLIP"] });  // PasteCommand
inventory::submit!(crate::command::CommandRegistration { names: &["PASTEORIG"] });
inventory::submit!(crate::command::CommandRegistration { names: &["PASTEBLOCK"] });
