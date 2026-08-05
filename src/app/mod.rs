mod alias;
#[cfg(not(target_arch = "wasm32"))]
mod automation;
pub(crate) mod config;
#[cfg(not(target_arch = "wasm32"))]
pub use automation::{export_headless, serve};
mod command_driver;
pub(crate) mod commands;
mod document;
mod expr_eval;
mod find_replace;
pub(crate) mod helpers;
mod history;
mod layers;
mod model_ops;
mod mtext_editor;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugin_host;
mod properties;
mod recent;
mod settings;
mod shortcuts;
mod style_ops;
mod text_inline;
mod update;
mod view;
mod visibility;

pub use style_ops::StyleKind;

use document::DocumentTab;

use crate::modules::ModuleEvent;
use crate::scene::CubeRegion;

/// Which UCS-icon grip is being dragged. `Origin` slides the UCS origin within
/// its own plane; `XAxis`/`YAxis` rotate the UCS so that axis points at the
/// cursor (the other in-plane axis follows, Z fixed). The cursor is mapped onto
/// the UCS plane every move, so no extra drag-start state is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UcsGripKind {
    Origin,
    XAxis,
    YAxis,
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
pub const HOVER_DWELL_MS: u128 = 500;
/// Dense resident sets also retain the previous rollover while moving; this
/// threshold gates that extra redraw-avoidance behavior.
pub const HOVER_DWELL_DENSE_WIRES: usize = 50_000;

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
/// (Lengthen / Radius / Arc Length / Rotate Text). Lengthen can also resolve
/// from a viewport point; otherwise the next typed number is parsed and routed
/// into `apply_grip_menu_value` for `(handle, grip_id, action)`.
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

/// Candidate set searched by Quick Select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectScope {
    CurrentSpace,
    CurrentSelection,
}

impl std::fmt::Display for QSelectScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match self {
            QSelectScope::CurrentSpace => "Current space",
            QSelectScope::CurrentSelection => "Current selection",
        };
        f.write_str(crate::t!(source).as_ref())
    }
}

/// Whether matching candidates are kept or removed from the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectMode {
    Include,
    Exclude,
}

/// Editor used by the Quick Select value field.
#[derive(Clone, Debug)]
pub enum QSelectValueEditor {
    Text,
    Number,
    Choice(Vec<String>),
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
    pub editor: QSelectValueEditor,
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
/// `(scope, type, property, op, value)` row plus result behavior.
#[derive(Clone, Debug)]
pub struct QSelectState {
    pub scope: QSelectScope,
    pub available_types: Vec<String>,
    pub available_properties: Vec<QSelectPropertyChoice>,
    pub candidate_count: usize,
    /// `None` = "(Any type)".
    pub type_filter: Option<String>,
    /// `None` = no property filter; the type filter alone applies.
    pub property: Option<QSelectPropertyChoice>,
    pub operator: QSelectOp,
    pub value: String,
    pub mode: QSelectMode,
    pub append: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FindReplaceState {
    pub search: String,
    pub replacement: String,
    pub status: String,
    pub current_match: Option<FindMatchKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindMatchKey {
    Entity(acadrust::Handle),
    BlockEntityInInsert {
        entity: acadrust::Handle,
        insert: acadrust::Handle,
    },
    InsertAttribute {
        insert: acadrust::Handle,
        index: usize,
    },
}
use crate::snap::Snapper;
use crate::ui::{CommandLine, Ribbon, StatusBar};
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::CadDocument;

use iced::time::Instant;
use iced::window;
use iced::{mouse, Point, Task, Theme};
use std::sync::Arc;

pub(super) const POLY_START_DELAY_MS: u128 = 150;
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
    pub recovery_read_stats: Option<acadrust::ReadStats>,
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
    layer_handle: acadrust::types::Handle,
    color: AcadColor,
    linetype_name: String,
    linetype_handle: acadrust::types::Handle,
    line_weight: i16,
    lt_scale: f64,
    dimstyle_name: String,
    dimstyle_handle: acadrust::types::Handle,
    tab_active_layer: String,
    tab_layers_current: String,
    ribbon_layer: String,
    ribbon_color: AcadColor,
    ribbon_linetype: String,
    ribbon_lineweight: LineWeight,
}

/// Which Start-page section a narrow (tabbed) Start page is showing.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum StartSection {
    Recent,
    Videos,
    #[default]
    Welcome,
    Discussions,
    Supporters,
}

pub(super) struct OpenCADStudio {
    start: Instant,
    tabs: Vec<DocumentTab>,
    active_tab: usize,
    hovered_doc_tab: Option<usize>,
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
    /// Block references whose properties panel shows per-axis Scale X/Y/Z even
    /// though the three factors are currently equal — the user unchecked the
    /// "Uniform scale" box for them (#427). Keyed by entity handle.
    props_asym_scale: std::collections::HashSet<u64>,
    /// Which Start-page section is shown when the page is too narrow for all
    /// three side by side and falls back to a tab bar.
    start_section: StartSection,
    /// Widest natural single-row width of the Start-page action buttons,
    /// measured by `WrapFlow` so side lists collapse before those buttons wrap.
    start_action_w: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// When the window is too narrow the properties panel collapses to a
    /// vertical bar; this is the user's toggle to expand it back out.
    props_expanded: bool,
    /// Persisted dock width and side of the Properties panel.
    properties_width: f32,
    properties_side: config::DockSide,
    properties_auto_collapse: bool,
    /// Transient hover/drag state for the docked Properties panel.
    properties_hovered: bool,
    properties_dragging: bool,
    properties_resizing: bool,
    properties_drag_last: Option<Point>,
    properties_dock_preview: Option<config::DockSide>,
    /// Read-only editor buffer backing the command-line history dropdown, so
    /// the log can be drag-selected across lines and copied (issue #232).
    /// Rebuilt from the history each time the dropdown is opened.
    history_content: iced::widget::text_editor::Content,
    /// Pointer state while the command-history panel's top edge is dragged.
    command_history_resizing: bool,
    command_history_drag_last: Option<Point>,
    status_bar: StatusBar,
    cursor_pos: Point,
    vp_size: (f32, f32),
    /// Full window size (w, h); used to anchor status-bar popups to their pill.
    win_size: (f32, f32),
    snapper: Snapper,
    snap_popup_open: bool,
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
    /// Quick Properties: show a compact floating property panel on selection.
    quick_properties: bool,
    /// Canvas-space cursor position where Quick Properties was last opened.
    quick_properties_anchor: Point,
    /// Selection cycling: clicking where objects overlap opens a list box
    /// to pick which one; the pick is added to the current selection.
    selection_cycling: bool,
    /// PICKADD (#226): true (default) = clicks accumulate; false = OS-style
    /// replace-on-click with Shift toggling.
    pick_add: bool,
    /// PICKDRAG (#226): false (default) = press-drag lassoes; true =
    /// press-drag draws a rectangle marquee.
    pick_drag_rect: bool,
    /// Frame-budget HUD (Phase 5.3): overlays the last wire re-tessellation
    /// cost and the shared performance trace on the active viewport. Toggled
    /// by the `PERF` command.
    perf_hud: bool,
    /// When set, the cycling list box is open: (canvas point, candidates).
    cycle_candidates: Option<(iced::Point, Vec<acadrust::Handle>)>,
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
    /// Show grid lines in the viewport (F7).
    show_grid: bool,
    /// Dynamic input overlay (F12): show coordinate tooltip near cursor.
    dyn_input: bool,
    /// Controls whether the TEXTEDIT command repeats automatically (0 = Multiple, 1 = Single).
    pub texteditmode: bool,
    /// When true (default), saving over an existing file first writes a `.bak`
    /// copy of it for recovery (#205). Toggle with the ISAVEBAK command.
    pub backup_on_save: bool,
    /// When true (default), the app registers itself as a .dwg/.dxf/.bak file
    /// handler on each launch. Toggle with the FILEASSOC command.
    pub file_assoc_enabled: bool,
    /// Minutes between autosaves to a `.sv$` recovery file (SAVETIME command);
    /// 0 disables autosave.
    pub savetime_min: i32,
    /// Persisted default viewport background, restored from settings and applied
    /// to every drawing tab (new and opened) so a chosen background survives
    /// restarts (#188). `None` = the built-in dark-grey / off-white defaults.
    /// The `a` channel is always 1.0.
    default_bg_color: Option<[f32; 4]>,
    default_paper_bg_color: Option<[f32; 4]>,
    /// `true` after a bare `VPORTS` in model space — the next command-line
    /// entry is treated as the tiled-config option (SIngle/2H/2V/4).
    awaiting_vports: bool,
    /// Set to the variable name after a bare system-variable query (e.g. a lone
    /// `MIRRTEXT`) — the next command-line entry is its new value, empty keeps
    /// the current one.
    pending_setvar: Option<String>,
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
    /// Handles hidden from the base tessellation during an in-progress grip
    /// drag. The edited entities are shown in the overlay until commit.
    grip_preview_handles: Vec<acadrust::Handle>,
    /// Pending rollover hit-test. Each idle cursor move stashes
    /// `(last_move_at, point, tab)` here and clears the live highlight;
    /// `HoverDwellTick` runs the pick once the cursor has been still for
    /// `HOVER_DWELL_MS`. Skipping the pick mid-stroke avoids the per-frame
    /// O(N) wire+hatch+mesh sweep that froze the cursor on large drawings.
    hover_dwell: Option<HoverDwell>,
    /// Snapshots of edited entities taken at the start of a grip drag. The drag
    /// mutates the document live, so Escape restores this group atomically.
    grip_originals: Vec<(acadrust::Handle, acadrust::EntityType)>,
    /// Document dirty state before the live grip mutation began.
    grip_dirty_before: Option<bool>,
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
    /// Show the UCS icon in the bottom-left corner of model space (UCSICON).
    show_ucs_icon: bool,
    /// Anchor the UCS icon to the projected UCS origin when it is on-screen,
    /// falling back to the corner otherwise (UCSICON ORigin / NOorigin).
    ucs_icon_at_origin: bool,
    /// Whether the ViewCube 3D gizmo is visible in model space (NAVVCUBE).
    show_viewcube: bool,
    /// Measured natural width (px, as `f32` bits) of the active viewport's
    /// render-mode control bar, written each frame by its `DensitySwap` and read
    /// next frame to decide whether the ViewCube still has room beside it — so
    /// the two corner widgets adapt to the bar's real width, not an estimate.
    render_bar_w: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// Whether the visual-style flyout beside the active viewport is open.
    render_mode_menu_open: bool,
    /// Mode whose sample is shown while the pointer moves through the flyout.
    /// This does not alter the drawing until the corresponding row is clicked.
    render_mode_preview: Option<acadrust::entities::ViewportRenderMode>,
    /// Whether the Properties panel is shown on the left (PROPERTIES).
    show_properties: bool,
    /// Docked Insert Block panel visibility.
    pub(crate) show_block_palette: bool,
    /// Docked Insert Block panel state (search, preview size, cached thumbnails).
    pub(crate) block_palette: crate::ui::window::block_palette::BlockPalette,
    /// Whether the narrow-window block-palette bar is expanded.
    block_palette_expanded: bool,
    /// Whether the document file tabs are shown at the top (FILETAB).
    show_file_tabs: bool,
    /// Whether the layout/paper-space tabs are shown at the bottom (LAYOUTTAB).
    show_layout_tabs: bool,
    /// Last point committed by a drawing command — used as ortho/polar base.
    last_point: Option<glam::DVec3>,
    /// Endpoint + unit exit-tangent of the most recently drawn line/arc, so
    /// `ARC_CONT` (Arc → Continue) can start tangentially from where drawing
    /// ended. `None` once a non-line/arc entity is committed.
    cont_anchor: Option<(glam::DVec3, glam::DVec3)>,
    /// OS window Id for the floating Layer Properties Manager (None when closed).
    /// OS window Id of the primary application window.
    main_window: Option<window::Id>,
    // ── Floating panel windows ────────────────────────────────────────────
    /// Active `iced_aw` colour picker: destination plus its initial colour.
    color_pick_target: Option<(ColorPickTarget, AcadColor)>,
    /// The open in-canvas modal dialog, if any (Plan B: shared overlay instead
    /// of OS windows).
    active_modal: Option<ModalKind>,
    /// FIND dialog inputs and current result cursor.
    find_replace: FindReplaceState,
    /// Set once the user acknowledges the AEC-drop warning, so re-entering the
    /// save path proceeds instead of re-showing the warning.
    aec_drop_acknowledged: bool,
    /// Number of unsupported objects shown in the AEC-drop warning modal.
    aec_drop_count: usize,
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
    /// Layer Manager: dragging the Name-column divider (width follows the
    /// shared ModalDragMove flow).
    layer_col_dragging: bool,
    /// Layer Manager Name column width in px, adjusted by the divider drag.
    layer_name_col_w: f32,
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
    attr_editor_handle: Option<acadrust::Handle>,
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
    /// Plugin ids the user turned off in the Plugin Manager. Disabled plugins
    /// keep their manifest listed but drop their ribbon tab and command
    /// dispatch. Persisted via [`settings::UserSettings::disabled_plugins`].
    disabled_plugins: rustc_hash::FxHashSet<String>,
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
    repo_release_tags:
        rustc_hash::FxHashMap<String, Vec<crate::plugin::external::ReleaseInfo>>,
    /// The release tag currently selected per linked repo.
    repo_selected_tag: rustc_hash::FxHashMap<String, String>,
    /// Repository currently shown in the Plugin Manager detail pane.
    selected_plugin_repo: Option<String>,
    /// Parsed GitHub README content or the last fetch error, cached per repo.
    plugin_readmes:
        rustc_hash::FxHashMap<String, Result<iced::widget::markdown::Content, String>>,
    /// README requests in flight, used to render a deterministic loading state.
    plugin_readme_loading: rustc_hash::FxHashSet<String>,
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
    /// Entities removed by the most recent ERASE, kept so OOPS can restore them.
    oops_cache: Vec<Arc<acadrust::EntityType>>,
    /// Paste anchor: lower-left corner of the clipboard entities' bounding box
    /// (or the point picked by COPYBASE). This point lands under the cursor at
    /// paste time.
    clipboard_base: glam::DVec3,
    /// Table records (layer / linetype / text + dim style) the clipboard
    /// entities reference, captured from the source drawing at copy time so a
    /// paste into a *different* drawing can recreate any that are missing —
    /// otherwise the pasted entities would dangle on a non-existent layer.
    clipboard_deps: ClipboardDeps,
    /// True while the Shift key is held — drives subtractive pick (Shift+click
    /// removes the picked entity from the selection). Tracked from keyboard
    /// modifier-change events since mouse click messages carry no modifiers.
    shift_down: bool,
    /// True while Ctrl/Cmd is held — drives Ctrl-click multi-select in the
    /// Layer Manager. Tracked from modifier-change events.
    ctrl_down: bool,
    /// Open in-place MText editor (toolbar + text area + live preview), if any.
    mtext_editor: Option<mtext_editor::MTextEditorState>,
    /// Open in-place single-line TEXT editor (plain text-entry box), if any.
    text_inline: Option<text_inline::TextInlineState>,
    /// Cursor-anchored one-shot snap override menu (Shift+RMB): the canvas
    /// point it opened at, or `None` when closed (#337).
    snap_override_popup: Option<iced::Point>,
    /// Hard axis lock (#312): the WCS direction captured when Shift went down
    /// during a rubber-band point pick. While `Some`, every candidate point —
    /// osnap hits included — projects onto this ray from `last_point` (or the
    /// grip origin). Cleared when Shift releases.
    axis_lock_dir: Option<glam::DVec3>,
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
    /// Pending model-space plot window (x0, y0, x1, y1) in world XY, or None.
    plot_window: Option<(f64, f64, f64, f64)>,
    plot_format: crate::io::paper_sizes::PaperSize,
    plot_orientation: crate::io::paper_sizes::Orientation,
    /// Backing state for the full Plot / Print dialog.
    plot_dialog: crate::ui::window::plot::PlotDialogState,
    /// Snapshot of the dialog's settings taken when it opened, restored by the
    /// `<previous>` list entry.
    plot_prev: Option<crate::ui::window::plot::PlotDialogState>,
    /// Paper layouts shown by Print All, in tab order with their selection.
    print_all_layouts: Vec<(String, bool)>,
    /// True while the Plot dialog is editing settings for Print All.
    print_all_options: bool,
    /// Settings restored when the Print All options dialog is cancelled.
    print_all_options_prev: Option<crate::ui::window::plot::PlotDialogState>,
    /// Plot style restored together with cancelled Print All options.
    print_all_plot_style_prev: Option<Option<crate::io::plot_style::PlotStyleTable>>,
    /// Plot window restored together with cancelled Print All options.
    print_all_plot_window_prev: Option<Option<(f64, f64, f64, f64)>>,

    // ── Plot Style Table ──────────────────────────────────────────────────
    /// Currently loaded CTB/STB table (None = no override).
    active_plot_style: Option<crate::io::plot_style::PlotStyleTable>,

    // ── MLineStyle Dialog ─────────────────────────────────────────────────
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
    mleaderstyle_tab: u8,
    mleaderstyle_compare: String,
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
    mls_align_space: String,
    mls_block_color: String,
    mls_block_rotation: String,
    mls_block_scale_x: String,
    mls_block_scale_y: String,
    mls_block_scale_z: String,

    // ── TableStyle Dialog ─────────────────────────────────────────────────
    tablestyle_selected: String,
    tablestyle_tab: u8,
    tablestyle_compare: String,
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
    textstyle_tab: u8,
    textstyle_compare: String,
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
    ui_theme: config::UiThemeConfig,
    theme_color_inputs: [String; 6],

    // ── Keyboard Shortcut Editor ──────────────────────────────────────────
    /// Complete editable key → command/action table.
    shortcut_bindings: rustc_hash::FxHashMap<String, String>,
    /// Working rows shown by the shortcut editor until Apply is pressed.
    shortcut_editor_rows: Vec<(String, String)>,

    // ── Command Aliases ───────────────────────────────────────────────────
    /// Command-line aliases: uppercase abbreviation → uppercase command
    /// ("L" → "LINE"). Loaded from `ocad.pgp` at boot and consulted before
    /// every dispatch (`resolve_alias`); user-editable via ALIASEDIT. See
    /// [`alias`].
    command_aliases: rustc_hash::FxHashMap<String, String>,
    /// Working buffer for the ALIASEDIT modal: `(alias, command)` rows being
    /// edited. Seeded from `command_aliases` on open, committed back on close.
    alias_editor_rows: Vec<(String, String)>,

    // ── Layout Manager Panel ──────────────────────────────────────────────
    layout_manager_selected: String,
    layout_manager_rename_buf: String,

    // ── Layer State Manager ───────────────────────────────────────────────
    layer_state_selected: Option<String>,
    layer_state_name_buf: String,
    layer_state_description_buf: String,
    layer_state_filter: String,
    layer_state_edit_draft: Option<acadrust::LayerState>,
    layer_state_edit_filter: String,
    layer_state_edit_color_open: Option<usize>,

    // ── Annotation-scale Manager ──────────────────────────────────────────
    scale_manager_selected: String,
    scale_manager_paper_buf: String,
    scale_manager_drawing_buf: String,
    /// The scale row being renamed inline (double-click), + its edit buffer.
    scale_rename: Option<String>,
    scale_rename_buf: String,
    /// The entity the Annotation Object Scale dialog is editing (its per-object
    /// annotation-scale membership).
    anno_object_scale_target: Option<acadrust::types::Handle>,
    /// Open transaction for the scale manager — restored if the window closes
    /// without Apply, mirroring the style managers' staging.
    scale_stage: Option<crate::app::style_ops::ScaleStage>,

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
    open_job_serial: u64,
    /// Last repair or failed-open report shown in the recovery modal.
    recovery_report: Option<crate::io::recovery::RecoveryReport>,
    /// Drawings handed to us by other launches while `opening` was busy.
    /// `opening` is a single slot that a second `OpenPathPicked` would
    /// overwrite, and `on_file_opened` drops any result arriving once it is
    /// clear — so overlapping opens silently lose documents. Selecting several
    /// drawings in a file manager produces exactly that (one process per file,
    /// all arriving at once), which makes this queue load-bearing, not polish.
    pub(super) pending_opens: std::collections::VecDeque<PathBuf>,
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
    pending_save_leases:
        std::collections::HashMap<u64, crate::io::edit_lock::EditLease>,
    /// Locked-file failure currently shown in the recovery dialog.
    #[cfg(not(target_arch = "wasm32"))]
    pending_save_failure: Option<PendingSaveFailure>,
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
    /// Which colour field currently has its expanded palette open (if any).
    ds_color_open: Option<DsField>,
    /// Active property group in the dimension style manager.
    dimstyle_tab: u8,
    /// Style used by the manager's comparison summary.
    dimstyle_compare: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) enum SavePurpose {
    Manual,
    SaveAs,
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
pub(super) struct PendingSaveFailure {
    tab_id: u64,
    path: PathBuf,
    version: acadrust::DxfVersion,
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
    version: acadrust::DxfVersion,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    set_current_path: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct ThumbnailCacheKey {
    epoch: u64,
    camera_generation: u64,
    bg_color: [u32; 4],
    png: bool,
    viewport: [u32; 2],
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
    version: acadrust::DxfVersion,
    previous_autosave: Option<PathBuf>,
    set_current_path: bool,
    purpose: SavePurpose,
    continuation: SaveContinuation,
    thumbnail_key: Option<ThumbnailCacheKey>,
    refreshed_preview: Option<Option<acadrust::Preview>>,
    result: Result<(), crate::io::SaveFailure>,
}

/// Where a colour chosen in the standalone palette window should be applied.
#[derive(Debug, Clone)]
pub enum ColorPickTarget {
    DimStyle(DsField),
    MLeader(&'static str),
    Table(u8, &'static str),
    /// Selected entities' colour (left properties panel).
    Properties,
    /// Selected entities' background colour (hatch / MTEXT background row).
    PropertiesBg,
    /// Current creation colour (ribbon).
    Ribbon,
    /// A layer's colour, by panel row index.
    Layer(usize),
    /// A saved layer-state layer's colour, by editor row index.
    LayerState(usize),
    /// The MText editor's selection (or global) colour.
    MText,
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
    /// Baked `*D` blocks referenced by copied dimensions, snapshotted from the
    /// source drawing. On paste each pasted dimension gets its own transformed
    /// copy of its block (its geometry is baked in WCS, so without this the
    /// paste renders at the source location — or, cross-drawing, not at all).
    /// See #290 (mirrors the in-drawing copy's #161 fix).
    pub dim_blocks: Vec<BlockDef>,
    /// Extension-dictionary object subtrees hanging off the copied entities
    /// (XCLIP spatial filters, attached XRecords, …). Each entity's whole
    /// `xdictionary` graph is snapshotted so a cross-drawing paste recreates it
    /// — without this a pasted clipped block loses its clip and renders whole.
    pub ext_objects: Vec<ClipExtObjects>,
    /// Groups whose *entire* membership is in the copied selection, snapshotted
    /// from the source drawing. On paste each is recreated over the pasted
    /// handles so a copied group stays grouped — cross-drawing too, the same way
    /// the in-drawing COPY path preserves it. Partly-selected groups are not
    /// captured (copying one member should not spawn a fragment). See #440.
    pub groups: Vec<acadrust::objects::Group>,
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

/// The extension-dictionary object graph captured for one copied entity.
/// `objects` holds every object reachable from the entity's `xdictionary`
/// (dictionaries + their leaf objects), keyed by their source handles; `root`
/// is the xdictionary handle. On paste the whole set is cloned into the target
/// document with fresh handles and the references are remapped.
#[derive(Clone)]
pub struct ClipExtObjects {
    pub entity_index: usize,
    pub src_entity_handle: acadrust::Handle,
    pub root: acadrust::Handle,
    pub objects: Vec<(acadrust::Handle, acadrust::objects::ObjectType)>,
    pub annotation_scales: Vec<(acadrust::Handle, acadrust::objects::Scale)>,
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
            n.is_empty() || n.eq_ignore_ascii_case("ByLayer") || n.eq_ignore_ascii_case("ByBlock")
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
        // Extension-dictionary subtree per entity (XCLIP filters etc.).
        let mut ext_objects = Vec::new();
        for (entity_index, e) in entities.iter().enumerate() {
            let c = e.common();
            if let Some(root) = c.xdictionary_handle {
                if root.is_null() {
                    continue;
                }
                let objects = Self::collect_ext_subtree(doc, root);
                if !objects.is_empty() {
                    let mut annotation_scales = Vec::new();
                    for (_, object) in &objects {
                        let acadrust::objects::ObjectType::ObjectContextData(context) = object else {
                            continue;
                        };
                        if annotation_scales
                            .iter()
                            .any(|(handle, _)| *handle == context.scale)
                        {
                            continue;
                        }
                        if let Some(acadrust::objects::ObjectType::Scale(scale)) =
                            doc.objects.get(&context.scale)
                        {
                            annotation_scales.push((context.scale, scale.clone()));
                        }
                    }
                    ext_objects.push(ClipExtObjects {
                        entity_index,
                        src_entity_handle: c.handle,
                        root,
                        objects,
                        annotation_scales,
                    });
                }
            }
        }

        // Baked `*D` blocks for copied dimensions (top-level and inside captured
        // blocks), so a pasted dimension can be given its own transformed block
        // instead of aliasing the source's — whose geometry is baked in WCS at
        // the source location. (#290)
        let mut dim_block_names: BTreeSet<String> = BTreeSet::new();
        for e in entities
            .iter()
            .chain(blocks.iter().flat_map(|d| d.entities.iter()))
        {
            if let EntityType::Dimension(d) = e {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    dim_block_names.insert(bn);
                }
            }
        }
        let dim_blocks: Vec<BlockDef> = dim_block_names
            .iter()
            .filter_map(|n| Self::snapshot_block(doc, n))
            .collect();

        // Groups fully contained in the top-level selection. Groups reference
        // top-level entities (not block internals), so match against `entities`
        // only. Mirrors the in-drawing `copy_complete_groups` membership test.
        let copied_handles: rustc_hash::FxHashSet<acadrust::Handle> =
            entities.iter().map(|e| e.common().handle).collect();
        let groups: Vec<acadrust::objects::Group> = doc
            .objects
            .values()
            .filter_map(|obj| match obj {
                acadrust::objects::ObjectType::Group(g)
                    if !g.entities.is_empty()
                        && g.entities.iter().all(|h| copied_handles.contains(h)) =>
                {
                    Some(g.clone())
                }
                _ => None,
            })
            .collect();

        ClipboardDeps {
            layers: layers
                .iter()
                .filter_map(|n| doc.layers.get(n).cloned())
                .collect(),
            linetypes: ltypes
                .iter()
                .filter_map(|n| doc.line_types.get(n).cloned())
                .collect(),
            text_styles: tstyles
                .iter()
                .filter_map(|n| doc.text_styles.get(n).cloned())
                .collect(),
            dim_styles: dstyles
                .iter()
                .filter_map(|n| doc.dim_styles.get(n).cloned())
                .collect(),
            blocks,
            dim_blocks,
            ext_objects,
            groups,
        }
    }

    /// Breadth-first collect of every object reachable from extension-dictionary
    /// `root` (dictionary entries, nested xdictionaries, dictionary defaults),
    /// returned as `(source_handle, object)` pairs. Cycle-safe.
    fn collect_ext_subtree(
        doc: &acadrust::CadDocument,
        root: acadrust::Handle,
    ) -> Vec<(acadrust::Handle, acadrust::objects::ObjectType)> {
        use acadrust::objects::ObjectType;
        use rustc_hash::FxHashSet;
        let mut seen: FxHashSet<acadrust::Handle> = FxHashSet::default();
        let mut queue = vec![root];
        let mut out = Vec::new();
        while let Some(h) = queue.pop() {
            if h.is_null() || !seen.insert(h) {
                continue;
            }
            let Some(obj) = doc.objects.get(&h) else {
                continue;
            };
            // Enqueue children referenced by this object.
            match obj {
                ObjectType::Dictionary(d) => {
                    queue.extend(d.entries.iter().map(|(_, ch)| *ch));
                    if let Some(x) = d.xdictionary_handle {
                        queue.push(x);
                    }
                }
                ObjectType::DictionaryWithDefault(d) => {
                    queue.extend(d.entries.iter().map(|(_, ch)| *ch));
                    queue.push(d.default_handle);
                }
                _ => {}
            }
            out.push((h, obj.clone()));
        }
        out
    }

    /// Snapshot every block definition the `entities` reference through an
    /// INSERT, walking nested INSERTs transitively. Model/paper space and
    /// xref blocks are skipped — those aren't portable definitions.
    fn capture_blocks(
        doc: &acadrust::CadDocument,
        entities: &[acadrust::EntityType],
    ) -> Vec<BlockDef> {
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
            let Some(def) = Self::snapshot_block(doc, &name) else {
                continue;
            };
            // Follow nested INSERTs so their definitions are captured too.
            for e in &def.entities {
                if let EntityType::Insert(ins) = e {
                    if seen.insert(ins.block_name.clone()) {
                        queue.push(ins.block_name.clone());
                    }
                }
            }
            defs.push(def);
        }
        defs
    }

    /// Snapshot one block definition as a portable `BlockDef`: its base point
    /// and owned entities, minus the structural Block/BlockEnd markers. Returns
    /// None for model/paper space and xref blocks (not portable definitions).
    fn snapshot_block(doc: &acadrust::CadDocument, name: &str) -> Option<BlockDef> {
        use acadrust::EntityType;
        let br = doc.block_records.get(name)?;
        if name.starts_with("*Model_Space")
            || name.starts_with("*Paper_Space")
            || br.flags.is_xref
        {
            return None;
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
            owned.push(e.clone());
        }
        Some(BlockDef {
            name: name.to_string(),
            base_point,
            entities: owned,
        })
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
    LayerStateManager,
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
    Unsaved,
    SaveDialog,
    Recovery,
    RecoveryPrompt,
    Options,
    FindReplace,
    AecDropWarning,
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
    ScaleManager,
    /// Add / remove the annotation scales a single selected object has a
    /// per-object representation for.
    AnnoObjectScale,
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

#[derive(Debug, Clone, Copy)]
pub enum ArrowKey {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick(Instant),
    /// Web: periodic check for per-script fonts a drawing needs but hasn't
    /// fetched yet (#141). Native: never emitted.
    PollWebFonts,
    /// Web: a per-script font finished fetching — `Ok(bytes)` or `Err(reason)`.
    WebFontLoaded(
        crate::scene::text::web_font::Script,
        Result<Vec<u8>, String>,
    ),
    /// Register a font already held by the shared web store with the UI renderer.
    ApplyWebFont(crate::scene::text::web_font::Script),
    /// Completion of the UI renderer's runtime font registration.
    WebUiFontLoaded(
        crate::scene::text::web_font::Script,
        Result<(), String>,
    ),
    /// Ctrl+V. Routed by `update` into an open text editor, the drawing-object
    /// clipboard, or the system text clipboard.
    PasteShortcut,
    /// Completion of a system text clipboard read requested by PASTECLIP.
    SystemClipboardPaste(SystemClipboardText),
    /// Ctrl/Cmd+A — select all layer rows when the Layer Manager is open, or all
    /// drawing objects otherwise (#236).
    SelectAllShortcut,
    /// Open the shared Find and Replace dialog (Ctrl/Cmd+F or Ctrl/Cmd+H).
    FindReplaceOpen,
    FindReplaceSearchChanged(String),
    FindReplaceReplacementChanged(String),
    FindReplaceNext,
    FindReplaceOne,
    FindReplaceAll,
    /// System-clipboard text read for the MText editor (`None` = empty/denied).
    MTextPasteClip(Option<String>),
    /// System-clipboard text read for the single-line TEXT editor.
    TextInlinePasteClip(Option<String>),
    OpenFile,
    /// File picker returned. `Some((path, size_in_bytes))` → start loading;
    /// `None` → user cancelled the dialog (no overlay shown).
    OpenPathPicked(Option<(PathBuf, u64)>),
    /// A file was dragged from the desktop and dropped on the window (#344).
    FileDropped(PathBuf),
    /// Web only: Ctrl+V captured by a focused text field, whose iced-internal
    /// paste is inert there — read the async clipboard instead (#346).
    WebFieldPaste,
    /// Web only: clipboard text arrived for a focused field — replay it as
    /// synthetic keystrokes on the canvas.
    WebFieldPasteText(Option<String>),
    /// Web only: Ctrl+C captured by a focused text field — collect the
    /// focused input's text via a widget operation.
    WebFieldCopy,
    /// Web only: the focused field's text — write it to the clipboard.
    WebFieldCopyText(Option<String>),
    /// Pick from the one-shot snap override menu (Shift+RMB): only this snap
    /// applies to the next point pick, then the configuration restores (#337).
    SnapOverridePick(crate::snap::SnapType),
    /// Close the one-shot snap override menu without picking.
    SnapOverrideClose,
    /// Open a path from the Start tab's recent-documents list (skips the
    /// file picker; the path is already known).
    OpenRecent(PathBuf),
    /// A second launch handed us a drawing to open (single instance). Queued
    /// behind any open already in flight — see `pending_opens`.
    OpenExternal(PathBuf),
    /// Open a URL in the system browser (start-page intro video, links).
    OpenUrl(String),
    /// Select which section a narrow (tabbed) Start page shows.
    StartSectionSelect(StartSection),
    /// Expand/collapse the properties panel when it has shrunk to a bar.
    TogglePropertiesBar,
    /// Close the docked Properties panel; the ribbon command can reopen it.
    PropertiesClose,
    /// Pin/unpin the docked Properties panel.
    PropertiesAutoCollapseToggle,
    /// Hover state drives expansion while auto-collapse is enabled.
    PropertiesHover(bool),
    /// Begin dragging the Properties title bar to the opposite dock edge.
    PropertiesDockGrab,
    /// Begin dragging the Properties/viewport divider.
    PropertiesResizeGrab,
    /// Reset the dock width to its default value.
    PropertiesWidthReset,
    /// Full-workspace pointer tracking shared by dock and resize drags.
    PropertiesDragMove(Point),
    PropertiesDragRelease,
    /// Scroll the status-bar layout-tab strip horizontally by `delta` px
    /// (negative = left). Driven by the ‹ › arrows next to the tabs.
    ScrollLayoutTabs(f32),
    /// Drop a path from the recent-documents list.
    RecentRemove(PathBuf),
    /// Set how many recent documents to keep (persisted).
    SetRecentLimit(usize),
    /// Live edit of the recent-limit input box (not applied until Enter).
    RecentLimitInput(String),
    /// User clicked Cancel on the loading overlay. The parser thread keeps
    /// running but its result is discarded.
    OpenCancel,
    #[cfg(target_arch = "wasm32")]
    WebFileOpened(u64, crate::io::WebOpenOutcome),
    #[cfg(target_arch = "wasm32")]
    WebFileCached(u64, crate::io::WebOpenOutcome, Result<(), String>),
    FileOpened(u64, Result<
        (String, PathBuf, CadDocument, crate::scene::DerivedCaches),
        crate::io::OpenLoadError,
    >),
    RecoveryClose,
    RecoveryAttempt,
    RecoveryDecline,
    RecoverySaveAs,
    RecoveryShowLog,
    /// Web: an asynchronous OPFS copy written after Save is ready for recents.
    #[cfg(target_arch = "wasm32")]
    WebRecentStored(Result<PathBuf, String>),
    SaveFile,
    SaveAs,
    // ── Custom Save-As dialog ─────────────────────────────────────────────
    SaveDialogFormatChanged(String),
    SaveDialogFilenameChanged(String),
    SaveDialogConfirm,
    SaveDialogCancel,
    /// Destination picked in the native OS save dialog (`None` = cancelled).
    SaveDialogPathPicked(Option<std::path::PathBuf>),
    /// Open the application-wide Options dialog.
    OptionsOpen,
    /// Set the default type/version used when first saving a new drawing.
    DefaultSaveFormatChanged(String),
    /// Select one of Iced's built-in themes or the editable Custom theme.
    OptionsThemeChanged(String),
    /// Edit one of Custom theme's six base colours as #RRGGBB.
    OptionsThemeColorChanged(usize, String),
    /// Switch the interface language and redraw localized views.
    LanguageChanged(crate::i18n::Language),
    ClearScene,
    SetWireframe(bool),
    /// Set the active tab's render mode (one of acadrust's seven visual
    /// styles). Replaces the binary `SetWireframe` over time; the older
    /// message stays for ribbon/CLI back-compat and forwards.
    SetRenderMode(acadrust::entities::ViewportRenderMode),
    /// Open or close the active viewport's visual-style flyout.
    ToggleRenderModeMenu(acadrust::entities::ViewportRenderMode),
    /// Close the visual-style flyout after Escape or an outside click.
    DismissRenderModeMenu,
    /// Change only the sample shown beside the visual-style list.
    PreviewRenderMode(acadrust::entities::ViewportRenderMode),
    /// Switch camera projection: true = Orthographic, false = Perspective.
    SetProjection(bool),
    /// Select a ribbon module tab by index.
    RibbonSelectTab(usize),
    /// Change the ribbon tool-panel density (persisted).
    SetRibbonCollapseMode(crate::ui::ribbon::CollapseMode),
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
    // ── Document tabs ──────────────────────────────────────────────────────
    /// Create a new empty document tab.
    TabNew,
    /// Switch to the given tab index.
    TabSwitch(usize),
    /// Drawing tab currently under the pointer; controls integrated close affordance.
    DocTabHover(Option<usize>),
    /// Move a drawing tab before/after another drawing tab.
    TabReorder {
        from: usize,
        to: usize,
        after: bool,
    },
    /// Close the given tab index.
    TabClose(usize),
    /// Save every drawing that already has a file path.
    DocTabSaveAll,
    /// Close every non-Start drawing tab.
    DocTabCloseAll,
    /// Close every non-Start drawing tab except the given one.
    DocTabCloseOthers(usize),
    /// Copy the saved drawing's absolute path to the system clipboard.
    DocTabCopyFullPath(usize),
    /// Reveal the saved drawing in the platform file manager.
    DocTabOpenFileLocation(usize),
    // ── Unsaved-changes confirmation dialog ───────────────────────────────
    /// User clicked "Save" in the unsaved-changes dialog.
    UnsavedDialogSave,
    /// User clicked "Discard" in the unsaved-changes dialog.
    UnsavedDialogDiscard,
    /// User clicked "Cancel" in the unsaved-changes dialog.
    UnsavedDialogCancel,
    // ── AEC / unsupported-object drop warning (lossy Save-As) ──────────────
    /// Save as the source DWG version so the unsupported objects survive.
    AecDropSameVersion,
    /// Proceed with the chosen target format, dropping the unsupported objects.
    AecDropProceed,
    /// Go back to the Save dialog from the AEC-drop warning.
    AecDropBack,
    /// Periodic autosave tick — write `.sv$` recovery files for dirty tabs.
    AutoSave,
    /// Native background save/autosave completed.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFinished(SaveOutcome),
    /// Retry the failed save after the other application releases the file.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseRetry,
    /// Choose a different destination for the failed save.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseSaveAs,
    /// Cancel the failed save and any pending close/quit continuation.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseCancel,
    /// Reload the externally changed drawing and discard local edits.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeReload,
    /// Keep local edits but choose a different destination.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeSaveAs,
    /// Replace the externally changed disk copy with the local drawing.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeOverwrite,
    /// Cancel the conflicted save and any pending close/quit continuation.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeCancel,
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
    /// Close the active Model viewport, merging it into a neighbour.
    /// Only meaningful when more than one model tile exists.
    CloseModelViewport,
    /// Recall previous command in history (↑ arrow key).
    CommandHistoryPrev,
    /// Recall next command in history (↓ arrow key).
    CommandHistoryNext,
    /// An unconsumed arrow key; the active editor gets first choice, otherwise
    /// the configurable shortcut table handles it.
    ArrowKeyPressed {
        direction: ArrowKey,
        shortcut: String,
        extend_selection: bool,
    },
    /// A widget captured Up/Down; resolve it only if the command input owns
    /// keyboard focus.
    CommandLineArrowProbe { direction: ArrowKey },
    /// Result of the command-input focus query for a captured Up/Down key.
    CommandLineArrowResolved { direction: ArrowKey, focused: bool },
    /// Toggle the dropdown listing the full command-line history.
    CommandHistoryToggle,
    /// Grab/move/release the expanded history panel's top resize edge.
    CommandHistoryResizeGrab,
    CommandHistoryResizeMove(Point),
    CommandHistoryResizeRelease,
    /// Restore the expanded history panel to its default height.
    CommandHistoryHeightReset,
    /// Start dragging the Layer Manager's Name-column divider.
    LayerNameColGrab,
    /// Toggle the persistent literal-space mode (the `>` button): while on,
    /// every command line behaves as if it started with `>` — Space stays in
    /// the input instead of submitting. Saved in the user config.
    CommandLiteralToggle,
    /// Copy the full command-line history (every line) to the system
    /// clipboard as plain text — issue #232, so output can be pasted for
    /// debugging instead of screenshotted.
    CommandHistoryCopy,
    /// Clear every line from the command-line history.
    CommandHistoryClear,
    /// Copy every line currently retained by the PERF panel.
    PerfCopy,
    /// Clear the PERF panel's retained trace.
    PerfClear,
    /// Text-editor action from the read-only history dropdown. Only
    /// non-editing actions (cursor moves, selection, scroll) are applied so
    /// the log stays read-only while remaining drag-selectable and copyable.
    CommandHistoryEdit(iced::widget::text_editor::Action),
    /// User clicked an autocomplete suggestion — fill the input with
    /// the chosen command name and dispatch it.
    CommandSuggestionPick(String),
    /// User clicked a command-line option button — feed its keyword to the
    /// active command as if typed (empty keyword = Enter). (#304)
    CommandOptionPick(String),
    ToggleLayers,
    LayerToggleVisible(usize),
    LayerToggleLock(usize),
    LayerToggleFreeze(usize),
    /// Sort the Layer Manager table by a clicked column header.
    LayerSort(crate::ui::window::layers::LayerSortCol),
    /// Toggle per-viewport freeze: (layer_index, vp_col_index)
    LayerToggleVpFreeze(usize, usize),
    LayerNew,
    LayerDelete,
    /// Confirm deleting a non-empty layer (erases its objects too).
    LayerDeleteConfirm,
    LayerSetCurrent,
    LayerSelect(usize),
    LayerRenameStart(usize),
    LayerRenameEdit(String),
    LayerColorPickerToggle(usize),
    LayerColorMorePalette,
    LayerColorSet(acadrust::types::Color),
    LayerLinetypeSet(String),
    LayerLineweightSet(LineWeight),
    LayerTransparencyEdit(usize, String),
    LayerRenameCommit,
    // ── Layer State Manager ─────────────────────────────────────────────
    LayerStateManagerOpen,
    LayerStateManagerSelect(String),
    LayerStateManagerNew,
    LayerStateManagerFilter(String),
    LayerStateManagerName(String),
    LayerStateManagerDescription(String),
    LayerStateManagerSave,
    LayerStateManagerRestore,
    LayerStateManagerDelete,
    LayerStateManagerEdit,
    LayerStateEditorMaskToggle(LayerStateProperty),
    LayerStateEditorLayerFlagToggle(usize, LayerStateLayerFlag),
    LayerStateEditorLayerColorToggle(usize),
    LayerStateEditorLayerColor(usize, acadrust::types::Color),
    LayerStateEditorLayerLinetype(usize, String),
    LayerStateEditorLayerLineweight(usize, LineWeight),
    LayerStateEditorLayerPlotStyle(usize, String),
    LayerStateEditorLayerTransparency(usize, Option<acadrust::types::Transparency>),
    LayerStateEditorName(String),
    LayerStateEditorDescription(String),
    LayerStateEditorCurrentLayer(String),
    LayerStateEditorFilter(String),
    LayerStateEditorSave,
    LayerStateEditorCancel,
    /// ViewCube-local cursor movement, tagged with the floating viewport that
    /// owned the overlay when the event was produced (`None` = Model layout).
    CursorMoved(Point, Option<acadrust::Handle>),
    /// ViewCube press with the same owner tag, so a stale overlay event can
    /// never fall through and rotate a different camera.
    ViewportClick(Option<acadrust::Handle>),
    ViewportMove(Point),
    ViewportLeftPress,
    ViewportLeftRelease,
    ViewportRightPress,
    ViewportRightRelease,
    ViewportMiddlePress,
    ViewportMiddleRelease,
    ViewportScroll(mouse::ScrollDelta),
    ViewportExit,
    // ── Per-pane Model viewport (pane_grid) ───────────────────────────────
    /// A pane_grid divider was dragged — resize the split.
    PaneResized(iced::widget::pane_grid::ResizeEvent),
    /// A pane body was clicked — focus that pane.
    PaneClicked(iced::widget::pane_grid::Pane),
    /// A pane was drag-and-dropped onto another — swap them.
    PaneDragged(iced::widget::pane_grid::DragEvent),
    /// Per-pane mouse events. `usize` = the pane's tile index; the `Point` is
    /// pane-local (offset to canvas coords + focus in the handler).
    PaneMove(usize, Point),
    PanePress(usize),
    PaneRelease(usize),
    PaneRightPress(usize),
    PaneRightRelease(usize),
    PaneMiddlePress(usize),
    PaneMiddleRelease(usize),
    PaneScroll(usize, mouse::ScrollDelta),
    /// Drag-handle pressed on the active pane's controls bar: arm a pane move —
    /// the next pane released over is swapped with the active pane.
    PaneMoveStart,
    ViewCubeSnap(CubeRegion),
    /// World-frame view snap from a compass cardinal (N/E/S/W), bypassing the
    /// UCS so the compass stays world-aligned.
    ViewCubeSnapWorld(CubeRegion),
    /// ViewCube home button → jump to the default isometric view.
    ViewCubeHome,
    /// ViewCube roll arrow → roll the view 90° (true = clockwise).
    ViewCubeRoll(bool),
    /// ViewCube nudge triangle → tip / spin the view 90°.
    ViewCubeNudge(crate::scene::NudgeDir),
    /// WCS/UCS selector under the cube — empty string = World.
    SetViewcubeUcs(String),
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
    /// Large projected interaction index finished preparing off the UI thread.
    InteractionIndexReady {
        tab_id: u64,
        epoch: u64,
        source: usize,
        wires: std::sync::Weak<Vec<crate::scene::WireModel>>,
        index: std::sync::Arc<
            crate::scene::pick::interaction_index::InteractionIndex,
        >,
        build_ms: f64,
    },
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
    /// Set polar tracking angle increment (POLAR picker / right-click cycle).
    SetPolarAngle(f32),
    /// Open/close the polar-tracking angle picker (POLAR pill caret).
    TogglePolarPopup,
    /// Close the polar-tracking angle picker.
    ClosePolarPopup,
    /// Live text edit of the polar picker's custom-angle field.
    PolarCustomInput(String),
    /// Apply the typed custom polar angle (Enter in the picker's field).
    SubmitPolarCustom,
    /// Set the model-space annotation scale (CANNOSCALE equivalent).
    SetAnnotationScale(String),
    /// Set the active viewport's custom_scale (paper space).
    SetViewportScale(String),
    ToggleAnnotationVisibility,
    ToggleAnnotationAutoAdd,
    SyncViewportAnnotationScale,
    /// Toggle the scale picker popup open/closed.
    ToggleScalePopup,
    /// Close the scale picker popup.
    CloseScalePopup,
    /// Open the annotation-scale manager (from the scale popup's Manage row).
    ScaleManagerOpen,
    /// Open the Annotation Object Scale dialog for the current single selection.
    AnnoObjectScaleOpen,
    /// Toggle whether the dialog's object has a representation for this scale.
    AnnoObjectScaleToggle(String),
    /// Select a scale row in the manager (loads it into the editor).
    ScaleManagerSelect(String),
    /// Add a new scale to the list (staged) and select it for editing.
    ScaleManagerNew,
    /// Duplicate the selected scale under a unique name (staged).
    ScaleManagerCopy,
    /// Begin inline rename of a scale row (double-click).
    ScaleRenameStart(String),
    /// Edit the inline-rename buffer.
    ScaleRenameEdit(String),
    /// Commit the inline rename.
    ScaleRenameCommit,
    /// Delete the selected scale.
    ScaleManagerDelete,
    /// Set the selected scale as the current annotation scale.
    ScaleManagerSetCurrent,
    /// Apply the editor fields to the selected scale (rename / re-ratio).
    ScaleManagerApply,
    /// Annotation-scale manager editor field edits.
    ScaleManagerPaperBuf(String),
    ScaleManagerDrawingBuf(String),
    /// Toggle the leftmost hamburger's Model/layout list dropdown.
    ToggleLayoutList,
    /// Close the Model/layout list dropdown.
    CloseLayoutList,
    /// Cycle the coordinate readout mode ($COORDS): static → live → polar.
    CycleCoordsMode,
    /// Toggle the status-bar customization menu open/closed.
    ToggleStatusBarMenu,
    /// Close the status-bar customization menu.
    CloseStatusBarMenu,
    /// Show/hide a single status-bar pill.
    ToggleStatusPill(crate::ui::statusbar::statusbar_config::StatusPill),
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
    /// Make every entity type selectable again (clear the filter).
    SelectionFilterSelectAll,
    /// Exclude every present entity type from selection.
    SelectionFilterClearAll,
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
    /// Toggle a collapsed ribbon panel's flyout open/closed (by panel title).
    ToggleRibbonPanel(String),
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
    /// Live text of the ribbon layer dropdown's search box (#343).
    RibbonLayerFilterChanged(String),
    /// Live text of the Layer Manager's search box (#343).
    LayerManagerFilterChanged(String),
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
    /// User stepped the Current Vertex selector by ±1 (polyline vertex nav).
    PropVertexStep(i8),
    /// User selected a hatch pattern from the pattern pick_list in Properties.
    PropHatchPatternChanged(String),
    /// Open or close the visual hatch-pattern picker.
    PropHatchPatternPickerToggle(String),
    /// Filter the visual hatch-pattern picker.
    PropHatchPatternSearchChanged(String),
    /// Move the visual pattern grid focus to a hovered card.
    PropHatchPatternFocus(usize),
    /// Move keyboard focus by one card or one two-column row.
    PropHatchPatternNavigate(i8),
    /// Select the keyboard-focused visual pattern card.
    PropHatchPatternConfirm,
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
    /// Toggle a collapsed coordinate group ("Position", "Scale", …) open or
    /// closed in the Properties panel, keyed `section:base`.
    PropGroupToggle(String),
    /// Toggle the editable-dropdown (block Name) option list open/closed.
    PropEditChoiceToggle,
    /// User is typing in a block-attribute value field (live buffer update),
    /// keyed by the attribute tag.
    PropAttrInput { tag: String, value: String },
    /// User committed a block-attribute value edit (Enter pressed).
    PropAttrCommit(String),
    /// Toggle the inline color picker dropdown open/closed.
    PropColorPickerToggle,
    /// Toggle the MTEXT background-colour picker dropdown open/closed.
    PropBgColorPickerToggle,
    /// User picked an MTEXT background fill colour — sets `background_color`
    /// and switches the background mode to Fill so it shows immediately.
    PropBgColorChanged(AcadColor),
    /// Toggle the generic per-field colour picker (e.g. a hatch gradient colour)
    /// identified by its property field name.
    PropColorFieldToggle(String),
    /// User picked a colour for a generic per-field colour row (hatch gradient
    /// `Color 1` / `Color 2`), routed by the field name.
    PropColorFieldChanged { field: String, color: AcadColor },
    /// Collapse the inline color picker dropdown. Fired when another
    /// properties-panel dropdown (a combo_box) opens, so at most one panel
    /// dropdown is open at a time and they can't overlap. (#235)
    PropColorPickerClose,
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
    /// Switch to an already-open BEDIT block tab.
    BlockEditSwitch(String),
    /// Move a paper layout before/after another paper layout.
    LayoutReorder {
        from: String,
        to: String,
        after: bool,
    },
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
    ShortcutEditorInput {
        idx: usize,
        field: crate::ui::window::shortcuts::ShortcutField,
        value: String,
    },
    ShortcutEditorAdd,
    ShortcutEditorRemove(usize),
    ShortcutEditorApply,
    /// Canonical key emitted by the global keyboard subscription.
    ShortcutPressed(String),
    // ── Command Alias Editor (ALIASEDIT) ────────────────────────────────
    /// Open the command-alias editor modal, seeding rows from the alias table.
    AliasEditorOpen,
    /// Live edit of the alias or command in row `idx`.
    AliasEditorInput {
        idx: usize,
        field: crate::ui::window::alias_editor::AliasField,
        value: String,
    },
    /// Append a blank alias row.
    AliasEditorAdd,
    /// Remove alias row `idx`.
    AliasEditorRemove(usize),
    /// Commit the edited rows to the alias table (Apply button); stays open.
    AliasEditorApply,
    // ── About window ────────────────────────────────────────────────────
    AboutOpen,
    /// Close whatever in-canvas modal dialog is open (Plan B).
    CloseModal,
    // ── Attribute editor dialog ───────────────────────────────────────────
    /// Open the attribute editor for an INSERT (double-click / ATTEDIT).
    AttrEditorOpen(acadrust::Handle),
    /// Switch the editor's active tab.
    AttrEditorTab(crate::ui::window::attribute_editor::AttrTab),
    /// Select the row the Text Options / Properties tabs act on.
    AttrEditorSelect(usize),
    /// Live edit of the attribute value at row `idx` (Attribute tab).
    AttrEditorInput { idx: usize, value: String },
    // Text Options — all act on the selected row:
    AttrEditorTextStyle(String),
    AttrEditorJustify(String),
    AttrEditorHeight(String),
    AttrEditorRotation(String),
    AttrEditorWidth(String),
    AttrEditorOblique(String),
    AttrEditorBackwards(bool),
    AttrEditorUpsideDown(bool),
    // Properties — all act on the selected row:
    AttrEditorLayer(String),
    AttrEditorLinetype(String),
    AttrEditorColor(String),
    AttrEditorLineweight(acadrust::types::LineWeight),
    /// Commit every attribute edit to the block, keeping the dialog open
    /// (Apply); the frame ✕ closes and discards any un-applied edits.
    AttrEditorApply,
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
    /// Filter installed and available plugin cards.
    PluginSearchInput(String),
    /// Link the repository currently in the text field.
    PluginRepoAdd,
    /// Unlink a repository.
    PluginRepoRemove(String),
    /// The curated registry was fetched.
    PluginRegistryFetched(Result<Vec<crate::plugin::external::RegistryEntry>, String>),
    /// Retry the curated registry request after a connection failure.
    PluginRegistryRetry,
    /// Expand or collapse raw registry error details.
    PluginRegistryErrorDetailsToggle,
    /// Copy registry URL, platform, version, and raw error details.
    PluginRegistryCopyDiagnostics,
    /// Patreon supporters fetched at boot for the Start page (name, USD cents).
    PatronsFetched(Result<Vec<(String, i64)>, String>),
    /// Tutorial-playlist videos fetched at boot for the Start page.
    VideosFetched(Result<Vec<crate::videos::VideoEntry>, String>),
    /// GitHub Discussions fetched at boot for the Start page.
    DiscussionsFetched(Result<Vec<crate::discussions::DiscussionEntry>, String>),
    /// Recent-file DWG preview thumbnails decoded on a background thread.
    RecentThumbsLoaded(
        Vec<(std::path::PathBuf, Option<iced::widget::image::Handle>)>,
    ),
    /// Installable releases and manifest API versions fetched for `owner/repo`.
    PluginReleasesFetched(
        String,
        Result<Vec<crate::plugin::external::ReleaseInfo>, String>,
    ),
    /// Choose a release tag for a repo (`repo`, `tag`).
    PluginReleaseSelect(String, String),
    /// Select a plugin source and show its GitHub README in the detail pane.
    PluginReadmeSelect(String),
    /// A GitHub README fetch finished (`repo`, Markdown or error).
    PluginReadmeFetched(String, Result<String, String>),
    /// Install the selected release of `owner/repo`.
    PluginInstall(String),
    /// Install a specific newer release from an installed plugin card.
    PluginUpdate(String, String),
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
    SetModifiers { shift: bool, ctrl: bool },
    // ── In-place MText editor ───────────────────────────────────────────
    /// Text-area edit action from the multi-line editor widget.
    MTextEdit(iced::widget::text_editor::Action),
    /// Toolbar character-format toggle applied to the selection.
    MTextFmt(mtext_editor::MTextFmt),
    /// Toolbar height field changed.
    MTextHeight(String),
    /// MText wrapping width changed from the editor slider.
    MTextRectWidth(f64),
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
    /// Toolbar colour picker (same widget as Properties) — applies to the
    /// selection, or the whole text when nothing is selected.
    MTextColorChanged(AcadColor),
    /// Open / close the MText colour picker popup.
    MTextColorPickerToggle,
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
    /// Apply the buffer to the entity but leave the editor open (the button).
    MTextApply,
    /// Grab the resizable modal's corner grip (a drag resizes it).
    ModalResizeGrab,
    /// The shared modal body finished layout at this size.
    ModalContentResized(iced::Size),
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
    /// Candidate scope: active space or the current selection.
    QSelectSetScope(QSelectScope),
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
    /// Include or exclude objects matching the filter.
    QSelectSetMode(QSelectMode),
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
    /// Suppress menu-root tooltips between clicking the root and leaving it.
    StatusMenuTooltipHidden(bool),
    /// GitHub releases API returned a result. `Some(version)` means a
    /// newer release exists; we open the update-notice window.
    UpdateCheckResult(Option<crate::io::update_check::UpdateInfo>),
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
    // ── Plot / Export ─────────────────────────────────────────────────────
    /// Show the SVG save-file dialog and trigger export.
    PlotExport,
    /// Callback after the user picks (or cancels) the export path.
    PlotExportPath(Option<std::path::PathBuf>),
    /// User picked a paper size for the model-space window plot.
    PlotFormat(crate::io::paper_sizes::PaperSize),
    /// User picked a sheet orientation for the model-space window plot.
    PlotOrientation(crate::io::paper_sizes::Orientation),
    /// Export the pending model-space plot window (from PLOTWINDOW) to PDF.
    PlotWindowExport,
    /// Callback after the user picks (or cancels) the window-export path.
    PlotWindowExportPath(Option<std::path::PathBuf>),
    /// Completion of a PDF/preview/print job performed outside the UI thread.
    /// The boolean restores the Plot dialog after a preview.
    BackgroundIoFinished(Result<String, String>, bool),
    /// Send current layout to the system printer (via lp / lpr).
    PrintToPrinter,
    /// Callback from the async printer job.
    PrintResult(Result<String, String>),
    /// Open the full Plot / Print dialog (seeds state from the layout).
    PlotDialogOpen,
    /// An edit inside the Plot / Print dialog.
    PlotDlg(crate::ui::window::plot::PlotDlgMsg),
    /// An edit inside the docked Insert Block panel.
    BlockPalette(crate::ui::window::block_palette::BlockPaletteMsg),
    /// Open the paper-layout batch output dialog.
    PrintAllOpen,
    /// Toggle one paper layout in the batch.
    PrintAllToggle(String),
    /// Select or clear every paper layout in the batch.
    PrintAllSelectAll,
    PrintAllSelectNone,
    /// Edit the shared batch output settings in the Plot dialog.
    PrintAllOptions,
    /// Save the selected layouts as one multi-page PDF.
    PrintAllPdf,
    /// Callback after the multi-page PDF path is picked or cancelled.
    PrintAllPdfPath(Option<std::path::PathBuf>),
    /// Send the selected layouts to the configured printer as one job.
    PrintAllPrint,
    /// Completion of a Print All PDF or printer job.
    PrintAllFinished(Result<String, String>),
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
    TextStyleDialogTab(u8),
    TextStyleDialogCompare(String),
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
    TableStyleDialogTab(u8),
    TableStyleDialogCompare(String),
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
    MlStyleDialogTab(u8),
    MlStyleDialogCompare(String),
    MlStyleDialogSetCurrent,
    MlStyleApply,
    MlStyleDialogNew,
    MlStyleDialogCopy,
    MlStyleDialogDelete,
    MlStyleEdit {
        field: &'static str,
        value: String,
    },
    MlStyleToggle(&'static str),
    MlStyleElementEdit {
        index: usize,
        field: &'static str,
        value: String,
    },
    MlStyleElementAdd,
    MlStyleElementDelete(usize),
    // ── MLeaderStyle Dialog ───────────────────────────────────────────────
    MLeaderStyleDialogOpen,
    #[allow(dead_code)]
    MLeaderStyleDialogClose,
    MLeaderStyleDialogSelect(String),
    MLeaderStyleDialogTab(u8),
    MLeaderStyleDialogCompare(String),
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
    MLeaderStyleLineWeightChanged(LineWeight),
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
    /// Change the style used by the comparison summary.
    DimStyleDialogCompare(String),
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
    /// Select the mutually-exclusive tolerance presentation.
    DsToleranceMode(String),
    /// Change the feet/inches mode in a zero-suppression bit field.
    DsZeroBase(DsField, i16),
    /// Toggle leading/trailing suppression in a zero-suppression bit field.
    DsZeroFlag(DsField, i16),
    /// Select no center mark, a mark, or centerlines.
    DsCenterMarkMode(String),
    /// Toggle the expanded colour palette for a DimStyle colour field.
    DsColorMore(DsField),
    /// Open the `iced_aw` colour picker for a field and its current colour.
    OpenColorWindow(ColorPickTarget, AcadColor),
    /// Close the colour picker without choosing.
    CloseColorPicker,
    /// A colour was chosen in the `iced_aw` picker.
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
    /// Background extraction/write completion.
    WblockWriteFinished(String, std::path::PathBuf, Result<(), String>),
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
    StlExportFinished(std::path::PathBuf, Result<(), String>),
    // ── STEP export ───────────────────────────────────────────────────────
    /// Trigger STEP AP203 export: show save dialog.
    StepExport,
    /// Callback after the user picks (or cancels) the STEP save path.
    StepExportPath(Option<std::path::PathBuf>),
    StepExportFinished(std::path::PathBuf, Result<(), String>),
    // ── OBJ import ────────────────────────────────────────────────────────
    /// Trigger OBJ import: show open-file dialog.
    ObjImport,
    /// Callback after the user picks (or cancels) the OBJ file path.
    ObjImportPath(Option<std::path::PathBuf>),
    ObjImportFinished(
        u64,
        std::path::PathBuf,
        Result<crate::scene::model::mesh_model::MeshModel, String>,
    ),
}

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
                Some((
                    v.id.clone(),
                    iced::widget::image::Handle::from_bytes(bytes),
                ))
            })
            .collect();
        self.videos = videos;
    }

    fn new() -> Self {
        let config = config::AppConfig::load();
        if let Err(error) = crate::i18n::set_language(config.settings.language) {
            eprintln!("Unable to apply saved UI language: {error}");
        }
        // Boot with only the Welcome/Start tab. The user creates drawings
        // explicitly (File → New); we never auto-spawn Drawing1.
        let start_tab = DocumentTab::new_start();
        let mut app = Self {
            start: Instant::now(),
            tabs: vec![start_tab],
            active_tab: 0,
            hovered_doc_tab: None,
            tab_counter: 0,
            ribbon: Ribbon::new(),
            // Populated from the consolidated config after construction
            // (`apply_config`); default empty here.
            recent_files: Vec::new(),
            recent_thumbs: std::collections::HashMap::new(),
            recent_limit: recent::RECENT_DEFAULT,
            recent_limit_input: recent::RECENT_DEFAULT.to_string(),
            command_line: CommandLine::new(),
            patrons: Vec::new(),
            videos: Vec::new(),
            video_thumbs: std::collections::HashMap::new(),
            videos_loading: false,
            discussions: Vec::new(),
            discussions_loading: false,
            props_asym_scale: std::collections::HashSet::new(),
            start_section: StartSection::default(),
            start_action_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            props_expanded: false,
            properties_width: 250.0,
            properties_side: config::DockSide::Left,
            properties_auto_collapse: false,
            properties_hovered: false,
            properties_dragging: false,
            properties_resizing: false,
            properties_drag_last: None,
            properties_dock_preview: None,
            history_content: iced::widget::text_editor::Content::new(),
            command_history_resizing: false,
            command_history_drag_last: None,
            status_bar: StatusBar::new(),
            cursor_pos: Point::ORIGIN,
            vp_size: (1280.0, 720.0),
            win_size: (1280.0, 720.0),
            snapper: Snapper::default(),
            snap_popup_open: false,
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
            clean_screen: false,
            quick_properties: false,
            quick_properties_anchor: Point::new(12.0, 12.0),
            selection_cycling: false,
            pick_add: true,
            pick_drag_rect: false,
            perf_hud: false,
            cycle_candidates: None,
            pre_cmd_tangent: None,
            add_selected_restore: None,
            rect_suppressed_ortho: false,
            ortho_mode: false,
            polar_mode: false,
            polar_increment_deg: 45.0,
            show_grid: false,
            dyn_input: true,
            texteditmode: false,
            backup_on_save: true,
            file_assoc_enabled: true,
            savetime_min: 10,
            default_bg_color: None,
            default_paper_bg_color: None,
            awaiting_vports: false,
            pending_setvar: None,
            ucs_icon_hover: false,
            ucs_icon_selected: false,
            ucs_grip_drag: None,
            pane_move_from: None,
            dyn_user_reshaped: false,
            dyn_coord_absolute: false,
            grip_hover: None,
            grip_popup: None,
            grip_pending: None,
            visibility_popup: None,
            grip_add_provisional: None,
            grip_preview_handles: Vec::new(),
            hover_dwell: None,
            grip_originals: Vec::new(),
            grip_dirty_before: None,
            grip_text_verts: Vec::new(),
            grip_text_slide: false,
            qselect: None,
            show_ucs_icon: true,
            ucs_icon_at_origin: true,
            show_viewcube: true,
            render_bar_w: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            render_mode_menu_open: false,
            render_mode_preview: None,
            show_properties: true,
            show_block_palette: false,
            block_palette: Default::default(),
            block_palette_expanded: false,
            show_file_tabs: true,
            show_layout_tabs: true,
            last_point: None,
            main_window: None,
            color_pick_target: None,
            active_modal: None,
            find_replace: FindReplaceState::default(),
            aec_drop_acknowledged: false,
            aec_drop_count: 0,
            layer_delete_pending: None,
            modal_offset: iced::Vector::ZERO,
            modal_drag_last: None,
            modal_dragging: false,
            layer_col_dragging: false,
            layer_name_col_w: 130.0,
            modal_resize: iced::Vector::ZERO,
            modal_content_size: None,
            modal_resizing: false,
            attr_editor_handle: None,
            attr_editor_block: String::new(),
            attr_editor_rows: Vec::new(),
            attr_editor_tab: crate::ui::window::attribute_editor::AttrTab::Attribute,
            attr_editor_selected: 0,
            disabled_plugins: rustc_hash::FxHashSet::default(),
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
            marketplace_status: String::new(),
            point_size_buf: String::new(),
            point_size_relative: true,
            default_assoc_prompted: false,
            read_only: false,
            update_notice_version: None,
            update_notice_body: None,
            clipboard: Vec::new(),
            oops_cache: Vec::new(),
            clipboard_base: glam::DVec3::ZERO,
            clipboard_deps: ClipboardDeps::default(),
            shift_down: false,
            ctrl_down: false,
            cont_anchor: None,
            mtext_editor: None,
            text_inline: None,
            snap_override_popup: None,
            axis_lock_dir: None,
            layout_rename_state: None,
            last_vp_click_time: None,
            last_vp_click_pos: None,
            mtext_click_time: None,
            mtext_click_off: 0,
            mtext_click_count: 0,
            plot_window: None,
            plot_format: crate::io::paper_sizes::PaperSize::A4,
            plot_orientation: crate::io::paper_sizes::Orientation::Landscape,
            plot_dialog: crate::ui::window::plot::PlotDialogState::default(),
            plot_prev: None,
            print_all_layouts: Vec::new(),
            print_all_options: false,
            print_all_options_prev: None,
            print_all_plot_style_prev: None,
            print_all_plot_window_prev: None,
            opening: None,
            open_job_serial: 0,
            recovery_report: None,
            pending_opens: std::collections::VecDeque::new(),
            active_interaction_index: None,
            queued_interaction_indices: std::collections::VecDeque::new(),
            pending_close: None,
            pending_tab_closes: std::collections::VecDeque::new(),
            #[cfg(not(target_arch = "wasm32"))]
            active_save_jobs: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            save_job_serial: 0,
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_leases: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pending_save_failure: None,
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
            // Color scheme (default: Oxocarbon)
            active_theme: Theme::Oxocarbon,
            ui_theme: config::UiThemeConfig::default(),
            theme_color_inputs: config::UiThemePalette::default().hex_values(),
            // Keyboard shortcuts
            shortcut_bindings: rustc_hash::FxHashMap::default(),
            shortcut_editor_rows: Vec::new(),
            // Command aliases (populated from ocad.pgp just after construction)
            command_aliases: rustc_hash::FxHashMap::default(),
            alias_editor_rows: Vec::new(),
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
            anno_object_scale_target: None,
            scale_rename_buf: String::new(),
            scale_stage: None,
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
            textstyle_tab: 0,
            textstyle_compare: String::new(),
            textstyle_font: String::new(),
            textstyle_width: "1.0".to_string(),
            textstyle_oblique: "0.0".to_string(),
            textstyle_height: "0.0".to_string(),
            textstyle_bigfont: String::new(),
            textstyle_ttf: String::new(),
            // TableStyle dialog
            tablestyle_selected: "Standard".to_string(),
            tablestyle_tab: 0,
            tablestyle_compare: String::new(),
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
            mlstyle_tab: 0,
            mlstyle_compare: String::new(),
            mln_description: String::new(),
            mln_start_angle: "90".to_string(),
            mln_end_angle: "90".to_string(),
            mln_fill_color: "256".to_string(),
            mln_elements: Vec::new(),
            // MLeaderStyle dialog
            mleaderstyle_selected: "Standard".to_string(),
            mleaderstyle_tab: 0,
            mleaderstyle_compare: String::new(),
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
            dimstyle_compare: String::new(),
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
        // Restore the consolidated user config (settings.json) into live state
        // so preferences, recents, status-bar layout, ribbon density and print
        // options survive across sessions (issue #68). `last_saved_config` is
        // seeded below so the first change — not the boot — triggers a write.
        app.apply_config(config);
        // Load command aliases from ocad.pgp (writes the shipped default file
        // on first launch). The hide-set keeps aliases out of autocomplete while
        // their target command still shows.
        app.command_aliases = alias::load_aliases();
        app.command_line.command_aliases = app.command_aliases.clone();
        // Load external plugin packages from the plugins folder once, then fold
        // their ribbon tabs into the ribbon. Skipped under test/wasm.
        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            for (id, res) in crate::plugin::external::load_at_startup(&mut app) {
                if let Err(e) = res {
                    app.command_line
                        .push_error(crate::tf!("Plugin '{id}' failed to load: {e}").as_ref());
                    app.plugin_load_errors.insert(id, e);
                }
            }
            app.loaded_plugin_ids = crate::plugin::external::loaded_ids().into_iter().collect();
            app.rebuild_ribbon_modules();
        }
        app.last_saved_config = Some(app.current_config());
        app.sync_ribbon_layers();
        app
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::new()
    }

    /// Install `cmd` as the active interactive command for tab `tab`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
        // File association is no longer re-registered on every launch. It is set
        // up once via the first-launch prompt below (when the user hasn't been
        // asked yet) and afterwards managed entirely by the FILEASSOC command.
        let state = Self::new();
        let (id, open_task) = window::open(window::Settings {
            maximized: true,
            icon: window::icon::from_rgba(build_window_icon(), 32, 32).ok(),
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
        let open_main = open_task.map(|_| Message::Noop);
        let check_update = Task::perform(
            crate::io::update_check::check_for_update(),
            Message::UpdateCheckResult,
        );
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
        let cli_open: Task<Message> = if !cfg.files.is_empty() {
            Task::batch(
                cfg.files
                    .into_iter()
                    .map(|p| Task::done(Message::OpenExternal(p))),
            )
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
                assoc_prompt,
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
        let focus = s.focus_cmd_input();
        let primary_font = crate::scene::text::web_font::preload_language(
            &crate::i18n::active_language_tag(),
        );
        let fonts = Task::batch([
            Task::done(Message::PollWebFonts),
            Task::done(Message::ApplyWebFont(primary_font)),
        ]);
        // Web can't reach the Patreon API directly (CORS); fetch the CI-built
        // supporters.json served on the same origin instead.
        let patrons = Task::perform(
            crate::patreon::fetch_patrons_web(),
            Message::PatronsFetched,
        );
        s.videos_loading = true;
        let videos = Task::perform(
            crate::videos::fetch_playlist_web(),
            Message::VideosFetched,
        );
        s.discussions_loading = true;
        let discussions = Task::perform(
            crate::discussions::fetch_discussions_web(),
            Message::DiscussionsFetched,
        );
        let thumbs_fetch = s.refresh_recent_thumbs();
        (
            s,
            Task::batch([focus, fonts, patrons, videos, discussions, thumbs_fetch]),
        )
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
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
}

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
    iced::application(
        OpenCADStudio::boot_web,
        OpenCADStudio::update,
        OpenCADStudio::view_main,
    )
    .subscription(OpenCADStudio::subscription)
    .title(|_state: &OpenCADStudio| "Open CAD Studio".to_string())
    .theme(|state: &OpenCADStudio| state.active_theme.clone())
    .font(iced_aw::ICED_AW_FONT_BYTES)
    .run()
}
