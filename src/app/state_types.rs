use super::*;
use document::types::{Color as AcadColor, LineWeight};
use iced::time::Instant;
use iced::Point;
#[cfg(not(target_arch = "wasm32"))]
use iced::Task;
use std::path::PathBuf;
use std::sync::Arc;

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
pub(in crate::app) struct BoxPreviewCache {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) geometry_epoch: u64,
    pub(in crate::app) camera_generation: u64,
    pub(in crate::app) anchor: iced::Point,
    pub(in crate::app) cursor: iced::Point,
    pub(in crate::app) crossing: bool,
    pub(in crate::app) bounds: iced::Rectangle,
    pub(in crate::app) view: glam::Mat4,
    pub(in crate::app) eye: glam::DVec3,
    pub(in crate::app) wires: Arc<Vec<crate::scene::WireModel>>,
    pub(in crate::app) index: Arc<crate::scene::pick::interaction_index::InteractionIndex>,
    pub(in crate::app) handles: rustc_hash::FxHashSet<document::Handle>,
    #[cfg(test)]
    pub(in crate::app) incremental: bool,
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

pub(in crate::app) struct SelectionCycle {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) point: Point,
    pub(in crate::app) handles: Vec<document::Handle>,
    pub(in crate::app) modifiers: Option<(bool, bool)>,
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

pub(crate) const POLY_START_DELAY_MS: u128 = 250;
/// Squared screen distance (12 px) past which a left press is a drag
/// without waiting for `POLY_START_DELAY_MS`.
#[allow(dead_code)]
pub(crate) const DRAG_START_FAST_PX2: f32 = 144.0;
pub(crate) const VARIES_LABEL: &str = "*VARIES*";

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
pub(in crate::app) struct AddSelectedRestore {
    pub(in crate::app) layer_name: String,
    pub(in crate::app) layer_handle: document::types::Handle,
    pub(in crate::app) color: AcadColor,
    pub(in crate::app) transparency: document::types::Transparency,
    pub(in crate::app) linetype_name: String,
    pub(in crate::app) linetype_handle: document::types::Handle,
    pub(in crate::app) line_weight: i16,
    pub(in crate::app) lt_scale: f64,
    pub(in crate::app) dimstyle_name: String,
    pub(in crate::app) dimstyle_handle: document::types::Handle,
    pub(in crate::app) tab_active_layer: String,
    pub(in crate::app) tab_layers_current: String,
    pub(in crate::app) ribbon_layer: String,
    pub(in crate::app) ribbon_color: AcadColor,
    pub(in crate::app) ribbon_linetype: String,
    pub(in crate::app) ribbon_lineweight: LineWeight,
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

/// What triggered the "unsaved changes" dialog.
#[derive(Debug, Clone)]
pub(crate) enum PendingClose {
    /// User tried to close the tab with this document id. Ids stay valid
    /// while the dialog is open; a vector index would go stale the moment
    /// another tab closes and Discard would remove the wrong tab.
    Tab(u64),
    /// User tried to quit the application.
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum SavePurpose {
    Manual,
    SaveAs,
    /// Control Save As without permission to replace an existing destination.
    SaveAsNew,
    Autosave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum SaveContinuation {
    None,
    CloseTab,
    Quit,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PendingNativeThumbnailSave {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) purpose: SavePurpose,
    pub(in crate::app) continuation: SaveContinuation,
    pub(in crate::app) set_current_path: bool,
    pub(in crate::app) check_external_change: bool,
    /// The user already chose "Save anyway" for this save's conversion loss.
    pub(in crate::app) allow_conversion_loss: bool,
}

/// A save waiting on the conversion-loss modal; "Save anyway" replays it.
#[derive(Debug, Clone)]
pub(crate) enum PendingConversionSave {
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
pub(crate) struct PendingWebThumbnailSave {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) filename: String,
    pub(in crate::app) ext: String,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) bounds: iced::Rectangle,
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
pub(crate) struct PendingSaveFailure {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) purpose: SavePurpose,
    pub(in crate::app) continuation: SaveContinuation,
    pub(in crate::app) set_current_path: bool,
    pub(in crate::app) error: String,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PendingExternalChange {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) purpose: SavePurpose,
    pub(in crate::app) continuation: SaveContinuation,
    pub(in crate::app) set_current_path: bool,
}

/// One tab's slot in a web SAVEALL round (`saveall_web`): the tab identity,
/// download name and race-guard markers snapshot when serialization started,
/// plus the writer result — the future fills `result` in before the
/// completion message is built. The web counterpart of native's
/// `SaveOutcome`, minus the path/continuation bookkeeping SAVEALL never had.
#[derive(Debug, Clone)]
#[cfg(target_arch = "wasm32")]
pub struct WebSaveallEntry {
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) filename: String,
    pub(in crate::app) ext: String,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) geometry_epoch: u64,
    pub(in crate::app) edit_revision: u64,
    pub(in crate::app) camera_generation: u64,
    pub(in crate::app) result: Result<Vec<u8>, String>,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub struct SaveOutcome {
    pub(in crate::app) job_id: u64,
    pub(in crate::app) tab_id: u64,
    pub(in crate::app) epoch: u64,
    pub(in crate::app) revision: u64,
    pub(in crate::app) camera_generation: u64,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) physical_path: Option<PathBuf>,
    pub(in crate::app) version: document::DxfVersion,
    pub(in crate::app) previous_autosave: Option<PathBuf>,
    pub(in crate::app) set_current_path: bool,
    pub(in crate::app) purpose: SavePurpose,
    pub(in crate::app) continuation: SaveContinuation,
    pub(in crate::app) refreshed_preview: Option<Option<document::Preview>>,
    pub(in crate::app) result: Result<(), crate::io::SaveFailure>,
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
    PluginConsent,
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
    /// The surface list of a Create Points > Surface command.
    SurfaceList,
    PointGroup,
    CorridorPoints,
    PointBlocks,
    BlockDuplicate,
    DuplicatePoint,
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
    /// The Plot dialog's Preview: the sheet as it will plot.
    PlotPreview,
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
    GeometryCleanupSettings,
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
    IfcPlan,
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

#[derive(Debug, Clone)]
pub enum SystemClipboardText {
    Text(String),
    EmptyOrUnsupported,
    Unavailable,
    Occupied,
    ConversionFailed,
}

/// MLeaderStyle dialog draft state extracted from [`OpenCADStudio`].
#[derive(Default)]
pub(in crate::app) struct MLeaderStyleDialogState {
    pub(in crate::app) color_open: Option<&'static str>,
    pub(in crate::app) landing_distance: String,
    pub(in crate::app) landing_gap: String,
    pub(in crate::app) arrowhead_size: String,
    pub(in crate::app) text_height: String,
    pub(in crate::app) scale_factor: String,
    pub(in crate::app) break_gap: String,
    pub(in crate::app) max_points: String,
    pub(in crate::app) default_text: String,
    pub(in crate::app) line_color: String,
    pub(in crate::app) text_color: String,
    pub(in crate::app) block_color: String,
    pub(in crate::app) block_scale: String,
}

/// TableStyle dialog draft state extracted from [`OpenCADStudio`].
pub(in crate::app) struct TableStyleDialogState {
    pub(in crate::app) description: String,
    pub(in crate::app) cell: String,
    pub(in crate::app) cell_height: String,
    pub(in crate::app) cell_angle: String,
    pub(in crate::app) cell_hmargin: String,
    pub(in crate::app) cell_vmargin: String,
    pub(in crate::app) border: crate::ui::style::tablestyle::BorderDraft,
    pub(in crate::app) cards: crate::ui::style::tablestyle::TsCards,
    pub(in crate::app) format_return: bool,
    pub(in crate::app) text_return: bool,
    pub(in crate::app) color_open: Option<&'static str>,
}

impl Default for TableStyleDialogState {
    fn default() -> Self {
        Self {
            description: String::new(),
            cell: document::objects::DATA_CELL_STYLE.to_string(),
            cell_height: String::new(),
            cell_angle: String::new(),
            cell_hmargin: String::new(),
            cell_vmargin: String::new(),
            border: Default::default(),
            cards: Default::default(),
            format_return: false,
            text_return: false,
            color_open: None,
        }
    }
}

/// Annotation scale popup & manager state extracted from [`OpenCADStudio`].
#[derive(Default)]
pub(in crate::app) struct ScaleManagerState {
    pub(in crate::app) popup_open: bool,
    pub(in crate::app) selected: String,
    pub(in crate::app) paper_buf: String,
    pub(in crate::app) drawing_buf: String,
    pub(in crate::app) rename: Option<String>,
    pub(in crate::app) rename_buf: String,
    pub(in crate::app) edit_adding: bool,
    pub(in crate::app) edit_error: Option<String>,
    pub(in crate::app) multi: Vec<String>,
    pub(in crate::app) delete_asking: bool,
    pub(in crate::app) reset_asking: bool,
    pub(in crate::app) stage: Option<crate::app::style_ops::ScaleStage>,
}

/// SpaceMouse dialog & view-pivot state extracted from [`OpenCADStudio`].
#[derive(Default)]
pub(in crate::app) struct SpaceMouseUiState {
    pub(in crate::app) preferences: crate::input::spacemouse::Preferences,
    pub(in crate::app) paused: bool,
    pub(in crate::app) focused: bool,
    pub(in crate::app) details: bool,
    pub(in crate::app) was_moving: bool,
    pub(in crate::app) pivot: Option<(crate::input::spacemouse::Target, glam::DVec3)>,
    pub(in crate::app) selection: super::navigation::SelectionCache,
}

/// Enhanced Attribute Editor dialog state extracted from [`OpenCADStudio`].
pub(in crate::app) struct AttrEditorUiState {
    pub(in crate::app) handle: Option<document::Handle>,
    pub(in crate::app) block: String,
    pub(in crate::app) rows: Vec<crate::ui::window::attribute_editor::AttrRow>,
    pub(in crate::app) tab: crate::ui::window::attribute_editor::AttrTab,
    pub(in crate::app) selected: usize,
    pub(in crate::app) session: Option<super::attr_editor::AttrEditorSession>,
    pub(in crate::app) error: Option<String>,
    pub(in crate::app) asking: bool,
    pub(in crate::app) editing: Option<crate::ui::window::attribute_editor::AttrField>,
}

impl Default for AttrEditorUiState {
    fn default() -> Self {
        Self {
            handle: None,
            block: String::new(),
            rows: Vec::new(),
            tab: crate::ui::window::attribute_editor::AttrTab::Attribute,
            selected: 0,
            session: None,
            error: None,
            asking: false,
            editing: None,
        }
    }
}

/// Browser OPFS autosave coordination state extracted from [`OpenCADStudio`].
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub(in crate::app) struct WebAutosaveState {
    pub(in crate::app) in_flight: bool,
    pub(in crate::app) warned: bool,
    pub(in crate::app) pending: Vec<crate::io::web_autosave::WebAutosaveEntry>,
    pub(in crate::app) restore_all_remaining: usize,
    pub(in crate::app) consumed: Option<(String, u64)>,
    pub(in crate::app) clear_after: Vec<String>,
}
