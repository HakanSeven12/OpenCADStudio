//! Object rotation only. The pinned codec derives DWG angles with XY atan2;
//! arbitrary text planes cannot be certified without a wider renderer change.
use super::*;

impl MTextEditorState {
    /// Rotation can be set: an MTEXT (not a multileader's text) on the
    /// drawing's XY plane.
    pub(in crate::app) fn rotation_supported(&self) -> bool {
        self.rotation_plane_supported
            && !(self.editing.is_some() && self.original.is_none())
            && self.original.as_ref().is_none_or(|m| {
                m.normal == Vector3::new(0.0, 0.0, 1.0)
                    && m.dwg_x_direction.is_none_or(|d| d.z == 0.0)
            })
    }

    /// The rotation to save in radians: the opened one until the field is
    /// edited, then the typed angle, `None` while that is not a number.
    pub(in crate::app) fn rotation_input_value(&self) -> Option<f64> {
        if !self.rotation_dirty {
            return Some(self.original.as_ref().map_or(0.0, |m| m.rotation));
        }
        if !self.rotation_supported() {
            return None;
        }
        self.rotation
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .map(f64::to_radians)
            .filter(|v| v.is_finite())
    }

    /// Write an edited rotation onto `mt`. The stored DWG direction belongs
    /// to the old angle, so it goes and the codec derives it again.
    pub(super) fn patch_rotation(&self, mt: &mut MText) {
        if self.rotation_dirty {
            if let Some(value) = self.rotation_input_value() {
                if mt.rotation != value {
                    mt.rotation = value;
                    mt.dwg_x_direction = None;
                }
            }
        }
    }
}

impl crate::app::OpenCADStudio {
    /// The rotation field: kept as typed; refused where rotation is not
    /// supported, leaving the field and the text as they were.
    pub(in crate::app) fn mtext_rotation(&mut self, value: String) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        if self.command_mtext_input || !ed.rotation_supported() {
            self.command_line
                .push_error(&crate::tr!("mtext-source-errors", "rotation-unsupported"));
            return;
        }
        ed.rotation = value;
        ed.rotation_dirty = true;
        self.rebuild_mtext_preview();
    }
}
