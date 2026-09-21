use iced::time::Instant;
use iced::Point;

/// Mouse / selection interaction state for the viewport.
#[derive(Clone, Default)]
pub struct SelectionState {
    pub vp_size: (f32, f32),
    pub box_anchor: Option<Point>,
    /// World point under the box-selection anchor, so the anchor can be
    /// re-projected to screen when the camera zooms/pans mid-drag instead of
    /// staying frozen at its original pixel (which selected the wrong area).
    /// (#234)
    pub box_anchor_world: Option<glam::DVec3>,
    pub box_current: Option<Point>,
    pub box_last: Option<(Point, Point)>,
    pub box_crossing: bool,
    /// Set when a Window / Crossing selection keyword fixed the sense of the
    /// box being drawn. Dragging normally decides it from the direction the
    /// corner travels, which would immediately overwrite what the user just
    /// asked for, so that derivation stands down while this holds.
    pub box_crossing_locked: bool,
    pub box_last_crossing: bool,
    /// A preview-only selection marquee `(anchor, current, crossing)` in canvas
    /// pixels, drawn identically to a real box-selection (green crossing fill /
    /// blue window fill) but with NO hit-test behaviour. Commands that pick a
    /// window by point (STRETCH's crossing window) set this so the region reads
    /// like a normal selection instead of a bare outline. (#291)
    pub preview_box: Option<(Point, Point, bool)>,
    pub poly_active: bool,
    pub poly_points: Vec<Point>,
    pub poly_crossing: bool,
    pub poly_last_crossing: bool,
    /// Canvas position the right-click context menu is anchored at while it
    /// is open. `None` = closed. Its rows are rebuilt from app state every
    /// frame (`ui::popup::context_menu::build_context_menu`).
    pub context_menu: Option<Point>,
    /// Transient UI state of the open context menu (expanded submenu,
    /// keyboard highlight). Reset every time the menu opens.
    pub context_menu_ui: ContextMenuUi,
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
    /// World point the current orbit drag revolves around (selection or model
    /// centre), captured when the drag starts so it stays fixed for the whole
    /// gesture. `None` when no orbit is in progress. (#229)
    pub orbit_pivot: Option<glam::DVec3>,
    /// While a command is active, a right-click acts as Enter; the *next*
    /// consecutive right-click opens the context menu instead. This tracks
    /// whether the previous right-click already fired Enter. Reset by any
    /// other interaction (left-click pick, a new command) and on viewport exit.
    pub right_click_entered: bool,
    pub middle_down: bool,
    pub middle_last_pos: Option<Point>,
    pub middle_last_press_time: Option<Instant>,
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
    /// Open the context menu at `at`, discarding the previous menu's
    /// expanded-submenu / highlight state.
    pub fn open_context_menu(&mut self, at: Point) {
        self.context_menu = Some(at);
        self.context_menu_ui = ContextMenuUi::default();
    }

    /// Close the context menu (no-op when it is not open).
    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
        self.context_menu_ui = ContextMenuUi::default();
    }

    /// End every left-button selection gesture without disturbing the previous
    /// completed-window record or command-owned preview marquee. Grip editing
    /// owns the left button while engaged and calls this before/after placement
    /// so a small pointer move cannot also arm a box or lasso selection.
    pub fn clear_left_selection_gesture(&mut self) {
        self.left_down = false;
        self.left_press_pos = None;
        self.left_press_time = None;
        self.left_dragging = false;
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

