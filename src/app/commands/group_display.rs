//! GROUPDISPLAYMODE, a profile setting: how a group picked whole shows
//! its grips — 0 each member's own, 1 one grip at the centre of the group's
//! extents, 2 that grip and a box round the extents.

use crate::app::OpenCADStudio;
use std::sync::atomic::{AtomicI16, Ordering};

static MODE: AtomicI16 = AtomicI16::new(2);

pub(crate) fn group_display_mode() -> i16 {
    MODE.load(Ordering::Relaxed)
}

pub(crate) fn set_group_display_mode(mode: i16) {
    MODE.store(mode.clamp(0, 2), Ordering::Relaxed);
}

impl OpenCADStudio {
    /// `GROUPDISPLAYMODE [value]`: a value outside 0–2 is refused and
    /// asked for again. Grips already shown keep their look until the
    /// selection is next made, as in the reference.
    pub(super) fn group_display_setvar(&mut self, value: Option<&str>) {
        let ask = |app: &mut Self| {
            app.command_line.push_output(&format!(
                "Enter new value for GROUPDISPLAYMODE <{}>:",
                group_display_mode()
            ));
            app.pending_setvar = Some("GROUPDISPLAYMODE".to_string());
        };
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            ask(self);
            return;
        };
        match value.parse::<i16>() {
            Ok(mode @ 0..=2) => {
                if mode != group_display_mode() {
                    set_group_display_mode(mode);
                    self.save_config();
                }
            }
            _ => {
                self.command_line
                    .push_error(crate::t!("Requires an integer between 0 and 2.").as_ref());
                ask(self);
            }
        }
    }
}

impl OpenCADStudio {
    /// `MATCHMODE [value]`: the properties MATCHPROP transfers, a profile
    /// value; outside 0–262143 it is refused and asked for again.
    pub(super) fn match_mode_setvar(&mut self, value: Option<&str>) {
        use crate::entities::match_props::{match_mode, set_match_mode, MatchOpts};
        let ask = |app: &mut Self| {
            app.command_line
                .push_output(&format!("Enter new value for MATCHMODE <{}>:", match_mode()));
            app.pending_setvar = Some("MATCHMODE".to_string());
        };
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            ask(self);
            return;
        };
        match value.parse::<u32>() {
            Ok(mode) if mode <= MatchOpts::ALL_BITS => {
                if mode != match_mode() {
                    set_match_mode(mode);
                    self.save_config();
                }
            }
            _ => {
                let (min, max) = (0, MatchOpts::ALL_BITS);
                self.command_line
                    .push_error(crate::tf!("Requires an integer between {min} and {max}.").as_ref());
                ask(self);
            }
        }
    }
}
