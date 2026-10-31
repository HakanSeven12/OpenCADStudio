//! Entity-level options outside the MTEXT source: the background mask
//! (Background menu) and the drawing direction (Position menu). A draft of
//! `None` writes nothing: the stored fields, the frame bit (0x10) and the
//! mask transparency are always kept unless the user changed that very option.
use super::*;
use crate::app::OpenCADStudio;
use codec::entities::DrawingDirection;
use codec::types::Color;

/// Mask flags: 0x01 fill on, 0x02 with the drawing window colour.
const FILL: i32 = 0x01;
const WINDOW: i32 = 0x02;

/// The Background menu's state while it differs from the stored mask.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskDraft {
    pub on: bool,
    /// Drawing background colour (AutoCAD's flags 3) or `color` (flags 1).
    pub window: bool,
    pub color: Color,
    /// The border offset factor field, as typed.
    pub scale: String,
}

impl MTextEditorState {
    /// A multileader's text has no MTEXT mask or drawing direction of its own.
    pub(in crate::app) fn entity_options_supported(&self) -> bool {
        !(self.editing.is_some() && self.original.is_none())
    }

    /// The mask the menu shows: the draft, else the stored or default one.
    pub(in crate::app) fn mask(&self) -> MaskDraft {
        if let Some(draft) = &self.mask_draft {
            return draft.clone();
        }
        let (flags, scale, color) = self
            .original
            .as_ref()
            .map_or((0, 1.5, Color::ByLayer), |m| {
                (
                    m.background_fill_flags,
                    m.background_scale,
                    m.background_color.clone(),
                )
            });
        MaskDraft {
            on: flags & (FILL | WINDOW) != 0,
            window: flags & FILL == 0 || flags & WINDOW != 0,
            color,
            scale: super::raw_span::number(scale),
        }
    }

    fn stored_mask_scale(&self) -> f64 {
        self.original.as_ref().map_or(1.5, |m| m.background_scale)
    }

    /// A new border factor is 1–5; an untouched stored value is kept as is.
    pub(in crate::app) fn mask_scale_value(&self) -> Option<f64> {
        let draft = self.mask();
        if draft.scale == super::raw_span::number(self.stored_mask_scale()) {
            return Some(self.stored_mask_scale());
        }
        draft
            .scale
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| (1.0..=5.0).contains(v))
    }

    /// The drawing direction the Position menu checks.
    pub(in crate::app) fn direction_value(&self) -> DrawingDirection {
        self.direction.unwrap_or_else(|| {
            self.original
                .as_ref()
                .map_or(DrawingDirection::LeftToRight, |m| m.drawing_direction)
        })
    }

    /// Write the drafts into a built entity; nothing else is touched.
    pub(super) fn apply_entity_options(&self, mt: &mut MText) {
        if let Some(direction) = self.direction {
            mt.drawing_direction = direction;
        }
        let Some(draft) = &self.mask_draft else {
            return;
        };
        let mut flags = mt.background_fill_flags & !(FILL | WINDOW);
        if draft.on {
            flags |= if draft.window { FILL | WINDOW } else { FILL };
        }
        mt.background_fill_flags = flags;
        if draft.on && !draft.window {
            mt.background_color = draft.color.clone();
        }
        if let Some(scale) = self.mask_scale_value() {
            mt.background_scale = scale;
        }
    }

    fn edit_mask(&mut self, change: impl FnOnce(&mut MaskDraft)) {
        let mut draft = self.mask();
        change(&mut draft);
        if draft != self.mask() {
            self.record_undo();
            self.mask_draft = Some(draft);
        }
    }
}

impl OpenCADStudio {
    fn with_options(&mut self, change: impl FnOnce(&mut MTextEditorState)) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        if !ed.entity_options_supported() || self.command_mtext_input {
            return;
        }
        change(ed);
        self.rebuild_mtext_preview();
    }

    pub(in crate::app) fn mtext_mask_toggle(&mut self) {
        self.with_options(|ed| ed.edit_mask(|d| d.on = !d.on));
    }

    /// Drawing background colour, or (`Some`) a colour of its own.
    pub(in crate::app) fn mtext_mask_color(&mut self, color: Option<Color>) {
        self.with_options(|ed| {
            if !ed.mask().on {
                return;
            }
            ed.edit_mask(|d| match color {
                Some(color) => {
                    d.window = false;
                    d.color = color;
                }
                None => d.window = true,
            });
        });
    }

    pub(in crate::app) fn mtext_mask_scale(&mut self, value: String) {
        self.with_options(|ed| ed.edit_mask(|d| d.scale = value));
    }

    /// Normal, vertical or by style: the one standard per-object direction.
    pub(in crate::app) fn mtext_direction(&mut self, direction: DrawingDirection) {
        self.with_options(|ed| {
            if ed.direction_value() != direction {
                ed.record_undo();
                ed.direction = Some(direction);
            }
        });
    }
}
