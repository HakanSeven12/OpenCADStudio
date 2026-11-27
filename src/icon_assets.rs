//! The SVG bytes of the application's icons. How they are drawn (themed,
//! tinted, cached) is `ui::icons`; the tools name their ribbon icons from here.

pub(crate) static TRI_DOWN: &[u8] = include_bytes!("../assets/icons/ui/tri_down.svg");

pub(crate) static TRI_UP: &[u8] = include_bytes!("../assets/icons/ui/tri_up.svg");

pub(crate) static TRI_RIGHT: &[u8] = include_bytes!("../assets/icons/ui/tri_right.svg");

pub(crate) static TRI_LEFT: &[u8] = include_bytes!("../assets/icons/ui/tri_left.svg");

pub(crate) static HOME: &[u8] = include_bytes!("../assets/icons/ui/home.svg");

pub static UNDO: &[u8] = include_bytes!("../assets/icons/ui/undo.svg");

pub(crate) static REDO: &[u8] = include_bytes!("../assets/icons/ui/redo.svg");

// OSNAP marker symbols. Rendered as SVG (not Unicode glyphs) so the snap menu
// shows the right shapes on the web build, whose bundled Fira Sans lacks the
// geometric glyphs and rendered them as tofu boxes. (#138)
pub(crate) static OSNAP_ENDPOINT: &[u8] = include_bytes!("../assets/icons/osnap/endpoint.svg");

pub(crate) static OSNAP_MIDPOINT: &[u8] = include_bytes!("../assets/icons/osnap/midpoint.svg");

pub(crate) static OSNAP_CENTER: &[u8] = include_bytes!("../assets/icons/osnap/center.svg");

pub(crate) static OSNAP_GEOMETRIC_CENTER: &[u8] =
    include_bytes!("../assets/icons/osnap/geometric_center.svg");

pub(crate) static OSNAP_NODE: &[u8] = include_bytes!("../assets/icons/osnap/node.svg");

pub(crate) static OSNAP_QUADRANT: &[u8] = include_bytes!("../assets/icons/osnap/quadrant.svg");

pub(crate) static OSNAP_INTERSECTION: &[u8] = include_bytes!("../assets/icons/osnap/intersection.svg");

pub(crate) static OSNAP_EXTENSION: &[u8] = include_bytes!("../assets/icons/osnap/extension.svg");

pub(crate) static OSNAP_INSERTION: &[u8] = include_bytes!("../assets/icons/osnap/insertion.svg");

pub(crate) static OSNAP_PERPENDICULAR: &[u8] = include_bytes!("../assets/icons/osnap/perpendicular.svg");

pub(crate) static OSNAP_TANGENT: &[u8] = include_bytes!("../assets/icons/osnap/tangent.svg");

pub(crate) static OSNAP_NEAREST: &[u8] = include_bytes!("../assets/icons/osnap/nearest.svg");

pub(crate) static OSNAP_APPARENT: &[u8] = include_bytes!("../assets/icons/osnap/apparent.svg");

pub(crate) static OSNAP_PARALLEL: &[u8] = include_bytes!("../assets/icons/osnap/parallel.svg");

pub(crate) static OSNAP_GRID: &[u8] = include_bytes!("../assets/icons/osnap/grid.svg");

pub(crate) static OSNAP_MTP: &[u8] = include_bytes!("../assets/icons/osnap/mtp.svg");

pub(crate) static NAV_PAN: &[u8] = include_bytes!("../assets/icons/pan.svg");

pub(crate) static NAV_ZOOM: &[u8] = include_bytes!("../assets/icons/zoom_in.svg");

pub(crate) static LAY_ON: &[u8] = include_bytes!("../assets/icons/layers/state_on.svg");

pub(crate) static LAY_OFF: &[u8] = include_bytes!("../assets/icons/layers/state_off.svg");

pub(crate) static LAY_FRZ: &[u8] = include_bytes!("../assets/icons/layers/state_freeze.svg");

pub(crate) static LAY_THW: &[u8] = include_bytes!("../assets/icons/layers/state_thaw.svg");

pub(crate) static LAY_VP_FRZ: &[u8] = include_bytes!("../assets/icons/layers/layvpfrz.svg");

pub(crate) static LAY_VP_THW: &[u8] = include_bytes!("../assets/icons/layers/layvpthw.svg");

pub(crate) static LAY_LCK: &[u8] = include_bytes!("../assets/icons/layers/state_lock.svg");

pub(crate) static LAY_ULK: &[u8] = include_bytes!("../assets/icons/layers/state_unlock.svg");

// Monochrome chrome glyphs (replace Unicode glyphs in buttons / menus / toolbars).
// All are black-on-transparent; recolour them at the call site with [`tinted`].
pub static CHECK: &[u8] = include_bytes!("../assets/icons/ui/check.svg");

pub static CLOSE: &[u8] = include_bytes!("../assets/icons/ui/close.svg");

pub static EYEDROPPER: &[u8] = include_bytes!("../assets/icons/ui/eyedropper.svg");

pub static PLUS: &[u8] = include_bytes!("../assets/icons/ui/plus.svg");

pub static IMPORT: &[u8] = include_bytes!("../assets/icons/ui/import.svg");

pub static REFRESH: &[u8] = include_bytes!("../assets/icons/ui/refresh.svg");

pub static SWAP: &[u8] = include_bytes!("../assets/icons/ui/swap.svg");

pub static MINUS: &[u8] = include_bytes!("../assets/icons/ui/minus.svg");

pub static TRASH: &[u8] = include_bytes!("../assets/icons/ui/trash.svg");

pub static NODE_GRAPH: &[u8] = include_bytes!("../assets/icons/ui/node_graph.svg");

pub static COPY: &[u8] = include_bytes!("../assets/icons/ui/copy.svg");

pub static MENU: &[u8] = include_bytes!("../assets/icons/ui/menu.svg");

pub static MOVE: &[u8] = include_bytes!("../assets/icons/ui/move.svg");

pub static RESIZE: &[u8] = include_bytes!("../assets/icons/ui/resize.svg");

/// Move a dialog or pallet into a window of its own.
pub static DETACH: &[u8] = include_bytes!("../assets/icons/ui/window_detach.svg");

/// Bring a window of its own back into the main window.
pub static DOCK_BACK: &[u8] = include_bytes!("../assets/icons/ui/window_dock_back.svg");

pub static PIN: &[u8] = include_bytes!("../assets/icons/ui/pin.svg");

/// `PIN` tilted 30°: the pin while auto-hide is on.
pub static PIN_ACTIVE: &[u8] = include_bytes!("../assets/icons/ui/pin_active.svg");

/// `PIN` turned 90°: a docked palette's pin while it auto-hides.
pub static PIN_SIDE: &[u8] = include_bytes!("../assets/icons/ui/pin_side.svg");

pub static SPLIT_V: &[u8] = include_bytes!("../assets/icons/ui/split_v.svg");

pub static SPLIT_H: &[u8] = include_bytes!("../assets/icons/ui/split_h.svg");

pub static GRID: &[u8] = include_bytes!("../assets/icons/ui/grid.svg");

pub static LIST: &[u8] = include_bytes!("../assets/icons/ui/list.svg");

pub static SEARCH: &[u8] = include_bytes!("../assets/icons/ui/search.svg");

pub static PREVIEW: &[u8] = include_bytes!("../assets/icons/ui/preview.svg");

pub static SNAP: &[u8] = include_bytes!("../assets/icons/ui/snap.svg");

pub static DOC_NEW: &[u8] = include_bytes!("../assets/icons/ui/doc_new.svg");

pub static DOC: &[u8] = include_bytes!("../assets/icons/ui/doc.svg");

pub static FOLDER_OPEN: &[u8] = include_bytes!("../assets/icons/ui/folder_open.svg");

/// A folder over a tree of files: the Files palette.
pub static FOLDER_TREE: &[u8] = include_bytes!("../assets/icons/ui/folder_tree.svg");

pub static SAVE: &[u8] = include_bytes!("../assets/icons/ui/save.svg");

pub static FILE_EXPORT: &[u8] = include_bytes!("../assets/icons/ui/file_export.svg");

pub static PRINT: &[u8] = include_bytes!("../assets/icons/ui/print.svg");

pub static HEART: &[u8] = include_bytes!("../assets/icons/ui/heart.svg");

pub static GEAR: &[u8] = include_bytes!("../assets/icons/ui/gear.svg");
/// The Ribbon and Toolbar layouts, as Interface Settings offers them.
pub static LAYOUT_RIBBON: &[u8] = include_bytes!("../assets/icons/ui/layout_ribbon.svg");
pub static LAYOUT_TOOLBAR: &[u8] = include_bytes!("../assets/icons/ui/layout_toolbar.svg");

pub static DOT: &[u8] = include_bytes!("../assets/icons/ui/dot.svg");

pub static DIRTY_DOT: &[u8] = include_bytes!("../assets/icons/ui/dirty_dot.svg");

pub static ARROW_LONG_RIGHT: &[u8] = include_bytes!("../assets/icons/ui/arrow_long_right.svg");

pub static MORE: &[u8] = include_bytes!("../assets/icons/ui/more.svg");

/// A docked side panel outline with the sidebar field filled: the generic
/// icon of plugin-registered pallets (`PanelId::Plugin`).
pub static PLUGIN_PANEL: &[u8] = include_bytes!("../assets/icons/ui/plugin_panel.svg");

pub static INFO: &[u8] = include_bytes!("../assets/icons/ui/info.svg");

pub static MESH: &[u8] = include_bytes!("../assets/icons/ui/mesh.svg");

pub static ALERT: &[u8] = include_bytes!("../assets/icons/ui/alert.svg");

pub static WARNING: &[u8] = include_bytes!("../assets/icons/ui/warning.svg");

pub static IMAGE: &[u8] = include_bytes!("../assets/icons/ui/image.svg");

pub static LOCK: &[u8] = include_bytes!("../assets/icons/ui/lock.svg");

pub static EYE: &[u8] = include_bytes!("../assets/icons/ui/eye.svg");

pub static EYE_OFF: &[u8] = include_bytes!("../assets/icons/ui/eye_off.svg");

/// The eye in blue: show everything (Show All).
pub static EYE_ALL: &[u8] = include_bytes!("../assets/icons/ui/eye_all.svg");

/// The tool palette with the authoring pencil: Authoring Palettes
/// (BAUTHORPALETTE), apart from Tool Palettes.
pub static AUTHORING_PALETTES: &[u8] = include_bytes!("../assets/icons/authoring_palettes.svg");

// ── Status-bar toggle icons (issue #216) ──────────────────────────────────
pub static ST_ORTHO: &[u8] = include_bytes!("../assets/icons/status/ortho.svg");

pub static ST_POLAR: &[u8] = include_bytes!("../assets/icons/status/polar.svg");

pub static ST_OSNAP: &[u8] = include_bytes!("../assets/icons/status/osnap.svg");

pub static ST_OTRACK: &[u8] = include_bytes!("../assets/icons/status/otrack.svg");

pub static ST_DYN: &[u8] = include_bytes!("../assets/icons/status/dyn.svg");

pub static ST_DUCS: &[u8] = include_bytes!("../assets/icons/status/ducs.svg");

pub static ST_LWT: &[u8] = include_bytes!("../assets/icons/status/lwt.svg");

pub static ST_TRANSPARENCY: &[u8] = include_bytes!("../assets/icons/status/transparency.svg");

pub static ST_ISOLATE: &[u8] = include_bytes!("../assets/icons/status/isolate.svg");

pub static ST_QUICKPROPS: &[u8] = include_bytes!("../assets/icons/status/quickprops.svg");

pub static ST_FILTER: &[u8] = include_bytes!("../assets/icons/status/filter.svg");

pub static ST_SELCYCLE: &[u8] = include_bytes!("../assets/icons/status/selcycle.svg");

pub static ST_CLEANSCREEN: &[u8] = include_bytes!("../assets/icons/status/cleanscreen.svg");

/// The application logo, as the window icon and the About dialog show it.
pub(crate) const LOGO: &[u8] = include_bytes!("../assets/logo.svg");

/// MTP menu icon: modal 2-pick modifier, not a persistent `SnapType` mode.
pub fn mtp_icon() -> &'static [u8] {
    OSNAP_MTP
}

/// Base marker and offset arrow for the FROM point modifier.
pub fn from_icon() -> &'static [u8] {
    br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#B4B6B9" stroke-width="1.5">
<circle cx="5" cy="18" r="3"/><path d="M7 16L19 4M12 4h7v7"/>
</svg>"##
}

pub fn undo_icon() -> &'static [u8] {
    UNDO
}

pub fn redo_icon() -> &'static [u8] {
    REDO
}
