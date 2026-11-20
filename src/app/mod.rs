pub(crate) mod alias;
#[cfg(windows)]
pub(crate) mod alt_code;
mod automation;
pub(crate) mod control;
pub(crate) fn automation_action_names() -> &'static [&'static str] {
    control::action_names()
}
pub(crate) mod config;
#[cfg(not(target_arch = "wasm32"))]
pub use automation::{export_headless, run_batch, serve};
mod annotation_data;
mod array_grips;
mod block_action_bars;
mod block_actions;
#[doc(hidden)]
pub use block_actions::{BlockBenchClipboard, BlockBenchHarness};
mod block_alignment;
mod block_authoring_edit;
mod block_constraint_status;
mod block_construction;
mod block_cycle_order;
mod block_editor;
#[cfg(test)]
mod block_editor_tests;
mod block_feedback;
mod block_grip_set;
mod block_lookup;
mod block_parameter_properties;
mod block_properties;
mod block_properties_table;
#[cfg(test)]
mod block_properties_tests;
mod block_save_as;
mod block_test;
#[cfg(test)]
mod block_test_tests;
mod block_visibility;
pub(crate) mod calc;
mod command_driver;
mod extraction_catalog;
mod attr_editor;
mod insert_attributes;
#[cfg(test)]
mod insert_attributes_tests;
pub(crate) use command_driver::{
    copy_to_clipboard_kernel, define_block_raw_doc, paste_entities_kernel, PasteSpace,
};
mod assistant;
pub(crate) mod commands;
#[doc(hidden)]
#[cfg(any(test, not(target_arch = "wasm32")))]
pub use commands::ModelImportBenchHarness;
#[cfg(test)]
mod dim_constraint_value_tests;
mod dim_variables;
pub(crate) mod dim_viewport;
#[cfg(test)]
mod dimension_convert_tests;
#[cfg(test)]
mod dimension_preview_tests;
mod dock;
mod drafting_settings;
mod drawing_properties;
pub(crate) mod filter_dialog;
mod find_replace;
pub(crate) mod text_match;
mod geographic_location;
pub(crate) mod helpers;
mod history;
mod layer_filter_store;
mod layer_filters;
#[doc(hidden)]
#[cfg(any(test, not(target_arch = "wasm32")))]
pub use layer_filters::LayerRefreshBenchHarness;
pub(crate) mod layer_groups;
pub(crate) mod layer_import;
mod layer_walk;
#[cfg(test)]
mod layer_walk_tests;
mod layers;
#[cfg(test)]
mod linetype_manager_tests;
mod measurement;
mod model_ops;
mod civil;
mod civil_packages;
mod surfacing;
mod solid_edit_ops;
mod mtext_editor;
pub(crate) use mtext_editor::prefs as mtext_prefs;
#[cfg(test)]
mod import_command_tests;
#[cfg(test)]
mod mtext_follows_style_tests;
#[cfg(test)]
mod mtext_import_tests;
#[cfg(test)]
mod mtext_style_from_selection_tests;
mod navigation;
pub(crate) mod node_graph;
#[cfg(test)]
mod object_isolation_tests;
mod options_session;
#[cfg(test)]
mod paste_special_tests;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugin_host;
mod presspull_ops;
pub(crate) mod properties;
mod purge_dialog;
#[cfg(test)]
mod qselect_tests;
mod quick_calc;
mod recent;
mod record_api;
mod rename_dialog;
mod design_center;
mod saved_filters;
pub(crate) mod settings;
mod shortcuts;
mod startup;
pub(crate) mod style_ops;
mod tab_document;
mod table_cell_edit;
#[cfg(test)]
mod table_inplace_tests;
mod text_inline;
#[cfg(test)]
mod text_style_data_tests;
mod text_style_draft;
#[cfg(test)]
mod text_style_font_picker_tests;
#[cfg(test)]
mod text_style_layout_tests;
#[cfg(test)]
mod text_style_manager_tests;
#[cfg(test)]
mod text_style_pending_tests;
#[cfg(test)]
mod text_style_ui_tests;
#[cfg(test)]
mod text_style_update_tests;
pub(crate) mod text_style_users;
mod tolerance_dialog;
pub(crate) mod tool_windows;
#[cfg(test)]
mod undo_sweep_tests;
mod update;
pub(crate) use update::style_import::{style_exists as style_import_exists, StyleImportDraft};
#[cfg(test)]
mod table_content_tests;
mod view;
#[cfg(test)]
mod viewport_dimension_tests;
mod visibility;

pub use style_ops::StyleKind;

// The expression evaluator lives in `entities::expr_eval`. This private
// import keeps `crate::app::expr_eval` working for the code in `app`, and
// only for it: code outside `app` uses the `entities` path.
use crate::entities::expr_eval;

/// The Quad display-option message. Defined in `update/context_menu.rs` next
/// to the quad glue and re-exported so the whole crate reaches it as
/// `crate::QuadOptionMsg` (Options window, status-bar popup).
pub use update::context_menu::QuadOptionMsg;

/// Re-exported `pub` (not `pub(crate)`) so the `cargo bench` harness
/// (`benches/`, an external crate) can measure the real grip-budget helper as
/// `ui_grip_budget`. The `properties` / `settings` modules themselves stay
/// private / `pub(crate)`; only these two names are reachable externally.
pub use properties::apply_grip_budget;
pub use settings::MAX_SELECTED_GRIPS;
/// Re-exported for the `cargo bench` harness (`ui_selection_overlay`
/// constructs `CrosshairOptions`, which names these types). Modules stay
/// as they are; only these two names are reachable externally.
pub use settings::{CursorType, IsoPlane};

use crate::io::plot_style::PlotStyleTableExt as _;
use tab_document::DocumentTab;

use crate::modules::ModuleEvent;
use crate::scene::CubeRegion;

/// Which UCS-icon grip is being dragged. `Origin` slides the UCS origin within
/// its own plane; `XAxis`/`YAxis` rotate the UCS so that axis points at the
/// cursor (the other in-plane axis follows, Z fixed); `ZAxis` turns Z to the
/// cursor, keeping X as close to where it was as it can. The cursor is mapped
/// onto the UCS plane every move, so no extra drag-start state is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UcsGripKind {
    Origin,
    XAxis,
    YAxis,
    ZAxis,
}

/// Cursor dwelling over a grip. When `started.elapsed() >=` the popup
/// threshold the multi-functional menu opens (`grip_popup`).
#[derive(Clone, Debug)]
pub struct GripHover {
    pub document: u64,
    pub handle: document::Handle,
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
    /// The camera generation when the cursor came to rest. A wheel zoom
    /// keeps the cursor still, so a dwell armed before the zoom would fire
    /// mid-navigation and stall it; a tick that finds this changed restarts
    /// the rest clock on the new view instead of picking (#3339).
    pub camera_generation: u64,
}

/// Raw (unfiltered) box hits from the last preview. This is only used for
/// an unchanged, indexed, flat model view; all other views use a full pick.
struct BoxPreviewCache {
    tab_id: u64,
    geometry_epoch: u64,
    camera_generation: u64,
    anchor: iced::Point,
    cursor: iced::Point,
    crossing: bool,
    bounds: iced::Rectangle,
    view: glam::Mat4,
    eye: glam::DVec3,
    wires: Arc<Vec<crate::scene::WireModel>>,
    index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    handles: rustc_hash::FxHashSet<document::Handle>,
    #[cfg(test)]
    incremental: bool,
}

/// What the last executed settled-rollover pick saw, so a re-armed tick can
/// tell that a repeat would run identically and skip the O(N) sweep
/// (docs/quad-menu.md §3.4). `point` is tile-local like [`HoverDwell::point`];
/// the move count disqualifies any cursor move since — including one away
/// and back — and the epoch / generation / user-pref tokens disqualify a
/// scene, camera or pick-relevant preference change without a move.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadDwellPick {
    pub tab: usize,
    pub point: iced::Point,
    pub move_seq: u64,
    pub geometry_epoch: u64,
    pub camera_generation: u64,
    pub selection_preview: u8,
    pub pick_box: i32,
}

/// Cursor dwell awaiting a deferred object-snap pass while a command is
/// active. Refreshed on every move; `SnapDwellTick` runs the full snap
/// (hit_test_wires + interaction_candidates_near + snapper.snap) once
/// `last_move_at.elapsed()` crosses `SNAP_DWELL_MS`. Until then only a
/// cheap ray-plane projection feeds the live preview so the cursor keeps
/// following the pointer on large drawings. Once a settled snap is resolved,
/// keep the gate armed but stop requesting ticks until the next pointer move.
#[derive(Clone, Debug)]
pub struct SnapDwell {
    pub last_move_at: iced::time::Instant,
    /// Last expensive discrete-snap preview pass. The crosshair still follows
    /// every pointer event; this caps large-scene preview snapping to 60 Hz.
    pub last_preview_snap_at: iced::time::Instant,
    pub point: iced::Point,
    pub tab: usize,
    /// Coalesce pointer moves into one preview rebuild on the next frame.
    pub preview_dirty: bool,
    /// Whether the current settled pointer position already has a full snap.
    pub snap_resolved: bool,
    /// When a moving pointer may next run the full snap, so Nearest, Tangent
    /// and the other continuous modes follow the cursor instead of waiting
    /// for it to stop.
    pub full_snap_due: iced::time::Instant,
}

impl SnapDwell {
    pub fn discrete_preview_due_at(&self, now: iced::time::Instant) -> bool {
        now.duration_since(self.last_preview_snap_at)
            >= std::time::Duration::from_millis(SNAP_PREVIEW_INTERVAL_MS as u64)
    }
}

/// How long the cursor must sit still before the idle rollover pick runs
/// when the moving pick (`hover_pick_due`) has not already answered it.
pub const HOVER_DWELL_MS: u128 = 500;

struct SelectionCycle {
    tab_id: u64,
    point: Point,
    handles: Vec<document::Handle>,
    modifiers: Option<(bool, bool)>,
}

/// The settled hover pick skips its wire sweep when the pick aperture holds
/// more than this many candidate segments: past it, every wire in the box is
/// a plausible hit and the sweep costs more than the rollover highlight is
/// worth. Click picks keep their full sweep — a deliberate user action earns
/// the cost.
pub const HOVER_CANDIDATE_CAP: usize = 16384;
/// How long the cursor must sit still before the active-command object-snap pass runs.
pub const SNAP_DWELL_MS: u128 = 60;
/// Shortest gap between full snap passes while the pointer keeps moving. A
/// pass that takes longer stretches the gap to four times its cost.
pub const SNAP_MOVING_MS: u64 = 50;
/// Maximum live discrete-snap preview rate while a drawing command is active.
pub const SNAP_PREVIEW_INTERVAL_MS: u128 = 16;
/// A full pointer-move pass costing at least this much cannot keep up with the
/// rate a mouse reports at, so the moves arriving behind it are coalesced
/// instead of queued. Under it nothing is deferred: a pass that fits inside a
/// frame already answers faster than the display can show, and the queue it
/// leaves behind is empty.
pub const MOVE_COALESCE_FROM_MS: u64 = 8;

/// Keyboard navigation inside the open right-click context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuNav {
    Up,
    Down,
    /// Pick the highlighted row (or the default row when none is highlighted).
    Enter,
    /// Pick the next row whose mnemonic letter matches.
    Mnemonic(char),
}

/// Where Ctrl has taken a hot polyline grip. `grip_id` and `world` describe the
/// grip as it was picked up; `active_grip_id` is the grip the current stage
/// holds, which differs while a new vertex is being placed.
#[derive(Clone, Copy, Debug)]
pub struct GripCtrlCycle {
    pub handle: document::Handle,
    pub grip_id: usize,
    pub is_translate: bool,
    pub world: glam::DVec3,
    pub stage: usize,
    pub active_grip_id: usize,
    /// Remove Vertex / Convert to Line: applied by the next click.
    pub one_shot: Option<crate::scene::model::object::GripMenuAction>,
}

/// Open multi-functional-grip popup state.
#[derive(Clone, Debug)]
pub struct GripPopup {
    pub handle: document::Handle,
    pub grip_id: usize,
    pub anchor: iced::Point,
    pub items: Vec<crate::scene::model::object::GripMenuItem>,
    pub selected: usize,
    /// Whether a click-opened menu stays visible away from its grip.
    pub pinned: bool,
}

/// Pending follow-up value for grip-menu actions that need a number
/// (Lengthen / Radius / Arc Length / Rotate Text). Lengthen can also resolve
/// from a viewport point; otherwise the next typed number is parsed and routed
/// into `apply_grip_menu_value` for `(handle, grip_id, action)`.
#[derive(Clone, Debug)]
pub struct GripPendingValue {
    pub handle: document::Handle,
    pub grip_id: usize,
    pub action: crate::scene::model::object::GripMenuAction,
    pub label: &'static str,
}

mod qselect_state;
pub use self::qselect_state::*;

/// The Find and Replace window. Its texts, history and options outlive the
/// window; the search position and the result list belong to one opening.
#[derive(Clone, Debug, Default)]
pub(crate) struct FindReplaceState {
    pub search: String,
    pub replacement: String,
    /// Earlier searches and replacements, the newest first.
    pub search_history: Vec<String>,
    pub replace_history: Vec<String>,
    pub scope: crate::ui::window::find_replace::FindScope,
    /// The objects picked for "Selected objects", in the order picked.
    pub selection: Vec<document::Handle>,
    pub options: crate::ui::window::find_replace::FindOptions,
    pub list_results: bool,
    pub more_options: bool,
    /// The match on show: Replace works on it and Find goes on after it.
    pub current_match: Option<FindMatchKey>,
    /// The matches of the running search, in search order.
    pub matches: Vec<FindResult>,
    /// The listed matches (List results).
    pub results: Vec<FindResult>,
    /// The highlighted rows of the list: a click picks one, Ctrl+click adds
    /// or removes one, Shift+click a run from `anchor`.
    pub highlighted: Vec<FindMatchKey>,
    pub anchor: Option<usize>,
    /// Objects changed since the search started, counted once each.
    pub changed: Vec<(document::Handle, Option<usize>)>,
    /// One undo step for the whole window.
    pub undo_pushed: bool,
    pub notice: Option<crate::ui::window::find_replace::FindNotice>,
    /// A replacement waiting for an answer about locked layers, a block or an
    /// external reference.
    pub run: Option<crate::ui::window::find_replace::ReplaceRun>,
    /// Handles changed since the last `after_find_changes` sync.
    pub pending_entities: Vec<document::Handle>,
    /// Non-xref block definitions whose members changed since last sync.
    pub pending_blocks: Vec<String>,
    /// Layers unlocked during replacement whose entities need appearance refresh.
    pub pending_unlocked_layers: Vec<String>,
    /// True if any external reference file was rewritten and reloaded.
    pub pending_xref_reload: bool,
}

/// One match: where it shows, what kind of text it is and the text.
#[derive(Clone, Debug)]
pub(crate) struct FindResult {
    pub key: FindMatchKey,
    /// "Model" or the layout's name.
    pub location: String,
    pub kind: crate::ui::window::find_replace::FindKind,
    pub text: String,
    /// The text's height, for zooming to it (0 for a hyperlink).
    pub height: f64,
}

/// Where a match sits: `owner` is the object in its space (the insert for a
/// text in a block or an external reference), `target` the object holding the
/// text and `part` which of its texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FindMatchKey {
    pub owner: document::Handle,
    pub target: document::Handle,
    pub part: FindPart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindPart {
    Text,
    Attribute(usize),
    LinkText,
    LinkUrl,
    Cell(usize, usize),
}
use crate::snap::Snapper;
use crate::ui::{CommandLine, Ribbon, StatusBar};
use document::types::{Color as AcadColor, LineWeight};

use iced::time::Instant;
use iced::window;
use iced::{mouse, Point, Task, Theme};
use std::sync::Arc;

pub(super) const POLY_START_DELAY_MS: u128 = 250;
/// Squared screen distance (12 px) past which a left press is a drag
/// without waiting for `POLY_START_DELAY_MS`.
pub(super) const DRAG_START_FAST_PX2: f32 = 144.0;
pub(super) const VARIES_LABEL: &str = "*VARIES*";

// ── File-open progress ─────────────────────────────────────────────────────
// Phase encoding for OpenProgress.phase atomic. Updated from the background
// loader thread, read by the UI overlay on every frame.
pub const OPEN_PHASE_READING: u8 = 0;
pub const OPEN_PHASE_PARSING: u8 = 1;
pub const OPEN_PHASE_XREF: u8 = 2;
pub const OPEN_PHASE_CACHING: u8 = 3;
pub const OPEN_PHASE_FINALIZING: u8 = 4;

#[derive(Debug, Clone)]
pub struct OpenProgress {
    pub id: u64,
    pub name: String,
    pub source_path: Option<std::path::PathBuf>,
    pub size_bytes: u64,
    pub state: Arc<crate::io::OpenProgressState>,
    pub started: Instant,
    pub recovery_error: Option<String>,
    pub recovery_read_stats: Option<document::ReadStats>,
    #[cfg(target_arch = "wasm32")]
    pub recovery_bytes: Option<std::sync::Arc<[u8]>>,
    /// Disk state captured before parsing starts. If another editor changes the
    /// file while it loads, the first Save must not silently overwrite it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fingerprint: Option<crate::io::edit_lock::FileFingerprint>,
}

// ── Application state ──────────────────────────────────────────────────────

/// Drawing defaults ADDSELECTED overrides while it draws a new object matching a
/// template, then restores so the current layer/colour/linetype/lineweight are
/// left unchanged (issue #239).
struct AddSelectedRestore {
    layer_name: String,
    layer_handle: document::types::Handle,
    color: AcadColor,
    transparency: document::types::Transparency,
    linetype_name: String,
    linetype_handle: document::types::Handle,
    line_weight: i16,
    lt_scale: f64,
    dimstyle_name: String,
    dimstyle_handle: document::types::Handle,
    tab_active_layer: String,
    tab_layers_current: String,
    ribbon_layer: String,
    ribbon_color: AcadColor,
    ribbon_linetype: String,
    ribbon_lineweight: LineWeight,
}

/// Which Start-page section a narrow (tabbed) Start page is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum StartSection {
    Recent,
    Videos,
    #[default]
    Welcome,
    Discussions,
    Supporters,
}

/// How the Start page lists recent files.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum RecentView {
    #[default]
    List,
    Icons,
    Preview,
}

/// What the Space / Enter keys currently mean at the command line. One
/// decision point (`OpenCADStudio::text_entry_mode`) for every keyboard
/// route that used to re-derive the answer from editor state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TextEntryMode {
    /// Normal command line: Space submits, Enter finalises.
    Command,
    /// MText preview pane: every key is literal content, Enter is a line break.
    MTextPreview,
    /// Free-form text prompt (table cell content, TEXT / MTEXT bodies):
    /// Space is literal, typed case is preserved, Enter finishes the edit,
    /// Shift+Enter inserts a line break.
    FreeText,
}

pub(crate) fn delobj_deletes_auxiliary(value: i16, creates_surface: bool) -> bool {
    value == 2 || (value == 3 && !creates_surface)
}

/// Screenshot `window`, or `None` when it has no area. A minimized window
/// has a 0x0 surface, and iced's renderer panics reading one back ("buffer
/// slices can not be empty"), so its size is asked first.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn screenshot_if_drawn(
    window: iced::window::Id,
) -> Task<Option<iced::window::Screenshot>> {
    iced::window::size(window).then(move |size| {
        if size.width <= 0.0 || size.height <= 0.0 {
            Task::done(None)
        } else {
            iced::window::screenshot(window).map(Some)
        }
    })
}

/// The viewport VPMAX enlarged, with the geometry and active-viewport choice
/// VPMIN must put back.
#[derive(Clone, Debug)]
pub(crate) struct VpMaxState {
    pub layout: String,
    pub handle: document::Handle,
    pub center: document::types::Vector3,
    pub width: f64,
    pub height: f64,
    pub view_height: f64,
    pub scale: f64,
    pub active_viewport: Option<document::Handle>,
}

pub(super) struct OpenCADStudio {
    start: Instant,
    control: control::State,
    tabs: Vec<DocumentTab>,
    active_tab: usize,
    hovered_doc_tab: Option<usize>,
    /// The drawing-tab bar's ☰ menu is open.
    doc_tab_menu: bool,
    /// The tab whose layouts list is open.
    doc_tab_layouts: Option<usize>,
    /// Bumped on every tab hover change, so a stale rest is ignored.
    doc_tab_hover_gen: u64,
    tab_counter: usize,
    ribbon: Ribbon,
    /// Recently opened files, newest first — backs the Start page panel.
    recent_files: Vec<std::path::PathBuf>,
    /// Decoded DWG preview thumbnails for the Start page, keyed by path.
    /// `None` = the file has no readable preview (DXF, missing, WMF). Filled
    /// lazily by `refresh_recent_thumbs`; never read from a `view` call.
    recent_thumbs:
        std::collections::HashMap<std::path::PathBuf, Option<iced::widget::image::Handle>>,
    /// How many recent files to keep (user-set on the Start page, persisted).
    recent_limit: usize,
    /// Live text of the recent-limit input box (may differ from `recent_limit`
    /// mid-edit; applied on Enter). Kept in sync when the +/- buttons change it.
    recent_limit_input: String,
    recent_view: RecentView,
    command_line: CommandLine,
    /// Recent Patreon supporters shown on the Start page (name, USD cents),
    /// fetched once at boot, highest payment first.
    patrons: Vec<(String, i64)>,
    /// Tutorial-playlist videos for the Start page: seeded from the on-disk
    /// cache at boot, refreshed by a live playlist fetch.
    videos: Vec<crate::videos::VideoEntry>,
    /// Decoded thumbnail handles keyed by video id — built once per video when
    /// the list arrives, so the view never re-decodes JPEG bytes per frame.
    video_thumbs: std::collections::HashMap<String, iced::widget::image::Handle>,
    /// True while the boot-time playlist fetch is still in flight.
    videos_loading: bool,
    /// GitHub Discussions shown on the Start page, with pinned entries first.
    discussions: Vec<crate::discussions::DiscussionEntry>,
    /// True while the boot-time Discussions refresh is still in flight.
    discussions_loading: bool,
    /// Collapsed Properties-panel section titles. This belongs to the app,
    /// rather than an individual document tab, so the same view preference is
    /// used by every currently open drawing/project.
    collapsed_property_sections: rustc_hash::FxHashSet<String>,
    /// Named layer filters (LAYERFILTER). A view over the layer table, so it is
    /// kept with the app rather than written to the drawing.
    layer_filters: Vec<layer_filters::LayerFilter>,
    /// The ribbon's layer list follows the Layer manager's node (LM-25).
    ribbon_layers_follow_filter: bool,
    /// Which Start-page section is shown when the page is too narrow for all
    /// three side by side and falls back to a tab bar.
    start_section: StartSection,
    /// Widest natural single-row width of the Start-page action buttons,
    /// measured by `WrapFlow` so side lists collapse before those buttons wrap.
    start_action_w: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// Read-only editor buffer backing the command-line history dropdown, so
    /// the log can be drag-selected across lines and copied (issue #232).
    /// Rebuilt from the history each time the dropdown is opened.
    history_content: iced::widget::text_editor::Content,
    /// Pointer state while the command-history panel's top edge is dragged.
    command_history_resizing: bool,
    command_history_drag_last: Option<Point>,
    command_line_moving: bool,
    status_bar: StatusBar,
    cursor_pos: Point,
    vp_size: (f32, f32),
    /// Full window size (w, h); used to anchor status-bar popups to their pill.
    win_size: (f32, f32),
    snapper: Snapper,
    snap_popup_open: bool,
    drafting_settings_state: Option<crate::ui::window::drafting_settings::DraftingSettingsState>,
    drafting_settings_saved: Option<crate::ui::window::drafting_settings::DraftingSettingsState>,
    drafting_settings_close_confirm: bool,
    scale_popup_open: bool,
    /// True while the polar-tracking angle picker is open.
    polar_popup_open: bool,
    /// Live text of the polar picker's custom-angle field.
    polar_custom_input: String,
    /// True while the status-bar customization menu is open.
    statusbar_menu_open: bool,
    /// True while the leftmost hamburger's Model/layout list dropdown is open.
    layout_list_open: bool,
    /// True while the drawing-units picker is open.
    units_popup_open: bool,
    /// True while the Isolate pill's action menu is open.
    isolate_popup_open: bool,
    /// True while the selection-filter type picker is open.
    selection_filter_popup_open: bool,
    /// Hide a status-menu tooltip after its root is clicked; reset when the
    /// pointer leaves the root for the opened menu.
    status_menu_tooltip_hidden: bool,
    /// Clean-screen mode: hide ribbon and side panels for a full canvas.
    clean_screen: bool,
    /// Properties opened on top of clean screen (Ctrl+1 while it is on).
    clean_screen_properties: bool,
    clean_screen_parameters_manager: bool,
    clean_screen_authoring_palettes: bool,
    clean_screen_tool_palettes: bool,
    /// A property list closed itself a moment ago. The overlay reports Escape
    /// before the global shortcut repeats it, so the repeat is dropped instead
    /// of clearing the selection the list was opened for.
    block_picker_dismissed: bool,
    /// Quick Properties: show a compact floating property panel on selection.
    quick_properties: bool,
    /// Canvas-space cursor position where Quick Properties was last opened.
    quick_properties_anchor: Point,
    /// QPLOCATION (see `Settings::quick_properties_float`).
    quick_properties_float: bool,
    quick_properties_pos: Point,
    /// Selection cycling display mode; its negative value keeps the mode disabled.
    selection_cycling: i16,
    cycle_badge: Option<(u64, Point)>,
    /// OSNAPZ (see `Settings::osnap_z`).
    osnap_z: bool,
    /// SELECTSIMILARMODE (see `Settings::select_similar_mode`).
    select_similar_mode: u16,
    /// PICKADD (#226): true (default) = clicks accumulate; false = OS-style
    /// replace-on-click with Shift toggling.
    pick_add: bool,
    pick_add_retains: bool,
    /// The Remove selection keyword: picks take objects out of the set instead
    /// of putting them in, until Add turns it off or the command ends. This is
    /// what Shift already does per-click, not PICKADD — PICKADD decides whether
    /// a pick *replaces* the set, so borrowing it here would wipe everything
    /// gathered so far rather than subtract one thing. (#596)
    select_remove_mode: bool,
    /// A selection count was just reported: the step's prompt is shown again.
    selection_reprompt: bool,
    /// What the last layer translation did, kept so the log can be written
    /// after the fact rather than forcing a path out of the user up front.
    last_layer_translation: Option<crate::modules::draw::layers::laytrans::Report>,
    /// The layer-translator dialog's working state while it is open.
    layer_translator: Option<crate::ui::window::layer_translator::State>,
    /// Working copy of the Drawing Units dialog; `None` while it is closed.
    drawing_units: Option<crate::ui::window::drawing_units::State>,
    /// Working copy of the Block Definition dialog; `None` while it is closed.
    block_definition: Option<crate::ui::window::block_definition::BlockDefinitionState>,
    /// The block the Block Definition dialog defines with "Open in block
    /// editor" on; it opens once the definition is committed.
    block_definition_open_editor: Option<String>,
    block_edit_picker: Option<crate::ui::window::block_edit_picker::State>,
    rename_dialog: Option<crate::ui::window::rename::State>,
    /// DESIGN CENTER; kept after closing so browsed drawings stay listed.
    design_center: Option<crate::ui::window::design_center::State>,
    drawing_properties: Option<crate::ui::window::drawing_properties::State>,
    geographic_location: Option<crate::ui::window::geographic_location::State>,
    purge_dialog: Option<crate::ui::window::purge::State>,
    /// The Purge dialog's choice, for the PURGE it runs.
    purge_only: Option<purge_dialog::PurgeOnly>,
    /// Select Similar settings being edited.
    select_similar_settings: Option<u16>,
    block_save_as: Option<crate::ui::window::block_save_as::State>,
    block_visibility: Option<crate::ui::window::block_visibility::State>,
    block_lookup: Option<crate::ui::window::block_lookup::State>,
    block_properties_table: Option<crate::ui::window::block_properties_table::State>,
    block_visibility_mode: bool,
    block_editor_preferences: settings::BlockEditorPreferences,
    block_editor_settings: Option<crate::ui::window::block_editor_settings::State>,
    /// Visual Styles Manager: the crease angle being typed; `None` while closed.
    visual_styles_crease: Option<String>,
    block_cycle_order: Option<crate::ui::window::block_cycle_order::State>,
    block_dependency_cache: std::cell::RefCell<Option<block_feedback::DependencyCache>>,
    /// PDF dialogs' working copies; `None` while closed.
    pdf_attach: Option<crate::ui::window::pdf_dialogs::PdfAttachState>,
    point_cloud_attach: Option<crate::ui::window::pdf_dialogs::PointCloudAttachState>,
    /// The read that the -POINTCLOUDATTACH file name prompt started: the
    /// tab and the file. A failed read ends that command when it ends.
    point_cloud_prompt_read: Option<(u64, std::path::PathBuf)>,
    /// The reader of the files that node graph nodes read; a test gives
    /// one that panics.
    graph_file_reader: crate::app::node_graph::files::Reader,
    /// The number of the newest read of an import window (3D Studio
    /// Import, DGN Import, Import PDF); see `ReadTicket`.
    import_window_read: u64,
    /// ATTDEF dialog and Edit Attribute Definition dialog; `None` while closed.
    attdef_dialog: Option<crate::ui::window::attdef_dialog::AttdefDialogState>,
    attdef_edit: Option<crate::ui::window::attdef_dialog::AttdefEditState>,
    insert_attr_dialog: Option<crate::ui::window::insert_attr_dialog::InsertAttrState>,
    /// Block Attribute Manager working copy; `None` while it is closed.
    attman: Option<crate::ui::window::attman_dialog::AttManState>,
    /// Field dialog working copy; `None` while closed.
    field_dialog: Option<crate::ui::window::field_dialog::FieldDialogState>,
    /// The Formula dialog kept while Insert Field has another one open.
    field_formula_parent: Option<crate::ui::window::field_dialog::FieldDialogState>,
    /// The number format the last formula / object field was placed with;
    /// a new field starts from it (#3288).
    field_number_format: crate::ui::window::field_dialog::NumberFormat,
    point_cloud_color_map: Option<crate::ui::window::pdf_dialogs::PointCloudColorMapState>,
    pc_section: Option<crate::ui::window::pdf_dialogs::PcSectionState>,
    /// -VIEW sEttings Background dialog.
    view_background: Option<crate::ui::window::pdf_dialogs::ViewBackgroundState>,
    /// IMAGEADJUST dialog.
    image_adjust: Option<crate::ui::window::pdf_dialogs::ImageAdjustState>,
    /// The Edit Reference dialog (REFEDIT).
    refedit_dialog: Option<crate::ui::window::refedit_dialog::RefEditDialogState>,
    /// The Password Protected PDF window.
    pdf_password: Option<crate::ui::window::pdf_password_dialog::PdfPasswordState>,
    /// The Inspection Dimension window.
    dim_inspect_dialog: Option<crate::ui::window::dim_inspect_dialog::DimInspectState>,
    /// The picked table cells' Cell Borders window.
    table_cell_borders: Option<crate::ui::window::table_cell_dialog::CellBordersState>,
    /// The picked table cell's Cell Contents window.
    table_cell_contents: Option<crate::ui::window::table_cell_dialog::CellContentsState>,
    /// The picked table cells' Cell Block window.
    table_cell_block: Option<crate::ui::window::table_cell_dialog::CellBlockState>,
    /// The picked table cells' Cell Data Format window.
    table_cell_format: Option<crate::ui::window::table_cell_dialog::CellFormatState>,
    /// The annotation scale asked for before a session's first annotative text.
    anno_scale_prompt: Option<crate::ui::window::anno_scale_prompt::AnnoScalePrompt>,
    /// The Paragraph, Column Settings, Background Mask or Find and Replace
    /// window the Text Editor tab opened.
    mtext_dialog: Option<crate::ui::window::mtext_dialogs::MTextDialog>,
    /// A data format waiting on the invalid data type warning, and the
    /// cells whose values read as no such type.
    table_cell_warning: Option<(
        crate::ui::window::table_cell_dialog::CellFormatState,
        Vec<String>,
    )>,
    /// A value typed into a cell that waits on the data type question.
    table_cell_entry: Option<commands::table_cells::CellEntry>,
    /// The next cell step puts its value in without the data type question
    /// (the type was just changed to fit it).
    table_cell_entry_typed: bool,
    /// The Data Link Manager was opened by Link Cell: its link feeds the
    /// picked cells when it closes.
    data_link_cell_target: bool,
    pdf_shx_dialog: Option<crate::ui::window::pdf_shx_dialog::ShxSettingsState>,
    underlay_layers: Option<crate::ui::window::pdf_dialogs::UnderlayLayersState>,
    pdf_import_settings: Option<crate::modules::insert::pdf_import::PdfImportSettings>,
    pdf_import_file: Option<crate::ui::window::pdf_dialogs::PdfImportFileState>,
    /// IMPORT and DGNIMPORT windows.
    import_dialogs: crate::ui::window::import_dialogs::ImportDialogs,
    /// The dialog Options was opened from; it comes back when Options closes.
    options_parent: Option<ModalKind>,
    /// A dialog opened from Options; Options comes back when it closes.
    options_child: Option<ModalKind>,
    /// Working copy of the Attach External Reference dialog; None while closed.
    xref_attach: Option<crate::ui::window::xref_attach::XrefAttachState>,
    /// Working copy of the Write Block (WBLOCK) dialog; `None` while it is closed.
    wblock: Option<crate::ui::window::wblock::WblockState>,
    /// Working copy of the structured feature-control-frame editor.
    geometric_tolerance: Option<crate::ui::window::geometric_tolerance::State>,
    /// PICKDRAG: 0 = two clicks, 1 = drag, 2 = either.
    pick_drag_mode: u8,
    pick_auto: u8,
    pick_first: bool,
    /// Frame-budget HUD (Phase 5.3): overlays the last wire re-tessellation
    /// cost and the shared performance trace on the active viewport. Toggled
    /// by the `PERF` command.
    perf_hud: bool,
    cycle_candidates: Option<SelectionCycle>,
    /// Which status-bar pills the user has chosen to show (persisted).
    statusbar_config: crate::ui::statusbar::statusbar_config::StatusBarConfig,
    /// Add selected scales to existing annotative objects.
    annotation_auto_scale: i8,
    /// Last persisted user preferences (DYN/OSNAP/OTRACK/POLAR/…). Compared
    /// after each message so a change is written to disk exactly once.
    last_saved_config: Option<config::AppConfig>,
    /// Active OTRACK alignment `(tracking_point, unit_direction)` when the
    /// cursor is on a tracking ray. Lets a typed distance place a point along
    /// the ray from the tracking point (issue #69). `None` when not aligned.
    otrack_active: Option<(glam::DVec3, glam::DVec3)>,
    /// The vector `otrack_active` crosses, when the lock is the meeting of two
    /// tracking vectors. Drawn beside the first so the user can see that the
    /// point is their intersection; a typed distance still runs along
    /// `otrack_active` alone. `None` for a single-ray alignment. (#1313)
    otrack_cross: Option<(glam::DVec3, glam::DVec3)>,
    /// Active OTRACK ray kind, separate from typed-distance geometry.
    otrack_kind: Option<crate::snap::TrackingKind>,
    /// Whether Tangent snap was enabled before a tangent-pick command started.
    pre_cmd_tangent: Option<bool>,
    /// Drawing defaults captured by ADDSELECTED before it adopts the template
    /// object's properties, restored when the launched draw command ends so
    /// ADDSELECTED doesn't permanently change CLAYER / CECOLOR / … (issue #239).
    add_selected_restore: Option<AddSelectedRestore>,
    /// Whether Ortho mode was temporarily suppressed by a command (e.g. RECTANG).
    rect_suppressed_ortho: bool,
    /// Orthogonal drawing constraint (F8): constrains picks to 0°/90°/180°/270°.
    ortho_mode: bool,
    /// Polar tracking (F10): constrains picks to configurable angle increments.
    polar_mode: bool,
    /// Polar tracking angle increment in degrees (15 / 30 / 45 / 90).
    polar_increment_deg: f32,
    /// Reverse the mouse-wheel zoom direction when true (ZOOMWHEEL = 1).
    zoom_wheel_reversed: bool,
    /// Which mouse gestures pan and orbit the model view.
    navigation_style: crate::app::settings::NavigationStyle,
    /// The mouse buttons held over the view, for the navigation style.
    nav_held: crate::app::settings::NavButtons,
    /// A left (`true`) or right button press a navigation style may turn
    /// into a drag: where it went down and the message it was, replayed as a
    /// click when the button comes up without a drag.
    nav_pending: Option<(bool, iced::Point, Message)>,
    /// The left/right buttons whose release navigation keeps for itself.
    nav_claimed: crate::app::settings::NavButtons,
    /// The navigation drag under way was started by the left or right
    /// button (or a Touchpad modifier), not the middle one.
    nav_drag_by_button: bool,
    /// Where the middle button went down, to tell a click from a drag.
    nav_middle_press: Option<iced::Point>,
    /// Where the right button went down during a middle drag (CATIA zoom).
    nav_right_press: Option<iced::Point>,
    /// CATIA: a right click during the middle drag made it zoom.
    nav_zoom_latched: bool,
    /// A pending press is being replayed through the normal path.
    nav_replay: bool,
    /// True while Alt is held (Maya-Gesture and Touchpad navigation).
    alt_down: bool,
    /// UCSDETECT: a command's first point on a planar solid face sets the
    /// UCS to that face until the command ends.
    dynamic_ucs: bool,
    /// The UCS a dynamic UCS replaced, by tab id, restored when that tab's
    /// command ends.
    dynamic_ucs_restore: Option<(u64, Option<document::tables::Ucs>)>,
    /// Scroll pans and Ctrl+scroll zooms (touchpads); see `scroll_intent`.
    touchpad_pan: bool,
    /// Multi-finger gestures on a touch screen.
    touch: crate::input::touch::TouchGestures,
    /// Where the drawing area sits in the window, learned from a lone finger.
    touch_offset: Option<iced::Vector>,
    /// Interface scale in percent, applied through the window scale factor.
    ui_scale_percent: u16,
    /// Mouse-wheel zoom sensitivity (ZOOMFACTOR), held inside
    /// `settings::ZOOM_FACTOR_MIN..=settings::ZOOM_FACTOR_MAX`. `SETVAR`
    /// only sets the range the system variable itself has; the Options
    /// window reaches the rest.
    zoom_factor: i32,
    /// The Options field behind `zoom_factor`. A text field is edited as
    /// text — cleared, half-typed — so the buffer is what the user sees and
    /// the value follows only once it parses.
    zoom_factor_input: String,
    /// Crosshair size setting (CURSORSIZE, 1..=100).
    cursor_size: i32,
    /// Selection-box size setting (PICKBOX, 0..=50).
    pick_box: i32,
    /// HPASSOC: new hatches follow their boundary objects.
    hatch_associative: bool,
    /// The Hatch panel's numbers as typed.
    hatch_drafts: crate::ui::window::hatch_panel::Drafts,
    /// The user's changes to the right-click menu.
    context_menu_custom: crate::ui::popup::context_menu::MenuCustomization,
    /// Customize User Interface while it is open.
    ribbon_customize: Option<crate::ui::window::ribbon_customize::State>,
    /// The right-click menu is being edited in place; the command typed
    /// to add.
    context_menu_editing: Option<String>,
    /// HPGAPTOL: gaps up to this size count as closed for HATCH/BOUNDARY
    /// internal-point picks (0 = exact).
    hatch_gap_tolerance: f64,
    /// HATCH / GRADIENT start with the boundary method and draw mode used
    /// last.
    hatch_prefs: crate::modules::draw::draw::hatch::HatchPrefs,
    /// The empty-area pick warning was turned off: print the message instead.
    hatch_boundary_warning_off: bool,
    /// seTtings showed the Hatch panel for the running HATCH / GRADIENT,
    /// even with HPDLGMODE 0.
    hatch_panel_shown: bool,
    /// The pointer is on the viewport chip's outline switch: the active
    /// viewport keeps its outline there, so a wheel step shows at once.
    outline_switch_hover: bool,
    /// The Hatch boundary warning window.
    hatch_boundary_warning: Option<crate::ui::window::hatch_dialogs::BoundaryWarningState>,
    /// MENUBAR: the classic pull-down menus show above the ribbon.
    show_menu_bar: bool,
    /// RIBBONCLOSE hid the ribbon; RIBBON brings it back.
    ribbon_closed: bool,
    /// COMMANDLINEHIDE (Ctrl+9) hid the command line; COMMANDLINE shows it.
    pub(super) command_line_hidden: bool,
    /// Use REFEDIT rather than BEDIT when double-clicking an attribute-free block.
    double_click_block_refedit: bool,
    /// Open ATTEDIT when double-clicking a block with attributes.
    double_click_block_attedit: bool,
    /// What a right-click in the drawing area does (SHORTCUTMENU).
    right_click_mode: settings::RightClickMode,
    /// Time-sensitive right-click hold threshold, ms (SHORTCUTMENUDURATION).
    right_click_hold_ms: i32,
    // ── Quad cursor menu ───────────────────────────────────────────────────
    // The persisted `UserSettings::quad*` fields are mirrored here like
    // `right_click_mode` above (startup restore + save-snapshot in
    // `update/file.rs` write both sides), so the update paths, the Options
    // window and the status bar read plain `self.quad*` fields.
    /// Whether the Quad cursor menu exists at all (F12, status-bar field).
    quad: bool,
    /// QUADDISPLAY bit: hovering an entity offers its default command quad.
    quad_on_hover: bool,
    /// QUADDISPLAY bit: a selection set offers its common commands.
    quad_on_select: bool,
    /// QUADDISPLAY bit: a targetless right-click opens the No Selection quad.
    quad_on_right_click: bool,
    /// QUADDISPLAY bit: hovering offers the quad only while nothing is
    /// selected.
    quad_suppress_hover_when_selection: bool,
    /// QUADWIDTH: columns the expanded quad lays out (2..=12).
    quad_width: u8,
    /// QUADCOMMANDLAUNCH: a right-click over an entity launches its default
    /// command directly.
    quad_command_launch: bool,
    /// Milliseconds the cursor must rest on an entity before the hover quad
    /// opens (0..=2000).
    quad_hover_delay_ms: u64,
    /// The panels the open quad currently shows — the pure model built by
    /// `quad_panels_for_target` when the quad opened. Cleared on close.
    pub quad_panels: Vec<crate::ui::popup::quad_menu::QuadPanel>,
    /// When the qualifying hover dwell began while the quad is closed
    /// (`None` = not dwelling). The settled rollover pick arms and reads it.
    pub quad_hover_since: Option<std::time::Instant>,
    /// The last settled-rollover pick the quad's hover dwell ran: its
    /// cursor point, the move sequence that had armed the dwell, and the
    /// cheap scene tokens the pick reads. While the quad dwell is counting
    /// and none of these changed, the dwell tick skips the O(N) pick and
    /// only re-arms itself (docs/quad-menu.md §3.4).
    pub(crate) quad_dwell_pick: Option<QuadDwellPick>,
    /// Monotonic count of hover-dwell arming moves: any cursor move since
    /// the recorded pick (including moving away and back to the same
    /// point) invalidates the pick-skip above.
    hover_move_seq: u64,
    /// Per-entity-type-key most-recently-used commands launched *from the
    /// quad* (and the global list for the No Selection quad). One shared
    /// instance for every tab — the memory follows the user, not the file.
    pub quad_mru: crate::ui::popup::quad_menu::QuadMru,
    /// Target classification captured when the quad last opened. The pick
    /// routes hover-vs-selection dispatch semantics against it, so movement
    /// of the live hover highlight between open and pick cannot change what
    /// a pick does.
    quad_open_target: Option<crate::ui::popup::quad_menu::QuadTarget>,
    /// Selected-object count past which grips stop being generated
    /// (GRIPOBJLIMIT, 0..=32767; 0 = no limit).
    grip_object_limit: i32,
    ncopy_bind: bool,
    /// Drawing viewport cursor style (CURSORTYPE).
    cursor_type: settings::CursorType,
    /// Explicit crosshair colour; `None` retains automatic contrast.
    crosshair_color: Option<[u8; 3]>,
    /// Editable Options buffer for the crosshair colour.
    crosshair_color_input: String,
    /// Explicit object-snap marker colour; `None` keeps the automatic one.
    snap_marker_color: Option<[u8; 3]>,
    /// Explicit command-line text colour; `None` keeps the theme's.
    command_text_color: Option<[u8; 3]>,
    /// Editable Options buffer for the command-line text colour.
    command_text_color_input: String,
    /// Editable Options buffer for the snap marker colour.
    snap_marker_color_input: String,
    /// Defer the ISOLINES mesh rebuild until the slider is released.
    isolines_awaiting_regen: bool,
    /// Edit buffer for the SNAPANG field on the Options Drafting page. Kept
    /// separate from `snap_angle_deg` so a half-typed angle is not parsed.
    snap_angle_input: String,
    /// Model-space lineweight preview scale, in percent (25..=200).
    lineweight_display_scale: i32,
    /// Isometric drafting state and active axis pair.
    isometric_drafting: bool,
    iso_plane: settings::IsoPlane,
    /// Drafting-grid/crosshair rotation in degrees (SNAPANG).
    snap_angle_deg: f32,
    /// Show grid lines in the viewport (F7).
    show_grid: bool,
    /// GRIDUNIT X/Y display spacing backing the DSettings grid-resize inputs.
    pub grid_spacing_x: f32,
    pub grid_spacing_y: f32,
    /// GRIDMAJOR: every Nth line draws as a brighter major line.
    pub grid_major_every: u32,
    /// Adaptive grid: scale GRIDUNIT up by 5x steps to stay readable.
    pub grid_adaptive: bool,
    /// Display the grid beyond LIMITS (infinite) instead of clipping to them.
    pub grid_beyond_limits: bool,
    /// Draw the coloured X/Y axis lines through the grid origin.
    pub grid_axes: bool,
    /// Dynamic input overlay (F12): show coordinate tooltip near cursor.
    dyn_input: bool,
    /// Dynamic input mode (DYNMODE): 0=off, 1=pointer, 2=dimensional, 3=both.
    dyn_mode: i16,
    /// DYNPIFORMAT: second points typed as pointer input are X,Y rather
    /// than distance<angle.
    dyn_pointer_cartesian: bool,
    /// DYNPICOORDS: second points typed as pointer input are absolute.
    dyn_pointer_absolute: bool,
    /// DYNPIVIS 0: pointer tooltips show only once coordinates are typed.
    dyn_pointer_on_type: bool,
    /// Currently visible page in the application Options dialog.
    options_tab: crate::ui::window::options::OptionsTab,
    /// Saved graphics options (backend, compat renderer); read at launch, so
    /// edits here only take effect after a restart.
    #[cfg(not(target_arch = "wasm32"))]
    graphics_prefs: crate::gpu_backend::GraphicsPrefs,
    /// The Options window's commit point (see `options_session`).
    options_saved: Option<options_session::OptionsSnapshot>,
    /// Close was pressed with unapplied changes; the discard guard is up.
    options_close_confirm: bool,
    spacemouse: crate::input::spacemouse::Service,
    spacemouse_preferences: crate::input::spacemouse::Preferences,
    spacemouse_paused: bool,
    spacemouse_focused: bool,
    spacemouse_details: bool,
    spacemouse_was_moving: bool,
    spacemouse_pivot: Option<(crate::input::spacemouse::Target, glam::DVec3)>,
    spacemouse_selection: navigation::SelectionCache,
    /// Controls whether the TEXTEDIT command repeats automatically (0 = Multiple, 1 = Single).
    pub texteditmode: bool,
    /// QDIM extension-origin priority: 0 = endpoints, 1 = intersections.
    pub quick_dimension_snap_priority: u8,
    /// Creation-style policy used by continuing and baseline dimensions.
    pub dimension_continue_mode: i16,
    /// When true (default), saving over an existing file first writes a `.bak`
    /// copy of it for recovery (#205). Toggle with the ISAVEBAK command.
    pub backup_on_save: bool,
    /// When true (default), the app registers itself as a .dwg/.dxf/.bak file
    /// handler on each launch. Toggle with the FILEASSOC command.
    pub file_assoc_enabled: bool,
    /// When true (default), a parametric constraint's viewport pill shows its
    /// glyph plus a driven value or named-parameter name. When false, every
    /// pill shows only the glyph.
    pub show_constraint_values: bool,
    pub auto_constrain_settings: settings::AutoConstrainSettings,
    /// TXT2MTXT settings (profile).
    pub txt2mtxt_settings: settings::Txt2MtxtSettings,
    pub dim_inspect_settings: settings::DimInspectSettings,
    /// The Text to MText Settings dialog's working copy.
    txt2mtxt_dialog: Option<settings::Txt2MtxtSettings>,
    /// MATCHPROP's Property Settings window.
    matchprop_dialog: Option<crate::ui::window::pdf_dialogs::MatchPropDialog>,
    /// Check Spelling (profile) and its open window.
    pub spell_prefs: settings::SpellPrefs,
    spell: Option<crate::ui::window::spell_dialog::SpellState>,
    auto_constrain_saved: Option<settings::AutoConstrainSettings>,
    auto_constrain_selected_row: usize,
    auto_constrain_distance_input: String,
    auto_constrain_angle_input: String,
    pub constraint_solve_mode: bool,
    pub constraint_infer: bool,
    /// How many `apply_cmd_result` frames are on the stack. A handler that
    /// applies a follow-up result must not clear or drain the report its
    /// caller is still filling.
    result_depth: u32,
    pub constraint_bar_display: i16,
    /// DCFORM: new dimensional constraints use the annotational form.
    pub constraint_form_annotational: bool,
    /// The dimensional constraint DIMCONSTRAINT offers by default: the last one used.
    pub dim_constraint_last: &'static str,
    /// The geometric constraint GEOMCONSTRAINT offers by default: the last one used.
    pub geom_constraint_last: &'static str,
    /// Set while a plugin drives the command line (`HostApi::execute_command`).
    /// Dispatching a plugin command from there would call back into the plugin
    /// that is still blocked waiting for this request, so plugin dispatch is
    /// skipped for the length of the call.
    pub(crate) suppress_plugin_dispatch: bool,
    /// Set while a plugin request drives commands (`run_command`,
    /// `constraint_create`): their edits join the plugin's open `push_undo`
    /// scope, which the message boundary after the request closes.
    pub(crate) plugin_request_active: bool,
    pub constraint_bar_mode: i16,
    /// Minutes between autosaves to a `.sv$` recovery file (SAVETIME command);
    /// 0 disables autosave.
    pub savetime_min: i32,
    /// SAVEFILEPATH: the folder recovery copies go to; empty for the default.
    pub save_file_path: String,
    /// SCRIPTCOMMANDS: scripts may run OCS commands (see `UserSettings`).
    pub script_commands: bool,
    /// Persisted default viewport background, restored from settings and applied
    /// to every drawing tab (new and opened) so a chosen background survives
    /// restarts (#188). `None` = the built-in dark-grey / off-white defaults.
    /// The `a` channel is always 1.0.
    default_bg_color: Option<[f32; 4]>,
    default_paper_bg_color: Option<[f32; 4]>,
    /// CLIPROMPTLINES: how many temporary prompt lines for a single command
    /// are displayed above the command window (0–50, Registry, default 3).
    cliprompt_lines: i32,
    /// COMMANDLINEFADETIME: overlay history visible time in ms (0–60000, default 3000).
    commandline_fade_ms: i32,
    /// MRU list of block names inserted via INSERT, most recent first, capped to 20.
    block_mru: Vec<String>,
    /// Insertion frequency per block name (uppercase key → count), capped.
    block_freq: std::collections::HashMap<String, u32>,
    /// BLOCKMRULIST: how many recent blocks the palette keeps (0–100).
    pub(crate) block_mru_list: u8,
    /// The selection the gathering command had after the last pick (keyed by
    /// the command), for the `N found, M total` line.
    pub(crate) selection_report: Option<(usize, Vec<document::Handle>)>,
    /// The objects the last selection step picked (a click, a window, a
    /// fence, Last / Previous / All) and whether it removed them.
    pub(crate) last_pick: Option<(Vec<document::Handle>, bool)>,
    /// The selection steps of the current Select objects: prompt (keyed by
    /// the gathering command), newest last: the objects each added, or with
    /// `true` the ones it removed. Undo takes them back one by one.
    pub(crate) selection_undo: (usize, Vec<(Vec<document::Handle>, bool)>),
    /// The next completed selection is an Undo: no report, no new step.
    pub(crate) selection_undoing: bool,
    /// Group at a Select objects: prompt is waiting for the group's name.
    pub(crate) selection_group_pending: bool,
    /// The selection step being reported came from Group (a named group):
    /// its count carries no group total.
    pub(crate) selection_group_named: bool,
    /// Selectable groups the reported selection step touched.
    pub(crate) last_pick_groups: usize,
    /// The last point entered by any command (LASTPOINT): a bare distance
    /// at a selection prompt measures from it when the prompt has none yet.
    pub(crate) last_entered_point: Option<glam::DVec3>,
    /// SUbobject mode at the Select objects: prompt of this gathering
    /// command (keyed like `selection_undo`); Object ends it.
    pub(crate) selection_subobject: Option<usize>,
    /// The first corner of a window typed at a selection prompt, keyed by
    /// the gathering command, until the opposite corner is typed.
    pub(crate) typed_window_corner: Option<(usize, glam::DVec3)>,
    /// The Edit Reference dialog's remembered settings.
    pub(crate) refedit_options: crate::app::settings::RefEditOptions,
    /// BLOCKREDEFINEMODE (0–2).
    pub(crate) block_redefine_mode: u8,
    /// BLOCKNAVIGATE: the Libraries tab's start folder ("." for none).
    pub(crate) block_navigate: String,
    /// Last time block-usage was flushed to disk (debounce per 2.4).
    #[cfg(not(target_arch = "wasm32"))]
    block_usage_last_persist: Option<std::time::Instant>,
    /// `true` after a bare `VPORTS` in model space — the next command-line
    /// entry is treated as the tiled-config option (SIngle/2H/2V/4).
    awaiting_vports: bool,
    /// VPMAX's saved viewport state; `None` while not maximized.
    vpmax_state: Option<VpMaxState>,
    /// Set to the variable name after a bare system-variable query (e.g. a lone
    /// `MIRRTEXT`) — the next command-line entry is its new value, empty keeps
    /// the current one.
    pending_setvar: Option<String>,
    /// The schema the next STEPOUT writes (`STEPOUT AP203|AP214|AP242`).
    step_schema: crate::io::step::Schema,
    /// DELOBJ system variable (0–3), shared by every open drawing.
    delete_objects: i16,
    /// Cursor is hovering over the UCS icon body — drives the hover highlight.
    ucs_icon_hover: bool,
    /// UCS icon is selected (clicked): its grips are shown and draggable.
    ucs_icon_selected: bool,
    /// Active direct-drag of a UCS icon grip (origin slide or axis rotate).
    /// Set on press over a grip, updated on move, committed on release.
    ucs_grip_drag: Option<UcsGripKind>,
    /// Pane-move drag in progress: the source pane's tile index, armed by the
    /// controls-bar drag handle. On release over another pane the two swap.
    pane_move_from: Option<usize>,
    /// `true` once the user has reshaped the dynamic-input field set via
    /// the `,` separator during the current command iteration. Tells
    /// `sync_dyn_fields` to preserve the user's chosen shape instead of
    /// reverting to the command-default when `has_base` flips. Cleared
    /// on point commit / command start. See #35.
    dyn_user_reshaped: bool,
    /// Dynamic cartesian entry mode for the current point. `false` is the
    /// usual relative-to-last-point mode; typing `#` selects absolute UCS
    /// coordinates and typing `@` selects relative coordinates again.
    /// Cleared after a point is committed or a new command starts.
    dyn_coord_absolute: bool,
    /// Grip the cursor is currently dwelling on. Set when the cursor
    /// stops within `GRIP_THRESHOLD_PX` of a grip; cleared when it
    /// drifts away. The instant lets `ViewportMove` detect when the
    /// dwell crosses the popup-open threshold.
    grip_hover: Option<GripHover>,
    /// Plugin-declared custom grip the cursor hovers (API v11); drives the
    /// same hover highlight overlay the built-in grip hover uses.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_hover: Option<crate::plugin::grip_points::PluginGripHover>,
    /// Delivery bookkeeping for the active custom-grip drag (API v12):
    /// every out-of-process phase is issued without waiting and collected
    /// at pump ticks — the UI loop never blocks on plugin answers.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_deliveries: crate::plugin::grip_points::PluginGripDeliveries,
    /// The visual-only live-geometry phase of the active custom-grip drag
    /// (API v12): stashed originals of the entities the plugin's move
    /// answers are swapping in; restored byte-identically before the settle
    /// (and on cancel), never a history step of their own.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_live_originals: Vec<(document::Handle, document::EntityType)>,
    /// The descriptor-backed grip drag's host state (API v14, additive):
    /// Some while a drag re-derives its Begin answer's live design per
    /// coalesced cursor move. Data-only; the machinery lives in the
    /// native-only drag lifecycle (`update/plugin_grips.rs`). Configured
    /// out on wasm together with the machinery: the type names the host
    /// module's wire types, which the wasm build does not compile.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_descriptor_drag: Option<crate::plugin::grip_points::DescriptorGripDrag>,
    /// The grip-settle watchdog floor: [`SETTLE_WATCHDOG`] in production;
    /// tests lower it to exercise the strike ledger with millisecond
    /// budgets instead of real seconds.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_settle_watchdog: std::time::Duration,
    /// The grip-settle strike ledger: plugin -> the missed-delivery times.
    /// Three misses inside the strike window tear the connection down (the
    /// plugin is genuinely wedged under a live pump); a settle landing in
    /// time clears the entry (one recovered settle resets the count).
    #[cfg(not(target_arch = "wasm32"))]
    plugin_grip_settle_strikes: rustc_hash::FxHashMap<String, Vec<std::time::Instant>>,
    /// Grip-declaration re-arms queued by strike tear-downs: the reap tick
    /// re-dispatches `SelectionChanged` (dedup cleared) once the plugin is
    /// visible again, so its declared grips come back without human
    /// intervention.
    #[cfg(not(target_arch = "wasm32"))]
    pending_plugin_grips_rearm: std::collections::BTreeSet<String>,
    /// Open multi-functional grip popup. Persists across mouse moves
    /// until dismissed (click outside, ESC, cursor leaves the grip).
    grip_popup: Option<GripPopup>,
    grip_pending: Option<GripPendingValue>,
    /// Open dynamic-block visibility-state dropdown.
    visibility_popup: Option<visibility::VisibilityPopup>,
    table_popup: Option<visibility::TablePopup>,
    /// A leader line just added via the "Add Leader" grip menu whose arrow is
    /// being placed (follows the cursor). `(entity handle, new-arrow grip id)`.
    /// Esc before the placement click removes it again.
    grip_add_provisional: Option<(document::Handle, usize)>,
    /// Ctrl cycle of a hot polyline grip (Stretch, Add Vertex, and Remove
    /// Vertex or Convert to Arc/Line).
    grip_ctrl_cycle: Option<GripCtrlCycle>,
    /// Handles hidden from the base tessellation during an in-progress grip
    /// drag. The edited entities are shown in the overlay until commit.
    grip_preview_handles: Vec<document::Handle>,
    /// Pending rollover hit-test. Each idle cursor move stashes
    /// `(last_move_at, point, tab)` here and clears the live highlight;
    /// `HoverDwellTick` runs the pick once the cursor has been still for
    /// `HOVER_DWELL_MS`. Skipping the pick mid-stroke avoids the per-frame
    /// O(N) wire+hatch+mesh sweep that froze the cursor on large drawings.
    hover_dwell: Option<HoverDwell>,
    /// When a moving cursor may next run the rollover pick, so the highlight
    /// follows the pointer instead of waiting for it to stop. Spaced like the
    /// moving snap pass: `SNAP_MOVING_MS`, or four times the last pick's cost.
    hover_pick_due: iced::time::Instant,
    /// The pointer position a coalesced move withheld, with its tab.
    ///
    /// Nothing used to drop a pointer move: every position the mouse reported
    /// was answered in full, which on a large drawing costs ~15 ms against a
    /// mouse that reports every few. The queue could then only grow — a
    /// measured drag spent 13.5 s over seven frames working through 986
    /// positions the cursor had long left. A superseded position is overwritten
    /// here unanswered, and `MoveCoalesceTick` answers the last one.
    move_owed: Option<(iced::Point, usize)>,
    /// Latest box-preview position awaiting a frame, and its tab.
    box_preview_pending: Option<(iced::Point, usize, u64)>,
    /// Tab whose Properties panel rebuild was deferred off the commit event
    /// (a huge selection's build can take ~100 ms). Drives the
    /// `PropertiesRefreshTick` frame subscription; cleared when it runs.
    properties_refresh_pending: Option<usize>,
    /// Adaptive earliest time for the next expensive preview calculation.
    box_preview_due: Option<Instant>,
    box_preview_cache: Option<BoxPreviewCache>,
    /// When the next full pointer-move pass may run, set only after a pass
    /// expensive enough to be worth coalescing (`MOVE_COALESCE_FROM_MS`).
    /// `None` means every move runs its pass, as before.
    move_pass_due: Option<Instant>,
    /// Moves withheld since the last answered pass, reported with it under
    /// `PERF=1`: each one is a pass that used to be paid for in full and
    /// thrown away, so this is the queue that is no longer forming.
    move_withheld: u32,
    /// Pending object-snap pass while a command is active. Each cursor
    /// move stashes `(last_move_at, point, tab)` here; `SnapDwellTick`
    /// runs the full snap once the cursor has been still for `SNAP_DWELL_MS`.
    snap_dwell: Option<SnapDwell>,
    /// Constraint kind shown after the ordinary rollover dwell while the
    /// cursor remains over one of its viewport indicators.
    constraint_glyph_tooltip: Option<crate::scene::parametric_constraints::ConstraintKind>,
    /// Snapshots of edited entities taken at the start of a grip drag. The drag
    /// mutates the document live, so Escape restores this group atomically.
    grip_originals: Vec<(document::Handle, document::EntityType)>,
    /// Solid-history objects paired with their owning entity before a grip drag.
    grip_history_originals: Vec<(
        document::Handle,
        Vec<(document::Handle, document::objects::ObjectType)>,
    )>,
    /// Document dirty state before the live grip mutation began.
    grip_dirty_before: Option<bool>,
    /// Where the press that picked up the active grip went down: released
    /// near it, the press was a click and the grip stays hot for a second
    /// click to place it.
    grip_engage_press: Option<Point>,
    /// A point an MTP / M2P modifier resolved while a grip was hot: the next
    /// grip placement uses it instead of the typed or cursor point.
    grip_point_override: Option<glam::DVec3>,
    /// Frozen pre-drag wire geometry used only as a visual/reference snapshot.
    ///
    /// It is deliberately NOT added to normal snap candidates: the live entity is
    /// hidden while dragging and this copy only preserves its original appearance
    /// and provides edge directions when the engaged grip is acquired for OTRACK.
    grip_reference_wires: Vec<crate::scene::model::wire_model::WireModel>,
    /// Drag-start snapshot of the dragged entity's SDF glyph quads. A whole-
    /// entity text move slides these each frame (translating the already-shaped
    /// glyphs) instead of re-tessellating the run every cursor move (issue #316).
    grip_text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
    /// True when the current grip drag is a rigid whole-entity move of pure text
    /// (TEXT / MTEXT, no wire geometry, insertion grip) — the case where the
    /// glyphs can just be slid. A reshape grip (e.g. an MTEXT width handle) or an
    /// entity with wires re-tessellates instead so the preview stays exact.
    grip_text_slide: bool,
    /// Open Quick Select panel state. `None` = panel closed. Filters are
    /// applied via `Message::QSelectApply`; the panel is dismissed on
    /// Apply / Cancel / Esc / outside-click.
    qselect: Option<QSelectState>,
    /// The FILTER dialog while it is open.
    filter_dialog: Option<filter_dialog::FilterDialogState>,
    /// LAYERP is putting layers back; that change is not one to return to.
    restoring_layer_previous: bool,
    /// The Layer manager's import dialog while it is open.
    layer_import: Option<layer_import::LayerImportState>,
    /// 'FILTER or 'QSELECT opened inside this command's "Select objects:";
    /// the dialogs hand their result to it (#2611).
    transparent_select: Option<String>,
    qselect_settings: Option<QSelectSettings>,
    /// Show the UCS icon in the bottom-left corner of model space (UCSICON).
    show_ucs_icon: bool,
    /// Show the viewport controls in the drawing's corner (VPCONTROL).
    show_viewport_controls: bool,
    /// Anchor the UCS icon to the projected UCS origin when it is on-screen,
    /// falling back to the corner otherwise (UCSICON ORigin / NOorigin).
    ucs_icon_at_origin: bool,
    /// Whether the ViewCube 3D gizmo is visible in model space (NAVVCUBE).
    show_viewcube: bool,
    /// NAVVCUBEOPACITY: the cube's opacity in percent while the cursor is
    /// away from it.
    navvcube_opacity: u8,
    /// Measured natural width (px, as `f32` bits) of the active viewport's
    /// render-mode control bar, written each frame by its `DensitySwap` and read
    /// next frame to decide whether the ViewCube still has room beside it — so
    /// the two corner widgets adapt to the bar's real width, not an estimate.
    render_bar_w: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// Whether the visual-style flyout beside the active viewport is open.
    render_mode_menu_open: bool,
    /// Mode whose sample is shown while the pointer moves through the flyout.
    /// This does not alter the drawing until the corresponding row is clicked.
    render_mode_preview: Option<document::entities::ViewportRenderMode>,
    /// Whether the Properties panel is shown on the left (PROPERTIES).
    show_properties: bool,
    /// The Layer Manager pallet (LAYERS) is open.
    pub(crate) show_layers: bool,
    /// Whether the Graphic Attributes palette is shown (GRAPHICATTRIBUTES).
    show_graphic_attributes: bool,
    /// The Command History pallet is open.
    pub(crate) show_command_history: bool,
    graphic_attributes: crate::ui::window::graphic_attributes::GraphicAttributesState,
    /// Docked Insert Block panel visibility.
    pub(crate) show_block_palette: bool,
    /// Docked Tool Palettes panel visibility (TOOLPALETTES / TP).
    pub(crate) show_tool_palettes: bool,
    /// Reusable block tools and palette tabs shown in Tool Palettes.
    pub(crate) tool_palettes: crate::ui::window::tool_palettes::ToolPalettesState,
    /// Authoring palette preference, visible only in a Block Editor session.
    show_authoring_palettes: bool,
    show_parameters_manager: bool,
    authoring_palette_tab: crate::ui::window::authoring_palettes::Tab,
    /// Node graph overlay over the viewport.
    pub(crate) show_node_graph: bool,
    /// How the node graph shows links; a view setting, saved with the
    /// preferences and not in the graph's file.
    pub(crate) node_graph_links: crate::ui::node_graph::LinkDisplay,
    /// Entity the Properties handlers act on instead of the selection, set
    /// only for the duration of a node-graph property write.
    pub(crate) property_target_override: Option<document::Handle>,
    /// A node-graph evaluation is recording; its writes join that one undo step.
    pub(crate) graph_undo_open: bool,
    /// Docked External References panel visibility (EXTERNALREFERENCES).
    pub(crate) show_external_references: bool,
    /// Whether the Browser panel is shown. Off until BROWSER opens it, so
    /// the default layout is unchanged for existing users.
    pub(crate) show_browser: bool,
    /// Point Cloud Manager palette (POINTCLOUDMANAGER).
    pub(crate) pc_manager: crate::ui::window::pc_manager::PcManager,
    pub(crate) count_palette: crate::ui::window::count_palette::CountPalette,
    /// The QuickCalc pallet (QUICKCALC / QC).
    pub(crate) quick_calc: crate::ui::window::quick_calc::QuickCalcPalette,
    /// The Assistant pallet (ASSISTANT): chat with an external AI agent about this drawing.
    pub(crate) assistant: crate::ui::window::assistant::AssistantPalette,
    /// Sheet Set Manager palette, the open sheet sets and their dialogs.
    pub(crate) sheet_set: crate::ui::window::sheet_set::SheetSetManager,
    /// A sheet opened from the manager: its drawing (path key) and layout,
    /// switched to once the drawing has loaded.
    pub(crate) sheet_set_pending_layout: Option<(String, String)>,
    /// The subset / set Import Layout as Sheet adds to, while its file picker is open.
    pub(crate) sheet_set_import_parent: Option<String>,
    /// General edge-stack dock layout for the side panels.
    pub(crate) dock: crate::ui::dock::DockState,
    /// Pallet revealed although it auto-hides (its edge's group, or its
    /// floating frame): hovered, being dragged, or just opened. Cleared when
    /// the pointer leaves.
    pub(crate) dock_peek: Option<crate::ui::dock::PanelId>,
    /// Reveal or hide waiting out the hover delay, with the generation its
    /// timer carries; any newer hover supersedes it.
    pub(crate) dock_hover_pending: Option<crate::ui::dock::HoverPending>,
    pub(crate) dock_hover_gen: u64,
    /// What the pointer is dragging in the dock (pallet, group, width,
    /// splitter, floating size, Layer Manager column).
    pub(crate) dock_drag: Option<crate::ui::dock::DockDrag>,
    /// Pallet whose strip icon (or floating frame) the pointer is over.
    pub(crate) dock_icon_hover: Option<crate::ui::dock::PanelId>,
    /// Last pointer position during a dock drag.
    pub(crate) dock_drag_last: Option<iced::Point>,
    /// Docked panel whose title bar the pointer is over (shows pin / close).
    pub(crate) dock_title_hover: Option<crate::ui::dock::PanelId>,
    /// Edge whose icon-strip pallet menu (+) is open.
    pub(crate) dock_edge_menu: Option<crate::app::config::DockSide>,
    /// Where a right-click opened that menu; `None` opens it at the + button.
    pub(crate) dock_edge_menu_y: Option<f32>,
    /// Last pointer height over an icon strip.
    pub(crate) dock_strip_pointer_y: f32,
    /// Floating pallet whose title-bar right-click menu is open.
    pub(crate) dock_float_menu: Option<crate::ui::dock::PanelId>,
    /// Open tool dialogs, back to front (see `tool_windows`).
    pub(crate) tool_windows: Vec<tool_windows::ToolWindow>,
    /// The tool swapped in as the active modal for the update running now.
    pub(crate) tool_focus: Option<ModalKind>,
    /// The tool dialog the keyboard goes to, as it went to a modal one.
    pub(crate) keyboard_tool: Option<ModalKind>,
    /// Desktop: a dialog window opened again while open, to bring to the
    /// front once the update settles.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) tool_window_to_raise: Option<iced::window::Id>,
    /// A tool dialog's frame being moved or resized.
    pub(crate) tool_drag: Option<tool_windows::ToolDrag>,
    /// Desktop: the windows of their own (tool dialogs and pallets).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) os_windows: Vec<tool_windows::OsWindow>,
    /// Desktop: the window that has the focus, main or of its own.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) focused_window: Option<window::Id>,
    /// Pointer position over a floating title bar (bar-local), and where
    /// its open menu was asked for.
    pub(crate) dock_float_pointer: iced::Point,
    pub(crate) dock_float_menu_at: iced::Point,
    /// Group whose edge band (group grip) the pointer is over.
    pub(crate) dock_grip_hover: Option<(crate::app::config::DockSide, usize)>,
    /// The group grip that has been hovered long enough to light.
    pub(crate) dock_grip_lit: Option<(crate::app::config::DockSide, usize)>,
    /// Counts grip hovers, so only the latest hover's timer lights a grip.
    pub(crate) dock_grip_gen: u32,
    /// Reference-table column currently being width-resized (column index),
    /// with the last pointer position. Mirrors the Layers Name-column drag.
    pub(crate) xref_col_drag: Option<usize>,
    pub(crate) xref_col_last: Option<iced::Point>,
    /// Table/lower-pane split divider drag in progress.
    pub(crate) xref_split_drag: bool,
    /// Docked Insert Block panel state (search, preview size, cached thumbnails).
    pub(crate) block_palette: crate::ui::window::block_palette::BlockPalette,
    /// Reference Manager palette state (display-only in Task 7).
    pub(crate) xref_manager: crate::ui::window::xref_manager::XrefManagerPanel,
    /// Whether the document file tabs are shown at the top (FILETAB).
    show_file_tabs: bool,
    /// The drawing tabs run across the window above the palettes instead
    /// of between them.
    file_tabs_above_docks: bool,
    /// Whether the layout/paper-space tabs are shown at the bottom (LAYOUTTAB).
    show_layout_tabs: bool,
    /// Last point committed by a drawing command — used as ortho/polar base.
    last_point: Option<glam::DVec3>,
    /// Viewport used by the current snap; the displayed point is in paper space.
    pub(crate) vp_snap_frame: Option<crate::scene::viewport_ref::ViewportFrame>,
    /// Acquired coordinates and source identities in command-step order.
    /// Cleared when the command starts or ends.
    accepted_snaps: Vec<crate::scene::viewport_ref::AcceptedSnap>,
    /// Click result retained until the command accepts its point.
    pending_click_snap: Option<(
        crate::snap::SnapResult,
        Option<crate::scene::viewport_ref::ViewportFrame>,
    )>,
    /// Endpoint + unit exit-tangent of the most recently drawn line/arc, so
    /// `ARC_CONT` (Arc → Continue) can start tangentially from where drawing
    /// ended. `None` once a non-line/arc entity is committed.
    cont_anchor: Option<(glam::DVec3, glam::DVec3)>,
    /// OS window Id for the floating Layer Properties Manager (None when closed).
    /// OS window Id of the primary application window.
    main_window: Option<window::Id>,
    /// Hides drawing UI overlays for one thumbnail capture frame.
    thumbnail_capture_clean: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pending_native_thumbnail_save: Option<PendingNativeThumbnailSave>,
    #[cfg(target_arch = "wasm32")]
    pending_web_thumbnail_save: Option<PendingWebThumbnailSave>,
    /// Web: whether the save in flight came from a plain Save
    /// (Save/QSAVE or the save-before-close flow), so the tab's retained
    /// File System Access handle (see io::fs_access) may be written
    /// through directly; Save As opens the dialog modal and must re-pick.
    /// Set true by `save_with_default_format`, false by
    /// `open_save_dialog_window`.
    #[cfg(target_arch = "wasm32")]
    save_plain_reuse_fs_handle: bool,
    /// One web autosave round is running; ticks that fire while it lasts are
    /// dropped instead of queued (the next interval retries).
    #[cfg(target_arch = "wasm32")]
    web_autosave_in_flight: bool,
    /// Web autosave storage already reported a failure — stay quiet until a
    /// round succeeds again, so a full quota does not spam every tick.
    #[cfg(target_arch = "wasm32")]
    web_autosave_warned: bool,
    /// Crash-recovery snapshots found at boot (newest first) that the restore
    /// prompt still has to offer, one at a time.
    #[cfg(target_arch = "wasm32")]
    web_autosave_pending: Vec<crate::io::web_autosave::WebAutosaveEntry>,
    /// Snapshots a "Restore all" click still expects to settle (opened,
    /// failed, cancelled, unreadable, or rejected by the open queue). Each
    /// settle funnels through `queue_next_web_autosave_prompt`, which counts
    /// this down; reaching zero arms `web_session_focus_pending`.
    #[cfg(target_arch = "wasm32")]
    web_autosave_restore_all_remaining: usize,
    /// A Restore-all batch finished settling — apply the remembered
    /// session's active tab at the next message boundary, after the last
    /// restored open has actually inserted its tab (`web_autosave_opened`
    /// runs before that insertion, so applying there would pick against a
    /// tab list still missing the last restore).
    #[cfg(target_arch = "wasm32")]
    web_session_focus_pending: bool,
    /// The tab list the previous session remembered (localStorage), loaded
    /// after the boot autosave scan. Advisory only: it feeds the restore
    /// modal's session line and the post-restore tab selection — nothing is
    /// ever auto-opened from it.
    #[cfg(target_arch = "wasm32")]
    web_session: Option<crate::io::web_session::WebSession>,
    /// The snapshot whose recovered open is in flight: `(autosave key,
    /// open id)`. The OPFS copy is the only one, so it is deleted only
    /// once the open with exactly that id succeeds — identity, never the
    /// display name (same-named files share one stored copy on web).
    #[cfg(target_arch = "wasm32")]
    web_autosave_consumed: Option<(String, u64)>,
    /// Autosave keys a web save wanted to remove while a snapshot round was
    /// still writing (the round's late write would resurrect the snapshot
    /// with a fresh mtime); the removes run when the round finishes.
    #[cfg(target_arch = "wasm32")]
    web_autosave_clear_after: Vec<String>,
    // ── Floating panel windows ────────────────────────────────────────────
    /// The open Select Color panel, if any.
    color_picker: Option<crate::ui::color_select::ColorPicker>,
    /// The built-in colour book and those in the colour book folder, read
    /// lazily when the Select Color panel first opens.
    color_books: std::sync::Arc<Vec<crate::ui::color_select::book::ColorBook>>,
    color_books_read: bool,
    /// Recently committed real colours, newest first.
    /// ACI and True Color remain semantically distinct even when their RGB matches.
    /// The open in-canvas modal dialog, if any (Plan B: shared overlay instead
    /// of OS windows).
    active_modal: Option<ModalKind>,
    /// State backing an active declarative plugin modal dialog.
    pub(crate) plugin_dialog_state: Option<crate::ui::window::plugin_dialog::PluginDialogState>,
    /// Persistent dockable side panels registered by plugins.
    pub(crate) plugin_panels:
        std::collections::BTreeMap<String, crate::ui::window::plugin_panel::PluginPanelState>,
    /// Numeric mapping for PanelId::Plugin(u32).
    pub(crate) plugin_panel_ids: std::collections::HashMap<String, u32>,
    pub(crate) plugin_panel_by_num: std::collections::HashMap<u32, String>,
    /// Selection and staged values owned by the Properties hyperlink dialog.
    hyperlink_editor_handles: Vec<document::Handle>,
    hyperlink_editor_block: Option<(u64, document::Handle)>,
    hyperlink_editor_url: String,
    hyperlink_editor_description: String,
    hyperlink_editor_mixed: bool,
    hyperlink_editor_dirty: bool,
    hyperlink_options: crate::ui::window::hyperlink_dialog::HyperlinkOptions,
    pending_startup_modals: std::collections::VecDeque<ModalKind>,
    /// What is drawing the scene, once the first frame has told us. Drives
    /// the graphics warning (popup, status-bar pill, command line).
    gpu_status: crate::scene::pipeline::GpuStatus,
    /// The pipeline's status generation this app has already looked at.
    gpu_status_generation: u64,
    /// `GpuStatus::identity()` of the verdict whose popup the user silenced
    /// with "Don't show again for this device". Persisted in the settings.
    gpu_warning_silenced: String,
    /// Plot modal geometry preserved while the Plot Style editor is open as
    /// a child dialog. None means Plotstyle was opened directly (e.g. command).
    plotstyle_parent_plot_geometry: Option<(iced::Vector, iced::Vector)>,
    /// FIND dialog inputs and current result cursor.
    find_replace: FindReplaceState,
    /// The text editor's Find and Replace options, kept between openings:
    /// match case, whole words, wildcards, diacritics, half/full width.
    mtext_find_prefs: Option<[bool; 5]>,
    overkill_dialog: Option<crate::ui::window::overkill::State>,
    /// Set once the user acknowledges the AEC-drop warning, so re-entering the
    /// save path proceeds instead of re-showing the warning.
    aec_drop_acknowledged: bool,
    /// Number of unsupported objects shown in the AEC-drop warning modal.
    aec_drop_count: usize,
    /// What a converter would lose, shown in the conversion-loss modal.
    conversion_losses: Vec<String>,
    /// The save held back while that modal asks.
    pending_conversion_save: Option<PendingConversionSave>,
    pending_ifccad_alpha_save: Option<PendingConversionSave>,
    /// Set by "Save anyway" for the save it releases; that save takes it.
    conversion_loss_allowed: bool,
    /// Layers awaiting a "delete non-empty layer(s)" confirmation: `(names,
    /// total object count)`. Set when the user deletes one or more layers that
    /// still have objects; the warning modal reads it, and confirming erases
    /// those objects too.
    layer_delete_pending: Option<(Vec<String>, usize)>,
    /// Pixel offset of the active modal from screen-centre (drag-to-move).
    /// Reset to zero whenever a modal closes so each dialog opens centred.
    modal_offset: iced::Vector,
    /// Cursor position from the previous drag-move while the modal title bar is
    /// held; `None` before the first move of a drag.
    modal_drag_last: Option<Point>,
    /// True while the modal title bar is held (a drag is in progress).
    modal_dragging: bool,
    /// Layer Manager Name column width in px, adjusted by the divider drag.
    layer_name_col_w: f32,
    /// Widths of the Layer Manager columns the user sized (the rest keep
    /// their default).
    layer_col_w: std::collections::HashMap<crate::ui::window::layers::LayerSortCol, f32>,
    /// Widths of Layer Walk's name and object-count columns.
    layer_walk_col_w: [f32; 2],
    /// How far the user has dragged the modal's corner resize grip from the
    /// dialog's natural size (added to its measured width/height). Reset
    /// with `modal_offset` so every dialog opens at its own size.
    modal_resize: iced::Vector,
    /// Last body size reported by the shared modal frame. Used for drag bounds
    /// and controls whose range follows the real, automatically measured width.
    modal_content_size: Option<iced::Size>,
    /// True while the modal's corner resize grip is held.
    modal_resizing: bool,
    // ── Attribute editor dialog (ATTEDIT / double-click a block) ───────────
    /// INSERT whose attributes the editor modal is editing (`None` = closed).
    attr_editor_handle: Option<document::Handle>,
    /// Block name shown in the editor's title bar.
    attr_editor_block: String,
    /// Working copy of the block's attributes, in the same order as
    /// `Insert::attributes`. Each row carries the value plus the text-option and
    /// common-property fields the editor edits; written back on OK.
    attr_editor_rows: Vec<crate::ui::window::attribute_editor::AttrRow>,
    /// Which editor tab is showing.
    attr_editor_tab: crate::ui::window::attribute_editor::AttrTab,
    /// Highlighted attribute row driving the Text Options / Properties tabs.
    attr_editor_selected: usize,
    /// The editing session: what was last applied, for Cancel and Apply.
    attr_editor_session: Option<attr_editor::AttrEditorSession>,
    /// A refused value, shown in the window.
    attr_editor_error: Option<String>,
    /// "Keep the changes?" before Select block picks another block.
    attr_editor_asking: bool,
    /// The field last typed in, checked when the cursor leaves it.
    attr_editor_editing: Option<crate::ui::window::attribute_editor::AttrField>,
    /// Plugin ids the user turned off in the Plugin Manager. Disabled plugins
    /// keep their manifest listed but drop their ribbon tab and command
    /// dispatch. Persisted via [`settings::UserSettings::disabled_plugins`].
    disabled_plugins: rustc_hash::FxHashSet<String>,
    /// AI tools each loaded plugin offers (API v25), read when the Plugin
    /// Manager opens or a client reads `plugins`.
    #[cfg(not(target_arch = "wasm32"))]
    plugin_tools: rustc_hash::FxHashMap<String, Vec<ocs_plugin_api::host::PluginTool>>,
    /// `(tab id, selection fingerprint)` last broadcast to V4 plugins, so
    /// `SelectionChangedV4` fires once per real change rather than per message.
    #[cfg(not(target_arch = "wasm32"))]
    last_plugin_selection: Option<(u64, u64)>,
    /// Monotonic per-tab selection revisions, bumped only on an actual
    /// selection change; carried by the v22 `SelectionChangedDigest`.
    #[cfg(not(target_arch = "wasm32"))]
    selection_revision_by_tab: rustc_hash::FxHashMap<u64, u64>,
    /// `(tab id, geometry epoch)` last published to the V4 document view.
    /// Built-in edits bypass HostSession, so this is checked at message
    /// boundaries as well as after plugin-initiated writes.
    #[cfg(not(target_arch = "wasm32"))]
    last_plugin_document: Option<(u64, u64)>,
    /// External add-on packages found in the plugins folder, refreshed when the
    /// Plugin Manager opens.
    external_plugins: Vec<crate::plugin::external::ExternalPlugin>,
    /// Ids of external packages actually loaded this session (a subset of
    /// `external_plugins` — compatible, with a library, dlopen'd at startup).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    loaded_plugin_ids: rustc_hash::FxHashSet<String>,
    /// Startup load failures keyed by plugin id. The Plugin Manager uses these
    /// to distinguish an installed-but-failed package from one awaiting restart.
    plugin_load_errors: rustc_hash::FxHashMap<String, String>,
    /// Curated plugin registry fetched from the OpenCADStudio repo.
    plugin_registry: Vec<crate::plugin::external::RegistryEntry>,
    /// True while the curated registry request is in flight.
    plugin_registry_loading: bool,
    /// Last curated-registry request error. Kept separate from general
    /// marketplace status so the UI can show a friendly retry card and retain
    /// copyable technical details.
    plugin_registry_error: Option<String>,
    /// Whether the registry error card exposes its raw diagnostic text.
    plugin_registry_error_details_open: bool,
    /// User-linked plugin source repos (`owner/repo`) beyond the curated list.
    plugin_repos: Vec<String>,
    /// Add-repository text field in the Plugin Manager.
    plugin_repo_input: String,
    /// Live filter for installed and available plugin cards.
    plugin_search_input: String,
    /// Installable release tags fetched per linked repo (for the dropdown).
    repo_release_tags: rustc_hash::FxHashMap<String, Vec<crate::plugin::external::ReleaseInfo>>,
    /// The release tag currently selected per linked repo.
    repo_selected_tag: rustc_hash::FxHashMap<String, String>,
    /// Repository currently shown in the Plugin Manager detail pane.
    selected_plugin_repo: Option<String>,
    /// Parsed GitHub README content or the last fetch error, cached per repo.
    plugin_readmes: rustc_hash::FxHashMap<String, Result<iced::widget::markdown::Content, String>>,
    /// README requests in flight, used to render a deterministic loading state.
    plugin_readme_loading: rustc_hash::FxHashSet<String>,
    /// Repositories whose release list is being fetched.
    plugin_releases_loading: rustc_hash::FxHashSet<String>,
    /// Repositories whose install or update is downloading.
    plugin_installing: rustc_hash::FxHashSet<String>,
    /// Embedded documents open in their own program: the tab id, the
    /// object, the unpacked file and when it was last seen saved.
    ole_edits: Vec<(u64, document::Handle, std::path::PathBuf, Option<std::time::SystemTime>)>,
    /// Repositories installed in this session and waiting for a restart:
    /// `true` for a fresh install, `false` for an update.
    plugin_installed_now: rustc_hash::FxHashMap<String, bool>,
    /// Last marketplace status / error line shown in the Plugin Manager.
    marketplace_status: String,
    /// PDSIZE text buffer for the Point Style (DDPTYPE) dialog.
    point_size_buf: String,
    /// Tab that owns the staged Point Style edits, so OK cannot write to a
    /// different document if the active tab changes while the dialog is open.
    point_style_tab_id: Option<u64>,
    /// Staged PDMODE selection; the document is changed only when the dialog
    /// is accepted.
    point_mode_buf: Option<i16>,
    /// Point Style size mode: `true` = relative to screen, `false` = absolute.
    /// Tracked separately from the PDSIZE sign so a size of 0 (sign-less) still
    /// remembers which radio is active.
    point_size_relative: bool,
    /// Whether the default-association prompt has been answered.
    default_assoc_prompted: bool,
    /// Offer to download missing `.shx` fonts on open (see `io::font_repo`).
    check_missing_fonts: bool,
    /// Custom font source base URL; empty selects the community repository.
    font_source_url: String,
    /// Template a new drawing starts from (QNEW); empty = blank.
    qnew_template: String,
    /// Source-relative, type-structured, or flat ETRANSMIT/ARCHIVE package.
    transmittal_layout: crate::io::transmit::TransmittalLayout,
    /// The start page's template list, while it is open: the drawing
    /// templates beside the default one.
    start_templates: Option<Vec<std::path::PathBuf>>,
    /// Editable copy of `font_source_url` shown in the missing-fonts prompt.
    font_source_input: String,
    donation_prompt_version: String,
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
    clipboard: Vec<document::EntityType>,
    /// Entities removed by the most recent ERASE, kept so OOPS can restore them.
    oops_cache: Vec<Arc<document::EntityType>>,
    /// Paste anchor: lower-left corner of the clipboard entities' bounding box
    /// (or the point picked by COPYBASE). This point lands under the cursor at
    /// paste time.
    clipboard_base: glam::DVec3,
    /// Table records (layer / linetype / text + dim style) the clipboard
    /// entities reference, captured from the source drawing at copy time so a
    /// paste into a *different* drawing can recreate any that are missing —
    /// otherwise the pasted entities would dangle on a non-existent layer.
    clipboard_deps: ClipboardDeps,
    /// The document (tab id) the clipboard was copied from: only a paste back
    /// into it can give a pasted associative array an array of its own.
    clipboard_source_tab: Option<u64>,
    /// True while the Shift key is held — drives subtractive pick (Shift+click
    /// removes the picked entity from the selection). Tracked from keyboard
    /// modifier-change events since mouse click messages carry no modifiers.
    shift_down: bool,
    /// True while Ctrl/Cmd is held — drives Ctrl-click multi-select in the
    /// Layer Manager. Tracked from modifier-change events.
    ctrl_down: bool,
    /// Open in-place MText editor (toolbar + text area + live preview), if any.
    mtext_editor: Option<mtext_editor::MTextEditorState>,
    table_cell_editor: Option<table_cell_edit::TableCellEditor>,
    table_edit_command_line: bool,
    /// Return rich text to a suspended drawing command.
    command_mtext_input: bool,
    pending_command_editor_text: Option<String>,
    /// Open in-place single-line TEXT editor (plain text-entry box), if any.
    text_inline: Option<text_inline::TextInlineState>,
    /// Cursor-anchored one-shot snap override menu (Shift+RMB): the canvas
    /// point it opened at, or `None` when closed (#337).
    snap_override_popup: Option<iced::Point>,
    /// The Shift+letter temporary override held during a point prompt
    /// (#3016): the typed key and what it changed, undone on release.
    held_override: Option<(char, crate::app::update::viewport_snap::HeldOverride)>,
    /// Hard axis lock (#312): the WCS direction captured when Shift went down
    /// during a rubber-band point pick. While `Some`, every candidate point —
    /// osnap hits included — projects onto this ray from `last_point` (or the
    /// grip origin). Cleared when Shift releases.
    axis_lock_dir: Option<glam::DVec3>,
    /// A direction typed as `<angle` at a point prompt: the next point stays
    /// on it until a point is placed.
    angle_override: Option<glam::DVec3>,
    /// A left press on a selected object: the point it grabbed, and whether
    /// the drag has turned into a MOVE (or, with Ctrl, a COPY) of the
    /// selection.
    drag_move: Option<(glam::DVec3, bool)>,
    /// Inline rename state: (original_name, current_edit_value).
    layout_rename_state: Option<(String, String)>,
    /// Timestamp of the previous viewport left-click release (for double-click detection).
    last_vp_click_time: Option<Instant>,
    /// Screen position of the previous viewport left-click release.
    last_vp_click_pos: Option<Point>,
    /// MText preview click tracking for double-click (select word) / triple-click
    /// (select all): the previous click's time + visible offset, and the run
    /// length of quick same-spot clicks (1..=3).
    mtext_click_time: Option<Instant>,
    mtext_click_off: usize,
    mtext_click_count: u8,
    /// The last text copied in the MTEXT editor: its plain text (what the
    /// system clipboard got) and the same characters as formatted MTEXT, so
    /// pasting it back keeps superscript, fractions and formatting.
    mtext_clipboard: Option<(String, String)>,
    /// Pending model-space plot window (x0, y0, x1, y1) in world XY, or None.
    plot_window: Option<(f64, f64, f64, f64)>,
    /// Sheet the Plot dialog opens with when the layout has no page setup;
    /// updated from every plot / apply so the next dialog remembers it.
    plot_paper: crate::io::paper_catalog::PaperSize,
    plot_orientation: crate::io::paper_catalog::Orientation,
    /// Backing state for the full Plot / Print dialog.
    plot_dialog: crate::ui::window::plot::PlotDialogState,
    /// Snapshot of the dialog's settings taken when it opened, restored by the
    /// `<previous>` list entry.
    plot_prev: Option<crate::ui::window::plot::PlotDialogState>,
    /// Tab/layout that last owned the Plot dialog. Reopening Plot in the same
    /// context keeps the user's last paper, scale, CTB and output choices
    /// instead of reapplying the layout page setup over them.
    plot_prev_context: Option<(usize, String)>,
    /// Printer whose capability reply is allowed to reconcile the current
    /// sheet. Set only after an explicit printer/page-setup choice; merely
    /// opening Plot must never replace the drawing's stored paper size.
    plot_reconcile_printer_media: Option<String>,
    /// Full source settings behind the fields currently shown in Plot.
    plot_setup_template: Option<document::objects::PlotSettings>,
    /// Paper layouts shown by Print All, in tab order with their selection.
    print_all_layouts: Vec<(String, bool)>,
    /// True while the Plot dialog is editing settings for Print All.
    print_all_options: bool,
    /// True when Print All should override each layout's page setup.
    print_all_settings_override: bool,
    /// Settings restored when the Print All options dialog is cancelled.
    print_all_options_prev: Option<crate::ui::window::plot::PlotDialogState>,
    /// Plot style restored together with cancelled Print All options.
    print_all_plot_style_prev: Option<Option<crate::io::plot_style::PlotStyleTable>>,
    /// Plot window restored together with cancelled Print All options.
    print_all_plot_window_prev: Option<Option<(f64, f64, f64, f64)>>,
    print_all_plot_setup_prev: Option<Option<document::objects::PlotSettings>>,

    // ── Plot Style Table ──────────────────────────────────────────────────
    /// Currently loaded CTB/STB table (None = no override).
    active_plot_style: Option<crate::io::plot_style::PlotStyleTable>,
    /// Table state at Plot Style editor open / last save. Edits preview live,
    /// but closing the editor restores this baseline until Save succeeds.
    pub(super) plotstyle_panel_baseline: Option<Option<crate::io::plot_style::PlotStyleTable>>,

    // ── MLineStyle Dialog ─────────────────────────────────────────────────
    linetype_selected: String,
    linetype_tab: u8,
    linetype_in_use_only: bool,
    /// Up and Down step the list. Off once a field is typed in, so an arrow
    /// there does not change the linetype; a press on a row turns it back on.
    linetype_keys_active: bool,
    /// `None` until the list reports a scroll position; a list that fits
    /// never does.
    linetype_list_view: Option<crate::ui::style::linetype::ListView>,
    linetype_counts: rustc_hash::FxHashMap<String, usize>,
    linetype_draft: crate::ui::style::linetype::LinetypeDraft,
    linetype_library_status: String,
    mlstyle_selected: String,
    mlstyle_tab: u8,
    mlstyle_compare: String,
    mln_description: String,
    mln_start_angle: String,
    mln_end_angle: String,
    mln_fill_color: String,
    mln_elements: Vec<[String; 3]>,

    // ── MLeaderStyle Dialog ───────────────────────────────────────────────
    mleaderstyle_selected: String,
    /// The manager's faces and choosers (New, Modify, Delete, notices).
    mls: crate::ui::style::mleaderstyle::Dialog,
    /// Colour field whose expanded palette is open (line/text/block).
    mls_color_open: Option<&'static str>,
    mls_landing_distance: String,
    mls_landing_gap: String,
    mls_arrowhead_size: String,
    mls_text_height: String,
    mls_scale_factor: String,
    mls_break_gap: String,
    mls_max_points: String,
    mls_default_text: String,
    mls_line_color: String,
    mls_text_color: String,
    mls_block_color: String,
    mls_block_scale: String,

    // ── TableStyle Dialog ─────────────────────────────────────────────────
    tablestyle_selected: String,
    tablestyle_tab: u8,
    tablestyle_compare: String,
    /// General table-style description buffer.
    ts_description: String,
    /// The cell style being edited ("_DATA", "_HEADER", "_TITLE" or a
    /// user cell style).
    ts_cell: String,
    /// Text buffers of the edited cell style: text height, text angle and
    /// margins.
    ts_cell_height: String,
    ts_cell_angle: String,
    ts_cell_hmargin: String,
    ts_cell_vmargin: String,
    /// The Borders page's line properties and the cards over the window
    /// (New / Manage cell styles, Select Linetype, Load).
    ts_border: crate::ui::style::tablestyle::BorderDraft,
    ts_cards: crate::ui::style::tablestyle::TsCards,
    /// The Cell Data Format window / Text Style manager is open from the
    /// table style window and returns to it.
    ts_format_return: bool,
    ts_text_return: bool,
    /// Colour field ("textcolor" / "fillcolor") whose expanded palette is
    /// open.
    ts_color_open: Option<&'static str>,

    // ── Shared style-manager inline rename ────────────────────────────────
    /// Original name of the style currently being renamed inline (double-click
    /// a style name in any style manager). `None` when not renaming.
    style_rename: Option<String>,
    /// Edit buffer for the inline rename text input.
    style_rename_buf: String,
    /// The style manager the open rename belongs to.
    style_rename_kind: Option<style_ops::StyleKind>,
    /// Active style-manager transaction. Edits mutate the document live for an
    /// in-dialog preview but only persist on Apply; closing without Apply
    /// restores this snapshot. `None` when no style manager is staging.
    style_stage: Option<style_ops::StyleStage>,
    /// The transaction of the manager a child manager was opened from (the
    /// Text Style manager from the multileader style window): it goes on
    /// when the child closes.
    style_parent_stage: Option<style_ops::StyleStage>,
    style_import: Option<update::style_import::StyleImportDraft>,

    // ── TextStyle Font Browser ────────────────────────────────────────────
    textstyle_selected: String,
    /// How many texts use each text style (ASCII-lowercase name), counted
    /// with the selected style's texts.
    textstyle_counts: rustc_hash::FxHashMap<String, usize>,
    /// The list shows only the styles in use.
    textstyle_in_use_only: bool,
    /// The preview's sample text; empty shows the default.
    textstyle_sample: String,
    textstyle_compare: String,
    /// Working copy of the selected style; written into the document before
    /// any list operation and on Apply.
    textstyle_draft: text_style_draft::TextStyleDraft,
    /// A question or notice over the text style window.
    textstyle_card: Option<crate::ui::style::textstyle::Card>,
    /// The text style whose actions (right click) show under it.
    textstyle_context: Option<String>,
    textstyle_font_picker: crate::ui::font_picker::FontPickerState,
    textstyle_bigfont_picker: crate::ui::font_picker::FontPickerState,
    /// The texts of every text style (ASCII-lowercase name), for "Where
    /// used" and the summary before Apply. Texts do not change while the
    /// manager is open; Apply walks the drawing again.
    textstyle_users: rustc_hash::FxHashMap<String, Vec<text_style_users::TextUser>>,

    // ── Color Scheme ──────────────────────────────────────────────────────
    active_theme: Theme,
    ui_theme: config::UiThemeConfig,
    theme_color_inputs: [String; 6],
    model_space: config::ModelSpaceThemeConfig,
    model_bg_input: String,
    paper_bg_input: String,
    desk_bg_input: String,
    pub(crate) saved_custom_palette: Option<config::UiThemePalette>,

    // ── Keyboard Shortcut Editor ──────────────────────────────────────────
    /// Complete editable key → command/action table.
    shortcut_bindings: rustc_hash::FxHashMap<String, String>,
    /// Working rows shown by the shortcut editor until Apply is pressed.
    shortcut_editor_rows: Vec<(String, String)>,
    /// Search text of the shortcut editor; filters the displayed rows only.
    shortcut_filter: String,
    /// Row whose Key cell is armed for capture: the next key combination
    /// pressed anywhere fills that row's key. While set, the keyboard
    /// subscription swallows every key so captured shortcuts do not run in
    /// the drawing.
    shortcut_capture_row: Option<usize>,
    /// True while a freshly added (top) row is an unfinished draft: it exists
    /// only until its key and command are filled (success) or it is cancelled
    /// (Esc / ✕ / dialog close), so the list never keeps an empty row.
    shortcut_pending_add: bool,
    /// The row whose Command box offers its suggestion list, with the text
    /// the list was opened for. Typing in a Command box sets it; the list
    /// shows only while that row still holds that text, so a row that moved
    /// or changed in any other way never shows a stale list.
    shortcut_suggest: Option<(usize, String)>,
    /// True while the "Reset to default" confirmation is showing.
    shortcut_reset_confirm: bool,
    /// True while the "unsaved changes will be discarded" confirmation
    /// overlays the editor: the user tried to close with un-applied rows.
    shortcut_close_confirm: bool,

    // ── Command Aliases ───────────────────────────────────────────────────
    /// Command-line aliases: uppercase abbreviation → uppercase command
    /// ("L" → "LINE"). Loaded from `ocad.pgp` at boot and consulted before
    /// every dispatch (`resolve_alias`); user-editable via ALIASEDIT. See
    /// [`alias`].
    command_aliases: rustc_hash::FxHashMap<String, String>,
    /// The menu-bar menu Alt+letter opened as a keyboard-driven shortcut
    /// menu; the shortcut menu shows it instead of its own rows.
    pub(super) menu_bar_popup: Option<usize>,
    /// Working buffer for the ALIASEDIT modal: `(alias, command)` rows being
    /// edited. Seeded from `command_aliases` on open, committed back on close.
    alias_editor_rows: Vec<(String, String)>,
    /// True while a freshly added (top) row is an unfinished draft: it exists
    /// only until its alias and command are filled (accept) or it is cancelled
    /// (Esc / ✕ / dialog close), so the list never keeps an empty row.
    /// Mirrors `shortcut_pending_add`.
    alias_pending_add: bool,
    /// True while the "Reset to default" confirmation is showing.
    alias_reset_confirm: bool,
    /// True while the "unsaved changes will be discarded" confirmation
    /// overlays the editor: the user tried to close with un-applied rows.
    alias_close_confirm: bool,
    /// Summary of the most recent PGP import; never activates the working rows.
    alias_import_report: String,

    // ── Named Parameters ──────────────────────────────────────────────────
    /// Working buffer for the PARAMETERS modal. Unlike `alias_editor_rows`,
    /// this isn't a copy of a separate app-level store — the real state
    /// lives per-document at `Scene::named_parameters`; this buffer is
    /// seeded from the active tab's table on open and only written back to
    /// it on Apply (`apply_named_parameter_editor_rows`,
    /// `src/app/named_parameters.rs`).
    named_parameter_editor_rows: Vec<crate::ui::window::named_parameters::ParamEditorRow>,

    // ── CAL ───────────────────────────────────────────────────────────────
    /// Variables and `ans` kept between CAL evaluations for this session.
    cal_context: calc::CalcContext,

    // ── Layout Manager Panel ──────────────────────────────────────────────
    layout_manager_selected: String,
    layout_manager_rename_buf: String,

    // ── Layer State Manager ───────────────────────────────────────────────
    layer_state_selected: Option<String>,
    layer_state_name_buf: String,
    layer_state_description_buf: String,
    layer_state_filter: String,
    layer_state_edit_draft: Option<document::LayerState>,
    layer_state_edit_filter: String,
    layer_state_edit_color_open: Option<usize>,

    // ── Annotation-scale Manager ──────────────────────────────────────────
    scale_manager_selected: String,
    scale_manager_paper_buf: String,
    scale_manager_drawing_buf: String,
    /// The scale the Edit card edits (double-click or Edit...), + the name
    /// buffer; the card adds a new scale when `scale_edit_adding`.
    scale_rename: Option<String>,
    scale_rename_buf: String,
    scale_edit_adding: bool,
    /// The scale card's refusal (no name, a taken name, a bad ratio).
    scale_edit_error: Option<String>,
    /// Every picked row of the scale list (Ctrl adds, Shift extends).
    scale_manager_multi: Vec<String>,
    /// Delete asks whether to drop the picked scales not in use.
    scale_delete_asking: bool,
    /// The entity the Annotation Object Scale dialog is editing (its per-object
    /// annotation-scale membership).
    anno_object_scale_target: Option<document::types::Handle>,
    /// The other objects the dialog edits together with the target.
    anno_object_scale_extra: Vec<document::types::Handle>,
    /// The scale list window asks which default list Reset restores.
    scale_reset_asking: bool,
    /// The Annotation Object Scale window's staged edits.
    anno_object_scale: crate::ui::style::anno_object_scale::State,
    /// Open transaction for the scale manager — restored if the window closes
    /// without Apply, mirroring the style managers' staging.
    scale_stage: Option<crate::app::style_ops::ScaleStage>,

    // ── Annotation table/data dialogs ────────────────────────────────────
    table_insert: crate::ui::window::annotation_data::TableInsertState,
    data_link_manager: crate::ui::window::annotation_data::DataLinkManagerState,
    data_link_parent_table: bool,
    /// The Table Style dialog was opened from Insert Table and returns there.
    tablestyle_parent_table: bool,
    /// The Table Style list shows only the styles in use.
    tablestyle_in_use_only: bool,
    data_extraction: crate::ui::window::annotation_data::DataExtractionState,

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
    /// Show a lightweight notice until the next layout redraw.
    pub(super) layout_settling: bool,
    open_job_serial: u64,
    /// Last repair or failed-open report shown in the recovery modal.
    recovery_report: Option<crate::io::recovery::RecoveryReport>,
    /// Missing SHX fonts of the last opened drawing, offered for download
    /// from the community repository (see `crate::io::font_repo`).
    missing_fonts: Option<Vec<String>>,
    /// Drawing that produced `missing_fonts`; the active tab may change while
    /// a download is in flight.
    missing_fonts_path: Option<PathBuf>,
    /// Prevent duplicate download tasks and keep modal contents visible while
    /// the background request is running.
    missing_fonts_downloading: bool,
    /// Missing fonts window: Skip turns the window off for good.
    missing_fonts_silence: bool,
    /// Fonts skipped or unavailable during this process lifetime. Reopening a
    /// drawing must not repeatedly nag for the same unresolved file.
    suppressed_missing_fonts: rustc_hash::FxHashSet<String>,
    /// Drawings handed to us by other launches while `opening` was busy.
    /// `opening` is a single slot that a second `OpenPathPicked` would
    /// overwrite, and `on_file_opened` drops any result arriving once it is
    /// clear — so overlapping opens silently lose documents. Selecting several
    /// drawings in a file manager produces exactly that (one process per file,
    /// all arriving at once), which makes this queue load-bearing, not polish.
    pub(super) pending_opens: std::collections::VecDeque<PathBuf>,
    pub(super) pending_startup_script_lines: Vec<String>,
    /// Script rows still to run, one at a time: a row waits until whatever
    /// the previous one started (an open, a save) has finished.
    pub(super) script_queue: std::collections::VecDeque<String>,
    /// A script row is running (the queue may already be empty).
    pub(super) script_running: bool,
    /// The next row waits for an open or save to finish.
    pub(super) script_paused: bool,
    /// SDI 1: opening a drawing closes the current one first.
    pub(super) sdi: bool,
    /// The app asked to exit; a headless batch run stops there.
    pub(super) exit_requested: bool,
    /// SIGTERM arrived: leave once the recovery copies are written.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) terminating: bool,
    /// One global interaction-index build at a time. Large drawings can each
    /// hold millions of entries, so file-open bursts must not multiply peak
    /// CPU and memory by the number of tabs.
    active_interaction_index: Option<(u64, u64, usize)>,
    /// Latest resident source requested by each waiting tab. Jobs run
    /// serially behind `active_interaction_index`.
    queued_interaction_indices: std::collections::VecDeque<(
        u64,
        u64,
        usize,
        std::sync::Arc<Vec<crate::scene::WireModel>>,
        f32,
    )>,

    // ── Unsaved-changes dialog ────────────────────────────────────────────
    /// Set when the user tries to close a tab or quit while there are unsaved changes.
    pending_close: Option<PendingClose>,
    pending_block_close: Option<(u64, block_editor::BlockEditContinuation, usize)>,
    /// Stable document ids waiting to be closed by "Close All" or
    /// "Close All Other Drawings". Dirty drawings pause this queue at the
    /// existing unsaved-changes dialog and resume after Save or Discard.
    pending_tab_closes: std::collections::VecDeque<u64>,
    /// Latest save job per stable tab id. Older completions may finish, but
    /// cannot mark a newer document state clean or redirect its path.
    #[cfg(not(target_arch = "wasm32"))]
    active_save_jobs: std::collections::HashMap<u64, u64>,
    #[cfg(not(target_arch = "wasm32"))]
    save_job_serial: u64,
    /// Destination leases held while Save As workers are active.
    #[cfg(not(target_arch = "wasm32"))]
    pending_save_leases: std::collections::HashMap<u64, crate::io::edit_lock::EditLease>,
    /// Locked-file failure currently shown in the recovery dialog.
    #[cfg(not(target_arch = "wasm32"))]
    pending_save_failure: Option<PendingSaveFailure>,
    /// Extraction tables an event (DXEVAL) found outdated, still to answer,
    /// and the message that carries the event on afterwards.
    pub(crate) extraction_outdated: Option<ExtractionPrompt>,
    /// The next DXEVAL event goes ahead unchecked: the one the window above
    /// just answered for.
    pub(crate) extraction_checked: bool,
    /// Save stopped because the drawing changed outside this editor.
    #[cfg(not(target_arch = "wasm32"))]
    pending_external_change: Option<PendingExternalChange>,
    /// OS window for the unsaved-changes confirmation dialog.

    // ── Custom Save-As dialog ─────────────────────────────────────────────
    /// OS window for the custom Save As dialog.
    /// Currently selected format string, e.g. "DWG 2013".
    save_dialog_format: String,
    /// Editable filename (without path), e.g. "drawing.dwg" — seeds the native
    /// OS save dialog's default name (native) or the download name (web).
    save_dialog_filename: String,
    /// True when triggered from the unsaved-changes flow.
    save_dialog_for_unsaved: bool,
    /// User preference for the first save of a new/unsaved drawing.
    default_save_format: String,
    /// Persisted interface language selection.
    language: crate::i18n::Language,

    // ── DimStyle Dialog ───────────────────────────────────────────────────
    /// Name of the style currently shown in the dialog.
    dimstyle_selected: String,
    /// The dimension style window's faces and drafts.
    dsd: crate::ui::style::dimstyle::Dialog,
    /// The next EnterViewport moves to another viewport within a running
    /// command (Ctrl+R) and must not cancel it.
    viewport_switch_keeps_command: bool,
    /// CHSPACE from paper space waiting for its target viewport: the objects.
    chspace_pending: Option<Vec<document::Handle>>,
    /// ALIGNSPACE between its model and paper halves: the viewport and the
    /// model points picked in it.
    alignspace_pending: Option<(document::Handle, Vec<glam::DVec2>)>,
}

/// What triggered the "unsaved changes" dialog.
#[derive(Debug, Clone)]
pub(super) enum PendingClose {
    /// User tried to close the tab with this document id. Ids stay valid
    /// while the dialog is open; a vector index would go stale the moment
    /// another tab closes and Discard would remove the wrong tab.
    Tab(u64),
    /// User tried to quit the application.
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) enum SavePurpose {
    Manual,
    SaveAs,
    /// Control Save As without permission to replace an existing destination.
    SaveAsNew,
    Autosave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) enum SaveContinuation {
    None,
    CloseTab,
    Quit,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct PendingNativeThumbnailSave {
    tab_id: u64,
    path: PathBuf,
    version: document::DxfVersion,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    set_current_path: bool,
    check_external_change: bool,
    /// The user already chose "Save anyway" for this save's conversion loss.
    allow_conversion_loss: bool,
}

/// A save waiting on the conversion-loss modal; "Save anyway" replays it.
#[derive(Debug, Clone)]
pub(super) enum PendingConversionSave {
    #[cfg(target_arch = "wasm32")]
    WebSaveAll { tab_id: u64 },
    #[cfg(not(target_arch = "wasm32"))]
    Native(PendingNativeThumbnailSave),
    #[cfg(target_arch = "wasm32")]
    Web {
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
    },
}

impl PendingConversionSave {
    /// The name of the converter the held-back save writes with.
    pub(super) fn format_name(&self) -> String {
        let (ext, version) = match self {
            #[cfg(target_arch = "wasm32")]
            Self::WebSaveAll { .. } => ("ifccad".to_string(), document::DxfVersion::AC1032),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(save) => (
                save.path
                    .extension()
                    .map(|ext| ext.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                save.version,
            ),
            #[cfg(target_arch = "wasm32")]
            Self::Web { ext, version, .. } => (ext.clone(), *version),
        };
        // A DWG/DXF loss depends on the target version, so name it too.
        ocs_save::external_format(&ext)
            .map(|format| ocs_save::external_format_label(format).to_string())
            .unwrap_or_else(|| {
                crate::io::format_for_version(version, ext.eq_ignore_ascii_case("dxf"))
            })
    }
}

#[derive(Debug, Clone)]
#[cfg(target_arch = "wasm32")]
pub(super) struct PendingWebThumbnailSave {
    tab_id: u64,
    filename: String,
    ext: String,
    version: document::DxfVersion,
    bounds: iced::Rectangle,
}

/// Extraction tables found outdated by a DXEVAL event.
#[derive(Debug, Clone)]
pub(crate) struct ExtractionPrompt {
    pub tables: Vec<crate::app::annotation_data::OutdatedExtraction>,
    /// How many there were, for the window's counter.
    pub count: usize,
    pub resume: Option<Message>,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct PendingSaveFailure {
    tab_id: u64,
    path: PathBuf,
    version: document::DxfVersion,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    set_current_path: bool,
    error: String,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct PendingExternalChange {
    tab_id: u64,
    path: PathBuf,
    version: document::DxfVersion,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    set_current_path: bool,
}

/// One tab's slot in a web SAVEALL round (`saveall_web`): the tab identity,
/// download name and race-guard markers snapshot when serialization started,
/// plus the writer result — the future fills `result` in before the
/// completion message is built. The web counterpart of native's
/// `SaveOutcome`, minus the path/continuation bookkeeping SAVEALL never had.
#[derive(Debug, Clone)]
#[cfg(target_arch = "wasm32")]
pub struct WebSaveallEntry {
    tab_id: u64,
    path: PathBuf,
    filename: String,
    ext: String,
    version: document::DxfVersion,
    geometry_epoch: u64,
    edit_revision: u64,
    camera_generation: u64,
    result: Result<Vec<u8>, String>,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub struct SaveOutcome {
    job_id: u64,
    tab_id: u64,
    epoch: u64,
    revision: u64,
    camera_generation: u64,
    path: PathBuf,
    physical_path: Option<PathBuf>,
    version: document::DxfVersion,
    previous_autosave: Option<PathBuf>,
    set_current_path: bool,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    refreshed_preview: Option<Option<document::Preview>>,
    result: Result<(), crate::io::SaveFailure>,
}
/// Which Options colour the interface colour picker is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiColor {
    ModelBackground,
    PaperBackground,
    DeskBackground,
    /// One of the six theme colours, by its row in Options.
    Theme(usize),
    Crosshair,
    SnapMarker,
    CommandText,
}

/// Where a colour chosen in the standalone palette window should be applied.
#[derive(Debug, Clone)]
pub enum ColorPickTarget {
    Gradient(u8),
    GraphicAttributesLine,
    GraphicAttributesSolid,
    DimStyle(&'static str),
    MLeader(&'static str),
    Table(&'static str),
    /// Plot-style colour override for the currently selected ACI entry.
    PlotStyle,
    /// Selected entities' colour (left properties panel).
    Properties,
    /// The MTEXT editor's background mask colour.
    MTextMask,
    /// Selected entities' background colour (hatch / MTEXT background row).
    PropertiesBg,
    /// A named per-entity Properties colour field.
    PropertiesField(String),
    /// An input of a node graph card.
    GraphPort(graph::Port),
    /// Current creation colour (ribbon).
    Ribbon,
    /// The ribbon's displayed layer, independent of Layer Manager selection.
    RibbonLayer(String),
    /// A layer's colour, by layer name.
    Layer(String),
    /// A layer's colour override in one layout viewport.
    LayerViewport(String, document::Handle),
    /// A saved layer-state layer's colour, by editor row index.
    LayerState(usize),
    /// The MText editor's selection (or global) colour.
    MText,
    /// An Options interface colour (true colour only).
    Interface(UiColor),
    /// A colour of the view Background dialog: 0 solid, 1 top, 2 middle, 3 bottom.
    ViewBackground(u8),
    /// The selected attribute of the enhanced attribute editor.
    AttrEditor,
}

mod clipboard;
pub use self::clipboard::*;

/// The right-click menu editor's actions.
#[derive(Debug, Clone)]
pub enum ContextMenuEditMsg {
    /// Show or hide the row with this key.
    Toggle(String),
    Draft(String),
    /// Add the typed command.
    Add,
    Remove(usize),
    /// Move a user command up (`true`) or down.
    Move(usize, bool),
    /// Leave the editor.
    Done,
}

/// Which in-canvas modal dialog is currently open (Plan B). At most one shows
/// at a time; dialog-specific data lives in its own fields. Closed via the
/// modal's ✕ (`Message::CloseModal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    About,
    Shortcuts,
    PluginManager,
    UpdateNotice,
    DonationPrompt,
    LayerStateManager,
    LayerTranslator,
    DrawingUnits,
    /// Customize User Interface (CUI): the user's ribbon panels.
    RibbonCustomize,
    BlockDefinition,
    PdfAttach,
    PointCloudAttach,
    PointCloudColorMap,
    PcSection,
    ViewBackground,
    ImageAdjust,
    RefEdit,
    PdfPassword,
    DimInspect,
    TableCellBorders,
    TableCellContents,
    TableCellBlock,
    TableCellFormat,
    TableCellWarning,
    /// The annotation scale asked for before a session's first annotative text.
    AnnotationScalePrompt,
    /// A window of the Text Editor tab (paragraph, columns, mask, find).
    MTextDialog,
    /// A value typed into a cell that is not of the cell's data type.
    TableCellEntry,
    PdfShxSettings,
    UnderlayLayers,
    PdfImportSettings,
    /// TXT2MTXT's Text to MText Settings, over the running command.
    Txt2MtxtSettings,
    /// MATCHPROP's Property Settings, over the running command.
    MatchPropSettings,
    Spell,
    PdfImportFile,
    XrefAttach,
    WriteBlock,
    GeometricTolerance,
    AttDef,
    AttDefEdit,
    InsertAttributes,
    /// Block Attribute Manager (BATTMAN / ATTMAN).
    AttMan,
    CountInvalidArea,
    Field,
    DraftingSettings,
    AutoConstrainSettings,
    LayerStateEditor,
    Plot,
    PrintAll,
    LayoutManager,
    Plotstyle,
    TextStyle,
    TableStyle,
    MlStyle,
    MLeaderStyle,
    DimStyle,
    LinetypeManager,
    StyleImport,
    Unsaved,
    BlockEditChanges,
    BlockEditPicker,
    Rename,
    DesignCenter,
    Purge,
    DrawingProperties,
    GeographicLocation,
    SelectSimilarSettings,
    BlockSaveAs,
    BlockVisibility,
    BlockLookup,
    BlockPropertiesTable,
    SaveDialog,
    Recovery,
    /// Fonts referenced by the drawing are missing on this machine.
    MissingFonts,
    RecoveryPrompt,
    Options,
    FindReplace,
    /// OVERKILL settings for the selected objects.
    Overkill,
    AecDropWarning,
    /// A converter would drop part of the drawing on save.
    ConversionLoss,
    IfccadAlphaWarning,
    #[cfg(not(target_arch = "wasm32"))]
    FileInUse,
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChange,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AssocPrompt,
    PointStyle,
    AttributeEditor,
    LayerDeleteWarning,
    Aliases,
    NamedParameters,
    BlockEditorSettings,
    VisualStyles,
    BlockCycleOrder,
    ScaleManager,
    /// Add / remove the annotation scales a single selected object has a
    /// per-object representation for.
    AnnoObjectScale,
    /// URL/description collection editor opened by the Hyperlink property row.
    Hyperlink,
    InsertTable,
    DataLinkManager,
    DataExtraction,
    /// DATAEXTRACTION on a drawing never saved: save it first.
    DataExtractionUnsaved,
    /// The scene is drawn by a software rasterizer, or not at all: what that
    /// means and what usually fixes it. Queued once per verdict; the status
    /// bar's ⚠ pill reopens it.
    GpuWarning,
    /// Reference Manager help window (toolbar Help button).
    XrefHelp,
    /// Sheet set wizard, properties and sheet / subset forms.
    SheetSet,
    /// IMPORT's Import File window.
    ImportFile,
    /// DGNIMPORT's DGN Import window.
    DgnImport,
    /// IMPORT's 3D Studio Import window.
    TdsImport,
    /// A 3D Studio import set to ask: one mesh with several materials.
    TdsMaterial,
    /// A texture map a 3D Studio import did not find, one window per map.
    TdsMissingMap,
    HatchBoundaryWarning,
    /// An extraction table outdated at open/save/plot/publish.
    ExtractionOutdated,
    /// Web: a crash-recovery autosave snapshot newer than the drawing's last
    /// save awaits a restore/discard decision. Distinct from the corrupt-file
    /// `RecoveryPrompt` — nothing is wrong with the drawing itself.
    #[cfg(target_arch = "wasm32")]
    AutosaveRestore,
    /// Declarative modal dialog opened by a plugin.
    PluginDialog,
}

/// A property group controlled by a layer state's restore mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerStateProperty {
    On,
    Frozen,
    Locked,
    Plot,
    NewViewport,
    Color,
    LineType,
    LineWeight,
    PlotStyle,
    Transparency,
}

/// A boolean saved for one layer inside a named layer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerStateLayerFlag {
    On,
    Frozen,
    Locked,
    Plot,
    NewViewport,
}

#[derive(Debug, Clone, Copy)]
pub enum ArrowKey {
    Up,
    Down,
    Left,
    Right,
}

/// The text input holding focus when Ctrl+V arrives (native only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteFocus {
    None,
    CommandLine,
    /// Any other text field (dialogs, panels).
    Field,
}

<<<<<<< HEAD
mod message;
pub use self::message::*;

#[derive(Debug, Clone)]
pub enum SystemClipboardText {
    Text(String),
    EmptyOrUnsupported,
    Unavailable,
    Occupied,
    ConversionFailed,
}

impl OpenCADStudio {
    /// Install the Start-page video list, decoding each thumbnail's JPEG into
    /// an image Handle exactly once (a fresh Handle per view frame would
    /// re-upload the texture every frame).
    fn set_videos(&mut self, videos: Vec<crate::videos::VideoEntry>) {
        if videos.is_empty() {
            return;
        }
        self.video_thumbs = videos
            .iter()
            .filter_map(|v| {
                let bytes = v.thumb.clone()?;
                Some((v.id.clone(), iced::widget::image::Handle::from_bytes(bytes)))
            })
            .collect();
        self.videos = videos;
    }

    pub(crate) fn new() -> Self {
        crate::sys::startup_times::mark("App::new: begin");
        let config = config::AppConfig::load();
        crate::sys::startup_times::mark("App::new: config loaded");
        if let Err(error) = crate::i18n::set_language(config.settings.language) {
            eprintln!("Unable to apply saved UI language: {error}");
        }
        // Boot with only the Welcome/Start tab. The user creates drawings
        // explicitly (File → New); we never auto-spawn Drawing1.
        let start_tab = DocumentTab::new_start();
        let mut app = Self {
            control: control::State::new(),
            start: Instant::now(),
            tabs: vec![start_tab],
            active_tab: 0,
            hovered_doc_tab: None,
            doc_tab_menu: false,
            doc_tab_layouts: None,
            doc_tab_hover_gen: 0,
            tab_counter: 0,
            ribbon: Ribbon::new(),
            // Populated from the consolidated config after construction
            // (`apply_config`); default empty here.
            recent_files: Vec::new(),
            recent_thumbs: std::collections::HashMap::new(),
            recent_limit: recent::RECENT_DEFAULT,
            recent_limit_input: recent::RECENT_DEFAULT.to_string(),
            recent_view: RecentView::default(),
            command_line: CommandLine::new(),
            patrons: Vec::new(),
            videos: Vec::new(),
            video_thumbs: std::collections::HashMap::new(),
            videos_loading: false,
            discussions: Vec::new(),
            discussions_loading: false,
            collapsed_property_sections: rustc_hash::FxHashSet::default(),
            layer_filters: Vec::new(),
            ribbon_layers_follow_filter: false,
            start_section: StartSection::default(),
            start_action_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            history_content: iced::widget::text_editor::Content::new(),
            command_history_resizing: false,
            command_history_drag_last: None,
            command_line_moving: false,
            status_bar: StatusBar::new(),
            cursor_pos: Point::ORIGIN,
            vp_size: (1280.0, 720.0),
            win_size: (1280.0, 720.0),
            snapper: Snapper::default(),
            snap_popup_open: false,
            drafting_settings_state: None,
            drafting_settings_saved: None,
            drafting_settings_close_confirm: false,
            scale_popup_open: false,
            polar_popup_open: false,
            polar_custom_input: String::new(),
            statusbar_menu_open: false,
            layout_list_open: false,
            units_popup_open: false,
            isolate_popup_open: false,
            selection_filter_popup_open: false,
            status_menu_tooltip_hidden: false,
            statusbar_config: crate::ui::statusbar::statusbar_config::StatusBarConfig::default(),
            annotation_auto_scale: -4,
            last_saved_config: None,
            otrack_active: None,
            otrack_cross: None,
            otrack_kind: None,
            clean_screen: false,
            clean_screen_properties: false,
            clean_screen_parameters_manager: false,
            clean_screen_authoring_palettes: false,
            clean_screen_tool_palettes: false,
            block_picker_dismissed: false,
            quick_properties: false,
            quick_properties_anchor: Point::new(12.0, 12.0),
            quick_properties_float: false,
            quick_properties_pos: Point::new(40.0, 40.0),
            selection_cycling: 0,
            cycle_badge: None,
            osnap_z: false,
            select_similar_mode: crate::scene::SELECT_SIMILAR_DEFAULT,
            pick_add: true,
            pick_add_retains: true,
            select_remove_mode: false,
            selection_reprompt: false,
            last_layer_translation: None,
            layer_translator: None,
            drawing_units: None,
            block_definition: None,
            block_definition_open_editor: None,
            block_edit_picker: None,
            rename_dialog: None,
            design_center: None,
            drawing_properties: None,
            geographic_location: None,
            purge_dialog: None,
            purge_only: None,
            select_similar_settings: None,
            block_save_as: None,
            block_visibility: None,
            block_lookup: None,
            block_properties_table: None,
            block_visibility_mode: false,
            block_editor_preferences: settings::BlockEditorPreferences::default(),
            block_editor_settings: None,
            visual_styles_crease: None,
            block_cycle_order: None,
            block_dependency_cache: std::cell::RefCell::new(None),
            pdf_attach: None,
            point_cloud_attach: None,
            point_cloud_prompt_read: None,
            graph_file_reader: crate::app::node_graph::files::read_file,
            import_window_read: 0,
            attdef_dialog: None,
            insert_attr_dialog: None,
            attman: None,
            attdef_edit: None,
            field_dialog: None,
            field_formula_parent: None,
            field_number_format: Default::default(),
            point_cloud_color_map: None,
            pc_section: None,
            view_background: None,
            image_adjust: None,
            refedit_dialog: None,
            pdf_password: None,
            dim_inspect_dialog: None,
            table_cell_borders: None,
            table_cell_contents: None,
            table_cell_block: None,
            table_cell_format: None,
            anno_scale_prompt: None,
            mtext_dialog: None,
            table_cell_warning: None,
            table_cell_entry: None,
            table_cell_entry_typed: false,
            data_link_cell_target: false,
            pdf_shx_dialog: None,
            underlay_layers: None,
            pdf_import_settings: None,
            pdf_import_file: None,
            import_dialogs: Default::default(),
            options_parent: None,
            options_child: None,
            xref_attach: None,
            wblock: None,
            geometric_tolerance: None,
            pick_drag_mode: 2,
            pick_auto: 5,
            pick_first: true,
            perf_hud: false,
            cycle_candidates: None,
            pre_cmd_tangent: None,
            add_selected_restore: None,
            rect_suppressed_ortho: false,
            ortho_mode: false,
            polar_mode: false,
            polar_increment_deg: 45.0,
            zoom_wheel_reversed: false,
            navigation_style: Default::default(),
            nav_held: Default::default(),
            nav_pending: None,
            nav_claimed: Default::default(),
            nav_drag_by_button: false,
            nav_middle_press: None,
            nav_right_press: None,
            nav_zoom_latched: false,
            nav_replay: false,
            alt_down: false,
            dynamic_ucs: true,
            dynamic_ucs_restore: None,
            // A Mac's built-in trackpad is the common case there.
            touchpad_pan: cfg!(target_os = "macos"),
            touch: Default::default(),
            touch_offset: None,
            ui_scale_percent: 100,
            zoom_factor: 60,
            zoom_factor_input: 60.to_string(),
            cursor_size: 5,
            pick_box: 3,
            hatch_associative: true,
            hatch_drafts: Default::default(),
            context_menu_custom: Default::default(),
            ribbon_customize: None,
            context_menu_editing: None,
            hatch_gap_tolerance: 0.0,
            hatch_prefs: Default::default(),
            hatch_boundary_warning_off: false,
            hatch_panel_shown: false,
            outline_switch_hover: false,
            hatch_boundary_warning: None,
            show_menu_bar: false,
            ribbon_closed: false,
            command_line_hidden: false,
            double_click_block_refedit: false,
            double_click_block_attedit: true,
            right_click_mode: settings::RightClickMode::ShortcutMenu,
            right_click_hold_ms: 250,
            // Quad cursor menu defaults mirror `UserSettings::default()`; a
            // restored config overwrites these in `apply_settings`.
            quad: false,
            quad_on_hover: true,
            quad_on_select: true,
            quad_on_right_click: true,
            quad_suppress_hover_when_selection: true,
            quad_width: crate::ui::popup::quad_menu::QUAD_WIDTH_DEFAULT,
            quad_command_launch: false,
            quad_hover_delay_ms: 300,
            quad_panels: Vec::new(),
            quad_hover_since: None,
            quad_dwell_pick: None,
            hover_move_seq: 0,
            quad_mru: crate::ui::popup::quad_menu::QuadMru::new(),
            quad_open_target: None,
            grip_object_limit: settings::DEFAULT_GRIP_OBJECT_LIMIT,
            ncopy_bind: false,
            cursor_type: settings::CursorType::Crosshair,
            crosshair_color: None,
            crosshair_color_input: String::new(),
            snap_marker_color: None,
            snap_marker_color_input: String::new(),
            command_text_color: None,
            command_text_color_input: String::new(),
            isolines_awaiting_regen: false,
            snap_angle_input: "0".to_string(),
            lineweight_display_scale: 100,
            isometric_drafting: false,
            iso_plane: settings::IsoPlane::Left,
            snap_angle_deg: 0.0,
            show_grid: false,
            grid_spacing_x: 10.0,
            grid_spacing_y: 10.0,
            grid_major_every: 5,
            grid_adaptive: true,
            grid_beyond_limits: true,
            grid_axes: true,
            dyn_input: true,
            dyn_mode: 3,
            dyn_pointer_cartesian: false,
            dyn_pointer_absolute: false,
            dyn_pointer_on_type: false,
            options_tab: crate::ui::window::options::OptionsTab::General,
            #[cfg(not(target_arch = "wasm32"))]
            graphics_prefs: crate::gpu_backend::load_prefs(),
            options_saved: None,
            options_close_confirm: false,
            spacemouse: {
                let service = crate::input::spacemouse::Service::default();
                service.set_actions(navigation::actions());
                service
            },
            spacemouse_preferences: crate::input::spacemouse::Preferences::default(),
            spacemouse_paused: false,
            spacemouse_focused: false,
            spacemouse_details: false,
            spacemouse_was_moving: false,
            spacemouse_pivot: None,
            spacemouse_selection: navigation::SelectionCache::default(),
            texteditmode: false,
            quick_dimension_snap_priority: 0,
            dimension_continue_mode: 1,
            backup_on_save: true,
            file_assoc_enabled: true,
            show_constraint_values: true,
            auto_constrain_settings: settings::AutoConstrainSettings::default(),
            txt2mtxt_settings: settings::Txt2MtxtSettings::default(),
            spell_prefs: settings::SpellPrefs::default(),
            spell: None,
            dim_inspect_settings: settings::DimInspectSettings::default(),
            txt2mtxt_dialog: None,
            matchprop_dialog: None,
            auto_constrain_saved: None,
            auto_constrain_selected_row: 0,
            auto_constrain_distance_input: "0.05".to_string(),
            auto_constrain_angle_input: "1".to_string(),
            constraint_solve_mode: true,
            constraint_infer: false,
            result_depth: 0,
            constraint_bar_display: 3,
            constraint_form_annotational: false,
            dim_constraint_last: "Aligned",
            geom_constraint_last: "Coincident",
            suppress_plugin_dispatch: false,
            plugin_request_active: false,
            constraint_bar_mode: 4095,
            savetime_min: 10,
            save_file_path: String::new(),
            script_commands: true,
            default_bg_color: None,
            default_paper_bg_color: None,
            cliprompt_lines: 3,
            commandline_fade_ms: 3000,
            block_mru: Vec::new(),
            block_freq: std::collections::HashMap::new(),
            block_mru_list: 50,
            selection_report: None,
            last_pick: None,
            selection_undo: (0, Vec::new()),
            selection_undoing: false,
            selection_group_pending: false,
            selection_group_named: false,
            last_pick_groups: 0,
            last_entered_point: None,
            selection_subobject: None,
            typed_window_corner: None,
            refedit_options: Default::default(),
            block_redefine_mode: 1,
            block_navigate: ".".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            block_usage_last_persist: None,
            awaiting_vports: false,
            vpmax_state: None,
            pending_setvar: None,
            step_schema: crate::io::step::Schema::Ap214,
            delete_objects: 1,
            ucs_icon_hover: false,
            ucs_icon_selected: false,
            ucs_grip_drag: None,
            pane_move_from: None,
            dyn_user_reshaped: false,
            dyn_coord_absolute: false,
            grip_hover: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_hover: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_deliveries: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_live_originals: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_descriptor_drag: None,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_settle_watchdog: crate::app::update::SETTLE_WATCHDOG,
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grip_settle_strikes: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            pending_plugin_grips_rearm: Default::default(),
            grip_popup: None,
            grip_pending: None,
            visibility_popup: None,
            table_popup: None,
            grip_add_provisional: None,
            grip_ctrl_cycle: None,
            grip_preview_handles: Vec::new(),
            hover_dwell: None,
            hover_pick_due: iced::time::Instant::now(),
            move_owed: None,
            box_preview_pending: None,
            properties_refresh_pending: None,
            box_preview_due: None,
            box_preview_cache: None,
            move_pass_due: None,
            move_withheld: 0,
            snap_dwell: None,
            constraint_glyph_tooltip: None,
            grip_originals: Vec::new(),
            grip_history_originals: Vec::new(),
            grip_dirty_before: None,
            grip_engage_press: None,
            grip_point_override: None,
            grip_reference_wires: Vec::new(),
            grip_text_verts: Vec::new(),
            grip_text_slide: false,
            qselect: None,
            filter_dialog: None,
            restoring_layer_previous: false,
            layer_import: None,
            transparent_select: None,
            qselect_settings: None,
            show_ucs_icon: true,
            show_viewport_controls: true,
            ucs_icon_at_origin: true,
            show_viewcube: true,
            navvcube_opacity: 50,
            render_bar_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            render_mode_menu_open: false,
            render_mode_preview: None,
            show_properties: true,
            show_layers: false,
            show_graphic_attributes: false,
            show_command_history: false,
            graphic_attributes: Default::default(),
            show_block_palette: false,
            show_tool_palettes: false,
            tool_palettes: Default::default(),
            show_authoring_palettes: false,
            show_parameters_manager: false,
            authoring_palette_tab: Default::default(),
            show_node_graph: false,
            node_graph_links: Default::default(),
            property_target_override: None,
            graph_undo_open: false,
            show_external_references: false,
            show_browser: false,
            pc_manager: Default::default(),
            count_palette: Default::default(),
            quick_calc: Default::default(),
            assistant: Default::default(),
            sheet_set: Default::default(),
            sheet_set_pending_layout: None,
            sheet_set_import_parent: None,
            block_palette: Default::default(),
            xref_manager: Default::default(),
            dock: Default::default(),
            dock_peek: None,
            dock_hover_pending: None,
            dock_hover_gen: 0,
            dock_float_menu: None,
            tool_windows: Vec::new(),
            tool_focus: None,
            keyboard_tool: None,
            #[cfg(not(target_arch = "wasm32"))]
            tool_window_to_raise: None,
            tool_drag: None,
            #[cfg(not(target_arch = "wasm32"))]
            os_windows: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            focused_window: None,
            dock_float_pointer: iced::Point::ORIGIN,
            dock_float_menu_at: iced::Point::ORIGIN,
            dock_drag: None,
            dock_drag_last: None,
            dock_title_hover: None,
            dock_edge_menu: None,
            dock_edge_menu_y: None,
            dock_strip_pointer_y: 0.0,
            dock_grip_hover: None,
            dock_grip_lit: None,
            dock_grip_gen: 0,
            dock_icon_hover: None,
            xref_col_drag: None,
            xref_col_last: None,
            xref_split_drag: false,
            show_file_tabs: true,
            file_tabs_above_docks: false,
            show_layout_tabs: true,
            last_point: None,
            vp_snap_frame: None,
            accepted_snaps: Vec::new(),
            pending_click_snap: None,
            main_window: None,
            thumbnail_capture_clean: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_native_thumbnail_save: None,
            #[cfg(target_arch = "wasm32")]
            pending_web_thumbnail_save: None,
            #[cfg(target_arch = "wasm32")]
            save_plain_reuse_fs_handle: false,
            #[cfg(target_arch = "wasm32")]
            web_autosave_in_flight: false,
            #[cfg(target_arch = "wasm32")]
            web_autosave_warned: false,
            #[cfg(target_arch = "wasm32")]
            web_autosave_pending: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            web_autosave_restore_all_remaining: 0,
            #[cfg(target_arch = "wasm32")]
            web_session_focus_pending: false,
            #[cfg(target_arch = "wasm32")]
            web_session: None,
            #[cfg(target_arch = "wasm32")]
            web_autosave_consumed: None,
            #[cfg(target_arch = "wasm32")]
            web_autosave_clear_after: Vec::new(),
            color_picker: None,
            color_books: Default::default(),
            color_books_read: false,
            active_modal: None,
            plugin_dialog_state: None,
            plugin_panels: std::collections::BTreeMap::new(),
            plugin_panel_ids: std::collections::HashMap::new(),
            plugin_panel_by_num: std::collections::HashMap::new(),
            hyperlink_editor_handles: Vec::new(),
            hyperlink_editor_block: None,
            hyperlink_editor_url: String::new(),
            hyperlink_editor_description: String::new(),
            hyperlink_editor_mixed: false,
            hyperlink_editor_dirty: false,
            hyperlink_options: Default::default(),
            pending_startup_modals: std::collections::VecDeque::new(),
            gpu_status: crate::scene::pipeline::GpuStatus::Unknown,
            gpu_status_generation: 0,
            gpu_warning_silenced: String::new(),
            plotstyle_parent_plot_geometry: None,
            find_replace: FindReplaceState::default(),
            mtext_find_prefs: None,
            overkill_dialog: None,
            aec_drop_acknowledged: false,
            aec_drop_count: 0,
            conversion_losses: Vec::new(),
            pending_conversion_save: None,
            pending_ifccad_alpha_save: None,
            conversion_loss_allowed: false,
            layer_delete_pending: None,
            modal_offset: iced::Vector::ZERO,
            modal_drag_last: None,
            modal_dragging: false,
            layer_name_col_w: 130.0,
            layer_col_w: std::collections::HashMap::new(),
            layer_walk_col_w: crate::ui::window::layer_walk::DEFAULT_COL_W,
            modal_resize: iced::Vector::ZERO,
            modal_content_size: None,
            modal_resizing: false,
            attr_editor_handle: None,
            attr_editor_block: String::new(),
            attr_editor_rows: Vec::new(),
            attr_editor_tab: crate::ui::window::attribute_editor::AttrTab::Attribute,
            attr_editor_selected: 0,
            attr_editor_session: None,
            attr_editor_error: None,
            attr_editor_asking: false,
            attr_editor_editing: None,
            disabled_plugins: rustc_hash::FxHashSet::default(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_tools: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            last_plugin_selection: None,
            #[cfg(not(target_arch = "wasm32"))]
            selection_revision_by_tab: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            last_plugin_document: None,
            external_plugins: Vec::new(),
            loaded_plugin_ids: rustc_hash::FxHashSet::default(),
            plugin_load_errors: rustc_hash::FxHashMap::default(),
            plugin_registry: Vec::new(),
            plugin_registry_loading: false,
            plugin_registry_error: None,
            plugin_registry_error_details_open: false,
            plugin_repos: Vec::new(),
            plugin_repo_input: String::new(),
            plugin_search_input: String::new(),
            repo_release_tags: rustc_hash::FxHashMap::default(),
            repo_selected_tag: rustc_hash::FxHashMap::default(),
            selected_plugin_repo: None,
            plugin_readmes: rustc_hash::FxHashMap::default(),
            plugin_readme_loading: rustc_hash::FxHashSet::default(),
            plugin_releases_loading: rustc_hash::FxHashSet::default(),
            plugin_installing: rustc_hash::FxHashSet::default(),
            ole_edits: Vec::new(),
            plugin_installed_now: rustc_hash::FxHashMap::default(),
            marketplace_status: String::new(),
            point_size_buf: String::new(),
            point_style_tab_id: None,
            point_mode_buf: None,
            point_size_relative: true,
            default_assoc_prompted: false,
            check_missing_fonts: true,
            font_source_url: String::new(),
            qnew_template: String::new(),
            transmittal_layout: crate::io::transmit::TransmittalLayout::Structured,
            start_templates: None,
            font_source_input: String::new(),
            donation_prompt_version: String::new(),
            read_only: false,
            update_notice_version: None,
            update_notice_body: None,
            clipboard: Vec::new(),
            oops_cache: Vec::new(),
            clipboard_base: glam::DVec3::ZERO,
            clipboard_deps: ClipboardDeps::default(),
            clipboard_source_tab: None,
            shift_down: false,
            ctrl_down: false,
            cont_anchor: None,
            mtext_editor: None,
            table_cell_editor: None,
            table_edit_command_line: false,
            command_mtext_input: false,
            pending_command_editor_text: None,
            text_inline: None,
            snap_override_popup: None,
            held_override: None,
            axis_lock_dir: None,
            angle_override: None,
            drag_move: None,
            layout_rename_state: None,
            last_vp_click_time: None,
            last_vp_click_pos: None,
            mtext_click_time: None,
            mtext_click_off: 0,
            mtext_click_count: 0,
            mtext_clipboard: None,
            plot_window: None,
            plot_paper: crate::io::paper_catalog::default_paper().clone(),
            plot_orientation: crate::io::paper_catalog::Orientation::Landscape,
            plot_dialog: crate::ui::window::plot::PlotDialogState::default(),
            plot_prev: None,
            plot_prev_context: None,
            plot_reconcile_printer_media: None,
            plot_setup_template: None,
            print_all_layouts: Vec::new(),
            print_all_options: false,
            print_all_settings_override: false,
            print_all_options_prev: None,
            print_all_plot_style_prev: None,
            print_all_plot_window_prev: None,
            print_all_plot_setup_prev: None,
            opening: None,
            layout_settling: false,
            open_job_serial: 0,
            recovery_report: None,
            missing_fonts: None,
            missing_fonts_path: None,
            missing_fonts_downloading: false,
            missing_fonts_silence: false,
            suppressed_missing_fonts: rustc_hash::FxHashSet::default(),
            pending_opens: std::collections::VecDeque::new(),
            pending_startup_script_lines: Vec::new(),
            script_queue: std::collections::VecDeque::new(),
            script_running: false,
            script_paused: false,
            sdi: false,
            exit_requested: false,
            #[cfg(not(target_arch = "wasm32"))]
            terminating: false,
            active_interaction_index: None,
            queued_interaction_indices: std::collections::VecDeque::new(),
            pending_close: None,
            pending_tab_closes: std::collections::VecDeque::new(),
            pending_block_close: None,
            #[cfg(not(target_arch = "wasm32"))]
            active_save_jobs: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            save_job_serial: 0,
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_leases: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_failure: None,
            extraction_outdated: None,
            extraction_checked: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_external_change: None,
            save_dialog_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            save_dialog_filename: "drawing.dwg".to_string(),
            save_dialog_for_unsaved: false,
            default_save_format: crate::io::DEFAULT_SAVE_FORMAT.to_string(),
            language: crate::i18n::Language::default(),
            // Plot style
            active_plot_style: crate::io::plot_style::PlotStyleTable::load_named(
                crate::io::plot_style::DEFAULT_PLOT_STYLE,
            )
            .or_else(|_| {
                crate::io::plot_style::PlotStyleTable::builtin(
                    crate::io::plot_style::DEFAULT_PLOT_STYLE,
                )
            })
            .ok(),
            plotstyle_panel_baseline: None,
            // Color scheme (default: Oxocarbon)
            active_theme: Theme::Oxocarbon,
            ui_theme: config::UiThemeConfig::default(),
            theme_color_inputs: config::UiThemePalette::default().hex_values(),
            model_space: config::ModelSpaceThemeConfig::default(),
            model_bg_input: String::new(),
            paper_bg_input: String::new(),
            desk_bg_input: String::new(),
            saved_custom_palette: None,
            // Keyboard shortcuts
            shortcut_bindings: rustc_hash::FxHashMap::default(),
            shortcut_editor_rows: Vec::new(),
            shortcut_filter: String::new(),
            shortcut_capture_row: None,
            shortcut_pending_add: false,
            shortcut_suggest: None,
            shortcut_reset_confirm: false,
            shortcut_close_confirm: false,
            // Command aliases (populated from ocad.pgp just after construction)
            command_aliases: rustc_hash::FxHashMap::default(),
            menu_bar_popup: None,
            alias_editor_rows: Vec::new(),
            alias_pending_add: false,
            alias_reset_confirm: false,
            alias_close_confirm: false,
            alias_import_report: String::new(),
            named_parameter_editor_rows: Vec::new(),
            cal_context: calc::CalcContext::default(),
            // Layout Manager
            layout_manager_selected: "Model".to_string(),
            layer_state_selected: None,
            layer_state_name_buf: String::new(),
            layer_state_description_buf: String::new(),
            layer_state_filter: String::new(),
            layer_state_edit_draft: None,
            layer_state_edit_filter: String::new(),
            layer_state_edit_color_open: None,
            scale_manager_selected: String::new(),
            scale_manager_paper_buf: String::new(),
            scale_manager_drawing_buf: String::new(),
            scale_rename: None,
            scale_edit_adding: false,
            scale_edit_error: None,
            scale_manager_multi: Vec::new(),
            scale_delete_asking: false,
            anno_object_scale_target: None,
            anno_object_scale_extra: Vec::new(),
            anno_object_scale: Default::default(),
            scale_reset_asking: false,
            scale_rename_buf: String::new(),
            scale_stage: None,
            table_insert: Default::default(),
            data_link_manager: Default::default(),
            data_link_parent_table: false,
            tablestyle_parent_table: false,
            tablestyle_in_use_only: false,
            data_extraction: Default::default(),
            layout_manager_rename_buf: String::new(),
            plotstyle_panel_aci: 1,
            ps_color_buf: String::new(),
            ps_lineweight_buf: "255".to_string(),
            ps_screening_buf: "100".to_string(),
            // TextStyle font browser
            style_rename: None,
            style_rename_buf: String::new(),
            style_rename_kind: None,
            style_stage: None,
            style_parent_stage: None,
            style_import: None,
            textstyle_selected: "Standard".to_string(),
            textstyle_counts: Default::default(),
            textstyle_in_use_only: false,
            textstyle_sample: String::new(),
            textstyle_compare: String::new(),
            textstyle_draft: Default::default(),
            textstyle_card: None,
            textstyle_context: None,
            textstyle_font_picker: Default::default(),
            textstyle_bigfont_picker: Default::default(),
            textstyle_users: Default::default(),
            // TableStyle dialog
            tablestyle_selected: "Standard".to_string(),
            tablestyle_tab: 0,
            tablestyle_compare: String::new(),
            ts_description: String::new(),
            ts_cell: document::objects::DATA_CELL_STYLE.to_string(),
            ts_cell_height: String::new(),
            ts_cell_angle: String::new(),
            ts_cell_hmargin: String::new(),
            ts_cell_vmargin: String::new(),
            ts_border: Default::default(),
            ts_cards: Default::default(),
            ts_format_return: false,
            ts_text_return: false,
            ts_color_open: None,
            // MLineStyle dialog
            linetype_selected: String::new(),
            linetype_tab: 0,
            linetype_in_use_only: false,
            linetype_keys_active: true,
            linetype_list_view: None,
            linetype_counts: Default::default(),
            linetype_draft: Default::default(),
            linetype_library_status: String::new(),
            mlstyle_selected: "Standard".to_string(),
            mlstyle_tab: 0,
            mlstyle_compare: String::new(),
            mln_description: String::new(),
            mln_start_angle: "90".to_string(),
            mln_end_angle: "90".to_string(),
            mln_fill_color: "256".to_string(),
            mln_elements: Vec::new(),
            // MLeaderStyle dialog
            mleaderstyle_selected: "Standard".to_string(),
            mls: Default::default(),
            mls_color_open: None,
            mls_landing_distance: String::new(),
            mls_landing_gap: String::new(),
            mls_arrowhead_size: String::new(),
            mls_text_height: String::new(),
            mls_scale_factor: String::new(),
            mls_break_gap: String::new(),
            mls_max_points: String::new(),
            mls_default_text: String::new(),
            mls_line_color: String::new(),
            mls_text_color: String::new(),
            mls_block_color: String::new(),
            mls_block_scale: String::new(),
            // DimStyle dialog
            dimstyle_selected: "Standard".to_string(),
            dsd: Default::default(),
            viewport_switch_keeps_command: false,
            chspace_pending: None,
            alignspace_pending: None,
        };
        crate::sys::startup_times::mark("App::new: struct built");
        // Restore the consolidated user config (settings.json) into live state
        // so preferences, recents, status-bar layout, ribbon density and print
        // options survive across sessions (issue #68). `last_saved_config` is
        // seeded below so the first change — not the boot — triggers a write.
        app.apply_config(config);
        // MCP control starts as the button was left (#3126); `--http` needs it.
        #[cfg(not(target_arch = "wasm32"))]
        let forced = crate::app::control::http_bridge::gui_http_requested();
        #[cfg(target_arch = "wasm32")]
        let forced = false;
        app.control.enabled = app.control.start_enabled || forced;
        crate::sys::startup_times::mark("App::new: apply_config done");
        // Load command aliases from ocad.pgp (writes the shipped default file
        // on first launch). The hide-set keeps aliases out of autocomplete while
        // their target command still shows.
        app.command_aliases = alias::load_aliases();
        app.command_line.command_aliases = app.command_aliases.clone();
        crate::sys::startup_times::mark("App::new: aliases loaded");
        // Load external plugin packages from the plugins folder once, then fold
        // their ribbon tabs into the ribbon. Skipped under test/wasm.
        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            crate::sys::startup_times::mark("App::new: loading external plugins");
            for (id, res) in crate::plugin::external::load_at_startup(&mut app) {
                if let Err(e) = res {
                    app.command_line
                        .push_error(crate::tf!("Plugin '{id}' failed to load: {e}").as_ref());
                    app.plugin_load_errors.insert(id, e);
                }
            }
            crate::sys::startup_times::mark("App::new: external plugins loaded");
            app.loaded_plugin_ids = crate::plugin::external::loaded_ids().into_iter().collect();
            app.rebuild_ribbon_modules();
        }
        crate::sys::startup_times::mark("App::new: done");
        app.last_saved_config = Some(app.current_config());
        app.sync_ribbon_layers();
        app
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let mut app = Self::new();
        // `new` loads the real settings file, so without this every test runs
        // against whatever the developer last set in the application — a suite
        // that passes on a clean machine and fails on a used one. It surfaced
        // when a persisted `GRIPOBJLIMIT` made the grip-limit test see 32767
        // where it expected the default, and the number of persisted settings
        // only grows.
        app.apply_config(crate::app::config::AppConfig::default());
        app.last_saved_config = Some(app.current_config());
        app
    }

    /// Test hook for transports outside `crate::app` (the REST routing
    /// tests): push a fresh tab and return its document id, so a second
    /// open document can be addressed without reaching into `tabs`.
    #[cfg(test)]
    pub(crate) fn push_test_document(&mut self) -> u64 {
        let tab = tab_document::DocumentTab::new_drawing(1000 + self.tabs.len());
        let id = tab.id;
        self.tabs.push(tab);
        id
    }

    /// Open a declarative plugin modal dialog on the currently active tab.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_plugin_dialog(&mut self, def: ocs_plugin_api::dialog::DialogDef) {
        let tab = self.active_tab;
        self.open_plugin_dialog_on_tab(tab, def, None);
    }

    /// Open a declarative plugin modal dialog associated with `tab` and an optional `explicit_plugin_id`.
    pub(crate) fn open_plugin_dialog_on_tab(
        &mut self,
        tab: usize,
        def: ocs_plugin_api::dialog::DialogDef,
        explicit_plugin_id: Option<String>,
    ) {
        let (tab_id, plugin_id, command_id) = if let Some(t) = self.tabs.get(tab) {
            let pid = explicit_plugin_id.or_else(|| {
                t.active_cmd
                    .as_ref()
                    .and_then(|c| c.owner_plugin_id().map(str::to_string))
            });
            let cid = t.active_cmd.as_ref().and_then(|c| c.plugin_command_id());
            (Some(t.id), pid, cid)
        } else {
            (None, explicit_plugin_id, None)
        };
        self.plugin_dialog_state = Some(
            crate::ui::window::plugin_dialog::PluginDialogState::new(def)
                .with_owner(tab_id, plugin_id, command_id),
        );
        self.active_modal = Some(ModalKind::PluginDialog);
    }

    /// Resolve the tab index that owns the currently open plugin dialog, falling back to `active_tab`.
    pub(crate) fn dialog_target_tab_index(&self) -> usize {
        if let Some(tid) = self.plugin_dialog_state.as_ref().and_then(|s| s.tab_id) {
            if let Some(idx) = self.tabs.iter().position(|t| t.id == tid) {
                return idx;
            }
        }
        self.active_tab
    }

    /// Apply queued `(panel_or_dialog_id, values)` updates from `take_pending_panel_updates`
    /// to matching registered panels or the currently open modal dialog.
    pub(crate) fn apply_panel_or_dialog_updates(
        &mut self,
        updates: Vec<(
            String,
            std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
        )>,
    ) {
        for (target_id, values) in updates {
            if let Some(panel) = self.plugin_panels.get_mut(&target_id) {
                panel.update_values(values);
                continue;
            }
            let lower = target_id.to_lowercase();
            if let Some((_, panel)) = self
                .plugin_panels
                .iter_mut()
                .find(|(k, _)| k.to_lowercase() == lower)
            {
                panel.update_values(values);
                continue;
            }
            if let Some(state) = &mut self.plugin_dialog_state {
                if target_id.is_empty()
                    || state.def.title == target_id
                    || state.def.title.eq_ignore_ascii_case(&target_id)
                {
                    state.update_values(values);
                }
            }
        }
    }

    /// Update control values in an active modal dialog in-place.
    #[allow(dead_code)]
    pub fn update_dialog(
        &mut self,
        values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    ) -> bool {
        if let Some(state) = &mut self.plugin_dialog_state {
            state.update_values(values);
            true
        } else {
            false
        }
    }

    /// Dispatch a reactive action event from an open plugin modal dialog to the owning command/plugin.
    pub(crate) fn dispatch_plugin_dialog_action(
        &mut self,
        control_id: String,
        action: String,
        value: Option<ocs_plugin_api::dialog::DialogValue>,
    ) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let (action_resp, panel_updates, fallback_notify) =
            if let Some(state) = &self.plugin_dialog_state {
                let current_values = state.collect_response_map();
                let dialog_title = state.def.title.clone();
                let owner_plugin_id = state.plugin_id.clone();
                let owner_command_id = state.command_id;
                let event = ocs_plugin_api::dialog::DialogEvent {
                    control_id: control_id.clone(),
                    action: action.clone(),
                    value: value.clone(),
                    current_values: current_values.clone(),
                };
                if let Some(cmd) = self
                    .tabs
                    .get_mut(tab_idx)
                    .and_then(|t| t.active_cmd.as_mut())
                {
                    let resp = cmd.on_dialog_action(event);
                    let updates = cmd.take_pending_panel_updates();
                    (
                        Some(resp),
                        updates,
                        Some((
                            dialog_title,
                            owner_plugin_id,
                            owner_command_id,
                            current_values,
                        )),
                    )
                } else {
                    (
                        None,
                        Vec::new(),
                        Some((
                            dialog_title,
                            owner_plugin_id,
                            owner_command_id,
                            current_values,
                        )),
                    )
                }
            } else {
                (None, Vec::new(), None)
            };

        self.apply_panel_or_dialog_updates(panel_updates);

        match action_resp {
            Some(ocs_plugin_api::dialog::DialogActionResponse::UpdateValues(values)) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    state.update_values(values);
                }
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::ReplaceDialog(new_def)) => {
                if let Some(state) = &mut self.plugin_dialog_state {
                    let tab_id = state.tab_id;
                    let plugin_id = state.plugin_id.clone();
                    let command_id = state.command_id;
                    *state = crate::ui::window::plugin_dialog::PluginDialogState::new(*new_def)
                        .with_owner(tab_id, plugin_id, command_id);
                }
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::Submit) => {
                return self.on_plugin_dialog_submit();
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::Close) => {
                return self.on_plugin_dialog_cancel();
            }
            Some(ocs_plugin_api::dialog::DialogActionResponse::None) | None => {
                if let Some((dialog_title, owner_plugin_id, owner_command_id, mut current_vals)) =
                    fallback_notify
                {
                    if let Some(v) = value {
                        current_vals.insert("__event_value__".to_string(), v);
                    }
                    crate::plugin::v4_support::publish_panel_action_to_plugin(
                        owner_plugin_id.as_deref(),
                        owner_command_id,
                        dialog_title,
                        control_id,
                        action,
                        Some(ocs_plugin_api::dialog::DialogValue::Fields(current_vals)),
                    );
                }
            }
        }
        Task::none()
    }

    pub(crate) fn on_plugin_dialog_submit(&mut self) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let owner_plugin_id = self
            .plugin_dialog_state
            .as_ref()
            .and_then(|s| s.plugin_id.clone());
        let response = if let Some(state) = &self.plugin_dialog_state {
            state.collect_response()
        } else {
            ocs_plugin_api::dialog::DialogResponse::Cancelled
        };
        self.close_active_modal();

        let (cmd_result, next_dialog, panel_updates) = if let Some(cmd) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.active_cmd.as_mut())
        {
            let res = cmd.on_dialog_response(response);
            let next = if matches!(
                res,
                crate::command::CmdResult::CommitAndExit(_)
                    | crate::command::CmdResult::CommitEntitiesAndExit(_)
                    | crate::command::CmdResult::Cancel
            ) {
                None
            } else {
                cmd.modal_dialog()
            };
            let updates = cmd.take_pending_panel_updates();
            (Some(res), next, updates)
        } else {
            (None, None, Vec::new())
        };

        self.apply_panel_or_dialog_updates(panel_updates);

        if let Some(def) = next_dialog {
            self.open_plugin_dialog_on_tab(tab_idx, def, owner_plugin_id);
        }

        if let Some(res) = cmd_result {
            self.apply_cmd_result(res)
        } else {
            Task::none()
        }
    }

    pub(crate) fn on_plugin_dialog_cancel(&mut self) -> Task<Message> {
        let tab_idx = self.dialog_target_tab_index();
        let owner_plugin_id = self
            .plugin_dialog_state
            .as_ref()
            .and_then(|s| s.plugin_id.clone());
        self.close_active_modal();

        let (cmd_result, next_dialog, panel_updates) = if let Some(cmd) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.active_cmd.as_mut())
        {
            let res = cmd.on_dialog_response(ocs_plugin_api::dialog::DialogResponse::Cancelled);
            let next = if matches!(
                res,
                crate::command::CmdResult::CommitAndExit(_)
                    | crate::command::CmdResult::CommitEntitiesAndExit(_)
                    | crate::command::CmdResult::Cancel
            ) {
                None
            } else {
                cmd.modal_dialog()
            };
            let updates = cmd.take_pending_panel_updates();
            (Some(res), next, updates)
        } else {
            (None, None, Vec::new())
        };

        self.apply_panel_or_dialog_updates(panel_updates);

        if let Some(def) = next_dialog {
            self.open_plugin_dialog_on_tab(tab_idx, def, owner_plugin_id);
        }

        if let Some(res) = cmd_result {
            self.apply_cmd_result(res)
        } else {
            Task::none()
        }
    }

    /// Install `cmd` as the active interactive command for tab `tab`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn set_active_command(
        &mut self,
        tab: usize,
        cmd: Box<dyn crate::command::CadCommand>,
    ) {
        let dialog = cmd.modal_dialog();
        let owner_pid = cmd.owner_plugin_id().map(str::to_string);
        let cmd_id = cmd.plugin_command_id();
        let tab_id = self.tabs.get(tab).map(|t| t.id);
        if let Some(t) = self.tabs.get_mut(tab) {
            t.active_cmd = Some(cmd);
        }
        if let Some(dialog_def) = dialog {
            self.open_plugin_dialog_on_tab(tab, dialog_def, owner_pid);
        } else if let Some(state) = &mut self.plugin_dialog_state {
            if state.tab_id.is_none() {
                state.tab_id = tab_id;
            }
            if state.plugin_id.is_none() {
                state.plugin_id = owner_pid;
            }
            if state.command_id.is_none() {
                state.command_id = cmd_id;
            }
        }
    }

    /// Push an error message from the plugin runtime to the command line.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn push_plugin_error(&mut self, msg: &str) {
        self.command_line.push_error(msg);
    }

    /// Boot function for `iced::daemon`: returns initial state plus a task that
    /// opens the primary application window. Native only — the web build uses
    /// [`Self::boot_web`].
    #[cfg(not(target_arch = "wasm32"))]
    fn boot() -> (Self, Task<Message>) {
        use helpers::build_window_icon;
        // File association is no longer re-registered on every launch. It is set
        // up once via the first-launch prompt below (when the user hasn't been
        // asked yet) and afterwards managed entirely by the FILEASSOC command.
        crate::sys::startup_times::mark("boot: begin");
        let state = Self::new();
        crate::sys::startup_times::mark("boot: App constructed");
        let (id, open_task) = window::open(window::Settings {
            maximized: true,
            icon: build_window_icon().and_then(|rgba| window::icon::from_rgba(rgba, 32, 32).ok()),
            exit_on_close_request: false,
            // A Wayland compositor has no StartupWMClass to go on: it resolves a
            // window's dock icon by matching the window's app_id against the
            // basename of an installed .desktop file. iced defaults the id to an
            // empty string, so nothing matched and the dock button came up blank
            // even though the AppImage ships both the .desktop and the icon.
            // (X11 uses the .desktop's StartupWMClass and was unaffected.)
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: crate::io::file_association::APP_ID.to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut s = state;
        s.main_window = Some(id);
        tool_windows::set_main_window(id);
        let open_main = open_task.map(|_| Message::Noop);
        let check_update = if crate::config::managed() {
            Task::none()
        } else {
            Task::perform(
                crate::io::update_check::check_for_update(),
                Message::UpdateCheckResult,
            )
        };
        let focus_cmd = s.focus_cmd_input();
        // Startup configuration from the command line (see `cli`). File
        // arguments — also how the OS file association launches us when
        // drawings are double-clicked — go through `OpenExternal`, the same
        // door a second launch hands files to: it existence-checks each path,
        // skips one already open, and queues the rest behind the load in
        // flight. Handing them straight to `OpenRecent` would let each
        // overwrite the single `opening` slot and silently drop all but one.
        // `--new` opens a fresh drawing tab instead of the welcome screen.
        // `--read-only` disables saving. `--script` queues command lines.
        let cfg = crate::cli::gui_config();
        s.read_only = cfg.read_only;
        // GPU backend / renderer fallback: the resolver ran before iced
        // booted, so surface its verdict here where the user can see it.
        if let Some(notice) = cfg.gpu_fallback_notice {
            s.command_line.push_warning(&notice);
            crate::scene::pipeline::report_gpu_line(&format!("[gpu] {notice}"));
        }
        if cfg.gpu_compat_auto {
            let notice = crate::gpu_backend::compat_notice();
            s.command_line.push_warning(&notice);
            crate::scene::pipeline::report_gpu_line(&format!("[gpu] {notice}"));
        }
        let has_files = !cfg.files.is_empty();
        let cli_open: Task<Message> = if has_files {
            Task::batch(
                cfg.files
                    .into_iter()
                    .map(|p| Task::done(Message::OpenExternal(p))),
            )
        } else if cfg.new || !cfg.script_lines.is_empty() {
            Task::done(Message::TabNew)
        } else {
            Task::none()
        };
        // Startup command script: if files were passed to open, defer the
        // script lines until the document finishes opening. Otherwise, dispatch
        // them on the newly created drawing tab.
        let has_script = !cfg.script_lines.is_empty();
        let script: Task<Message> = if cfg.script_lines.is_empty() {
            Task::none()
        } else if has_files {
            s.pending_startup_script_lines = cfg.script_lines;
            Task::none()
        } else {
            s.script_queue.extend(cfg.script_lines);
            Task::done(Message::ScriptPump)
        };
        // Unsaved drawings a crashed session left behind.
        #[cfg(not(target_arch = "wasm32"))]
        s.recover_ended_session_autosaves();
        // An unattended script must not stop at a startup dialog.
        if !has_script {
            s.queue_startup_prompts();
        }
        // Fetch the Patreon supporters list once at boot for the Start page.
        #[cfg(not(target_arch = "wasm32"))]
        let patrons_fetch = Task::perform(
            async { crate::patreon::fetch_patrons() },
            Message::PatronsFetched,
        );
        #[cfg(target_arch = "wasm32")]
        let patrons_fetch = Task::none();
        // Tutorial videos: show the on-disk cache instantly, refresh from the
        // live playlist in the background. Nothing ships in the binary. The
        // fetch runs on its own OS thread — its several sequential HTTP
        // requests would otherwise sit on the async executor and hold up the
        // rest of the boot tasks (the Start page waited on it).
        #[cfg(not(target_arch = "wasm32"))]
        let videos_fetch = {
            s.set_videos(crate::videos::load_cached());
            s.videos_loading = true;
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::videos::fetch_playlist());
            });
            Task::perform(
                async move {
                    rx.await
                        .unwrap_or_else(|_| Err("video fetch thread died".into()))
                },
                Message::VideosFetched,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let videos_fetch = Task::none();
        // GitHub Discussions: seed from the last successful fetch, then refresh
        // the public feed and pinned section on a background thread.
        #[cfg(not(target_arch = "wasm32"))]
        let discussions_fetch = {
            s.discussions = crate::discussions::load_cached();
            s.discussions_loading = true;
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::discussions::fetch_discussions());
            });
            Task::perform(
                async move {
                    rx.await
                        .unwrap_or_else(|_| Err("discussion fetch thread died".into()))
                },
                Message::DiscussionsFetched,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let discussions_fetch = Task::none();
        // Recent-file thumbnails: decoded off-thread — parsing every recent
        // DWG's preview on the boot path held the first frame back.
        let thumbs_fetch = s.refresh_recent_thumbs();
        (
            s,
            Task::batch([
                open_main,
                check_update,
                focus_cmd,
                cli_open,
                script,
                patrons_fetch,
                videos_fetch,
                discussions_fetch,
                thumbs_fetch,
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
        let embed_mode = crate::io::host_bridge::is_embed_mode();
        if embed_mode {
            s.donation_prompt_version = env!("OCS_APP_VERSION").to_string();
            s.control.automation_seen = true;
            s.control.enabled = crate::io::host_bridge::is_automation_allowed();
            crate::io::host_bridge::install();
        }
        // A web host that intends to automate can opt in before the first
        // control request by setting `opencadstudio.automation` to "1" in
        // localStorage (the same pre-load mechanism the settings use): the
        // startup donation prompt then never queues (#1349).
        if !embed_mode
            && web_sys::window()
                .and_then(|window| window.local_storage().ok().flatten())
                .and_then(|storage| storage.get_item("opencadstudio.automation").ok().flatten())
                == Some("1".to_string())
        {
            s.control.automation_seen = true;
        }
        s.queue_startup_prompts();
        let focus = s.focus_cmd_input();
        let primary_font =
            crate::scene::text::web_font::preload_language(&crate::i18n::active_language_tag());
        let fonts = Task::batch([
            Task::done(Message::PollWebFonts),
            Task::done(Message::ApplyWebFont(primary_font)),
        ]);
        // Web can't reach the Patreon API directly (CORS); fetch the CI-built
        // supporters.json served on the same origin instead.
        let patrons = Task::perform(crate::patreon::fetch_patrons_web(), Message::PatronsFetched);
        s.videos_loading = true;
        let videos = Task::perform(crate::videos::fetch_playlist_web(), Message::VideosFetched);
        s.discussions_loading = true;
        let discussions = Task::perform(
            crate::discussions::fetch_discussions_web(),
            Message::DiscussionsFetched,
        );
        let thumbs_fetch = if embed_mode {
            Task::none()
        } else {
            s.refresh_recent_thumbs()
        };
        // Crash-recovery scan: OPFS snapshots newer than a drawing's last
        // saved copy come back as a restore prompt (stale ones are deleted).
        let autosave_scan = if embed_mode {
            Task::none()
        } else {
            Task::perform(
                crate::io::web_autosave::scan(),
                Message::WebAutosavesScanned,
            )
        };
        // URL deep link (`?file=…`): a shareable/embeddable link that opens a
        // drawing at boot. An http(s) URL is fetched with the same capped
        // streaming reader the remote-image fetch uses and handed to the
        // guarded drop-open queue; a bare name reads the OPFS recent copy the
        // same way `open_recent_web` does. Everything else (other schemes,
        // non-.dwg/.dxf URL tails) is refused with a console warning — the
        // same scheme gate the image fetch applies. Pure parsing/validation
        // lives in `io::web_link` and is unit-tested there.
        let deep_link: Task<Message> = {
            let search = if embed_mode {
                String::new()
            } else {
                web_sys::window()
                    .and_then(|window| window.location().search().ok())
                    .unwrap_or_default()
            };
            match crate::io::web_link::parse_link(&search) {
                crate::io::web_link::ParsedLink::Target(target) => {
                    // Opening a URL from the query string is an implicit trust
                    // decision: name the origin before the fetch starts (info
                    // only — the bytes still go through the parse + recovery
                    // machinery like any other open). Same-origin links stay
                    // quiet: that drawing already comes from this site.
                    if let crate::io::web_link::LinkTarget::Url { url, name } = &target {
                        if !crate::io::web_link::same_origin(url) {
                            let origin = crate::io::web_link::display_origin(url)
                                .unwrap_or_else(|| url.clone());
                            s.command_line
                                .push_info(&format!("Opening {name} from {origin}…"));
                        }
                    }
                    Task::perform(crate::io::web_link::open_link(target), |outcome| {
                        Message::WebLinkSettled(outcome)
                    })
                }
                crate::io::web_link::ParsedLink::Ignored(refusal, value) => {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(
                        &crate::io::web_link::refusal_warning(refusal, &value),
                    ));
                    Task::none()
                }
                crate::io::web_link::ParsedLink::Absent => Task::none(),
            }
        };
        (
            s,
            Task::batch([
                focus,
                fonts,
                patrons,
                videos,
                discussions,
                thumbs_fetch,
                autosave_scan,
                deep_link,
            ]),
        )
    }
}

use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> iced::Result {
    // UI text follows the desktop's interface font: generic sans-serif, iced's
    // default font, is remapped to it before the first window opens.
    crate::sys::startup_times::mark("run: entry");
    crate::sys::ui_font::apply(crate::sys::ui_font::detect().as_ref());
    crate::sys::startup_times::mark("run: ui_font applied");
    iced::daemon(
        OpenCADStudio::boot,
        OpenCADStudio::update_root,
        OpenCADStudio::view,
    )
    .settings(iced::Settings {
        power_preference: iced::backend::PowerPreference::HighPerformance,
        ..iced::Settings::default()
    })
    .subscription(OpenCADStudio::subscription)
    .scale_factor(|state: &OpenCADStudio, _| {
        let scale = f32::from(state.ui_scale_percent) / 100.0;
        // Icons fit their art to the physical pixels iced draws at: the
        // window's scale times this one. Noted where iced reads it, so the
        // two never disagree.
        crate::ui::icons::set_ui_scale(scale);
        scale
    })
    .title(|state: &OpenCADStudio, window_id: window::Id| {
        // A tool dialog or pallet in a window of its own carries its name.
        if let Some(title) = state.os_window_title(window_id) {
            return title;
        }
        if let Some(tab) = state.tabs.get(state.active_tab) {
            let dot = if tab.dirty { "● " } else { "" };
            let name = tab.tab_display_name();
            format!(
                "{}Open CAD Studio {} - {}",
                dot,
                env!("OCS_APP_VERSION"),
                name
            )
        } else {
            concat!("Open CAD Studio ", env!("OCS_APP_VERSION")).to_string()
        }
    })
    .theme(|state: &OpenCADStudio, _| state.active_theme.clone())
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
    .inspect(|()| {
        // Started once this process has let go of its window and its
        // single-instance port, so the new one boots as the only editor.
        if RESTART_AFTER_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
    })
}

/// Set when the user quits through Restart (the Plugin Manager after an
/// install); `run` starts the app again once its loop ends.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) static RESTART_AFTER_EXIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl Drop for OpenCADStudio {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        crate::sys::set_unsaved_changes_warning(false);
        // Kill plugin runner processes as soon as the application state is
        // dropped, instead of waiting for the thread-local manager destructor.
        // This makes host shutdown deterministic and fast on every exit path.
        #[cfg(not(target_arch = "wasm32"))]
        crate::plugin::external::shutdown_plugins();
    }
}

/// Single-window entry for the web (wasm) build. Uses `iced::application`
/// instead of `iced::daemon`: the browser canvas is the only window, so the
/// main-window view is rendered directly and the manager/dialog windows are
/// unavailable for now (see issue #45). Native keeps the multi-window `run`.
#[cfg(target_arch = "wasm32")]
pub fn run_web() -> iced::Result {
    // winit never emits DroppedFile on the web, so the document drag & drop
    // listeners are installed here and feed the control poll (#344).
    crate::io::web_drop::install();
    // Best-effort, fire-and-forget: ask the browser not to evict the origin's
    // storage under pressure — settings (localStorage) and the OPFS recent
    // copies / autosave snapshots live there. A denial changes nothing.
    if !crate::io::host_bridge::is_embed_mode() {
        wasm_bindgen_futures::spawn_local(async {
            if let Some(window) = web_sys::window() {
                if let Ok(promise) = window.navigator().storage().persist() {
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                }
            }
        });
    }
    iced::application(
        OpenCADStudio::boot_web,
        OpenCADStudio::update_root,
        OpenCADStudio::view_main,
    )
    .subscription(OpenCADStudio::subscription)
    .title(|_state: &OpenCADStudio| {
        concat!("Open CAD Studio ", env!("OCS_APP_VERSION")).to_string()
    })
    .theme(|state: &OpenCADStudio| state.active_theme.clone())
    .backend(iced::Backend::Hardware(iced::backend::Api::OpenGL))
    // No MSAA for canvas geometry: WebGL resolves the multisampled target with
    // the last draw's scissor still on, so only that rectangle came through.
    // Pattern previews stayed blank, stale crosshairs and grips lingered over
    // other panels (#2248, #2249, #2250).
    .antialiasing(false)
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
}
