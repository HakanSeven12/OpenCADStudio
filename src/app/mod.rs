mod cmd_result;
mod commands;
mod document;
mod helpers;
mod history;
mod layers;
mod properties;
mod update;
mod view;

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
    pub started: std::time::Instant,
}

/// Open multi-functional-grip popup state.
#[derive(Clone, Debug)]
pub struct GripPopup {
    pub handle: acadrust::Handle,
    pub grip_id: usize,
    pub anchor: iced::Point,
    pub items: Vec<crate::scene::object::GripMenuItem>,
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
    pub action: crate::scene::object::GripMenuAction,
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
    /// Whether Tangent snap was enabled before a tangent-pick command started.
    pre_cmd_tangent: Option<bool>,
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
    layer_window: Option<window::Id>,
    /// OS window Id of the primary application window.
    main_window: Option<window::Id>,
    // ── Floating panel windows ────────────────────────────────────────────
    page_setup_window: Option<window::Id>,
    textstyle_window: Option<window::Id>,
    tablestyle_window: Option<window::Id>,
    mlstyle_window: Option<window::Id>,
    layout_manager_window: Option<window::Id>,
    plotstyle_window: Option<window::Id>,
    dimstyle_window: Option<window::Id>,
    shortcuts_window: Option<window::Id>,
    about_window: Option<window::Id>,
    /// New-release notification window — opened on startup when the
    /// GitHub releases API reports a newer version than this build.
    update_notice_window: Option<window::Id>,
    /// Tag of the latest available release (without the leading "v"),
    /// e.g. `"0.3.0"`. `None` when up-to-date or check hasn't returned.
    update_notice_version: Option<String>,
    /// Release-notes body for the version above (GitHub release "body"
    /// markdown, as returned by the API). May be empty when the release
    /// shipped without notes.
    update_notice_body: Option<String>,
    /// In-memory clipboard: cloned entities waiting to be pasted.
    clipboard: Vec<acadrust::EntityType>,
    /// Centroid of the clipboard entities (XZ plane, Y-up).
    clipboard_centroid: glam::Vec3,
    /// Which layout tab has its context menu open (None = closed).
    layout_context_menu: Option<String>,
    /// Inline rename state: (original_name, current_edit_value).
    layout_rename_state: Option<(String, String)>,
    /// Timestamp of the previous viewport left-click release (for double-click detection).
    last_vp_click_time: Option<Instant>,
    /// Screen position of the previous viewport left-click release.
    last_vp_click_pos: Option<Point>,
    // page_setup_open: moved to page_setup_window: Option<window::Id>
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

    // ── TableStyle Dialog ─────────────────────────────────────────────────
    tablestyle_selected: String,

    // ── TextStyle Font Browser ────────────────────────────────────────────
    textstyle_selected: String,
    /// Edit buffer for font file name.
    textstyle_font: String,
    /// Edit buffer for width factor.
    textstyle_width: String,
    /// Edit buffer for oblique angle (degrees).
    textstyle_oblique: String,

    // ── Color Scheme ──────────────────────────────────────────────────────
    active_theme: Theme,

    // ── Keyboard Shortcut Editor ──────────────────────────────────────────
    /// User-defined function-key overrides: "F3" → command string.
    shortcut_overrides: std::collections::HashMap<String, String>,

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
    unsaved_dialog_window: Option<window::Id>,

    // ── Custom Save-As dialog ─────────────────────────────────────────────
    /// OS window for the custom Save As dialog.
    save_dialog_window: Option<window::Id>,
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
}

/// What triggered the "unsaved changes" dialog.
#[derive(Debug, Clone)]
pub(super) enum PendingClose {
    /// User tried to close the tab at this index.
    Tab(usize),
    /// User tried to quit the application.
    Quit,
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
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick(Instant),
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
    /// Timer pulse while the cursor is dwelling on a grip; drives the
    /// dwell-to-popup transition without requiring further mouse motion.
    GripDwellTick,
    WindowResized(f32, f32),
    /// Enter pressed globally — finalises the active command (no text-input involvement).
    CommandFinalize,
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
    AboutCopyInfo,
    // ── Quick Select / Select Similar ───────────────────────────────────
    /// Extend the current selection with every entity in the active
    /// layout that matches a selected entity by (type, layer).
    SelectSimilar,
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
    // ── TableStyle Dialog ─────────────────────────────────────────────────
    TableStyleDialogOpen,
    #[allow(dead_code)]
    TableStyleDialogClose,
    TableStyleDialogSelect(String),
    TableStyleDialogNew,
    TableStyleDialogDelete,
    // ── MLineStyle Dialog ─────────────────────────────────────────────────
    MlStyleDialogOpen,
    #[allow(dead_code)]
    MlStyleDialogClose,
    MlStyleDialogSelect(String),
    MlStyleDialogSetCurrent,
    MlStyleDialogNew,
    MlStyleDialogDelete,
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
    /// Set the selected style as the document's current dim style.
    DimStyleDialogSetCurrent,
    /// Delete the selected style.
    DimStyleDialogDelete,
    // Field edit messages:
    DsEdit(DsField, String),
    DsToggle(DsField),
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
            pre_cmd_tangent: None,
            ortho_mode: false,
            polar_mode: false,
            polar_increment_deg: 45.0,
            show_grid: false,
            dyn_input: true,
            awaiting_vports: false,
            tile_drag: None,
            dyn_user_reshaped: false,
            grip_hover: None,
            grip_popup: None,
            grip_pending: None,
            qselect: None,
            show_ucs_icon: true,
            show_viewcube: true,
            show_properties: true,
            show_file_tabs: true,
            show_layout_tabs: true,
            last_point: None,
            layer_window: None,
            main_window: None,
            page_setup_window: None,
            textstyle_window: None,
            tablestyle_window: None,
            mlstyle_window: None,
            layout_manager_window: None,
            plotstyle_window: None,
            dimstyle_window: None,
            shortcuts_window: None,
            about_window: None,
            update_notice_window: None,
            update_notice_version: None,
            update_notice_body: None,
            clipboard: Vec::new(),
            clipboard_centroid: glam::Vec3::ZERO,
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
            unsaved_dialog_window: None,
            save_dialog_window: None,
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
            shortcut_overrides: std::collections::HashMap::new(),
            // Layout Manager
            layout_manager_selected: "Model".to_string(),
            layout_manager_rename_buf: String::new(),
            plotstyle_panel_aci: 1,
            ps_color_buf: String::new(),
            ps_lineweight_buf: "255".to_string(),
            ps_screening_buf: "100".to_string(),
            // TextStyle font browser
            textstyle_selected: "Standard".to_string(),
            textstyle_font: String::new(),
            textstyle_width: "1.0".to_string(),
            textstyle_oblique: "0.0".to_string(),
            // TableStyle dialog
            tablestyle_selected: "Standard".to_string(),
            // MLineStyle dialog
            mlstyle_selected: "Standard".to_string(),
            // DimStyle dialog
            dimstyle_selected: "Standard".to_string(),
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
        };
        app.sync_ribbon_layers();
        app
    }

    /// Boot function for `iced::daemon`: returns initial state plus a task that
    /// opens the primary application window.
    fn boot() -> (Self, Task<Message>) {
        use helpers::build_window_icon;
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
        (s, Task::batch([open_main, check_update, focus_cmd]))
    }
}

use std::path::PathBuf;

pub fn run() -> iced::Result {
    iced::daemon(OpenCADStudio::boot, OpenCADStudio::update, OpenCADStudio::view)
        .subscription(OpenCADStudio::subscription)
        .title(|state: &OpenCADStudio, window_id: window::Id| {
            if Some(window_id) == state.layer_window {
                return "Layer Properties Manager".into();
            }
            if Some(window_id) == state.page_setup_window {
                return "Page Setup".into();
            }
            if Some(window_id) == state.textstyle_window {
                return "Text Style".into();
            }
            if Some(window_id) == state.tablestyle_window {
                return "Table Style".into();
            }
            if Some(window_id) == state.mlstyle_window {
                return "Multiline Style".into();
            }
            if Some(window_id) == state.layout_manager_window {
                return "Layout Manager".into();
            }
            if Some(window_id) == state.plotstyle_window {
                return "Plot Style Table Editor".into();
            }
            if Some(window_id) == state.dimstyle_window {
                return "Dimension Style Manager".into();
            }
            if Some(window_id) == state.shortcuts_window {
                return "Keyboard Shortcuts".into();
            }
            if Some(window_id) == state.about_window {
                return "About Open CAD Studio".into();
            }
            if Some(window_id) == state.update_notice_window {
                return "Update Available".into();
            }
            if Some(window_id) == state.unsaved_dialog_window {
                return "Unsaved Changes".into();
            }
            if Some(window_id) == state.save_dialog_window {
                return "Save As".into();
            }
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
