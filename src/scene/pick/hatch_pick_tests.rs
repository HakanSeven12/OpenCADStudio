//! A pattern fill is picked on its lines, a solid one anywhere inside it, and
//! of the fills hit at one point the one drawn on top is taken.

use super::hit_test::{click_hit_hatch, click_hit_insert_hatch};
use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use crate::scene::view::camera::Camera;
use document::Handle;
use iced::{Point, Rectangle, Size};
use rustc_hash::{FxHashMap, FxHashSet};

/// Pixels the pick box reaches from the cursor.
const APERTURE: f32 = 4.0;
/// Survey coordinates: the fills sit far from the origin.
const ORIGIN: [f64; 2] = [500_000.0, 4_000_000.0];

fn family(angle_deg: f32, spacing: f32, dashes: &[f32]) -> PatFamily {
    PatFamily {
        angle_deg,
        x0: 0.0,
        y0: 0.0,
        dx: 0.0,
        dy: spacing,
        dashes: dashes.to_vec(),
    }
}

/// A `width` x `height` rectangle with its lower-left corner `at` from
/// [`ORIGIN`], filled with `pattern`.
fn fill(at: [f32; 2], width: f32, height: f32, pattern: HatchPattern) -> HatchModel {
    let [x, y] = at;
    HatchModel {
        entity: None,
        effective_layer: None,
        pattern_origin: None,
        render_instance: None,
        world_origin: ORIGIN,
        boundary: std::sync::Arc::new(vec![
            [x, y],
            [x + width, y],
            [x + width, y + height],
            [x, y + height],
        ]),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: document::entities::HatchStyleType::Normal,
        pattern,
        name: "TEST".into(),
        color: [1.0; 4],
        aci: 7,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    }
}

/// Horizontal lines `spacing` apart, the first through [`ORIGIN`].
fn horizontal(spacing: f32) -> HatchPattern {
    HatchPattern::Pattern(vec![family(0.0, spacing, &[])])
}

/// Vertical lines `spacing` apart, the first through [`ORIGIN`].
fn vertical(spacing: f32) -> HatchPattern {
    HatchPattern::Pattern(vec![family(90.0, spacing, &[])])
}

/// A plan view of the 20 x 10 units at [`ORIGIN`], 100 pixels to the unit.
struct View {
    camera: Camera,
    bounds: Rectangle,
}

impl View {
    fn plan() -> Self {
        let bounds = Rectangle::with_size(Size::new(2000.0, 1000.0));
        let mut camera = Camera::default();
        camera.fit_to_bounds_f64(
            glam::DVec3::new(ORIGIN[0], ORIGIN[1], 0.0),
            glam::DVec3::new(ORIGIN[0] + 20.0, ORIGIN[1] + 10.0, 0.0),
            2.0,
        );
        Self { camera, bounds }
    }

    /// The pixel of the point `[x, y]` from [`ORIGIN`].
    fn pixel(&self, at: [f64; 2]) -> Point {
        let point = self
            .camera
            .project(
                glam::DVec3::new(ORIGIN[0] + at[0], ORIGIN[1] + at[1], 0.0),
                self.bounds,
            )
            .expect("a point in view");
        Point::new(point.x, point.y)
    }

    fn pick(
        &self,
        fills: &[(u64, HatchModel)],
        depths: &[(u64, f32)],
        at: [f64; 2],
    ) -> Option<u64> {
        let hatches: FxHashMap<Handle, HatchModel> = fills
            .iter()
            .map(|(handle, fill)| (Handle::new(*handle), fill.clone()))
            .collect();
        let draw_depth: FxHashMap<u64, [f32; 2]> = depths
            .iter()
            .map(|&(handle, depth)| (handle, [depth, 0.0]))
            .collect();
        click_hit_hatch(
            self.pixel(at),
            &hatches,
            self.camera.view_proj_rte(self.bounds),
            self.camera.eye(),
            self.bounds,
            APERTURE,
            &draw_depth,
            None,
        )
        .map(|handle| handle.value())
    }
}

/// On a line of a pattern the fill is picked; between two lines, well clear
/// of the pick box, nothing is, though the point is inside the boundary.
#[test]
fn a_pattern_fill_is_picked_on_its_lines_not_between_them() {
    let view = View::plan();
    // Lines at y = 0, 1, 2 …: 100 pixels apart.
    let fills = [(1, fill([2.0, 1.0], 16.0, 8.0, horizontal(1.0)))];
    assert_eq!(view.pick(&fills, &[], [10.0, 5.0]), Some(1), "on a line");
    assert_eq!(
        view.pick(&fills, &[], [10.0, 5.03]),
        Some(1),
        "3 px off a line"
    );
    assert_eq!(
        view.pick(&fills, &[], [10.0, 5.5]),
        None,
        "midway between two lines"
    );
    assert_eq!(
        view.pick(&fills, &[], [10.0, 5.06]),
        None,
        "6 px off a line"
    );
    assert_eq!(
        view.pick(&fills, &[], [1.0, 5.0]),
        None,
        "on a line's run outside the boundary"
    );
}

/// A solid fill and a gradient are picked anywhere inside the boundary.
#[test]
fn a_solid_fill_is_picked_anywhere_inside() {
    let view = View::plan();
    let gradient = HatchPattern::Gradient {
        angle_deg: 0.0,
        color2: [0.0, 0.0, 0.0, 1.0],
        kind: crate::scene::model::hatch_model::GradientKind::Linear,
        invert: false,
        shift: 0.0,
    };
    for pattern in [HatchPattern::Solid, gradient] {
        let fills = [(1, fill([2.0, 1.0], 16.0, 8.0, pattern))];
        assert_eq!(view.pick(&fills, &[], [10.0, 5.5]), Some(1));
        assert_eq!(
            view.pick(&fills, &[], [1.0, 5.5]),
            None,
            "outside the boundary"
        );
    }
}

/// Two pattern fills over the same area: the one whose line is under the
/// cursor is picked, whichever the map lists first.
#[test]
fn of_two_overlapping_patterns_the_one_under_the_cursor_is_picked() {
    let view = View::plan();
    let fills = [
        (1, fill([2.0, 1.0], 16.0, 8.0, horizontal(1.0))),
        (2, fill([2.0, 1.0], 16.0, 8.0, vertical(1.0))),
    ];
    // On a horizontal line, midway between two vertical ones, and the reverse.
    assert_eq!(view.pick(&fills, &[], [10.5, 5.0]), Some(1));
    assert_eq!(view.pick(&fills, &[], [10.0, 5.5]), Some(2));
    assert_eq!(
        view.pick(&fills, &[], [10.5, 5.5]),
        None,
        "between the lines of both"
    );
}

/// Where the lines of two fills cross, or two solids overlap, the fill drawn
/// on top is the one picked.
#[test]
fn of_two_fills_hit_at_one_point_the_one_on_top_is_picked() {
    let view = View::plan();
    let crossing = [
        (1, fill([2.0, 1.0], 16.0, 8.0, horizontal(1.0))),
        (2, fill([2.0, 1.0], 16.0, 8.0, vertical(1.0))),
    ];
    let solids = [
        (1, fill([2.0, 1.0], 16.0, 8.0, HatchPattern::Solid)),
        (2, fill([2.0, 1.0], 16.0, 8.0, HatchPattern::Solid)),
    ];
    for fills in [&crossing, &solids] {
        assert_eq!(
            view.pick(fills, &[(1, 0.2), (2, 0.6)], [10.0, 5.0]),
            Some(2)
        );
        assert_eq!(
            view.pick(fills, &[(1, 0.6), (2, 0.2)], [10.0, 5.0]),
            Some(1)
        );
        assert_eq!(
            view.pick(fills, &[], [10.0, 5.0]),
            Some(2),
            "of equals, the newest"
        );
    }
}

/// A solid under a pattern: on a line of the pattern the pattern is picked,
/// between its lines the solid behind shows and is picked.
#[test]
fn between_the_lines_of_a_pattern_the_solid_behind_is_picked() {
    let view = View::plan();
    let fills = [
        (1, fill([2.0, 1.0], 16.0, 8.0, HatchPattern::Solid)),
        (2, fill([2.0, 1.0], 16.0, 8.0, horizontal(1.0))),
    ];
    let depths = [(1, 0.2), (2, 0.6)];
    assert_eq!(view.pick(&fills, &depths, [10.0, 5.0]), Some(2));
    assert_eq!(view.pick(&fills, &depths, [10.0, 5.5]), Some(1));
}

/// A dashed pattern is picked on its dashes and dots, not in its gaps.
#[test]
fn a_dashed_pattern_is_picked_on_its_dashes_not_in_its_gaps() {
    let view = View::plan();
    // Along each line from x = 0: a dash of 1, a gap of 1, a dot, a gap of 1.
    let dashed = HatchPattern::Pattern(vec![family(0.0, 1.0, &[1.0, -1.0, 0.0, -1.0])]);
    let fills = [(1, fill([2.0, 1.0], 16.0, 8.0, dashed))];
    assert_eq!(
        view.pick(&fills, &[], [9.5, 5.0]),
        Some(1),
        "on a dash (x 9 to 10)"
    );
    assert_eq!(
        view.pick(&fills, &[], [10.5, 5.0]),
        None,
        "in the gap after it"
    );
    assert_eq!(
        view.pick(&fills, &[], [11.0, 5.0]),
        Some(1),
        "on the dot at x = 11"
    );
    assert_eq!(
        view.pick(&fills, &[], [11.5, 5.0]),
        None,
        "in the gap after the dot"
    );
}

/// A fill inside a block is picked by the same rule, and gives its reference.
#[test]
fn a_block_fill_is_picked_on_its_lines_too() {
    let view = View::plan();
    let mut inserts: FxHashMap<Handle, Vec<HatchModel>> = FxHashMap::default();
    inserts.insert(
        Handle::new(7),
        vec![fill([2.0, 1.0], 16.0, 8.0, horizontal(1.0))],
    );
    let pick = |at: [f64; 2], candidates: Option<&FxHashSet<Handle>>| {
        click_hit_insert_hatch(
            view.pixel(at),
            &inserts,
            view.camera.view_proj_rte(view.bounds),
            view.camera.eye(),
            view.bounds,
            APERTURE,
            &FxHashMap::default(),
            candidates,
        )
        .map(|handle| handle.value())
    };
    let candidates: FxHashSet<Handle> = [Handle::new(7)].into_iter().collect();
    for candidates in [None, Some(&candidates)] {
        assert_eq!(pick([10.0, 5.0], candidates), Some(7));
        assert_eq!(pick([10.0, 5.5], candidates), None);
    }
}

/// The distance to the pattern is the distance to the strokes
/// `pattern_segments` lays out, so the pick follows what is drawn: several
/// families, a turned and scaled pattern, dashes and a stagger along the line.
#[test]
fn the_pattern_distance_is_the_distance_to_the_drawn_strokes() {
    let mut staggered = family(30.0, 0.7, &[0.9, -0.4, 0.0, -0.3]);
    staggered.dx = 0.35;
    staggered.x0 = 0.2;
    staggered.y0 = -0.1;
    let mut hatch = fill(
        [-40.0, -40.0],
        80.0,
        80.0,
        HatchPattern::Pattern(vec![
            family(0.0, 1.3, &[]),
            family(115.0, 0.9, &[]),
            staggered,
        ]),
    );
    hatch.angle_offset = 0.37;
    hatch.scale = 1.6;
    // A zero-length dash comes out as a short stroke about its dot: measure
    // against the dots themselves.
    let strokes = hatch.pattern_segments_for_plot();
    assert!(strokes.len() > 1000, "the pattern has strokes to measure");
    let to_stroke = |point: [f64; 2], [a, b]: [[f64; 2]; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length_sq = dx * dx + dy * dy;
        let t = if length_sq > 0.0 {
            (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (point[0] - a[0] - t * dx).hypot(point[1] - a[1] - t * dy)
    };
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..400 {
        // Well inside the boundary, so the clipped strokes hold the nearest one.
        let point = [
            ORIGIN[0] + (next() - 0.5) * 20.0,
            ORIGIN[1] + (next() - 0.5) * 20.0,
        ];
        let drawn = strokes
            .iter()
            .map(|&stroke| to_stroke(point, stroke))
            .fold(f64::INFINITY, f64::min);
        let measured = hatch.pattern_distance(point).expect("a pattern with lines");
        assert!(
            (measured - drawn).abs() < 1e-6,
            "at {point:?}: {measured} from the pattern, {drawn} from the drawn strokes"
        );
    }
}
