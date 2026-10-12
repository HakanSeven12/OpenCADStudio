use iced::time::Instant;
use iced::Point;

/// Mouse / selection interaction state for the viewport.
///
/// Split (#32) into five sub-structs by family so a per-frame clone can copy
/// only the family it needs and gesture clears touch one family at a time.
/// The `RefCell` around this struct (`Scene::selection`) is untouched.
#[derive(Clone, Default)]
pub struct SelectionState {
    pub view: SelectionView,
    pub gesture: SelectionGesture,
    pub input: SelectionInput,
    pub menu: SelectionMenu,
    pub orbit: SelectionOrbit,
}

/// View-dependent snapshot: the canvas size selection geometry is expressed in.
#[derive(Clone, Default)]
pub struct SelectionView {
    pub vp_size: (f32, f32),
}

/// One box-or-lasso gesture: anchor/current corners, crossing sense, and the
/// in-progress polygon. Cleared together by [`SelectionGesture::clear_left`].
#[derive(Clone, Default)]
pub struct SelectionGesture {
    pub box_anchor: Option<Point>,
    /// World point under the box-selection anchor, so the anchor can be
    /// re-projected to screen when the camera zooms/pans mid-drag instead of
    /// staying frozen at its original pixel (which selected the wrong area).
    /// (#234)
    pub box_anchor_world: Option<glam::DVec3>,
    pub box_current: Option<Point>,
    pub box_crossing: bool,
    /// Set when a Window / Crossing selection keyword fixed the sense of the
    /// box being drawn. Dragging normally decides it from the direction the
    /// corner travels, which would immediately overwrite what the user just
    /// asked for, so that derivation stands down while this holds.
    pub box_crossing_locked: bool,
    /// A preview-only selection marquee `(anchor, current, crossing)` in canvas
    /// pixels, drawn identically to a real box-selection (green crossing fill /
    /// blue window fill) but with NO hit-test behaviour. Commands that pick a
    /// window by point (STRETCH's crossing window) set this so the region reads
    /// like a normal selection instead of a bare outline. (#291)
    pub preview_box: Option<(Point, Point, bool)>,
    pub poly_active: bool,
    pub poly_points: Vec<Point>,
    pub poly_crossing: bool,
}

impl SelectionGesture {
    /// End the in-progress box/lasso gesture (left-button release path).
    pub fn clear_left(&mut self) {
        self.box_anchor = None;
        self.box_anchor_world = None;
        self.box_current = None;
        self.box_crossing = false;
        self.box_crossing_locked = false;
        self.poly_active = false;
        self.poly_points.clear();
        self.poly_crossing = false;
    }
}

/// Raw button/pointer state: press positions, drag flags, orbit pivot input.
#[derive(Clone, Default)]
pub struct SelectionInput {
    pub last_move_pos: Option<Point>,
    pub left_down: bool,
    pub left_press_pos: Option<Point>,
    pub left_press_time: Option<Instant>,
    pub left_dragging: bool,
    pub right_down: bool,
    pub right_press_pos: Option<Point>,
    pub right_press_time: Option<Instant>,
    pub right_dragging: bool,
    pub right_last_pos: Option<Point>,
    /// While a command is active, a right-click acts as Enter; the *next*
    /// consecutive right-click opens the context menu instead. This tracks
    /// whether the previous right-click already fired Enter. Reset by any
    /// other interaction (left-click pick, a new command) and on viewport exit.
    pub right_click_entered: bool,
    pub middle_down: bool,
    pub middle_last_pos: Option<Point>,
    pub middle_last_press_time: Option<Instant>,
    /// Which way a ZOOM Dynamic drag is currently zooming: `true` while it is
    /// zooming out. Only the magnifier cursor reads it. The sign is latched
    /// rather than taken from the live `dy` so that the sub-pixel jitter of a
    /// slow drag cannot strobe the `+` / `−` glyph.
    pub zoom_dir_out: bool,
}

impl SelectionInput {
    /// Release the left button (click-commit path).
    pub fn clear_left_buttons(&mut self) {
        self.left_down = false;
        self.left_press_pos = None;
        self.left_press_time = None;
        self.left_dragging = false;
    }
}

/// Right-click context-menu state.
#[derive(Clone, Default)]
pub struct SelectionMenu {
    /// Canvas position the right-click context menu is anchored at while it
    /// is open. `None` = closed. Its rows are rebuilt from app state every
    /// frame (`ui::popup::context_menu::build_context_menu`).
    pub open_at: Option<Point>,
    /// Transient UI state of the open context menu (expanded submenu,
    /// keyboard highlight). Reset every time the menu opens.
    pub ui: ContextMenuUi,
}

impl SelectionMenu {
    /// Open the context menu at `at`, discarding the previous menu's
    /// expanded-submenu / highlight state.
    pub fn open(&mut self, at: Point) {
        self.open_at = Some(at);
        self.ui = ContextMenuUi::default();
    }

    /// Close the context menu (no-op when it is not open).
    pub fn close(&mut self) {
        self.open_at = None;
        self.ui = ContextMenuUi::default();
    }
}

/// Orbit-drag state.
#[derive(Clone, Default)]
pub struct SelectionOrbit {
    /// World point the current orbit drag revolves around (selection or model
    /// centre), captured when the drag starts so it stays fixed for the whole
    /// gesture. `None` when no orbit is in progress. (#229)
    pub pivot: Option<glam::DVec3>,
}

/// Transient state of the open right-click context menu.
#[derive(Clone, Default)]
pub struct ContextMenuUi {
    /// The accordion submenu currently expanded (at most one).
    pub open_submenu: Option<crate::ui::popup::context_menu::SubmenuId>,
    /// Keyboard highlight as an index into `ContextMenu::selectable()`.
    /// `None` until an arrow key / mnemonic is pressed, so a mouse user is
    /// never shown a highlighted row that is not under the pointer.
    pub highlighted: Option<usize>,
}

impl SelectionState {
    /// End every left-button selection gesture without disturbing the previous
    /// completed-window record or command-owned preview marquee. Grip editing
    /// owns the left button while engaged and calls this before/after placement
    /// so a small pointer move cannot also arm a box or lasso selection.
    pub fn clear_left_selection_gesture(&mut self) {
        self.input.clear_left_buttons();
        self.gesture.clear_left();
    }
}
