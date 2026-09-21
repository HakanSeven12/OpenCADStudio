//! Status-bar customization — which toggle pills are shown on the bar.
//!
//! The customization menu (opened from the bar's far-right handle) lists every
//! pill with a check mark next to the ones currently shown. Toggling a row
//! adds or removes that pill from the bar. The choice is persisted so it
//! survives across sessions.

use rustc_hash::FxHashSet as HashSet;
use serde::{Deserialize, Serialize};

/// Identifies a toggleable status-bar pill.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum StatusPill {
    SpaceMouse,
    Coords,
    Ortho,
    Lwt,
    Polar,
    Dyn,
    Otrack,
    Osnap,
    Space,
    Scale,
    AnnoVisibility,
    AnnoAutoAdd,
    VpScaleSync,
    Units,
    Transparency,
    Isolate,
    QuickProps,
    SelFilter,
    SelCycle,
    Vp,
    CleanScreen,
}

impl StatusPill {
    /// Every pill, in status-bar display order. Drives both the bar layout and
    /// the customization menu.
    pub const ALL: &'static [StatusPill] = &[
        StatusPill::SpaceMouse,
        StatusPill::Coords,
        StatusPill::Ortho,
        StatusPill::Lwt,
        StatusPill::Polar,
        StatusPill::Dyn,
        StatusPill::Otrack,
        StatusPill::Osnap,
        StatusPill::Space,
        StatusPill::Scale,
        StatusPill::AnnoVisibility,
        StatusPill::AnnoAutoAdd,
        StatusPill::VpScaleSync,
        StatusPill::Units,
        StatusPill::Transparency,
        StatusPill::Isolate,
        StatusPill::QuickProps,
        StatusPill::SelFilter,
        StatusPill::SelCycle,
        StatusPill::Vp,
        StatusPill::CleanScreen,
    ];

    /// Stable identifier used for persistence.
    pub fn id(self) -> &'static str {
        match self {
            StatusPill::SpaceMouse => "spacemouse",
            StatusPill::Coords => "coords",
            StatusPill::Ortho => "ortho",
            StatusPill::Lwt => "lwt",
            StatusPill::Polar => "polar",
            StatusPill::Dyn => "dyn",
            StatusPill::Otrack => "otrack",
            StatusPill::Osnap => "osnap",
            StatusPill::Space => "space",
            StatusPill::Scale => "scale",
            StatusPill::AnnoVisibility => "anno_visibility",
            StatusPill::AnnoAutoAdd => "anno_auto_add",
            StatusPill::VpScaleSync => "vp_scale_sync",
            StatusPill::Units => "units",
            StatusPill::Transparency => "transparency",
            StatusPill::Isolate => "isolate",
            StatusPill::QuickProps => "quickprops",
            StatusPill::SelFilter => "selfilter",
            StatusPill::SelCycle => "selcycle",
            StatusPill::Vp => "vp",
            StatusPill::CleanScreen => "cleanscreen",
        }
    }

    /// Label shown in the customization menu.
    pub fn label(self) -> &'static str {
        match self {
            StatusPill::SpaceMouse => "SpaceMouse",
            StatusPill::Coords => "Coordinates",
            StatusPill::Ortho => "Ortho Mode",
            StatusPill::Lwt => "Show Lineweight",
            StatusPill::Polar => "Polar Tracking",
            StatusPill::Dyn => "Dynamic Input",
            StatusPill::Otrack => "Object Snap Tracking",
            StatusPill::Osnap => "Object Snap",
            StatusPill::Space => "Model/Paper Space",
            StatusPill::Scale => "Annotation Scale",
            StatusPill::AnnoVisibility => "Show Annotation Objects",
            StatusPill::AnnoAutoAdd => "Automatically Add Scales",
            StatusPill::VpScaleSync => "Viewport / Annotation Scale Sync",
            StatusPill::Units => "Drawing Units",
            StatusPill::Transparency => "Show Transparency",
            StatusPill::Isolate => "Isolate Objects",
            StatusPill::QuickProps => "Quick Properties",
            StatusPill::SelFilter => "Selection Filtering",
            StatusPill::SelCycle => "Selection Cycling",
            StatusPill::Vp => "Viewport Count",
            StatusPill::CleanScreen => "Clean Screen",
        }
    }

}

/// Tracks which pills the user has hidden. Serialized as the "statusbar" section
/// of the app config ([`crate::app::config`]).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    hidden: HashSet<StatusPill>,
}

impl Default for StatusBarConfig {
    /// The out-of-the-box bar: a few informational / niche pills are hidden by
    /// default to keep it uncluttered, and the user turns them on from the
    /// customization (⚙) menu if wanted. The mode toggles that matter most
    /// (Ortho, Polar, Otrack, Osnap, …) stay visible.
    fn default() -> Self {
        let hidden = [
            StatusPill::Coords,
            StatusPill::Lwt,
            StatusPill::Dyn,
            StatusPill::Space,
            StatusPill::Units,
            StatusPill::Transparency,
            StatusPill::SelCycle,
            StatusPill::Vp,
        ]
        .into_iter()
        .collect();
        Self { hidden }
    }
}

impl StatusBarConfig {
    pub fn is_visible(&self, pill: StatusPill) -> bool {
        !self.hidden.contains(&pill)
    }

    /// Flip a pill's visibility. Persistence is handled by the caller via the
    /// consolidated app config (`save_config`).
    pub fn toggle(&mut self, pill: StatusPill) {
        if !self.hidden.remove(&pill) {
            self.hidden.insert(pill);
        }
    }
}
