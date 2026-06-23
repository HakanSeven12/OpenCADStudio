#[cfg(not(target_arch = "wasm32"))]
mod automation;
#[cfg(not(target_arch = "wasm32"))]
pub use automation::{export_headless, serve};
mod cmd_result;
mod commands;
mod document;
mod expr_eval;
mod helpers;
mod history;
mod layers;
mod model_ops;
mod mtext_editor;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugin_host;
mod properties;
mod settings;
mod style_ops;
mod text_inline;
mod update;
mod view;
mod visibility;

pub use style_ops::StyleKind;

use document::DocumentTab;

use crate::modules::ModuleEvent;
use crate::scene::{CubeRegion, TileEdgeOrient};

/// In-flight drag of a Model-tile inner divider. `last_applied` is the
/// most recent normalized coordinate handed to `move_model_tile_edge` —
/// the next `ViewportMove` uses it as the `old_coord` argument so the
/// drag follows the same edge across frames even after sub-pixel
/// adjustments from the clamp inside `move_model_tile_edge`.
#[derive(Clone, Debug)]
pub struct TileDrag {
    pub orient: TileEdgeOrient,
    pub last_applied: f32,
}

/// Cursor dwelling over a grip. When `started.elapsed() >=` the popup
/// threshold the multi-functional menu opens (`grip_popup`).
#[derive(Clone, Debug)]
pub struct GripHover {
    pub handle: acadrust::Handle,
    pub grip_id: usize,
    pub screen: iced::Point,
    pub started: iced::time::Instant,
}

/// Cursor dwell awaiting a rollover hit-test. Refreshed on every idle
/// move; `HoverDwellTick` runs the pick once `last_move_at.elapsed()`
/// crosses `HOVER_DWELL_MS`. `point` and `tile_size` are tile-local so
/// the deferred pick uses the same projection the move handler would
/// have — picking with the full canvas bounds in a tiled layout matches
/// the wrong entity under the cursor.
#[derive(Clone, Debug)]
pub struct HoverDwell {
    pub last_move_at: iced::time::Instant,
    pub point: iced::Point,
    pub tile_size: (f32, f32),
    pub tab: usize,
}

/// How long the cursor must sit still before the idle rollover pick runs.
pub const HOVER_DWELL_MS: u128 = 120;

/// Open multi-functional-grip popup state.
#[derive(Clone, Debug)]
pub struct GripPopup {
    pub handle: acadrust::Handle,
    pub grip_id: usize,
    pub anchor: iced::Point,
    pub items: Vec<crate::scene::model::object::GripMenuItem>,
    pub selected: usize,
}

/// Pending follow-up value for grip-menu actions that need a number
/// (Lengthen / Radius / Arc Length / Rotate Text). The next number
/// typed in the command line is parsed and routed into
/// `apply_grip_menu_value` for `(handle, grip_id, action)`.
#[derive(Clone, Debug)]
pub struct GripPendingValue {
    pub handle: acadrust::Handle,
    pub grip_id: usize,
    pub action: crate::scene::model::object::GripMenuAction,
    pub label: &'static str,
}

/// Operator the Quick Select filter applies between an entity's
/// property value and the user-typed test value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectOp {
    /// `*Any value` — the entity matches as long as the type filter
    /// passes; the value column is ignored.
    Any,
    Eq,
    Neq,
    Gt,
    Lt,
}

impl std::fmt::Display for QSelectOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            QSelectOp::Any => "* Any value",
            QSelectOp::Eq => "= Equals",
            QSelectOp::Neq => "!= Not equal",
            QSelectOp::Gt => "> Greater than",
            QSelectOp::Lt => "< Less than",
        };
        f.write_str(s)
    }
}

/// One row in the Quick Select Properties pick_list. `field` is the
/// stable identifier (`"layer"`, `"start_x"`, …) used to look up the
/// value on each candidate entity; `label` is the human label rendered
/// in the dropdown. Equality only compares `field` so the pick_list
/// round-trips selection correctly even when labels are duplicated.
#[derive(Clone, Debug)]
pub struct QSelectPropertyChoice {
    pub field: String,
    pub label: String,
}

impl PartialEq for QSelectPropertyChoice {
    fn eq(&self, other: &Self) -> bool {
        self.field == other.field
    }
}

impl Eq for QSelectPropertyChoice {}

impl std::fmt::Display for QSelectPropertyChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Open Quick Select panel state. The filter is one
/// `(type, property, op, value)` row plus an Append-to-current-selection
/// toggle, mirroring the classic QSELECT dialog: the panel filters
/// candidate entities (entire layout) by type, then by the chosen
/// property compared to the typed value using the operator.
#[derive(Clone, Debug)]
pub struct QSelectState {
    /// `None` = "(Any type)".
    pub type_filter: Option<String>,
    /// `None` = no property filter; the type filter alone applies.
    pub property: Option<QSelectPropertyChoice>,
    pub operator: QSelectOp,
    pub value: String,
    pub append: bool,
}
use crate::snap::Snapper;
use crate::ui::{AppMenu, CommandLine, Ribbon, StatusBar};
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::CadDocument;

use iced::time::Instant;
use iced::window;
use iced::{mouse, Point, Task, Theme};
use std::sync::atomic::AtomicU8;
use std::sync::Arc;

pub(super) const POLY_START_DELAY_MS: u128 = 150;
pub(super) const VARIES_LABEL: &str = "*VARIES*";

// ── File-open progress ─────────────────────────────────────────────────────
// Phase encoding for OpenProgress.phase atomic. Updated from the background
// loader thread, read by the UI overlay on every frame.
pub const OPEN_PHASE_READING: u8 = 0;
pub const OPEN_PHASE_PARSING: u8 = 1;
pub const OPEN_PHASE_CACHING: u8 = 2;
pub const OPEN_PHASE_FINALIZING: u8 = 3;

#[derive(Debug, Clone)]
pub struct OpenProgress {
    pub name: String,
    pub size_bytes: u64,
    pub phase: Arc<AtomicU8>,
    pub started: Instant,
}

// ── Application state ──────────────────────────────────────────────────────

pub(super) struct OpenCADStudio {
    start: Instant,
    tabs: Vec<DocumentTab>,
    active_tab: usize,
    tab_counter: usize,
    ribbon: Ribbon,
    app_menu: AppMenu,
    command_line: CommandLine,
    status_bar: StatusBar,
    cursor_pos: Point,
    vp_size: (f32, f32),
    snapper: Snapper,
    snap_popup_open: bool,
    scale_popup_open: bool,
    /// True while the status-bar customization menu is open.
    statusbar_menu_open: bool,
    /// True while the drawing-units picker is open.
    units_popup_open: bool,
    /// True while the Isolate pill's action menu is open.
    isolate_popup_open: bool,
    /// True while the selection-filter type picker is open.
    selection_filter_popup_open: bool,
    /// Clean-screen mode: hide ribbon and side panels for a full canvas.
    clean_screen: bool,
    /// Quick Properties: show a compact floating property panel on selection.
    quick_properties: bool,
    /// Selection cycling: clicking where objects overlap opens a list box
    /// to pick which one; the pick is added to the current selection.
    selection_cycling: bool,
    /// Frame-budget HUD (Phase 5.3): overlays the last wire re-tessellation
    /// cost on the active viewport. Toggled by the `PERF` command.
    perf_hud: bool,
    /// When set, the cycling list box is open: (canvas point, candidates).
    cycle_candidates: Option<(iced::Point, Vec<acadrust::Handle>)>,
    /// Which status-bar pills the user has chosen to show (persisted).
    statusbar_config: crate::ui::statusbar_config::StatusBarConfig,
    /// Last persisted user preferences (DYN/OSNAP/OTRACK/POLAR/…). Compared
    /// after each message so a change is written to disk exactly once.
    last_saved_settings: Option<settings::UserSettings>,
    /// Active OTRACK alignment `(tracking_point, unit_direction)` when the
    /// cursor is on a tracking ray. Lets a typed distance place a point along
    /// the ray from the tracking point (issue #69). `None` when not aligned.
    otrack_active: Option<(glam::Vec3, glam::Vec3)>,
    /// Whether Tangent snap was enabled before a tangent-pick command started.
    pre_cmd_tangent: Option<bool>,
    /// Whether Ortho mode was temporarily suppressed by a command (e.g. RECTANG).
    rect_suppressed_ortho: bool,
    /// Orthogonal drawing constraint (F8): constrains picks to 0°/90°/180°/270°.
    ortho_mode: bool,
    /// Polar tracking (F10): constrains picks to configurable angle increments.
    polar_mode: bool,
    /// Polar tracking angle increment in degrees (15 / 30 / 45 / 90).
    polar_increment_deg: f32,
    /// Show grid lines in the viewport (F7).
    show_grid: bool,
    /// Dynamic input overlay (F12): show coordinate tooltip near cursor.
    dyn_input: bool,
    /// Controls whether the TEXTEDIT command repeats automatically (0 = Multiple, 1 = Single).
    pub texteditmode: bool,
    /// `true` after a bare `VPORTS` in model space — the next command-line
    /// entry is treated as the tiled-config option (SIngle/2H/2V/4).
    awaiting_vports: bool,
    /// Active drag of a Model-tile divider. Set on press over an inner
    /// edge, updated on move, cleared on release (which also runs the
    /// collapse pass).
    tile_drag: Option<TileDrag>,
    /// `true` once the user has reshaped the dynamic-input field set via
    /// the `,` separator during the current command iteration. Tells
    /// `sync_dyn_fields` to preserve the user's chosen shape instead of
    /// reverting to the command-default when `has_base` flips. Cleared
    /// on point commit / command start. See #35.
    dyn_user_reshaped: bool,
    /// Grip the cursor is currently dwelling on. Set when the cursor
    /// stops within `GRIP_THRESHOLD_PX` of a grip; cleared when it
    /// drifts away. The instant lets `ViewportMove` detect when the
    /// dwell crosses the popup-open threshold.
    grip_hover: Option<GripHover>,
    /// Open multi-functional grip popup. Persists across mouse moves
    /// until dismissed (click outside, ESC, cursor leaves the grip).
    grip_popup: Option<GripPopup>,
    grip_pending: Option<GripPendingValue>,
    /// Open dynamic-block visibility-state dropdown.
    visibility_popup: Option<visibility::VisibilityPopup>,
    /// A leader line just added via the "Add Leader" grip menu whose arrow is
    /// being placed (follows the cursor). `(entity handle, new-arrow grip id)`.
    /// Esc before the placement click removes it again.
    grip_add_provisional: Option<(acadrust::Handle, usize)>,
    /// Handle hidden from the base tessellation during an in-progress grip
    /// drag. While dragging, the edited entity is excluded from the cached
    /// wire set and shown as a cheap overlay preview instead, so each move
    /// updates only the overlay rather than re-tessellating the whole model.
    /// Committed (un-hidden + one re-tess) when the drag ends. `None` = idle.
    grip_preview_handle: Option<acadrust::Handle>,
    /// Pending rollover hit-test. Each idle cursor move stashes
    /// `(last_move_at, point, tab)` here and clears the live highlight;
    /// `HoverDwellTick` runs the pick once the cursor has been still for
    /// `HOVER_DWELL_MS`. Skipping the pick mid-stroke avoids the per-frame
    /// O(N) wire+hatch+mesh sweep that froze the cursor on large drawings.
    hover_dwell: Option<HoverDwell>,
    /// Snapshot of the edited entity taken at the start of a grip drag, used to
    /// restore it if the user presses Esc to cancel the drag. The drag mutates
    /// the document live (so grips / properties track), so cancel reverts from
    /// this backup. Dropped (kept) on a normal commit.
    grip_original: Option<acadrust::EntityType>,
    /// Open Quick Select panel state. `None` = panel closed. Filters are
    /// applied via `Message::QSelectApply`; the panel is dismissed on
    /// Apply / Cancel / Esc / outside-click.
    qselect: Option<QSelectState>,
    /// Show the UCS icon in the bottom-left corner of model space (UCSICON).
    show_ucs_icon: bool,
    /// Whether the ViewCube 3D gizmo is visible in model space (NAVVCUBE).
    show_viewcube: bool,
    /// Whether the Properties panel is shown on the left (PROPERTIES).
    show_properties: bool,
    /// Whether the document file tabs are shown at the top (FILETAB).
    show_file_tabs: bool,
    /// Whether the layout/paper-space tabs are shown at the bottom (LAYOUTTAB).
    show_layout_tabs: bool,
    /// Last point committed by a drawing command — used as ortho/polar base.
    last_point: Option<glam::Vec3>,
    /// OS window Id for the floating Layer Properties Manager (None when closed).
    /// OS window Id of the primary application window.
    main_window: Option<window::Id>,
    // ── Floating panel windows ────────────────────────────────────────────
    /// Standalone "Select Color" palette window + the field it targets.
    color_pick_target: Option<ColorPickTarget>,
    /// The open in-canvas modal dialog, if any (Plan B: shared overlay instead
    /// of OS windows).
    active_modal: Option<ModalKind>,
    /// Pixel offset of the active modal from screen-centre (drag-to-move).
    /// Reset to zero whenever a modal closes so each dialog opens centred.
    modal_offset: iced::Vector,
    /// Cursor position from the previous drag-move while the modal title bar is
    /// held; `None` before the first move of a drag.
    modal_drag_last: Option<Point>,
    /// True while the modal title bar is held (a drag is in progress).
    modal_dragging: bool,
    /// Plugin ids the user turned off in the Plugin Manager. Disabled plugins
    /// keep their manifest listed but drop their ribbon tab and command
    /// dispatch. Persisted via [`settings::UserSettings::disabled_plugins`].
    disabled_plugins: rustc_hash::FxHashSet<String>,
    /// External add-on packages found in the plugins folder, refreshed when the
    /// Plugin Manager opens.
    external_plugins: Vec<crate::plugin::external::ExternalPlugin>,
    /// Ids of external packages actually loaded this session (a subset of
    /// `external_plugins` — compatible, with a library, dlopen'd at startup).
    loaded_plugin_ids: rustc_hash::FxHashSet<String>,
    /// Curated plugin registry fetched from the OpenCADStudio repo.
    plugin_registry: Vec<crate::plugin::external::RegistryEntry>,
    /// User-linked plugin source repos (`owner/repo`) beyond the curated list.
    plugin_repos: Vec<String>,
    /// Add-repository text field in the Plugin Manager.
    plugin_repo_input: String,
    /// Installable release tags fetched per linked repo (for the dropdown).
    repo_release_tags: rustc_hash::FxHashMap<String, Vec<String>>,
    /// The release tag currently selected per linked repo.
    repo_selected_tag: rustc_hash::FxHashMap<String, String>,
    /// Last marketplace status / error line shown in the Plugin Manager.
    marketplace_status: String,
    /// PDSIZE text buffer for the Point Style (DDPTYPE) dialog.
    point_size_buf: String,
    /// Point Style size mode: `true` = relative to screen, `false` = absolute.
    /// Tracked separately from the PDSIZE sign so a size of 0 (sign-less) still
    /// remembers which radio is active.
    point_size_relative: bool,
    /// New-release notification window — opened on startup when the
    /// GitHub releases API reports a newer version than this build.
    /// First-launch "make Open CAD Studio the default for .dwg/.dxf?" prompt
    /// window. Shown once, gated on `default_assoc_prompted`.
    /// Whether the one-time default-association prompt has already been shown.
    /// Persisted via [`settings::UserSettings`] so it survives restarts.
    default_assoc_prompted: bool,
    /// Read-only session (`--read-only`): editing is allowed but every save
    /// path is refused. Set once at boot from the CLI config.
    read_only: bool,
    /// Tag of the latest available release (without the leading "v"),
    /// e.g. `"0.3.0"`. `None` when up-to-date or check hasn't returned.
    update_notice_version: Option<String>,
    /// Release-notes body for the version above (GitHub release "body"
    /// markdown, as returned by the API). May be empty when the release
    /// shipped without notes.
    update_notice_body: Option<String>,
    /// In-memory clipboard: cloned entities waiting to be pasted.
    clipboard: Vec<acadrust::EntityType>,
    /// Centroid of the clipboard entities (world XY plane).
    clipboard_centroid: glam::Vec3,
    /// The source drawing's `world_offset` at copy time. The centroid above is
    /// measured in that drawing's offset-relative frame; pasting into a drawing
    /// with a different offset (e.g. a large-coordinate file → a fresh one)
    /// must correct the paste translation by the offset difference, else the
    /// objects land `source_offset − target_offset` away from the cursor.
    clipboard_world_offset: [f64; 3],
    /// Table records (layer / linetype / text + dim style) the clipboard
    /// entities reference, captured from the source drawing at copy time so a
    /// paste into a *different* drawing can recreate any that are missing —
    /// otherwise the pasted entities would dangle on a non-existent layer.
    clipboard_deps: ClipboardDeps,
    /// True while the Shift key is held — drives subtractive pick (Shift+click
    /// removes the picked entity from the selection). Tracked from keyboard
    /// modifier-change events since mouse click messages carry no modifiers.
    shift_down: bool,
    /// Open in-place MText editor (toolbar + text area + live preview), if any.
    mtext_editor: Option<mtext_editor::MTextEditorState>,
    /// Open in-place single-line TEXT editor (plain text-entry box), if any.
    text_inline: Option<text_inline::TextInlineState>,
    /// Which layout tab has its context menu open (None = closed).
    layout_context_menu: Option<String>,
    /// Inline rename state: (original_name, current_edit_value).
    layout_rename_state: Option<(String, String)>,
    /// Timestamp of the previous viewport left-click release (for double-click detection).
    last_vp_click_time: Option<Instant>,
    /// Screen position of the previous viewport left-click release.
    last_vp_click_pos: Option<Point>,
    /// Editable paper width buffer for the Page Setup panel (string while typing).
    page_setup_w: String,
    /// Editable paper height buffer for the Page Setup panel (string while typing).
    page_setup_h: String,
    /// Plot area type: "Layout" | "Extents".
    page_setup_plot_area: String,
    /// Center the drawing on the page when exporting.
    page_setup_center: bool,
    /// Plot offset X in mm (applied after optional centering).
    page_setup_offset_x: String,
    /// Plot offset Y in mm.
    page_setup_offset_y: String,
    /// Plot rotation in degrees: "0" | "90" | "180" | "270".
    page_setup_rotation: String,
    /// Plot scale: "Fit" | "1:1" | "1:2" | "1:4" | "1:5" | "1:10" | "1:20" | "1:50" | "1:100" | "2:1".
    page_setup_scale: String,

    // ── Plot Style Table ──────────────────────────────────────────────────
    /// Currently loaded CTB/STB table (None = no override).
    active_plot_style: Option<crate::io::plot_style::PlotStyleTable>,

    // ── MLineStyle Dialog ─────────────────────────────────────────────────
    mlstyle_selected: String,

    // ── MLeaderStyle Dialog ───────────────────────────────────────────────
    mleaderstyle_selected: String,
    /// Colour field whose expanded palette is open (line/text/block).
    mls_color_open: Option<&'static str>,
    mls_landing_distance: String,
    mls_landing_gap: String,
    mls_arrowhead_size: String,
    mls_text_height: String,
    mls_scale_factor: String,
    mls_break_gap: String,
    mls_first_seg_angle: String,
    mls_second_seg_angle: String,
    mls_max_points: String,
    mls_default_text: String,
    mls_line_color: String,
    mls_text_color: String,
    mls_description: String,
    mls_line_weight: String,
    mls_align_space: String,
    mls_block_color: String,
    mls_block_rotation: String,
    mls_block_scale_x: String,
    mls_block_scale_y: String,
    mls_block_scale_z: String,

    // ── TableStyle Dialog ─────────────────────────────────────────────────
    tablestyle_selected: String,
    /// Edit buffers for the table style's general margins.
    ts_hmargin: String,
    ts_vmargin: String,
    /// General table-style description buffer.
    ts_description: String,
    /// Per-cell edit buffers, indexed 0=Data, 1=Header, 2=Title.
    /// Table cell colour field (row class, "textcolor"/"fillcolor") whose
    /// expanded palette is open.
    ts_color_open: Option<(u8, &'static str)>,
    ts_cell_textstyle: [String; 3],
    ts_cell_height: [String; 3],
    ts_cell_textcolor: [String; 3],
    ts_cell_fillcolor: [String; 3],
    ts_cell_datatype: [String; 3],
    ts_cell_unittype: [String; 3],
    ts_cell_format: [String; 3],
    /// Per-cell, per-border numeric buffers ([cell][border], border order:
    /// 0=left 1=right 2=top 3=bottom 4=horizontal-inside 5=vertical-inside).
    ts_border_lw: [[String; 6]; 3],
    ts_border_color: [[String; 6]; 3],
    ts_border_spacing: [[String; 6]; 3],

    // ── Shared style-manager inline rename ────────────────────────────────
    /// Original name of the style currently being renamed inline (double-click
    /// a style name in any style manager). `None` when not renaming.
    style_rename: Option<String>,
    /// Edit buffer for the inline rename text input.
    style_rename_buf: String,
    /// Active style-manager transaction. Edits mutate the document live for an
    /// in-dialog preview but only persist on Apply; closing without Apply
    /// restores this snapshot. `None` when no style manager is staging.
    style_stage: Option<style_ops::StyleStage>,

    // ── TextStyle Font Browser ────────────────────────────────────────────
    textstyle_selected: String,
    /// Edit buffer for font file name.
    textstyle_font: String,
    /// Edit buffer for width factor.
    textstyle_width: String,
    /// Edit buffer for oblique angle (degrees).
    textstyle_oblique: String,
    /// Edit buffer for fixed text height (0 = variable).
    textstyle_height: String,
    /// Edit buffer for big-font file name.
    textstyle_bigfont: String,
    /// Edit buffer for TrueType font name.
    textstyle_ttf: String,

    // ── Color Scheme ──────────────────────────────────────────────────────
    active_theme: Theme,

    // ── Keyboard Shortcut Editor ──────────────────────────────────────────
    /// User-defined function-key overrides: "F3" → command string.
    shortcut_overrides: rustc_hash::FxHashMap<String, String>,

    // ── Layout Manager Panel ──────────────────────────────────────────────
    layout_manager_selected: String,
    layout_manager_rename_buf: String,

    // ── Plot Style Panel ──────────────────────────────────────────────────
    /// Selected ACI index in the panel (1-255).
    plotstyle_panel_aci: u8,
    /// Edit buffers for the selected entry.
    ps_color_buf: String,
    ps_lineweight_buf: String,
    ps_screening_buf: String,

    // ── File-open progress ────────────────────────────────────────────────
    /// `Some` while a CAD file is loading — drives the modal overlay.
    /// Cleared when the load finishes, errors, or the user cancels.
    pub(super) opening: Option<OpenProgress>,

    // ── Unsaved-changes dialog ────────────────────────────────────────────
    /// Set when the user tries to close a tab or quit while there are unsaved changes.
    pending_close: Option<PendingClose>,
    /// OS window for the unsaved-changes confirmation dialog.

    // ── Custom Save-As dialog ─────────────────────────────────────────────
    /// OS window for the custom Save As dialog.
    /// Currently selected format string, e.g. "DWG 2013".
    save_dialog_format: String,
    /// Editable filename (without path), e.g. "drawing.dwg".
    save_dialog_filename: String,
    /// Currently browsed folder (PathBuf for reliable fs ops).
    save_dialog_folder: std::path::PathBuf,
    /// Cached directory listing: (display_name, is_dir, full_path).
    save_dialog_entries: Vec<(String, bool, std::path::PathBuf)>,
    /// True when triggered from the unsaved-changes flow.
    save_dialog_for_unsaved: bool,

    // ── DimStyle Dialog ───────────────────────────────────────────────────
    /// Name of the style currently shown in the dialog.
    dimstyle_selected: String,
    /// Which colour field currently has its expanded palette open (if any).
    ds_color_open: Option<DsField>,
    /// Active tab: 0=Lines, 1=Arrows, 2=Text, 3=Scale/Units, 4=Tolerances.
    dimstyle_tab: u8,
    // Edit buffers (strings while typing):
    ds_dimdle: String,
    ds_dimdli: String,
    ds_dimgap: String,
    ds_dimexe: String,
    ds_dimexo: String,
    ds_dimsd1: bool,
    ds_dimsd2: bool,
    ds_dimse1: bool,
    ds_dimse2: bool,
    ds_dimasz: String,
    ds_dimcen: String,
    ds_dimtsz: String,
    ds_dimtxt: String,
    ds_dimtxsty: String,
    ds_dimtad: String,
    ds_dimtih: bool,
    ds_dimtoh: bool,
    ds_dimscale: String,
    ds_dimlfac: String,
    ds_dimlunit: String,
    ds_dimdec: String,
    ds_dimpost: String,
    ds_dimtol: bool,
    ds_dimlim: bool,
    ds_dimtp: String,
    ds_dimtm: String,
    ds_dimtdec: String,
    ds_dimtfac: String,
    ds_annotative: bool,
    // Lines (colors / lineweights / fixed-length extension)
    ds_dimclrd: String,
    ds_dimlwd: String,
    ds_dimclre: String,
    ds_dimlwe: String,
    ds_dimfxl: String,
    ds_dimfxlon: bool,
    // Symbols & Arrows
    ds_dimsah: bool,
    ds_dimarcsym: String,
    ds_dimjogang: String,
    // Text
    ds_dimclrt: String,
    ds_dimjust: String,
    ds_dimtvp: String,
    ds_dimtfill: String,
    ds_dimtfillclr: String,
    ds_dimtxtdirection: bool,
    // Fit
    ds_dimatfit: String,
    ds_dimtix: bool,
    ds_dimsoxd: bool,
    ds_dimtmove: String,
    ds_dimupt: bool,
    ds_dimtofl: bool,
    ds_dimfit: String,
    // Primary units
    ds_dimdsep: String,
    ds_dimrnd: String,
    ds_dimzin: String,
    ds_dimfrac: String,
    ds_dimaunit: String,
    ds_dimadec: String,
    ds_dimunit: String,
    ds_dimazin: String,
    // Alternate units
    ds_dimalt: bool,
    ds_dimaltf: String,
    ds_dimaltd: String,
    ds_dimaltu: String,
    ds_dimalttd: String,
    ds_dimaltrnd: String,
    ds_dimapost: String,
    ds_dimaltz: String,
    ds_dimalttz: String,
    // Tolerances (extra)
    ds_dimtolj: String,
    ds_dimtzin: String,
}

/// What triggered the "unsaved changes" dialog.
#[derive(Debug, Clone)]
pub(super) enum PendingClose {
    /// User tried to close the tab at this index.
    Tab(usize),
    /// User tried to quit the application.
    Quit,
}

/// Where a colour chosen in the standalone palette window should be applied.
#[derive(Debug, Clone)]
pub enum ColorPickTarget {
    DimStyle(DsField),
    MLeader(&'static str),
    Table(u8, &'static str),
    /// Selected entities' colour (left properties panel).
    Properties,
    /// Current creation colour (ribbon).
    Ribbon,
    /// A layer's colour, by panel row index.
    Layer(usize),
}

/// Table records the clipboard entities depend on, snapshotted from the source
/// drawing at copy time. On paste into a drawing that lacks any of them, the
/// missing records are recreated (with a fresh handle) so the entities keep
/// their layer / linetype / style instead of dangling. See #129.
#[derive(Default, Clone)]
pub struct ClipboardDeps {
    pub layers: Vec<acadrust::tables::Layer>,
    pub linetypes: Vec<acadrust::tables::LineType>,
    pub text_styles: Vec<acadrust::tables::TextStyle>,
    pub dim_styles: Vec<acadrust::tables::DimStyle>,
    /// Block definitions the copied INSERTs reference (transitively),
    /// snapshotted from the source drawing. Recreated on paste so a block
    /// reference doesn't render empty in a drawing that lacks the
    /// definition. (#135)
    pub blocks: Vec<BlockDef>,
}

/// A captured block definition: its base point and the entities it owns
/// (in block-local coordinates), minus the structural Block/BlockEnd
/// markers which are rebuilt on paste.
#[derive(Clone)]
pub struct BlockDef {
    pub name: String,
    pub base_point: acadrust::types::Vector3,
    pub entities: Vec<acadrust::EntityType>,
}

impl ClipboardDeps {
    /// Snapshot the records `entities` reference that exist in `doc`.
    pub fn capture(doc: &acadrust::CadDocument, entities: &[acadrust::EntityType]) -> Self {
        use acadrust::EntityType;
        use std::collections::BTreeSet;
        let (mut layers, mut ltypes, mut tstyles, mut dstyles) = (
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let is_special = |n: &str| {
            n.is_empty()
                || n.eq_ignore_ascii_case("ByLayer")
                || n.eq_ignore_ascii_case("ByBlock")
        };
        // Scan the copied entities AND the entities inside every captured
        // block definition, so a block-internal object's layer / linetype /
        // style is recreated too — not just the top-level selection's.
        let blocks = Self::capture_blocks(doc, entities);
        let block_entities = blocks.iter().flat_map(|d| d.entities.iter());
        for e in entities.iter().chain(block_entities) {
            let c = e.common();
            if !c.layer.is_empty() {
                layers.insert(c.layer.clone());
            }
            if !is_special(&c.linetype) && !c.linetype.eq_ignore_ascii_case("Continuous") {
                ltypes.insert(c.linetype.clone());
            }
            match e {
                EntityType::Text(t) if !t.style.is_empty() => {
                    tstyles.insert(t.style.clone());
                }
                EntityType::MText(t) if !t.style.is_empty() => {
                    tstyles.insert(t.style.clone());
                }
                EntityType::AttributeEntity(a) if !a.text_style.is_empty() => {
                    tstyles.insert(a.text_style.clone());
                }
                EntityType::AttributeDefinition(a) if !a.text_style.is_empty() => {
                    tstyles.insert(a.text_style.clone());
                }
                EntityType::Dimension(d) if !d.base().style_name.is_empty() => {
                    dstyles.insert(d.base().style_name.clone());
                }
                EntityType::Leader(l) if !l.dimension_style.is_empty() => {
                    dstyles.insert(l.dimension_style.clone());
                }
                _ => {}
            }
        }
        ClipboardDeps {
            layers: layers.iter().filter_map(|n| doc.layers.get(n).cloned()).collect(),
            linetypes: ltypes.iter().filter_map(|n| doc.line_types.get(n).cloned()).collect(),
            text_styles: tstyles.iter().filter_map(|n| doc.text_styles.get(n).cloned()).collect(),
            dim_styles: dstyles.iter().filter_map(|n| doc.dim_styles.get(n).cloned()).collect(),
            blocks,
        }
    }

    /// Snapshot every block definition the `entities` reference through an
    /// INSERT, walking nested INSERTs transitively. Model/paper space and
    /// xref blocks are skipped — those aren't portable definitions.
    fn capture_blocks(doc: &acadrust::CadDocument, entities: &[acadrust::EntityType]) -> Vec<BlockDef> {
        use acadrust::EntityType;
        use rustc_hash::FxHashSet;
        let mut seen: FxHashSet<String> = FxHashSet::default();
        let mut queue: Vec<String> = Vec::new();
        for e in entities {
            if let EntityType::Insert(ins) = e {
                if seen.insert(ins.block_name.clone()) {
                    queue.push(ins.block_name.clone());
                }
            }
        }
        let mut defs = Vec::new();
        while let Some(name) = queue.pop() {
            let Some(br) = doc.block_records.get(&name) else {
                continue;
            };
            if name.starts_with("*Model_Space")
                || name.starts_with("*Paper_Space")
                || br.flags.is_xref
            {
                continue;
            }
            let base_point = match doc.get_entity(br.block_entity_handle) {
                Some(EntityType::Block(b)) => b.base_point,
                _ => acadrust::types::Vector3::ZERO,
            };
            let mut owned = Vec::new();
            for &eh in &br.entity_handles {
                let Some(e) = doc.get_entity(eh) else {
                    continue;
                };
                if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                    continue;
                }
                if let EntityType::Insert(ins) = e {
                    if seen.insert(ins.block_name.clone()) {
                        queue.push(ins.block_name.clone());
                    }
                }
                owned.push(e.clone());
            }
            defs.push(BlockDef {
                name,
                base_point,
                entities: owned,
            });
        }
        defs
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
            && self.linetypes.is_empty()
            && self.text_styles.is_empty()
            && self.dim_styles.is_empty()
            && self.blocks.is_empty()
    }
}

/// Which in-canvas modal dialog is currently open (Plan B). At most one shows
/// at a time; dialog-specific data lives in its own fields. Closed via the
/// modal's ✕ (`Message::CloseModal`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    About,
    Shortcuts,
    PluginManager,
    UpdateNotice,
    Layers,
    PageSetup,
    LayoutManager,
    Plotstyle,
    TextStyle,
    TableStyle,
    MlStyle,
    MLeaderStyle,
    DimStyle,
    Unsaved,
    SaveDialog,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AssocPrompt,
    PointStyle,
}

/// Identifies a DimStyle field that can be edited in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DsField {
    Dimdle,
    Dimdli,
    Dimgap,
    Dimexe,
    Dimexo,
    Dimsd1,
    Dimsd2,
    Dimse1,
    Dimse2,
    Dimasz,
    Dimcen,
    Dimtsz,
    Dimtxt,
    Dimtxsty,
    Dimtad,
    Dimtih,
    Dimtoh,
    Dimscale,
    Dimlfac,
    Dimlunit,
    Dimdec,
    Dimpost,
    Dimtol,
    Dimlim,
    Dimtp,
    Dimtm,
    Dimtdec,
    Dimtfac,
    Annotative,
    Dimclrd,
    Dimlwd,
    Dimclre,
    Dimlwe,
    Dimfxl,
    Dimfxlon,
    Dimsah,
    Dimarcsym,
    Dimjogang,
    Dimclrt,
    Dimjust,
    Dimtvp,
    Dimtfill,
    Dimtfillclr,
    Dimtxtdirection,
    Dimatfit,
    Dimtix,
    Dimsoxd,
    Dimtmove,
    Dimupt,
    Dimtofl,
    Dimfit,
    Dimdsep,
    Dimrnd,
    Dimzin,
    Dimfrac,
    Dimaunit,
    Dimadec,
    Dimunit,
    Dimazin,
    Dimalt,
    Dimaltf,
    Dimaltd,
    Dimaltu,
    Dimalttd,
    Dimaltrnd,
    Dimapost,
    Dimaltz,
    Dimalttz,
    Dimtolj,
    Dimtzin,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick(Instant),
    /// Web: periodic check for per-script fonts a drawing needs but hasn't
    /// fetched yet (#141). Native: never emitted.
    PollWebFonts,
    /// Web: a per-script font finished fetching — `Ok(bytes)` or `Err(reason)`.
    WebFontLoaded(crate::scene::text::web_font::Script, Result<Vec<u8>, String>),
    /// Ctrl+V. Routed by `update`: into the open text/MText editor (via an async
    /// system-clipboard read, which is the only paste path that works on the
    /// web) or, with no editor open, the entity paste command.
    PasteShortcut,
    /// System-clipboard text read for the MText editor (`None` = empty/denied).
    MTextPasteClip(Option<String>),
    /// System-clipboard text read for the single-line TEXT editor.
    TextInlinePasteClip(Option<String>),
    OpenFile,
    /// File picker returned. `Some((path, size_in_bytes))` → start loading;
    /// `None` → user cancelled the dialog (no overlay shown).
    OpenPathPicked(Option<(PathBuf, u64)>),
    /// Open a path from the Start tab's recent-documents list (skips the
    /// file picker; the path is already known).
    OpenRecent(PathBuf),
    /// Drop a path from the recent-documents list.
    RecentRemove(PathBuf),
    /// User clicked Cancel on the loading overlay. The parser thread keeps
    /// running but its result is discarded.
    OpenCancel,
    FileOpened(Result<(String, PathBuf, CadDocument, crate::scene::DerivedCaches), String>),
    SaveFile,
    SaveAs,
    // ── Custom Save-As dialog ─────────────────────────────────────────────
    SaveDialogFormatChanged(String),
    SaveDialogFilenameChanged(String),
    /// Navigate the file-chooser to the given directory.
    SaveDialogNavigate(std::path::PathBuf),
    /// User clicked a file entry (fill filename) or directory entry (navigate).
    SaveDialogEntryClicked(std::path::PathBuf, bool),
    SaveDialogConfirm,
    SaveDialogCancel,
    ClearScene,
    SetWireframe(bool),
    /// Set the active tab's render mode (one of acadrust's seven visual
    /// styles). Replaces the binary `SetWireframe` over time; the older
    /// message stays for ribbon/CLI back-compat and forwards.
    SetRenderMode(acadrust::entities::ViewportRenderMode),
    /// Switch camera projection: true = Orthographic, false = Perspective.
    SetProjection(bool),
    /// Select a ribbon module tab by index.
    RibbonSelectTab(usize),
    /// A ribbon tool button was clicked — highlights the tool and dispatches its event.
    RibbonToolClick {
        tool_id: String,
        event: ModuleEvent,
    },
    /// Result of a plugin-requested file picker (`ModuleEvent::PluginFileDialog`).
    /// `path` is `None` when the user cancels. On `Some`, the host dispatches
    /// `"<command> <path>"` to the plugins with original case preserved.
    PluginFileDialogResult {
        command: String,
        path: Option<std::path::PathBuf>,
    },
    // ── Application menu ──────────────────────────────────────────────────
    ToggleAppMenu,
    CloseAppMenu,
    /// Close the menu and immediately dispatch a CAD command.
    CloseAppMenuAndRun(String),
    AppMenuSearch(String),
    // ── Document tabs ──────────────────────────────────────────────────────
    /// Create a new empty document tab.
    TabNew,
    /// Switch to the given tab index.
    TabSwitch(usize),
    /// Close the given tab index.
    TabClose(usize),
    // ── Unsaved-changes confirmation dialog ───────────────────────────────
    /// User clicked "Save" in the unsaved-changes dialog.
    UnsavedDialogSave,
    /// User clicked "Discard" in the unsaved-changes dialog.
    UnsavedDialogDiscard,
    /// User clicked "Cancel" in the unsaved-changes dialog.
    UnsavedDialogCancel,
    /// Save-as path picked for the unsaved-changes → save → close flow.
    UnsavedPickedSavePath(Option<std::path::PathBuf>),
    // ─────────────────────────────────────────────────────────────────────
    CommandInput(String),
    CommandSubmit,
    Command(String),
    /// Append one typed character to the command-line input from the
    /// global key-press subscription. Used when the text-input widget
    /// itself isn't focused (focus parked on viewport / button / etc.)
    /// so typing still routes to the command line.
    CommandAppendChar(String),
    /// Pop the trailing character off the command-line input — backspace
    /// counterpart to `CommandAppendChar`.
    CommandBackspace,
    /// TAB pressed: move focus to the next dynamic-input field (wraps).
    DynTabNext,
    /// Split the active Model viewport in two. `true` → horizontal divider
    /// (top / bottom); `false` → vertical divider (left / right).
    SplitModelViewport(bool),
    /// Recall previous command in history (↑ arrow key).
    CommandHistoryPrev,
    /// Recall next command in history (↓ arrow key).
    CommandHistoryNext,
    /// Toggle the dropdown listing the full command-line history.
    CommandHistoryToggle,
    /// User clicked an autocomplete suggestion — fill the input with
    /// the chosen command name and dispatch it.
    CommandSuggestionPick(String),
    ToggleLayers,
    LayerToggleVisible(usize),
    LayerToggleLock(usize),
    LayerToggleFreeze(usize),
    /// Sort the Layer Manager table by a clicked column header.
    LayerSort(crate::ui::layers::LayerSortCol),
    /// Toggle per-viewport freeze: (layer_index, vp_col_index)
    LayerToggleVpFreeze(usize, usize),
    LayerNew,
    LayerDelete,
    LayerSetCurrent,
    LayerSelect(usize),
    LayerRenameStart(usize),
    LayerRenameEdit(String),
    LayerColorPickerToggle(usize),
    LayerColorMorePalette,
    LayerColorSet(u8),
    LayerLinetypeSet(String),
    LayerLineweightSet(LineWeight),
    LayerTransparencyEdit(usize, String),
    LayerRenameCommit,
    CursorMoved(Point),
    ViewportClick,
    ViewportMove(Point),
    ViewportLeftPress,
    ViewportLeftRelease,
    ViewportRightPress,
    ViewportRightRelease,
    ViewportMiddlePress,
    ViewportMiddleRelease,
    ViewportScroll(mouse::ScrollDelta),
    ViewportExit,
    ViewCubeSnap(CubeRegion),
    /// User picked an item in the multi-functional grip popup menu —
    /// the index is into `grip_popup.items`.
    GripMenuPick(usize),
    /// User picked a dynamic-block visibility state — index into the
    /// visibility dropdown's items.
    VisibilityPick(usize),
    /// Timer pulse while the cursor is dwelling on a grip; drives the
    /// dwell-to-popup transition without requiring further mouse motion.
    GripDwellTick,
    /// Timer pulse while a rollover hit-test is queued; fires when the
    /// cursor has been still long enough to safely run the pick.
    HoverDwellTick,
    WindowResized(f32, f32),
    /// Enter pressed globally — finalises the active command (no text-input involvement).
    CommandFinalize,
    /// Space pressed globally — a literal space in the MText preview, otherwise
    /// finalises like Enter.
    CommandSpace,
    /// Escape pressed globally — cancels the active command.
    CommandEscape,
    /// Toggle the global snap on/off (OSNAP button body click).
    ToggleSnapEnabled,
    /// Toggle grid-snap on/off — F9 / SNAP status-bar button.
    ToggleGridSnap,
    /// Toggle the ViewCube 3D gizmo visibility (NAVVCUBE).
    ToggleViewCube,
    /// Toggle the Properties panel visibility (PROPERTIES).
    ToggleProperties,
    /// Toggle the document file tabs at the top (FILETAB).
    ToggleFileTabs,
    /// Toggle the layout tabs at the bottom (LAYOUTTAB).
    ToggleLayoutTabs,
    /// Toggle grid display in the viewport — F7 / GRID status-bar button.
    ToggleGrid,
    /// Toggle orthogonal drawing constraint — F8 / ORTHO status-bar button.
    ToggleOrtho,
    /// Toggle LWDISPLAY header flag — LWT status-bar button.
    ToggleLineweightDisplay,
    /// Toggle polar-angle constraint — F10 / POLAR status-bar button.
    TogglePolar,
    /// Set polar tracking angle increment (right-click POLAR button).
    SetPolarAngle(f32),
    /// Set the model-space annotation scale (CANNOSCALE equivalent).
    SetAnnotationScale(f32),
    /// Set the active viewport's custom_scale (paper space).
    SetViewportScale(f64),
    /// Toggle the scale picker popup open/closed.
    ToggleScalePopup,
    /// Close the scale picker popup.
    CloseScalePopup,
    /// Toggle the status-bar customization menu open/closed.
    ToggleStatusBarMenu,
    /// Close the status-bar customization menu.
    CloseStatusBarMenu,
    /// Show/hide a single status-bar pill.
    ToggleStatusPill(crate::ui::statusbar_config::StatusPill),
    /// Toggle clean-screen mode (hide ribbon + side panels).
    ToggleCleanScreen,
    /// Toggle whether entity transparency is shown on screen.
    ToggleTransparencyDisplay,
    /// Toggle the Quick Properties floating panel.
    ToggleQuickProperties,
    /// Toggle selection cycling for overlapping objects.
    ToggleSelectionCycling,
    /// Add an object from the selection-cycling list box to the selection.
    CycleSelect(acadrust::Handle),
    /// Preview (highlight) a cycling-list row's object, or clear with `None`.
    CycleHover(Option<acadrust::Handle>),
    /// Cursor left a cycling-list row; clear the preview only if it still
    /// points at this row (guards against enter/exit event reordering).
    CycleHoverExit(acadrust::Handle),
    /// Dismiss the selection-cycling list box without picking.
    CycleCancel,
    /// Toggle the selection-filter type picker open/closed.
    ToggleSelectionFilterPopup,
    /// Close the selection-filter type picker.
    CloseSelectionFilterPopup,
    /// Include/exclude an entity type from interactive selection.
    ToggleSelectionFilterType(String),
    /// Toggle the drawing-units picker open/closed.
    ToggleUnitsPopup,
    /// Close the drawing-units picker.
    CloseUnitsPopup,
    /// Set the drawing units (INSUNITS) for the active drawing.
    SetDrawingUnits(i16),
    /// Toggle the Isolate pill's action menu open/closed.
    ToggleIsolatePopup,
    /// Close the Isolate action menu.
    CloseIsolatePopup,
    /// Toggle dynamic input overlay (F12).
    ToggleDynInput,
    /// Toggle object snap tracking (F11).
    ToggleOTrack,
    /// Toggle an individual snap mode (from popup row click).
    ToggleSnap(crate::snap::SnapType),
    /// Open / close the OSNAP popup (▾ arrow click).
    ToggleSnapPopup,
    /// Close the OSNAP popup (click-catcher outside the panel).
    CloseSnapPopup,
    /// Enable all snap modes.
    SnapSelectAll,
    /// Disable all snap modes.
    SnapClearAll,
    /// Toggle a ribbon dropdown open/closed.
    ToggleRibbonDropdown(String),
    /// Close any open ribbon dropdown (click-catcher outside the panel).
    CloseRibbonDropdown,
    /// User selected a specific item from a ribbon dropdown.
    DropdownSelectItem {
        dropdown_id: &'static str,
        cmd: &'static str,
    },
    /// Delete key — erase all currently selected entities.
    DeleteSelected,
    Undo,
    Redo,
    UndoMany(usize),
    RedoMany(usize),
    // ── Ribbon ────────────────────────────────────────────────────────────
    /// User selected a layer from the layer combobox in the ribbon.
    RibbonLayerChanged(String),
    /// User changed the active color in the Properties toolbar.
    RibbonColorChanged(AcadColor),
    /// Toggle the full ACI palette inside the ribbon color picker.
    RibbonColorPaletteToggle,
    /// User changed the active linetype in the Properties toolbar.
    RibbonLinetypeChanged(String),
    /// User changed the active lineweight in the Properties toolbar.
    RibbonLineweightChanged(LineWeight),
    /// User selected a style from a style combobox in the ribbon.
    RibbonStyleChanged {
        key: crate::modules::StyleKey,
        name: String,
    },

    // ── Properties panel ──────────────────────────────────────────────────
    /// User selected a layer from the layer pick_list in the Properties panel.
    PropLayerChanged(String),
    PropSelectionGroupChanged(crate::ui::properties::SelectionGroup),
    /// User picked a color from the Properties color picker.
    PropColorChanged(AcadColor),
    /// User selected a lineweight from the Properties pick_list.
    PropLwChanged(LineWeight),
    /// User selected a linetype from the linetype pick_list.
    PropLinetypeChanged(String),
    /// User toggled a boolean property (e.g. Invisible).
    PropBoolToggle(&'static str),
    /// User selected a hatch pattern from the pattern pick_list in Properties.
    PropHatchPatternChanged(String),
    /// User selected a generic choice field in the Properties panel.
    PropGeomChoiceChanged {
        field: &'static str,
        value: String,
    },
    /// User is typing in an editable geometry field (live buffer update).
    PropGeomInput {
        field: &'static str,
        value: String,
    },
    /// User committed a geometry/common field edit (Enter pressed).
    PropGeomCommit(&'static str),
    /// Toggle the inline color picker dropdown open/closed.
    PropColorPickerToggle,
    /// Toggle the full ACI colour palette expansion.
    PropColorPaletteToggle,
    /// Enter the model-space editing mode inside the given viewport (MSPACE).
    EnterViewport(acadrust::Handle),
    /// Exit MSPACE and return to paper-space editing (PSPACE).
    ExitViewport,
    /// MS command: enter MSPACE for the first available viewport.
    MspaceCommand,
    /// PS command: exit MSPACE (PSPACE).
    PspaceCommand,
    /// Switch to a named layout ("Model" or paper space layout name).
    LayoutSwitch(String),
    /// Create a new paper space layout.
    LayoutCreate,
    /// Delete the named paper space layout (Model cannot be deleted).
    LayoutDelete(String),
    /// Begin inline rename for the given layout tab.
    LayoutRenameStart(String),
    /// Live-update the rename text input buffer.
    LayoutRenameEdit(String),
    /// Commit the rename (Enter pressed in the text input).
    LayoutRenameCommit,
    /// Cancel an in-progress rename (Escape).
    LayoutRenameCancel,
    /// Open the right-click context menu for the given layout tab.
    LayoutContextMenu(String),
    /// Close the layout context menu.
    LayoutContextMenuClose,
    // ── Layout Manager Panel ────────────────────────────────────────────
    LayoutManagerOpen,
    #[allow(dead_code)]
    LayoutManagerClose,
    LayoutManagerSelect(String),
    LayoutManagerRenameBuf(String),
    LayoutManagerRenameCommit,
    LayoutManagerNew,
    LayoutManagerDelete,
    LayoutManagerMoveLeft,
    LayoutManagerMoveRight,
    LayoutManagerSetCurrent,
    /// Switch the UI color scheme.
    SetTheme(Theme),
    // ── Keyboard Shortcut Editor ────────────────────────────────────────
    ShortcutsPanelOpen,
    #[allow(dead_code)]
    ShortcutsPanelClose,
    // ── About window ────────────────────────────────────────────────────
    AboutOpen,
    /// Close whatever in-canvas modal dialog is open (Plan B).
    CloseModal,
    /// Title-bar pressed: begin dragging the active modal.
    ModalGrab,
    /// Cursor moved while dragging the modal title bar.
    ModalDragMove(Point),
    /// Title-bar released: stop dragging.
    ModalDragRelease,
    AboutCopyInfo,
    // ── Plugin Manager window ───────────────────────────────────────────
    PluginManagerOpen,
    #[allow(dead_code)]
    PluginManagerClose,
    /// Enable (`true`) or disable (`false`) the plugin with this id.
    SetPluginEnabled(String, bool),
    // ── Plugin marketplace (install from a linked repo's releases) ─────────
    /// Edit the add-repository text field.
    PluginRepoInput(String),
    /// Link the repository currently in the text field.
    PluginRepoAdd,
    /// Unlink a repository.
    PluginRepoRemove(String),
    /// The curated registry was fetched.
    PluginRegistryFetched(Result<Vec<crate::plugin::external::RegistryEntry>, String>),
    /// Installable release tags fetched for `owner/repo`.
    PluginReleasesFetched(String, Result<Vec<String>, String>),
    /// Choose a release tag for a repo (`repo`, `tag`).
    PluginReleaseSelect(String, String),
    /// Install the selected release of `owner/repo`.
    PluginInstall(String),
    /// Result of an install: the plugin id, or an error message.
    PluginInstalled(Result<String, String>),
    /// Delete an installed plugin's folder (effective next restart).
    PluginUninstall(String),
    // ── Point Style (DDPTYPE) dialog ──────────────────────────────────────
    /// Set the full PDMODE value from a glyph-grid cell.
    PointStyleSetMode(i16),
    /// Choose size mode: `true` = relative to screen (PDSIZE < 0), `false` =
    /// absolute units (PDSIZE > 0).
    PointStyleSizeRelative(bool),
    /// Edit the point-size text field (magnitude only).
    PointStyleSizeInput(String),
    /// Commit the point-size text field to the header with the current sign.
    PointStyleApplySize,
    /// Apply the point-size field and close the dialog (OK button).
    PointStyleOk,
    // ── Quick Select / Select Similar ───────────────────────────────────
    /// Extend the current selection with every entity in the active
    /// layout that matches a selected entity by (type, layer).
    SelectSimilar,
    /// Replace the current selection with every other selectable object in
    /// the active layout (the complement of what's selected now).
    InvertSelection,
    /// Keyboard modifier state changed — tracks whether Shift is held so the
    /// pick path can do subtractive (Shift+click) selection.
    SetShiftDown(bool),
    // ── In-place MText editor ───────────────────────────────────────────
    /// Text-area edit action from the multi-line editor widget.
    MTextEdit(iced::widget::text_editor::Action),
    /// Toolbar character-format toggle applied to the selection.
    MTextFmt(mtext_editor::MTextFmt),
    /// Toolbar height field changed.
    MTextHeight(String),
    /// Toolbar text-style dropdown changed.
    MTextStyle(String),
    /// Toolbar font dropdown changed.
    MTextFont(String),
    /// Toolbar oblique-angle field changed.
    MTextOblique(String),
    /// Toolbar width-factor field changed.
    MTextWidth(String),
    /// Toolbar character-spacing field changed.
    MTextCharSpace(String),
    /// Toolbar colour dropdown — global text colour (ACI index, 256 = ByLayer).
    MTextColor(u16),
    /// Toolbar justification / attachment-point change.
    MTextJustify(acadrust::entities::mtext::AttachmentPoint),
    /// Toolbar paragraph-alignment change.
    MTextAlign(mtext_editor::ParaAlign),
    /// Toolbar line-spacing change.
    MTextLineSpacing(f32),
    /// Switch the editor body between raw code input (`false`) and the
    /// rendered preview (`true`).
    MTextShowPreview(bool),
    /// Begin a preview text selection at the given visible-character offset.
    MTextSelStart(usize),
    /// Extend the preview selection to the given visible-character offset.
    MTextSelTo(usize),
    /// Move the preview caret by N visible characters.
    MTextCaretMove(i32),
    /// Timer tick toggling the preview caret's blink phase.
    MTextCaretBlink,
    /// Commit the editor: create or update the MText entity.
    MTextOk,
    /// Discard the editor without creating / changing the entity.
    MTextCancel,
    // ── In-place single-line TEXT editor ────────────────────────────────
    /// Text-field input changed.
    TextInlineInput(String),
    /// Commit the editor: create or update the TEXT entity.
    TextInlineOk,
    // ── Draw Order context menu ─────────────────────────────────────────
    /// Toggle the Draw Order sub-items in the viewport context menu.
    DrawOrderSubmenuToggle,
    /// Begin an interactive reference-object pick to move the current
    /// selection above (`true`) or below (`false`) the picked object.
    DrawOrderPickRef(bool),
    /// Open the Quick Select panel. Initialises filters from the current
    /// selection's first entity (type + layer) when one is selected.
    QSelectOpen,
    /// Close the Quick Select panel without applying.
    QSelectClose,
    /// Type filter — `None` means "any type".
    QSelectSetType(Option<String>),
    /// Property to compare. `None` means "no property filter — just type
    /// filter applies"; the operator and value fields are ignored in
    /// that case.
    QSelectSetProperty(Option<QSelectPropertyChoice>),
    /// Comparison operator.
    QSelectSetOperator(QSelectOp),
    /// Compare-against value (free-text input).
    QSelectSetValue(String),
    /// Append-to-current-selection toggle.
    QSelectSetAppend(bool),
    /// Apply the current filter and close the panel.
    QSelectApply,
    /// The user clicked the title-bar ✕ (fires before the window closes).
    WindowCloseRequested(window::Id),
    /// A window was fully closed (fires after `window::close()` is called).
    OsWindowClosed(window::Id),
    /// No-op — used as a fallback when a TabEvent has no host mapping.
    Noop,
    /// GitHub releases API returned a result. `Some(version)` means a
    /// newer release exists; we open the update-notice window.
    UpdateCheckResult(Option<crate::update_check::UpdateInfo>),
    /// User dismissed the update-notice window.
    UpdateNoticeClose,
    /// First-launch default-association prompt: user accepted — register this
    /// app as the default handler for .dwg / .dxf.
    AssocPromptYes,
    /// First-launch default-association prompt: user declined (or "not now").
    AssocPromptNo,
    /// Result of the platform default-association call.
    AssocResult(Result<String, String>),
    /// User clicked the "Open release page" button — opens the GitHub
    /// release URL in the OS default browser and closes the notice.
    UpdateNoticeOpenRelease,
    // ── Page Setup ────────────────────────────────────────────────────────
    /// Open the Page Setup panel for the current layout.
    PageSetupOpen,
    /// Close (cancel) the Page Setup panel without applying changes.
    PageSetupClose,
    /// Live-edit of the paper width field.
    PageSetupWidthEdit(String),
    /// Live-edit of the paper height field.
    PageSetupHeightEdit(String),
    /// User selected a paper size preset (e.g. "A4 Portrait").
    PageSetupPreset(String),
    /// User changed the plot area type ("Layout" or "Extents").
    PageSetupPlotArea(String),
    /// Toggle center-on-page.
    PageSetupCenterToggle,
    /// Live-edit of plot offset X.
    PageSetupOffsetXEdit(String),
    /// Live-edit of plot offset Y.
    PageSetupOffsetYEdit(String),
    /// User changed plot rotation.
    PageSetupRotation(String),
    PageSetupScale(String),
    /// Apply the changes entered in Page Setup.
    PageSetupCommit,
    // ── Plot / Export ─────────────────────────────────────────────────────
    /// Show the SVG save-file dialog and trigger export.
    PlotExport,
    /// Callback after the user picks (or cancels) the export path.
    PlotExportPath(Option<std::path::PathBuf>),
    /// Send current layout to the system printer (via lp / lpr).
    PrintToPrinter,
    /// Callback from the async printer job.
    PrintResult(Result<String, String>),
    // ── Plot Style Table ─────────────────────────────────────────────────
    /// Open file dialog to load a CTB/STB plot style table.
    PlotStyleLoad,
    /// Callback when the user picks (or cancels) a CTB/STB file.
    PlotStyleLoaded(Option<crate::io::plot_style::PlotStyleTable>),
    /// Clear the active plot style table.
    PlotStyleClear,
    /// Open/close the Plot Style panel.
    PlotStylePanelOpen,
    #[allow(dead_code)]
    PlotStylePanelClose,
    /// Select an ACI entry in the panel.
    PlotStylePanelSelectAci(u8),
    /// Edit buffers changed.
    PlotStylePanelColorBuf(String),
    PlotStylePanelLwBuf(String),
    PlotStylePanelScreenBuf(String),
    /// Apply current edit buffers to the selected ACI entry.
    PlotStylePanelApply,
    /// Save the modified table back to disk.
    PlotStylePanelSave,
    /// Save callback.
    PlotStylePanelSavePath(Option<std::path::PathBuf>),
    // ── TextStyle Font Browser ────────────────────────────────────────────
    TextStyleDialogOpen,
    #[allow(dead_code)]
    TextStyleDialogClose,
    TextStyleDialogSelect(String),
    TextStyleDialogSetCurrent,
    TextStyleDialogNew,
    TextStyleDialogCopy,
    // Shared inline-rename messages for every style manager. `StyleKind`
    // routes the commit to the right backing store.
    StyleRenameStart(StyleKind, String),
    StyleRenameEdit(String),
    StyleRenameCommit(StyleKind),
    StyleRenameCancel,
    TextStyleDialogDelete,
    /// Edit a string field (FontFile / Width / Oblique).
    TextStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Commit edits to the selected text style.
    TextStyleApply,
    /// Select a font from the built-in font list.
    TextStyleFontPick(String),
    /// Flip a boolean flag on the selected text style (backward / upside_down /
    /// vertical / annotative), applied immediately.
    TextStyleToggle(&'static str),
    // ── TableStyle Dialog ─────────────────────────────────────────────────
    TableStyleDialogOpen,
    #[allow(dead_code)]
    TableStyleDialogClose,
    TableStyleDialogSelect(String),
    TableStyleDialogNew,
    TableStyleDialogCopy,
    TableStyleDialogDelete,
    TableStyleDialogSetCurrent,
    /// Toggle the Annotative flag on the selected table style.
    TableStyleToggleAnnotative,
    /// Toggle a boolean flag (title_suppressed / header_suppressed / flow) on
    /// the selected table style.
    TableStyleToggle(&'static str),
    /// Update a general edit buffer (hmargin / vmargin).
    TableStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Write the general edit buffers back into the selected table style.
    TableStyleApply,
    /// Update a per-cell edit buffer (row 0=Data,1=Header,2=Title).
    TableStyleCellEdit {
        row: u8,
        field: &'static str,
        value: String,
    },
    /// Toggle the expanded colour palette for a table cell colour field.
    TableColorMore(u8, &'static str),
    /// Toggle background fill on a cell style.
    TableStyleCellToggleFill(u8),
    /// Set the alignment of a cell style from the dropdown.
    TableStyleCellSetAlign {
        row: u8,
        value: String,
    },
    /// Write a cell's edit buffers back into the selected table style.
    TableStyleCellApply(u8),
    /// Set the table flow direction from the dropdown.
    TableStyleSetFlow(String),
    /// Update a per-cell, per-border numeric edit buffer.
    TableStyleBorderEdit {
        cell: u8,
        border: u8,
        field: &'static str,
        value: String,
    },
    /// Set a border's line type (Single / Double).
    TableStyleBorderSetType {
        cell: u8,
        border: u8,
        value: String,
    },
    /// Toggle a border's visibility.
    TableStyleBorderToggleInvisible {
        cell: u8,
        border: u8,
    },
    // ── MLineStyle Dialog ─────────────────────────────────────────────────
    MlStyleDialogOpen,
    #[allow(dead_code)]
    MlStyleDialogClose,
    MlStyleDialogSelect(String),
    MlStyleDialogSetCurrent,
    MlStyleApply,
    MlStyleDialogNew,
    MlStyleDialogCopy,
    MlStyleDialogDelete,
    // ── MLeaderStyle Dialog ───────────────────────────────────────────────
    MLeaderStyleDialogOpen,
    #[allow(dead_code)]
    MLeaderStyleDialogClose,
    MLeaderStyleDialogSelect(String),
    MLeaderStyleDialogSetCurrent,
    MLeaderStyleDialogNew,
    MLeaderStyleDialogCopy,
    MLeaderStyleDialogDelete,
    MLeaderStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Toggle the expanded colour palette for an MLeaderStyle colour field.
    MLeaderColorMore(&'static str),
    MLeaderStyleToggle(&'static str),
    MLeaderStyleSetEnum {
        field: &'static str,
        value: String,
    },
    /// Set an Option<Handle> field (linetype / arrowhead / text style / block)
    /// from a dropdown of record names ("None" clears it).
    MLeaderStyleSetHandle {
        field: &'static str,
        value: String,
    },
    MLeaderStyleApply,
    // ── DimStyle Dialog ───────────────────────────────────────────────────
    DimStyleDialogOpen,
    DimStyleDialogClose,
    /// Apply edits to the selected style.
    DimStyleDialogApply,
    /// Select a different style in the dialog list.
    DimStyleDialogSelect(String),
    /// Switch the active tab.
    DimStyleDialogTab(u8),
    /// Create a new empty style (prompts via command line).
    DimStyleDialogNew,
    DimStyleDialogCopy,
    /// Set the selected style as the document's current dim style.
    DimStyleDialogSetCurrent,
    /// Delete the selected style.
    DimStyleDialogDelete,
    // Field edit messages:
    DsEdit(DsField, String),
    DsToggle(DsField),
    /// Toggle the expanded colour palette for a DimStyle colour field.
    DsColorMore(DsField),
    /// Open the standalone palette window targeting the given field.
    OpenColorWindow(ColorPickTarget),
    /// Close the nested colour-picker modal without choosing (Plan B).
    CloseColorPicker,
    /// A colour was chosen in the standalone palette window.
    ColorWindowPick(acadrust::types::Color),
    /// Set a block/linetype Handle field on the selected dim style from a
    /// dropdown of available block-records / linetypes (by name).
    DsSetHandle {
        field: &'static str,
        value: String,
    },
    // ── Raster Image ──────────────────────────────────────────────────────
    /// Open file-picker dialog for IMAGE command (async).
    ImagePick,
    /// Result of the image file picker + pixel dimension decode.
    ImagePickResult(Result<(std::path::PathBuf, u32, u32), String>),
    // ── XREF ──────────────────────────────────────────────────────────────
    /// Open file-picker dialog for XATTACH command (async).
    XAttachPick,
    /// Result of the XATTACH file picker.
    XAttachPickResult(Result<std::path::PathBuf, String>),
    // ── WBLOCK ────────────────────────────────────────────────────────────
    /// Trigger the WBLOCK save dialog for `block_name` (or `*` = selection).
    WblockSave(String),
    /// Result of the WBLOCK save path dialog.
    WblockSaveResult(String, Option<std::path::PathBuf>),
    // ── DATAEXTRACTION ────────────────────────────────────────────────────
    /// Save the pre-built CSV string to a file chosen by the user.
    DataExtractionSave(String),
    /// Path chosen (or None = cancelled).
    DataExtractionSaveResult(String, Option<std::path::PathBuf>),
    // ── STL export ────────────────────────────────────────────────────────
    /// Trigger STL export: collect meshes and show save dialog.
    StlExport,
    /// Callback after the user picks (or cancels) the STL save path.
    StlExportPath(Option<std::path::PathBuf>),
    // ── STEP export ───────────────────────────────────────────────────────
    /// Trigger STEP AP203 export: show save dialog.
    StepExport,
    /// Callback after the user picks (or cancels) the STEP save path.
    StepExportPath(Option<std::path::PathBuf>),
    // ── OBJ import ────────────────────────────────────────────────────────
    /// Trigger OBJ import: show open-file dialog.
    ObjImport,
    /// Callback after the user picks (or cancels) the OBJ file path.
    ObjImportPath(Option<std::path::PathBuf>),
}

impl OpenCADStudio {
    fn new() -> Self {
        // Boot with only the Welcome/Start tab. The user creates drawings
        // explicitly (File → New); we never auto-spawn Drawing1.
        let start_tab = DocumentTab::new_start();
        let mut app_menu = AppMenu::new();
        // Restore recents from disk so the Start page lists them across runs.
        app_menu.load_persistent_recents();
        let mut app = Self {
            start: Instant::now(),
            tabs: vec![start_tab],
            active_tab: 0,
            tab_counter: 0,
            ribbon: Ribbon::new(),
            app_menu,
            command_line: CommandLine::new(),
            status_bar: StatusBar::new(),
            cursor_pos: Point::ORIGIN,
            vp_size: (1280.0, 720.0),
            snapper: Snapper::default(),
            snap_popup_open: false,
            scale_popup_open: false,
            statusbar_menu_open: false,
            units_popup_open: false,
            isolate_popup_open: false,
            selection_filter_popup_open: false,
            statusbar_config: crate::ui::statusbar_config::StatusBarConfig::load(),
            last_saved_settings: None,
            otrack_active: None,
            clean_screen: false,
            quick_properties: false,
            selection_cycling: false,
            perf_hud: false,
            cycle_candidates: None,
            pre_cmd_tangent: None,
            rect_suppressed_ortho: false,
            ortho_mode: false,
            polar_mode: false,
            polar_increment_deg: 45.0,
            show_grid: false,
            dyn_input: true,
            texteditmode: false,
            awaiting_vports: false,
            tile_drag: None,
            dyn_user_reshaped: false,
            grip_hover: None,
            grip_popup: None,
            grip_pending: None,
            visibility_popup: None,
            grip_add_provisional: None,
            grip_preview_handle: None,
            hover_dwell: None,
            grip_original: None,
            qselect: None,
            show_ucs_icon: true,
            show_viewcube: true,
            show_properties: true,
            show_file_tabs: true,
            show_layout_tabs: true,
            last_point: None,
            main_window: None,
            color_pick_target: None,
            active_modal: None,
            modal_offset: iced::Vector::ZERO,
            modal_drag_last: None,
            modal_dragging: false,
            disabled_plugins: rustc_hash::FxHashSet::default(),
            external_plugins: Vec::new(),
            loaded_plugin_ids: rustc_hash::FxHashSet::default(),
            plugin_registry: Vec::new(),
            plugin_repos: Vec::new(),
            plugin_repo_input: String::new(),
            repo_release_tags: rustc_hash::FxHashMap::default(),
            repo_selected_tag: rustc_hash::FxHashMap::default(),
            marketplace_status: String::new(),
            point_size_buf: String::new(),
            point_size_relative: true,
            default_assoc_prompted: false,
            read_only: false,
            update_notice_version: None,
            update_notice_body: None,
            clipboard: Vec::new(),
            clipboard_centroid: glam::Vec3::ZERO,
            clipboard_world_offset: [0.0; 3],
            clipboard_deps: ClipboardDeps::default(),
            shift_down: false,
            mtext_editor: None,
            text_inline: None,
            layout_context_menu: None,
            layout_rename_state: None,
            last_vp_click_time: None,
            last_vp_click_pos: None,
            page_setup_w: String::new(),
            page_setup_h: String::new(),
            page_setup_plot_area: "Layout".to_string(),
            page_setup_center: true,
            page_setup_offset_x: "0.0".to_string(),
            page_setup_offset_y: "0.0".to_string(),
            page_setup_rotation: "0".to_string(),
            page_setup_scale: "Fit".to_string(),
            opening: None,
            pending_close: None,
            save_dialog_format: "DWG 2018".to_string(),
            save_dialog_filename: "drawing.dwg".to_string(),
            save_dialog_folder: std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string())
                .into(),
            save_dialog_entries: Vec::new(),
            save_dialog_for_unsaved: false,
            // Plot style
            active_plot_style: None,
            // Color scheme (default: dark CAD-style)
            active_theme: Theme::Dark,
            // Keyboard shortcuts
            shortcut_overrides: rustc_hash::FxHashMap::default(),
            // Layout Manager
            layout_manager_selected: "Model".to_string(),
            layout_manager_rename_buf: String::new(),
            plotstyle_panel_aci: 1,
            ps_color_buf: String::new(),
            ps_lineweight_buf: "255".to_string(),
            ps_screening_buf: "100".to_string(),
            // TextStyle font browser
            style_rename: None,
            style_rename_buf: String::new(),
            style_stage: None,
            textstyle_selected: "Standard".to_string(),
            textstyle_font: String::new(),
            textstyle_width: "1.0".to_string(),
            textstyle_oblique: "0.0".to_string(),
            textstyle_height: "0.0".to_string(),
            textstyle_bigfont: String::new(),
            textstyle_ttf: String::new(),
            // TableStyle dialog
            tablestyle_selected: "Standard".to_string(),
            ts_hmargin: "1.5".to_string(),
            ts_vmargin: "1.5".to_string(),
            ts_description: String::new(),
            ts_color_open: None,
            ts_cell_textstyle: Default::default(),
            ts_cell_height: Default::default(),
            ts_cell_textcolor: Default::default(),
            ts_cell_fillcolor: Default::default(),
            ts_cell_datatype: Default::default(),
            ts_cell_unittype: Default::default(),
            ts_cell_format: Default::default(),
            ts_border_lw: Default::default(),
            ts_border_color: Default::default(),
            ts_border_spacing: Default::default(),
            // MLineStyle dialog
            mlstyle_selected: "Standard".to_string(),
            // MLeaderStyle dialog
            mleaderstyle_selected: "Standard".to_string(),
            mls_color_open: None,
            mls_landing_distance: String::new(),
            mls_landing_gap: String::new(),
            mls_arrowhead_size: String::new(),
            mls_text_height: String::new(),
            mls_scale_factor: String::new(),
            mls_break_gap: String::new(),
            mls_first_seg_angle: String::new(),
            mls_second_seg_angle: String::new(),
            mls_max_points: String::new(),
            mls_default_text: String::new(),
            mls_line_color: String::new(),
            mls_text_color: String::new(),
            mls_description: String::new(),
            mls_line_weight: String::new(),
            mls_align_space: String::new(),
            mls_block_color: String::new(),
            mls_block_rotation: String::new(),
            mls_block_scale_x: String::new(),
            mls_block_scale_y: String::new(),
            mls_block_scale_z: String::new(),
            // DimStyle dialog
            dimstyle_selected: "Standard".to_string(),
            ds_color_open: None,
            dimstyle_tab: 0,
            ds_dimdle: "0".to_string(),
            ds_dimdli: "3.75".to_string(),
            ds_dimgap: "0.625".to_string(),
            ds_dimexe: "1.25".to_string(),
            ds_dimexo: "0.625".to_string(),
            ds_dimsd1: false,
            ds_dimsd2: false,
            ds_dimse1: false,
            ds_dimse2: false,
            ds_dimasz: "0.18".to_string(),
            ds_dimcen: "0.09".to_string(),
            ds_dimtsz: "0".to_string(),
            ds_dimtxt: "0.18".to_string(),
            ds_dimtxsty: "Standard".to_string(),
            ds_dimtad: "1".to_string(),
            ds_dimtih: false,
            ds_dimtoh: false,
            ds_dimscale: "1".to_string(),
            ds_dimlfac: "1".to_string(),
            ds_dimlunit: "2".to_string(),
            ds_dimdec: "2".to_string(),
            ds_dimpost: "<>".to_string(),
            ds_dimtol: false,
            ds_dimlim: false,
            ds_dimtp: "0".to_string(),
            ds_dimtm: "0".to_string(),
            ds_dimtdec: "2".to_string(),
            ds_dimtfac: "1".to_string(),
            ds_annotative: false,
            ds_dimclrd: "0".to_string(),
            ds_dimlwd: "-2".to_string(),
            ds_dimclre: "0".to_string(),
            ds_dimlwe: "-2".to_string(),
            ds_dimfxl: "1".to_string(),
            ds_dimfxlon: false,
            ds_dimsah: false,
            ds_dimarcsym: "0".to_string(),
            ds_dimjogang: "45".to_string(),
            ds_dimclrt: "0".to_string(),
            ds_dimjust: "0".to_string(),
            ds_dimtvp: "0".to_string(),
            ds_dimtfill: "0".to_string(),
            ds_dimtfillclr: "0".to_string(),
            ds_dimtxtdirection: false,
            ds_dimatfit: "3".to_string(),
            ds_dimtix: false,
            ds_dimsoxd: false,
            ds_dimtmove: "0".to_string(),
            ds_dimupt: false,
            ds_dimtofl: false,
            ds_dimfit: "3".to_string(),
            ds_dimdsep: "46".to_string(),
            ds_dimrnd: "0".to_string(),
            ds_dimzin: "0".to_string(),
            ds_dimfrac: "0".to_string(),
            ds_dimaunit: "0".to_string(),
            ds_dimadec: "0".to_string(),
            ds_dimunit: "2".to_string(),
            ds_dimazin: "0".to_string(),
            ds_dimalt: false,
            ds_dimaltf: "25.4".to_string(),
            ds_dimaltd: "2".to_string(),
            ds_dimaltu: "2".to_string(),
            ds_dimalttd: "2".to_string(),
            ds_dimaltrnd: "0".to_string(),
            ds_dimapost: String::new(),
            ds_dimaltz: "0".to_string(),
            ds_dimalttz: "0".to_string(),
            ds_dimtolj: "1".to_string(),
            ds_dimtzin: "0".to_string(),
        };
        // Restore persisted UI preferences (DYN/OSNAP/OTRACK/POLAR/…) so they
        // survive across sessions (issue #68). Seed `last_saved_settings` from
        // the resulting state so the first change — not the boot — triggers a
        // write.
        if let Some(s) = settings::UserSettings::load() {
            app.apply_settings(&s);
        }
        // Load external plugin packages from the plugins folder once, then fold
        // their ribbon tabs into the ribbon. Skipped under test/wasm.
        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            for (id, res) in crate::plugin::external::load_at_startup(&mut app) {
                if let Err(e) = res {
                    app.command_line
                        .push_error(&format!("Plugin '{id}' failed to load: {e}"));
                }
            }
            app.loaded_plugin_ids =
                crate::plugin::external::loaded_ids().into_iter().collect();
            app.rebuild_ribbon_modules();
        }
        app.last_saved_settings = Some(app.current_settings());
        app.sync_ribbon_layers();
        app
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::new()
    }

    /// Install `cmd` as the active interactive command for tab `tab`.
    pub(crate) fn set_active_command(
        &mut self,
        tab: usize,
        cmd: Box<dyn crate::command::CadCommand>,
    ) {
        if let Some(t) = self.tabs.get_mut(tab) {
            t.active_cmd = Some(cmd);
        }
    }

    /// Push an error message from the plugin runtime to the command line.
    pub(crate) fn push_plugin_error(&mut self, msg: &str) {
        self.command_line.push_error(msg);
    }

    #[cfg(test)]
    pub(crate) fn command_history_info(&self) -> Vec<String> {
        use crate::ui::command_line::EntryKind;
        self.command_line
            .history
            .iter()
            .filter(|e| e.kind == EntryKind::Info)
            .map(|e| e.text.clone())
            .collect()
    }

    /// Boot function for `iced::daemon`: returns initial state plus a task that
    /// opens the primary application window. Native only — the web build uses
    /// [`Self::boot_web`].
    #[cfg(not(target_arch = "wasm32"))]
    fn boot() -> (Self, Task<Message>) {
        use helpers::build_window_icon;
        // Silently (re)register this binary as a .dwg/.dxf handler so it appears
        // in the OS "Open with" list. Best-effort and idempotent; covers the
        // portable .exe / AppImage that no installer has registered. Detached so
        // it never delays startup.
        std::thread::spawn(|| {
            if let Err(e) = crate::io::file_association::register_as_handler() {
                eprintln!("file association: handler registration failed: {e}");
            }
        });
        let state = Self::new();
        let (id, open_task) = window::open(window::Settings {
            maximized: true,
            icon: window::icon::from_rgba(build_window_icon(), 32, 32).ok(),
            exit_on_close_request: false,
            ..Default::default()
        });
        let mut s = state;
        s.main_window = Some(id);
        let open_main = open_task.map(|_| Message::Noop);
        let check_update = Task::perform(
            crate::update_check::check_for_update(),
            Message::UpdateCheckResult,
        );
        let focus_cmd = s.focus_cmd_input();
        // Startup configuration from the command line (see `cli`). A file
        // argument — also how the OS file association launches us when a .dwg
        // is double-clicked — opens via `OpenRecent`, which existence-checks
        // the path and reports a clean error if it is bogus. `--new` opens a
        // fresh drawing tab instead of the welcome screen. `--read-only`
        // disables saving. `--script` queues command lines to run once up.
        let cfg = crate::cli::gui_config();
        s.read_only = cfg.read_only;
        let cli_open: Task<Message> = if let Some(p) = cfg.file {
            Task::done(Message::OpenRecent(p))
        } else if cfg.new {
            Task::done(Message::TabNew)
        } else {
            Task::none()
        };
        // Startup command script: each line dispatched as if typed at the
        // command line, in order, after any file open is requested.
        let script: Task<Message> = if cfg.script_lines.is_empty() {
            Task::none()
        } else {
            Task::batch(
                cfg.script_lines
                    .into_iter()
                    .map(|line| Task::done(Message::Command(line))),
            )
        };
        // One-time prompt offering to make Open CAD Studio the default app for
        // .dwg / .dxf. Shown only on the first launch that hasn't answered it
        // yet; the flag is persisted so we never ask twice.
        let assoc_prompt: Task<Message> = Task::none();
        if !s.default_assoc_prompted {
            s.active_modal = Some(ModalKind::AssocPrompt);
        }
        (
            s,
            Task::batch([
                open_main,
                check_update,
                focus_cmd,
                cli_open,
                script,
                assoc_prompt,
            ]),
        )
    }

    /// Single-window boot for the web build: no OS-window creation (the browser
    /// canvas is the only window), no file-association registration or CLI file
    /// open. Secondary manager windows are unavailable on the web for now.
    #[cfg(target_arch = "wasm32")]
    fn boot_web() -> (Self, Task<Message>) {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut s = Self::new();
        let focus = s.focus_cmd_input();
        (s, focus)
    }
}

use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> iced::Result {
    iced::daemon(
        OpenCADStudio::boot,
        OpenCADStudio::update,
        OpenCADStudio::view,
    )
    .subscription(OpenCADStudio::subscription)
    .title(|state: &OpenCADStudio, window_id: window::Id| {
        let _ = window_id; // all dialogs are in-canvas modals now
        if let Some(tab) = state.tabs.get(state.active_tab) {
            let dot = if tab.dirty { "● " } else { "" };
            let name = tab.tab_display_name();
            format!("{}Open CAD Studio — {}", dot, name)
        } else {
            "Open CAD Studio".to_string()
        }
    })
    .theme(|state: &OpenCADStudio, _| state.active_theme.clone())
    .run()
}

/// Single-window entry for the web (wasm) build. Uses `iced::application`
/// instead of `iced::daemon`: the browser canvas is the only window, so the
/// main-window view is rendered directly and the manager/dialog windows are
/// unavailable for now (see issue #45). Native keeps the multi-window `run`.
#[cfg(target_arch = "wasm32")]
pub fn run_web() -> iced::Result {
    iced::application(
        OpenCADStudio::boot_web,
        OpenCADStudio::update,
        OpenCADStudio::view_main,
    )
    .subscription(OpenCADStudio::subscription)
    .title(|_state: &OpenCADStudio| "Open CAD Studio".to_string())
    .theme(|state: &OpenCADStudio| state.active_theme.clone())
    .run()
}
