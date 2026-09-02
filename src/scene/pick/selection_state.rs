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
    pub context_menu: Option<Point>,
    /// True while the context menu's Draw Order sub-items are expanded.
    pub draworder_submenu: bool,
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

impl SelectionState {
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

#[cfg(test)]
mod bench_selection_clone_tests {
    use super::SelectionState;
    use iced::Point;
    use std::hint::black_box;
    use std::sync::Arc;
    use std::time::Instant;

    fn make_state(poly_len: usize) -> SelectionState {
        let mut s = SelectionState {
            vp_size: (1920.0, 1080.0),
            poly_points: vec![Point::new(10.0, 10.0); poly_len],
            ..Default::default()
        };
        s.box_anchor = Some(Point::new(0.0, 0.0));
        s.box_current = Some(Point::new(100.0, 100.0));
        s
    }

    #[test]
    #[ignore]
    fn bench_selection_state_clone() {
        let state = make_state(64);
        for _ in 0..20 {
            black_box(state.clone());
        }
        let n = 5000u32;
        let start = Instant::now();
        for _ in 0..n {
            black_box(state.clone());
        }
        let elapsed = start.elapsed();
        let per = elapsed / n;
        println!(
            "SelectionState::clone (deep, 64 pts): {:?} per clone (n={}, total {:?})",
            per, n, elapsed
        );
        assert!(per.as_secs_f64() > 0.0);
    }

    #[test]
    #[ignore]
    fn bench_selection_arc_clone() {
        let state = Arc::new(make_state(64));
        for _ in 0..20 {
            black_box(Arc::clone(&state));
        }
        let n = 5000u32;
        let start = Instant::now();
        for _ in 0..n {
            black_box(Arc::clone(&state));
        }
        let elapsed = start.elapsed();
        let per = elapsed / n;
        println!(
            "Arc<SelectionState>::clone (Arc bump, 64 pts): {:?} per clone (n={}, total {:?})",
            per, n, elapsed
        );
        assert!(per.as_secs_f64() > 0.0);
    }

    #[test]
    #[ignore]
    fn bench_selection_state_clone_empty() {
        let state = make_state(0);
        for _ in 0..20 {
            black_box(state.clone());
        }
        let n = 5000u32;
        let start = Instant::now();
        for _ in 0..n {
            black_box(state.clone());
        }
        let elapsed = start.elapsed();
        let per = elapsed / n;
        println!(
            "SelectionState::clone (deep, 0 pts): {:?} per clone (n={}, total {:?})",
            per, n, elapsed
        );
        assert!(per.as_secs_f64() > 0.0);
    }
}
