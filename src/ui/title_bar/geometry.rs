//! Where the window buttons and the resize edges are in a window's client
//! pixels, for the system's hit test on Windows ([`super::windows`]). Plain
//! arithmetic, so it is tested on every platform.

use crate::ui::style::ribbon_metrics as metrics;

/// A rectangle in client pixels; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// An edge or corner of the window, where a press resizes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
    Top,
    Bottom,
}

/// The Windows-style maximize button in a `client` area at `scale` (the
/// monitor's DPI times the app's scale): second from the right, as high as
/// the bar.
pub fn maximize_button(client: Rect, scale: f32) -> Rect {
    let w = (metrics::CAPTION_W * scale).round() as i32;
    // Down, not to the nearest: the hit box never reaches past the drawn bar.
    let h = (metrics::TITLE_BAR_H_WINDOWS * scale).floor() as i32;
    Rect {
        left: client.right - 2 * w,
        top: client.top,
        right: client.right - w,
        bottom: client.top + h,
    }
}

/// The edge or corner of a restored window under (`x`, `y`); `None` inside.
/// Corners win over edges.
pub fn edge(client: Rect, scale: f32, x: i32, y: i32) -> Option<Edge> {
    let e = (metrics::RESIZE_EDGE * scale).round().max(1.0) as i32;
    let left = x < client.left + e;
    let right = x >= client.right - e;
    let top = y < client.top + e;
    let bottom = y >= client.bottom - e;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => Edge::TopLeft,
        (_, true, true, _) => Edge::TopRight,
        (true, _, _, true) => Edge::BottomLeft,
        (_, true, _, true) => Edge::BottomRight,
        (true, ..) => Edge::Left,
        (_, true, ..) => Edge::Right,
        (_, _, true, _) => Edge::Top,
        (_, _, _, true) => Edge::Bottom,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{edge, maximize_button, Edge, Rect};

    const CLIENT: Rect = Rect {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 700,
    };

    #[test]
    fn the_maximize_button_is_second_from_the_right() {
        let at_100 = maximize_button(CLIENT, 1.0);
        assert_eq!(
            at_100,
            Rect {
                left: 1000 - 92,
                top: 0,
                right: 1000 - 46,
                bottom: 29
            }
        );
        // 150 %: 46 × 1.5 = 69 px a button, 29 × 1.5 = 43.5 → 43 px high.
        let at_150 = maximize_button(CLIENT, 1.5);
        assert_eq!((at_150.left, at_150.right, at_150.bottom), (862, 931, 43));
        assert!(at_150.contains(900, 10));
        assert!(
            !at_150.contains(931, 10),
            "the right edge is the close button's"
        );
    }

    #[test]
    fn corners_win_over_edges() {
        assert_eq!(edge(CLIENT, 1.0, 0, 0), Some(Edge::TopLeft));
        assert_eq!(edge(CLIENT, 1.0, 999, 2), Some(Edge::TopRight));
        assert_eq!(edge(CLIENT, 1.0, 3, 699), Some(Edge::BottomLeft));
        assert_eq!(edge(CLIENT, 1.0, 996, 696), Some(Edge::BottomRight));
        assert_eq!(edge(CLIENT, 1.0, 2, 300), Some(Edge::Left));
        assert_eq!(edge(CLIENT, 1.0, 997, 300), Some(Edge::Right));
        assert_eq!(edge(CLIENT, 1.0, 500, 4), Some(Edge::Top));
        assert_eq!(edge(CLIENT, 1.0, 500, 698), Some(Edge::Bottom));
        assert_eq!(edge(CLIENT, 1.0, 500, 5), None, "5 px in is inside");
        // 200 %: the edge is 10 px.
        assert_eq!(edge(CLIENT, 2.0, 500, 9), Some(Edge::Top));
        assert_eq!(edge(CLIENT, 2.0, 500, 10), None);
    }
}
