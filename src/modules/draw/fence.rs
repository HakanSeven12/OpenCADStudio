// A path picked point by point, and the way it is drawn while it is being
// picked.
//
// Two features ask the user for the same gesture. TRIM and EXTEND take a fence
// through the drawing and cut whatever it crosses; the Fence / WPolygon /
// CPolygon selection modes take one and select whatever it crosses or encloses.
// Only what happens to the result differs, so the gesture itself — the points,
// the rubber-banded dashed preview, and whether the path closes back on itself
// — lives here and both bind to it.

use crate::scene::model::wire_model::WireModel;

/// Dash rhythm shared by every in-progress selection gesture, so a fence being
/// picked reads the same wherever it is offered.
const DASH_LENGTH: f32 = 0.8;
const DASH_PATTERN: [f32; 8] = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

/// The crossing-box green, matching the selection marquee the option imitates.
const CROSSING_GREEN: [f32; 4] = [0.35, 0.85, 0.35, 0.9];

/// A path being picked one point at a time.
#[derive(Clone, Debug, Default)]
pub struct FencePick {
    /// Points committed so far, in drawing coordinates.
    pub points: Vec<[f64; 2]>,
    /// Whether the path closes back to its first point. A polygon encloses an
    /// area and can be asked what lies inside it; an open fence only ever
    /// answers what it cuts.
    pub closed: bool,
}

impl FencePick {
    /// An open fence: it selects what it crosses, and encloses nothing.
    pub fn fence() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
        }
    }

    /// A closed polygon, which encloses an area as well as cutting through one.
    pub fn polygon() -> Self {
        Self {
            points: Vec::new(),
            closed: true,
        }
    }

    pub fn push(&mut self, point: [f64; 2]) {
        self.points.push(point);
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Enough points to mean anything: two for a fence to cut with, three for a
    /// polygon to enclose with.
    pub fn is_usable(&self) -> bool {
        self.points.len() >= if self.closed { 3 } else { 2 }
    }

    /// The picked path, brought back to its first point when it is a polygon.
    pub fn path(&self) -> Vec<[f64; 2]> {
        let mut path = self.points.clone();
        if self.closed {
            if let Some(first) = self.points.first().copied() {
                path.push(first);
            }
        }
        path
    }

    /// The path so far, trailing the cursor, dashed.
    pub fn preview(&self, cursor: [f64; 2], name: &str) -> WireModel {
        self.preview_styled(cursor, name, true)
    }

    /// The path so far, trailing the cursor: dashed, or solid for a window
    /// polygon, which takes only what lies wholly inside, as the reference
    /// draws it.
    pub fn preview_styled(&self, cursor: [f64; 2], name: &str, dashed_line: bool) -> WireModel {
        let mut path = self.points.clone();
        path.push(cursor);
        if self.closed {
            if let Some(first) = self.points.first().copied() {
                path.push(first);
            }
        }
        if dashed_line {
            return dashed(path, name, WireModel::CYAN);
        }
        let points: Vec<[f32; 3]> = path
            .into_iter()
            .map(|p| [p[0] as f32, p[1] as f32, 0.0])
            .collect();
        WireModel::solid(name.into(), points, WireModel::CYAN, false)
    }
}

/// Rubber-banded crossing rectangle, drawn like the selection marquee it stands
/// in for so the option reads as the selection it performs. (#336)
pub fn crossing_box_preview(corner: [f64; 2], cursor: [f64; 2], name: &str) -> WireModel {
    dashed(
        vec![
            corner,
            [corner[0], cursor[1]],
            cursor,
            [cursor[0], corner[1]],
            corner,
        ],
        name,
        CROSSING_GREEN,
    )
}

fn dashed(path: Vec<[f64; 2]>, name: &str, color: [f32; 4]) -> WireModel {
    let points: Vec<[f32; 3]> = path
        .into_iter()
        .map(|p| [p[0] as f32, p[1] as f32, 0.0])
        .collect();
    let mut wire = WireModel::solid(name.into(), points, color, false);
    wire.pattern_length = DASH_LENGTH;
    wire.pattern = DASH_PATTERN;
    wire
}
