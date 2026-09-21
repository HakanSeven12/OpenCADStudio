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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutoConstraintKind {
    Coincident,
    Collinear,
    Parallel,
    Perpendicular,
    Tangent,
    Concentric,
    Horizontal,
    Vertical,
    Equal,
}

impl AutoConstraintKind {
    pub const ALL: [Self; 9] = [
        Self::Coincident,
        Self::Collinear,
        Self::Parallel,
        Self::Perpendicular,
        Self::Tangent,
        Self::Concentric,
        Self::Horizontal,
        Self::Vertical,
        Self::Equal,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Coincident => "Coincident",
            Self::Collinear => "Collinear",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Tangent => "Tangent",
            Self::Concentric => "Concentric",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Equal => "Equal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConstrainSettings {
    pub priority: Vec<AutoConstraintKind>,
    pub enabled: Vec<AutoConstraintKind>,
    pub tangent_must_share_point: bool,
    pub perpendicular_must_intersect: bool,
    pub distance_tolerance: f64,
    pub angle_tolerance_deg: f64,
}

impl Default for AutoConstrainSettings {
    fn default() -> Self {
        Self {
            priority: AutoConstraintKind::ALL.to_vec(),
            // Keep Equal available without creating redundant relations
            // between equal-length segments by default.
            enabled: AutoConstraintKind::ALL
                .into_iter()
                .filter(|kind| *kind != AutoConstraintKind::Equal)
                .collect(),
            tangent_must_share_point: true,
            perpendicular_must_intersect: true,
            distance_tolerance: 0.05,
            angle_tolerance_deg: 1.0,
        }
    }
}

impl AutoConstrainSettings {
    pub fn sanitize(&mut self) {
        let mut priority = Vec::with_capacity(AutoConstraintKind::ALL.len());
        for kind in self
            .priority
            .iter()
            .copied()
            .chain(AutoConstraintKind::ALL)
        {
            if !priority.contains(&kind) {
                priority.push(kind);
            }
        }
        self.priority = priority;
        self.enabled
            .retain(|kind| AutoConstraintKind::ALL.contains(kind));
        self.enabled.sort_by_key(|kind| {
            self.priority
                .iter()
                .position(|candidate| candidate == kind)
                .unwrap_or(usize::MAX)
        });
        self.enabled.dedup();
        if !self.distance_tolerance.is_finite() || self.distance_tolerance < 0.0 {
            self.distance_tolerance = 0.05;
        }
        if !self.angle_tolerance_deg.is_finite() || self.angle_tolerance_deg < 0.0 {
            self.angle_tolerance_deg = 1.0;
        }
    }
}

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

/// What a right-click in the drawing area does (SHORTCUTMENU in commercial solutions /
/// "Right-click Customization").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RightClickMode {
    /// The default of commercial solutions: a right-click always opens the shortcut menu (Enter /
    /// Cancel / the command's options while a command runs; Repeat / edit
    /// tools when idle).
    #[default]
    ShortcutMenu,
    /// The "time-sensitive right-click" of commercial solutions: a quick click is Enter (or
    /// repeats the last command when idle); holding the button longer than
    /// `right_click_hold_ms` opens the shortcut menu.
    TimeSensitive,
    /// Original Open CAD Studio behaviour: while a command runs the first
    /// right-click is Enter and a second consecutive one opens the menu;
    /// when idle a right-click opens the menu.
    EnterFirst,
}

impl RightClickMode {
    pub const ALL: [Self; 3] = [Self::ShortcutMenu, Self::TimeSensitive, Self::EnterFirst];

    pub fn label(self) -> &'static str {
        match self {
            Self::ShortcutMenu => "Shortcut menu",
            Self::TimeSensitive => "Time-sensitive (quick click = Enter)",
            Self::EnterFirst => "Enter first, second click = menu",
        }
    }
}

/// SHORTCUTMENUDURATION bounds: below 100 ms every click reads as a hold,
/// above 1000 ms the menu becomes unreachable in practice.
pub fn clamp_right_click_hold_ms(v: i32) -> i32 {
    v.clamp(100, 1000)
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
/// `None` for OCS-only snaps (Grid, ObjectPick) and the 3D solid snaps
/// (Vertex, EdgeMidpoint, FaceCenter, Knot, FacePerpendicular, NearestFace),
/// which live in the separate 3D set and have no standard bit.
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
        SnapType::Grid
        | SnapType::ObjectPick
        | SnapType::Vertex
        | SnapType::EdgeMidpoint
        | SnapType::FaceCenter
        | SnapType::Knot
        | SnapType::FacePerpendicular
        | SnapType::NearestFace => return None,
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

fn deserialize_options_tab<'de, D>(
    deserializer: D,
) -> Result<crate::ui::window::options::OptionsTab, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use crate::ui::window::options::OptionsTab;
    use serde::Deserialize;
    let name = String::deserialize(deserializer).unwrap_or_default();
    Ok(match name.as_str() {
        "files" => OptionsTab::Files,
        "open-and-save" => OptionsTab::OpenAndSave,
        "display" => OptionsTab::Display,
        "drafting" => OptionsTab::Drafting,
        "modeling" => OptionsTab::Modeling,
        "selection" => OptionsTab::Selection,
        "user-preferences" => OptionsTab::UserPreferences,
        _ => OptionsTab::General,
    })
}

/// Render a drafting angle without a trailing `.0`, so `22.5` but `30`.
///
/// The polar pop-up formats its presets the same way; both are showing the
/// same kind of number to the same person.
pub fn format_snap_angle(deg: f32) -> String {
    if (deg - deg.round()).abs() < 1e-4 {
        format!("{}", deg.round() as i32)
    } else {
        format!("{deg}")
    }
}

/// GRIPOBJLIMIT default: past this many selected objects, no grips are drawn.
pub const DEFAULT_GRIP_OBJECT_LIMIT: i32 = 100;

/// The "settings" section of the consolidated config ([`crate::app::config`]).
/// Field defaults mirror the app's in-code defaults so a missing key restores
/// the value the app boots with.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub spacemouse: crate::input::spacemouse::Preferences,
    pub dyn_input: bool,
    pub polar: bool,
    pub polar_increment_deg: f32,
    pub zoom_wheel_reversed: bool,
    pub zoom_factor: i32,
    /// CURSORSIZE: normalized crosshair reach; 5 retains the original 60 px arms.
    pub cursor_size: i32,
    /// PICKBOX: normalized visible-box and click-aperture size.
    pub pick_box: i32,
    /// When true, double-clicking a block reference starts REFEDIT instead of BEDIT.
    pub double_click_block_refedit: bool,
    /// When true, double-clicking a block with attributes opens ATTEDIT.
    pub double_click_block_attedit: bool,
    /// What a right-click in the drawing area does (SHORTCUTMENU).
    pub right_click_mode: RightClickMode,
    /// Hold duration that turns a time-sensitive right-click into the menu,
    /// in milliseconds (SHORTCUTMENUDURATION, 100..=1000).
    pub right_click_hold_ms: i32,
    /// GRIPOBJLIMIT: past this many selected objects, no grips are drawn at
    /// all. 0 means no limit. The drawing header carries no slot for it.
    pub grip_object_limit: i32,
    /// Nested-copy symbol handling: false inserts, true binds.
    pub ncopy_bind: bool,
    /// Last Options page; unknown saved names fall back without rejecting the config.
    #[serde(default, deserialize_with = "deserialize_options_tab")]
    pub options_tab: crate::ui::window::options::OptionsTab,
    /// Show the navigation cube (NAVVCUBE).
    pub show_viewcube: bool,
    /// Show the UCS icon (UCSICON).
    pub show_ucs_icon: bool,
    /// Draw the UCS icon at the origin rather than in the corner
    /// (UCSICON ORigin / NOorigin).
    pub ucs_icon_at_origin: bool,
    /// Selection cycling: a click where objects overlap opens a picker.
    pub selection_cycling: bool,
    /// CURSORTYPE: crosshair or the platform pointer over the drawing.
    pub cursor_type: CursorType,
    /// Explicit crosshair RGB. `None` keeps automatic background contrast.
    pub crosshair_color: Option<[u8; 3]>,
    /// Model-space lineweight preview scale as a percentage.
    pub lineweight_display_scale: i32,
    /// Isometric drafting changes the grid and crosshair to the active axis
    /// pair. Session-only: a persisted On turned every drawing's crosshair
    /// into the isoplane pair (one vertical, one diagonal line) on every
    /// launch, with no visible control to switch it back off. Turn it on for
    /// a session with ISODRAFT; ISOPLANE keeps its persisted value.
    #[serde(skip)]
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
    /// Offer to download missing `.shx` fonts from the community repository
    /// when a drawing opens (see `crate::io::font_repo`).
    #[serde(default = "default_check_missing_fonts")]
    pub check_missing_fonts: bool,
    /// Custom font source base URL (empty = the OpenCADStudio community
    /// repository). Each missing font is fetched as `{base}/{file_name}`,
    /// so an intranet folder or a private GitHub raw folder both work.
    #[serde(default)]
    pub font_source_url: String,
    /// App version whose donation prompt has been displayed.
    pub donation_prompt_version: String,
    /// The graphics verdict (`GpuStatus::identity()`) whose warning popup the
    /// user chose not to see again. Empty = always show. Keyed by verdict so
    /// silencing "software rendering on llvmpipe" does not silence a later,
    /// different failure.
    pub gpu_warning_silenced: String,
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
    /// DELOBJ: source-geometry deletion policy (0–3). Registry-style,
    /// app-level preference; first-run default is 1.
    #[serde(default = "default_delete_objects")]
    pub delete_objects: i16,
    /// TEXTFILL: fill TrueType glyphs (true) or draw them hollow (false).
    pub textfill: bool,
    /// When true, saving over an existing file first copies it to a sibling
    /// `<name>.bak` so a faulty or accidental save can be recovered (#205).
    pub backup_on_save: bool,
    /// When true (default), the app (re)registers itself as a .dwg/.dxf/.bak
    /// handler on every launch. Toggle with the FILEASSOC command.
    pub file_assoc_enabled: bool,
    /// When true (default), a sketch constraint's viewport pill shows its
    /// glyph plus a driven value or named-parameter name. When false, every
    /// pill shows just the bare glyph, so the value/name text doesn't cover
    /// canvas detail on a dense sketch.
    #[serde(default = "default_show_constraint_values")]
    pub show_constraint_values: bool,
    /// Inference types, priority, intersection rules, and tolerances used by
    /// the Auto Constrain command.
    #[serde(default)]
    pub auto_constrain: AutoConstrainSettings,
    /// Keep existing geometry size while solving after a constraint edit.
    #[serde(default = "default_constraint_solve_mode")]
    pub constraint_solve_mode: bool,
    /// Apply eligible geometric constraints while creating geometry.
    #[serde(default)]
    pub constraint_infer: bool,
    /// Constraint bar display bit mask: 1 after applying, 2 on selection.
    #[serde(default = "default_constraint_bar_display")]
    pub constraint_bar_display: i16,
    /// Geometric constraint type bit mask (1..2048, combined; default all).
    #[serde(default = "default_constraint_bar_mode")]
    pub constraint_bar_mode: i16,
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
    /// SNAPUNIT X/Y spacing used by grid snap. 10 matches the Drafting
    /// Settings dialog defaults; older configs without these keys fall
    /// back via `default_snap_spacing`.
    #[serde(default = "default_snap_spacing")]
    pub snap_spacing_x: f32,
    #[serde(default = "default_snap_spacing")]
    pub snap_spacing_y: f32,
    /// GRIDUNIT X/Y display spacing (grid resizing). Falls back to 10.
    #[serde(default = "default_snap_spacing")]
    pub grid_spacing_x: f32,
    #[serde(default = "default_snap_spacing")]
    pub grid_spacing_y: f32,
    /// GRIDMAJOR: every Nth grid line is a brighter major line.
    #[serde(default = "default_grid_major")]
    pub grid_major_every: u32,
    /// Adaptive grid scaling (default on).
    #[serde(default = "default_true")]
    pub grid_adaptive: bool,
    /// Display grid beyond LIMITS (default on, matches dialog).
    #[serde(default = "default_true")]
    pub grid_beyond_limits: bool,
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

fn default_delete_objects() -> i16 {
    1
}

fn default_commandline_fade_ms() -> i32 {
    3000
}

/// Default SNAPUNIT spacing shown in the Drafting Settings dialog.
fn default_snap_spacing() -> f32 {
    10.0
}

fn default_grid_major() -> u32 {
    5
}

fn default_true() -> bool {
    true
}

/// Clamp a major-line interval to the range the dialog accepts.
pub fn sanitize_grid_major(v: u32) -> u32 {
    v.clamp(2, 100)
}

/// Clamp a snap spacing to the positive range the dialog accepts.
pub fn sanitize_snap_spacing(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 && v <= 1e9 {
        v
    } else {
        10.0
    }
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

fn default_check_missing_fonts() -> bool {
    true
}

fn default_show_constraint_values() -> bool {
    true
}

fn default_constraint_solve_mode() -> bool {
    true
}

fn default_constraint_bar_display() -> i16 {
    3
}

fn default_constraint_bar_mode() -> i16 {
    4095
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
            spacemouse: crate::input::spacemouse::Preferences::default(),
            dyn_input: true,
            polar: false,
            polar_increment_deg: 45.0,
            zoom_wheel_reversed: false,
            zoom_factor: 60,
            cursor_size: 5,
            pick_box: 3,
            options_tab: crate::ui::window::options::OptionsTab::General,
            show_viewcube: true,
            show_ucs_icon: true,
            ucs_icon_at_origin: true,
            selection_cycling: false,
            double_click_block_refedit: false,
            double_click_block_attedit: true,
            right_click_mode: RightClickMode::ShortcutMenu,
            right_click_hold_ms: 250,
            grip_object_limit: DEFAULT_GRIP_OBJECT_LIMIT,
            ncopy_bind: false,
            cursor_type: CursorType::Crosshair,
            crosshair_color: None,
            lineweight_display_scale: 100,
            isometric_drafting: false,
            iso_plane: IsoPlane::Left,
            snap_angle_deg: 0.0,
            otrack: false,
            default_assoc_prompted: false,
            check_missing_fonts: true,
            font_source_url: String::new(),
            donation_prompt_version: String::new(),
            gpu_warning_silenced: String::new(),
            disabled_plugins: Vec::new(),
            plugin_repos: Vec::new(),
            literal_spaces: false,
            command_history_height: crate::ui::command_line::HISTORY_HEIGHT_DEFAULT,
            // Snapper::default(): END|MID|CEN|NODE|QUAD|INT|NEA (575), master
            // on. Object snap is a drafting aid users expect to be live from
            // the first click; the suppress bit (16384) is left for the user
            // to set via the status-bar pill or OSNAP.
            osmode: 575,
            texteditmode: false,
            quick_dimension_snap_priority: 0,
            dimension_continue_mode: 1,
            delete_objects: 1,
            textfill: true,
            backup_on_save: true,
            file_assoc_enabled: true,
            show_constraint_values: true,
            auto_constrain: AutoConstrainSettings::default(),
            constraint_solve_mode: true,
            constraint_infer: false,
            constraint_bar_display: 3,
            constraint_bar_mode: 4095,
            savetime_min: 10,
            default_save_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            pick_add: true,
            pick_drag_rect: false,
            quick_properties: false,
            language: crate::i18n::Language::default(),
            cliprompt_lines: 3,
            commandline_fade_ms: 3000,
            snap_spacing_x: 10.0,
            snap_spacing_y: 10.0,
            grid_spacing_x: 10.0,
            grid_spacing_y: 10.0,
            grid_major_every: 5,
            grid_adaptive: true,
            grid_beyond_limits: true,
            block_mru: Vec::new(),
            block_freq: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object snap ships live. The modes were always pre-selected; only the
    /// master switch was off, so a new user got a configured snap set that
    /// did nothing until they found the status-bar pill.
    ///
    /// This is asserted rather than checked by eye because the default is
    /// only observable in a fresh profile: the app persists settings on
    /// change, so an existing settings.json keeps whatever osmode it already
    /// holds and never reveals what a new install would do.
    #[test]
    fn snapping_is_enabled_in_a_fresh_profile() {
        let settings = UserSettings::default();
        assert_eq!(
            settings.osmode & OSMODE_SUPPRESS,
            0,
            "the suppress bit is set, so snapping ships off"
        );

        let (modes, master_on) = snaps_from_osmode(settings.osmode);
        assert!(master_on, "decoding the default must report snapping on");
        for expected in [
            SnapType::Endpoint,
            SnapType::Midpoint,
            SnapType::Center,
            SnapType::Intersection,
        ] {
            assert!(modes.contains(&expected), "{expected:?} is not in the default set");
        }

        // The Snapper and the persisted default have to agree, or the running
        // state and the saved state disagree the moment anything is written.
        assert_eq!(crate::snap::Snapper::default().snap_enabled, master_on);
    }

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

    #[test]
    fn a_page_name_that_no_longer_exists_does_not_cost_the_other_settings() {
        let json = r#"{
            "settings": {
                "options_tab": "drawing",
                "pick_add": false,
                "savetime_min": 42
            }
        }"#;
        let cfg: crate::app::config::AppConfig =
            serde_json::from_str(json).expect("an unknown page name must not fail the parse");
        assert_eq!(
            cfg.settings.options_tab,
            crate::ui::window::options::OptionsTab::General,
        );
        assert!(!cfg.settings.pick_add, "the rest of the file must survive");
        assert_eq!(cfg.settings.savetime_min, 42);
    }

    #[test]
    fn isometric_drafting_never_persists_or_loads() {
        // A persisted On used to bring the isoplane crosshair (vertical +
        // diagonal arms) back on every launch for every drawing. The flag is
        // session-only now: it is dropped when saving and ignored when a
        // settings file still carries it.
        let mut settings = UserSettings::default();
        settings.isometric_drafting = true;
        let json = serde_json::to_string(&settings).expect("serialize settings");
        assert!(
            !json.contains("isometric_drafting"),
            "the saved settings must not carry the session-only flag: {json}"
        );

        let with_flag = r#"{ "isometric_drafting": true, "iso_plane": "Left" }"#;
        let loaded: UserSettings =
            serde_json::from_str(with_flag).expect("an old settings file must still parse");
        assert!(
            !loaded.isometric_drafting,
            "a persisted On must not turn isometric drafting back on"
        );
    }
}
