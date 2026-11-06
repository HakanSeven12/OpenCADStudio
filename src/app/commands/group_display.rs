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
            app.command_line.push_output(&crate::ui::command_line::value_prompt("GROUPDISPLAYMODE", &group_display_mode()
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
                    .push_rejection(crate::t!("Requires an integer between 0 and 2.").as_ref());
                ask(self);
            }
        }
    }
}

impl OpenCADStudio {
    /// `MATCHMODE [value]`: the properties MATCHPROP transfers, a profile
    /// value. Any whole number from 0 is kept as given; anything else is
    /// refused and asked for again.
    pub(super) fn match_mode_setvar(&mut self, value: Option<&str>) {
        use crate::entities::match_props::{match_mode, set_match_mode};
        let ask = |app: &mut Self| {
            let (name, current) = ("MATCHMODE", match_mode());
            app.command_line
                .push_output(&crate::tf!("Enter new value for {name} <{current}>:"));
            app.pending_setvar = Some("MATCHMODE".to_string());
        };
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            ask(self);
            return;
        };
        // A larger whole number keeps its low 32 bits, as the reference does
        // (4294967296 → 0, 99999999999 → 1215752191).
        let wrapped = value.parse::<u128>().ok().map(|v| (v % (1u128 << 32)) as u32);
        match wrapped.ok_or(()) {
            Ok(mode) => {
                if mode != match_mode() {
                    set_match_mode(mode);
                    self.save_config();
                }
            }
            Err(_) => {
                self.command_line
                    .push_rejection(crate::t!("Requires an integer value.").as_ref());
                ask(self);
            }
        }
    }

    /// `OFFSETGAPTYPE [value]`, a profile setting: 0 extends, 1 fillets and
    /// 2 chamfers the corners of a polyline offset outward. A value outside
    /// 0–2 is refused and asked for again.
    pub(super) fn offset_gap_setvar(&mut self, value: Option<&str>) {
        use crate::modules::draw::modify::offset::{gap_type, set_gap_type};
        let ask = |app: &mut Self| {
            let (name, current) = ("OFFSETGAPTYPE", gap_type());
            app.command_line
                .push_output(&crate::tf!("Enter new value for {name} <{current}>:"));
            app.pending_setvar = Some("OFFSETGAPTYPE".to_string());
        };
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            ask(self);
            return;
        };
        match value.parse::<u8>() {
            Ok(gap @ 0..=2) => {
                if gap != gap_type() {
                    set_gap_type(gap);
                    self.save_config();
                }
            }
            _ => {
                self.command_line
                    .push_rejection(crate::t!("Requires an integer between 0 and 2.").as_ref());
                ask(self);
            }
        }
    }
}
