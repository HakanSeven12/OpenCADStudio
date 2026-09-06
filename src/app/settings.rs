//! Persisted user preferences — DYN, POLAR (+ increment), OTRACK, and assorted
//! app-level flags (backup, autosave, plugin lists, viewport background). These
//! are UI choices, not drawing data, so they live in the consolidated per-user
//! config ([`crate::app::config`], the "settings" section) and survive across
//! sessions. Drawing-scoped state (Ortho `$ORTHOMODE`, running OSNAP `$OSMODE`,
//! lineweight display `$LWDISPLAY`, …) belongs to the file, not here.
//!
//! Also home to the `$OSMODE` bit conversions ([`osmode_from_snaps`] /
//! [`snaps_from_osmode`]) that bridge the running-snap set and the drawing header.

use crate::snap::SnapType;
use serde::{Deserialize, Serialize};

/// Cursor shown over the drawing viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorType {
    #[default]
    Crosshair,
    Pointer,
}

impl CursorType {
    pub const ALL: [Self; 2] = [Self::Crosshair, Self::Pointer];

    pub fn label(self) -> &'static str {
        match self {
            Self::Crosshair => "Crosshair",
            Self::Pointer => "Desktop pointer",
        }
    }
}

/// Active pair of axes while isometric drafting is enabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsoPlane {
    #[default]
    Left,
    Top,
    Right,
}

impl IsoPlane {
    pub const ALL: [Self; 3] = [Self::Left, Self::Top, Self::Right];

    pub fn next(self) -> Self {
        match self {
            Self::Left => Self::Top,
            Self::Top => Self::Right,
            Self::Right => Self::Left,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Top => "Top",
            Self::Right => "Right",
        }
    }

    /// The two drafting directions, in degrees in the active UCS plane.
    pub fn angles(self) -> [f64; 2] {
        match self {
            Self::Left => [90.0, 150.0],
            Self::Top => [30.0, 150.0],
            Self::Right => [30.0, 90.0],
        }
    }
}

/// Canonical order of the user-toggleable object-snap modes. Drives the
/// deterministic order when decoding the `$OSMODE` bitmask (see
/// [`snaps_from_osmode`]).
const SNAP_ORDER: &[SnapType] = &[
    SnapType::Endpoint,
    SnapType::Midpoint,
    SnapType::Center,
    SnapType::Node,
    SnapType::Quadrant,
    SnapType::Intersection,
    SnapType::Extension,
    SnapType::Insertion,
    SnapType::Perpendicular,
    SnapType::Tangent,
    SnapType::Nearest,
    SnapType::ApparentIntersection,
    SnapType::Parallel,
    // Grid snap (SNAPMODE) is a per-drawing view setting stored on the VPort,
    // not a global OSNAP preference, so it is deliberately excluded from the
    // persisted set. (#121)
];

/// `$OSMODE` bit for each running object-snap mode.
/// `None` for OCS-only snaps (Grid, ObjectPick) that have no standard bit.
fn snap_bit(s: SnapType) -> Option<i32> {
    Some(match s {
        SnapType::Endpoint => 1,
        SnapType::Midpoint => 2,
        SnapType::Center => 4,
        SnapType::Node => 8,
        SnapType::Quadrant => 16,
        SnapType::Intersection => 32,
        SnapType::Insertion => 64,
        SnapType::Perpendicular => 128,
        SnapType::Tangent => 256,
        SnapType::Nearest => 512,
        SnapType::ApparentIntersection => 2048,
        SnapType::Extension => 4096,
        SnapType::Parallel => 8192,
        SnapType::Grid | SnapType::ObjectPick => return None,
    })
}

/// Bit 14 of `$OSMODE` — object snap turned off (master suppress).
const OSMODE_SUPPRESS: i32 = 16384;

/// Encode the running-snap set + master toggle into an `$OSMODE` bitmask for the
/// drawing header. OCS-only snaps (Grid, ObjectPick) have no bit and are
/// dropped. A cleared master toggle sets the suppress bit.
pub(crate) fn osmode_from_snaps<'a>(
    enabled: impl IntoIterator<Item = &'a SnapType>,
    snap_enabled: bool,
) -> i32 {
    let mut bits = 0;
    for t in enabled {
        if let Some(b) = snap_bit(*t) {
            bits |= b;
        }
    }
    if !snap_enabled {
        bits |= OSMODE_SUPPRESS;
    }
    bits
}

/// Decode an `$OSMODE` bitmask into `(running-snap set, master enabled)`. Only
/// the standard mappable modes are produced; the suppress bit maps (inverted) to
/// the master toggle.
pub(crate) fn snaps_from_osmode(osmode: i32) -> (Vec<SnapType>, bool) {
    let modes = SNAP_ORDER
        .iter()
        .copied()
        .filter(|t| snap_bit(*t).is_some_and(|b| osmode & b != 0))
        .collect();
    (modes, osmode & OSMODE_SUPPRESS == 0)
}

/// The "settings" section of the consolidated config ([`crate::app::config`]).
/// Field defaults mirror the app's in-code defaults so a missing key restores
/// the value the app boots with.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub dyn_input: bool,
    pub polar: bool,
    pub polar_increment_deg: f32,
    pub zoom_wheel_reversed: bool,
    pub zoom_factor: i32,
    /// CURSORSIZE: normalized crosshair reach; 5 retains the original 60 px arms.
    pub cursor_size: i32,
    /// PICKBOX: normalized visible-box and click-aperture size.
    pub pick_box: i32,
    /// CURSORTYPE: crosshair or the platform pointer over the drawing.
    pub cursor_type: CursorType,
    /// Explicit crosshair RGB. `None` keeps automatic background contrast.
    pub crosshair_color: Option<[u8; 3]>,
    /// Model-space lineweight preview scale as a percentage.
    pub lineweight_display_scale: i32,
    /// Isometric drafting changes the grid and crosshair to the active axis pair.
    pub isometric_drafting: bool,
    pub iso_plane: IsoPlane,
    /// SNAPANG in degrees, applied in the active UCS plane.
    pub snap_angle_deg: f32,
    pub otrack: bool,
    // Ortho ($ORTHOMODE) and the running OSNAP set ($OSMODE) are per-drawing —
    // stored in the document header, not here (they used to be persisted app-
    // globally, which duplicated the file's own state).
    /// Whether the one-time "make Open CAD Studio the default for .dwg/.dxf?"
    /// prompt has already been shown. Set once the user answers (either way),
    /// so we never nag again on subsequent launches.
    pub default_assoc_prompted: bool,
    /// App version whose donation prompt has been displayed.
    pub donation_prompt_version: String,
    /// Ids of plugins the user turned off in the Plugin Manager. Disabled
    /// plugins keep their manifest listed but drop their ribbon tab and command
    /// dispatch.
    pub disabled_plugins: Vec<String>,
    /// Linked plugin source repositories (`owner/repo`) the marketplace installs
    /// from.
    pub plugin_repos: Vec<String>,
    /// Command-line literal-space mode: when on, Space stays in the input
    /// instead of submitting (as if every line started with `>`), until the
    /// user toggles it back off.
    pub literal_spaces: bool,
    /// Height of the expanded command-history editor in logical pixels.
    pub command_history_height: f32,
    /// Running object-snap set + master toggle as an `$OSMODE`-style bitmask.
    /// App-level, not per-drawing: modern DWG (R2000+) has no file slot for
    /// OSMODE (it moved to the registry), so the set follows the user. A
    /// legacy R13/R14 or DXF file carrying a nonzero `$OSMODE` still overrides
    /// it on open (see `adopt_header_sysvars`).
    pub osmode: i32,
    /// Controls whether the TEXTEDIT command repeats automatically (0 = Multiple, 1 = Single).
    pub texteditmode: bool,
    /// QDIM extension-origin priority: 0 = endpoints, 1 = intersections.
    #[serde(default)]
    pub quick_dimension_snap_priority: u8,
    /// DIMCONTINUEMODE: 1 inherits the base dimension's layer/style; 0 uses
    /// the current creation layer/style. Registry-style, app-level preference.
    #[serde(default = "default_dimension_continue_mode")]
    pub dimension_continue_mode: i16,
    /// TEXTFILL: fill TrueType glyphs (true) or draw them hollow (false).
    pub textfill: bool,
    /// When true, saving over an existing file first copies it to a sibling
    /// `<name>.bak` so a faulty or accidental save can be recovered (#205).
    pub backup_on_save: bool,
    /// When true (default), the app (re)registers itself as a .dwg/.dxf/.bak
    /// handler on every launch. Toggle with the FILEASSOC command.
    pub file_assoc_enabled: bool,
    /// Minutes between autosaves to a `.sv$` recovery file (SAVETIME command).
    /// 0 disables autosave.
    pub savetime_min: i32,
    /// File type and version used when a new/unsaved drawing is first saved.
    /// Existing drawings keep their own type and version.
    pub default_save_format: String,
    /// PICKADD (#226): `true` (default) = a plain click ADDS to the selection
    /// (Shift removes); `false` = OS-style — a click REPLACES the selection
    /// and Shift+click toggles membership.
    pub pick_add: bool,
    /// PICKDRAG (#226): `false` (default) = press-drag draws the freeform
    /// lasso; `true` = press-drag draws a rectangle marquee instead.
    pub pick_drag_rect: bool,
    /// Show the floating Quick Properties panel when objects are selected.
    pub quick_properties: bool,
    /// Persisted viewport background colours (0–255 RGB); `None` = app default
    /// (dark grey model / off-white paper). Applied to every drawing tab on
    /// launch and to tabs opened later, so a chosen background survives restarts
    /// (#188).
    pub bg_color: Option<[u8; 3]>,
    pub paper_bg_color: Option<[u8; 3]>,
    /// Interface language preference. `System` negotiates against the
    /// platform locale on every launch.
    pub language: crate::i18n::Language,
    /// CLIPROMPTLINES: how many temporary prompt lines for a single command
    /// are displayed above the command window (0–50, Registry, default 3).
    #[serde(default = "default_clipromptlines", deserialize_with = "deserialize_clipromptlines")]
    pub cliprompt_lines: i32,
    /// COMMANDLINEFADETIME: how long command-line overlay history lines stay
    /// visible, in milliseconds (0–60000, default 3000). 0 skips transient lines.
    #[serde(
        default = "default_commandline_fade_ms",
        deserialize_with = "deserialize_commandline_fade_ms"
    )]
    pub commandline_fade_ms: i32,
    /// Most-recently-inserted block names, most recent first, capped to 20.
    /// Used to rank INSERT suggestions without touching the drawing file.
    #[serde(default)]
    pub block_mru: Vec<String>,
    /// Insertion frequency per block name (uppercase key → count).
    #[serde(default)]
    pub block_freq: std::collections::HashMap<String, u32>,
}

fn default_clipromptlines() -> i32 {
    3
}

fn default_commandline_fade_ms() -> i32 {
    3000
}

fn deserialize_commandline_fade_ms<'de, D>(de: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = i32::deserialize(de).unwrap_or(3000);
    Ok(v.clamp(0, 60000))
}

pub fn clamp_commandline_fade_ms(v: i32) -> i32 {
    v.clamp(0, 60000)
}

fn default_dimension_continue_mode() -> i16 {
    1
}

fn deserialize_clipromptlines<'de, D>(de: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = i32::deserialize(de).unwrap_or(3);
    Ok(v.clamp(0, 50))
}

pub fn clamp_clipromptlines(v: i32) -> i32 {
    v.clamp(0, 50)
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            dyn_input: true,
            polar: false,
            polar_increment_deg: 45.0,
            zoom_wheel_reversed: false,
            zoom_factor: 60,
            cursor_size: 5,
            pick_box: 3,
            cursor_type: CursorType::Crosshair,
            crosshair_color: None,
            lineweight_display_scale: 100,
            isometric_drafting: false,
            iso_plane: IsoPlane::Left,
            snap_angle_deg: 0.0,
            otrack: false,
            default_assoc_prompted: false,
            donation_prompt_version: String::new(),
            disabled_plugins: Vec::new(),
            plugin_repos: Vec::new(),
            literal_spaces: false,
            command_history_height: crate::ui::command_line::HISTORY_HEIGHT_DEFAULT,
            // Snapper::default(): END|MID|CEN|NODE|QUAD|INT|NEA (575), master
            // off (suppress bit 16384).
            osmode: 575 | OSMODE_SUPPRESS,
            texteditmode: false,
            quick_dimension_snap_priority: 0,
            dimension_continue_mode: 1,
            textfill: true,
            backup_on_save: true,
            file_assoc_enabled: true,
            savetime_min: 10,
            default_save_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            pick_add: true,
            pick_drag_rect: false,
            quick_properties: false,
            bg_color: None,
            paper_bg_color: None,
            language: crate::i18n::Language::default(),
            cliprompt_lines: 3,
            commandline_fade_ms: 3000,
            block_mru: Vec::new(),
            block_freq: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osmode_encodes_bits_and_suppress() {
        // Endpoint(1) + Midpoint(2) + Intersection(32) = 35, master on.
        let on = [SnapType::Endpoint, SnapType::Midpoint, SnapType::Intersection];
        assert_eq!(osmode_from_snaps(on.iter(), true), 35);
        // Master off sets the suppress bit (16384).
        assert_eq!(osmode_from_snaps(on.iter(), false), 35 | 16384);
        // OCS-only snaps carry no bit and are dropped.
        assert_eq!(osmode_from_snaps([SnapType::Grid, SnapType::ObjectPick].iter(), true), 0);
    }

    #[test]
    fn osmode_decodes_bits_and_suppress() {
        let (modes, enabled) = snaps_from_osmode(35);
        let set: std::collections::HashSet<_> = modes.into_iter().collect();
        assert!(enabled);
        assert_eq!(set.len(), 3);
        assert!(set.contains(&SnapType::Endpoint));
        assert!(set.contains(&SnapType::Midpoint));
        assert!(set.contains(&SnapType::Intersection));
        // Suppress bit → master off; the mode bits still decode.
        let (_m, en) = snaps_from_osmode(35 | 16384);
        assert!(!en);
    }

    #[test]
    fn osmode_round_trips_every_mappable_mode() {
        // All 13 running snaps + master on encode and decode back to the same set.
        let all: Vec<SnapType> = SNAP_ORDER.to_vec();
        let bits = osmode_from_snaps(all.iter(), true);
        let (back, enabled) = snaps_from_osmode(bits);
        assert!(enabled);
        let a: std::collections::HashSet<_> = all.into_iter().collect();
        let b: std::collections::HashSet<_> = back.into_iter().collect();
        assert_eq!(a, b);
    }
}
