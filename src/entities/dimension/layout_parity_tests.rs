// Tests for layout_parity.
use super::*;
use crate::t;

use super::*;

/// A document whose Standard style has imperial sizes and horizontal text.
fn document() -> Document {
    let mut document = Document::new();
    let style = document
        .dim_styles
        .get_mut("Standard")
        .expect("Standard style");
    style.dimasz = 0.18;
    style.dimtxt = 0.18;
    style.dimgap = 0.09;
    style.dimtih = true;
    style.dimtoh = true;
    style.dimtad = 0;
    style.dimdec = 2;
    document
}

fn style(document: &Document) -> DimStyle {
    document
        .dim_styles
        .get("Standard")
        .expect("Standard style")
        .clone()
}

/// Stroke endpoints and arrowhead triangle count of the drawn dimension.
fn drawn(document: &Document, dim: &Dimension) -> (Vec<DVec3>, usize) {
    let wires = dim.tessellate(
        document,
        Handle::new(0x40),
        false,
        [1.0; 4],
        1.0,
        1.0,
        &rustc_hash::FxHashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        None,
        None,
        &rustc_hash::FxHashSet::default(),
    );
    let mut points = Vec::new();
    let mut fills = 0;
    for wire in &wires {
        for p in wire.points.iter().filter(|p| p[0].is_finite()) {
            points.push(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        }
        fills += wire.fill_tris.len() / 3;
    }
    (points, fills)
}

/// Like [`drawn`], but reconstructs each vertex from its double-single
/// pair — the coordinates the renderer actually shows. The high f32 alone
/// is ~0.5 m off at UTM scale, which is exactly the defect under test.
fn drawn_precise(document: &Document, dim: &Dimension) -> (Vec<DVec3>, usize) {
    let wires = dim.tessellate(
        document,
        Handle::new(0x40),
        false,
        [1.0; 4],
        1.0,
        1.0,
        &rustc_hash::FxHashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        None,
        None,
        &rustc_hash::FxHashSet::default(),
    );
    let mut points = Vec::new();
    let mut fills = 0;
    let collect = |high: &[[f32; 3]], low: &[[f32; 3]], points: &mut Vec<DVec3>| {
        for (index, p) in high.iter().enumerate() {
            if !p[0].is_finite() {
                continue;
            }
            let l = low.get(index).copied().unwrap_or([0.0; 3]);
            points.push(DVec3::new(
                p[0] as f64 + l[0] as f64,
                p[1] as f64 + l[1] as f64,
                p[2] as f64 + l[2] as f64,
            ));
        }
    };
    for wire in &wires {
        collect(&wire.points, &wire.points_low.iter().collect::<Vec<_>>(), &mut points);
        collect(&wire.fill_tris, &wire.fill_tris_low, &mut points);
        // Key vertices are dually used for snapping/picking; they must be
        // world-space like the rendered points.
        points.extend(
            wire.key_vertices
                .iter()
                .map(|p| DVec3::new(p[0], p[1], p[2])),
        );
        fills += wire.fill_tris.len() / 3;
    }
    (points, fills)
}

#[test]
fn linear_dimension_stays_on_its_points_at_utm_scale() {
    let document = document();
    let mut local = DimensionLinear::horizontal(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
    local.definition_point = Vector3::new(0.0, 5.0, 0.0);
    local.base.definition_point = local.definition_point;

    let offset = Vector3::new(1_649_905.55, 4_945_694.94, 0.0);
    let mut moved = local.clone();
    moved.first_point = moved.first_point + offset;
    moved.second_point = moved.second_point + offset;
    moved.definition_point = moved.definition_point + offset;
    moved.base.definition_point = moved.base.definition_point + offset;

    let (local_points, local_fills) = drawn_precise(&document, &Dimension::Linear(local));
    let (moved_points, moved_fills) = drawn_precise(&document, &Dimension::Linear(moved));
    assert_eq!(local_points.len(), moved_points.len());
    assert_eq!(local_fills, moved_fills);
    let shift = DVec3::new(offset.x, offset.y, offset.z);
    for (local, moved) in local_points.iter().zip(&moved_points) {
        let moved = *moved - shift;
        assert!(
            local.distance(moved) < 1e-3,
            "local {local:?} vs translated-back {moved:?} (offset {:?})",
            local.distance(moved)
        );
    }
}

fn vertical(top: f64) -> DimensionLinear {
    let mut d = DimensionLinear::default();
    d.first_point = Vector3::new(0.0, 0.0, 0.0);
    d.second_point = Vector3::new(0.0, top, 0.0);
    d.definition_point = Vector3::new(-0.5, top, 0.0);
    d.rotation = std::f64::consts::FRAC_PI_2;
    d
}

// Group 11 is authoritative whenever present, whether or not the user moved
// the text.
#[test]
fn stored_text_point_places_automatic_text() {
    let document = document();
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 5.0, 0.0);
    d.base.text_middle_point = Vector3::new(7.0, 5.2, 0.0);
    d.base.text_user_positioned = false;
    let dim = Dimension::Linear(d);
    let position = dimension_text_pos_f64(&dim, Some(&style(&document)), 0.18, 1.0, None);
    assert_eq!((position.x, position.y), (7.0, 5.2));
}

// Text pushed outside by a narrow gap sits past the two-arrow stub and a
// gap, not directly against the arrowhead.
#[test]
fn outside_text_clears_the_stub_and_a_gap() {
    let document = document();
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.2, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 1.0, 0.0);
    let dim = Dimension::Linear(d);
    let s = style(&document);
    let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0, None);
    let half_width = dimension_text_cells(&dim, Some(&s)).unwrap() * 0.18 * CELL_WIDTH * 0.5;
    let text_start = position.x - half_width;
    let stub_end = 0.2 + 2.0 * 0.18;
    assert!(
        (text_start - (stub_end + 0.09)).abs() < 1e-6,
        "text starts at {text_start}, expected {}",
        stub_end + 0.09
    );
}

// Horizontal text near the top of a vertical dimension runs into the top
// arrowhead, so both arrowheads flip outside and stubs grow past the
// extension lines. Text in the middle leaves them inside.
#[test]
fn text_that_hits_an_arrow_moves_the_arrows_outside() {
    let document = document();
    let top = 1.12;
    let mut near_top = vertical(top);
    near_top.base.text_middle_point = Vector3::new(-0.5, 0.98, 0.0);
    let (points, _) = drawn(&document, &Dimension::Linear(near_top));
    assert!(
        points.iter().any(|p| p.y > top + 0.1) && points.iter().any(|p| p.y < -0.1),
        "arrows should be outside with stubs past both extension lines"
    );

    let mut centred = vertical(top);
    centred.base.text_middle_point = Vector3::new(-0.5, top * 0.5, 0.0);
    let (points, _) = drawn(&document, &Dimension::Linear(centred));
    assert!(
        points.iter().all(|p| p.y <= top + 0.01 && p.y >= -0.01),
        "centred text leaves the arrows inside"
    );
}

#[test]
fn dimtmove_one_hangs_far_text_on_a_leader_from_the_line_middle() {
    // Dimension line y=2 from x=0 to 0.8; text three units above it, beyond
    // max(DIMASZ + DIMGAP, DIMTXT / 2 + DIMGAP + DIMEXE) of the line.
    let make = |movement, text_y| {
        let mut document = document();
        document.dim_styles.get_mut("Standard").unwrap().dimtmove = movement;
        let mut d =
            DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.8, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 2.0, 0.0);
        d.base.text_middle_point = Vector3::new(0.4, text_y, 0.0);
        d.base.text_user_positioned = true;
        drawn(&document, &Dimension::Linear(d)).0
    };
    let leader_start = |points: &[DVec3]| {
        points
            .iter()
            .any(|p| (p.x - 0.4).abs() <= 1e-6 && (p.y - 2.0).abs() <= 1e-6)
    };
    let far = make(1, 5.0);
    assert!(
        leader_start(&far),
        "DIMTMOVE=1 leader starts at the dimension line's middle: {far:?}"
    );
    assert!(
        !leader_start(&make(2, 5.0)),
        "DIMTMOVE=2 never draws a leader"
    );
    assert!(
        !leader_start(&make(1, 2.1)),
        "text near its dimension line keeps to it without a leader"
    );
}

// A negative DIMLFAC scales only a paper-space dimension that measures
// model space through a viewport. The association record carries the
// applied factor; zero means the dimension reads its sheet distance.
#[test]
fn negative_dimlfac_follows_the_recorded_association_factor() {
    use document::xdata::{ExtendedDataRecord, XDataValue};
    let document = Document::new();
    let mut source = style(&document);
    source.dimlfac = -4.0;
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(4.457, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 1.0, 0.0);
    let with_factor = |factor: f64| {
        let mut d = d.clone();
        let mut record = ExtendedDataRecord::new("ACAD_DIMASSOC_CALC_DIMLFAC");
        record.add_value(XDataValue::Real(factor));
        d.base.common.extended_data.add_record(record);
        Dimension::Linear(d)
    };
    let sheet = resolved_dimension_style(&source, &with_factor(0.0), &document);
    assert_eq!(sheet.dimlfac, 1.0, "no factor applied: sheet distance");
    let through_viewport = resolved_dimension_style(&source, &with_factor(-4.0), &document);
    assert_eq!(
        through_viewport.dimlfac, 4.0,
        "the recorded factor, made positive"
    );
    let unknown = resolved_dimension_style(&source, &Dimension::Linear(d.clone()), &document);
    assert_eq!(
        unknown.dimlfac, 1.0,
        "model space without a record: unscaled"
    );
    let invalid = resolved_dimension_style(&source, &with_factor(f64::NAN), &document);
    assert_eq!(invalid.dimlfac, 1.0, "invalid recorded factor: unscaled");
}

// DIMTMOVE 1: the stored point is where the hook starts. The hook runs
// one arrow toward the text, and the text begins a gap beyond it.
#[test]
fn dimtmove_one_anchors_the_hook_start_at_the_stored_point() {
    let mut document = document();
    document.dim_styles.get_mut("Standard").unwrap().dimtmove = 1;
    let mut d = DimensionRadius::default();
    d.angle_vertex = Vector3::new(0.0, 0.0, 0.0);
    d.definition_point = Vector3::new(1.0, 0.0, 0.0);
    d.base.text_middle_point = Vector3::new(3.0, 1.5, 0.0);
    let dim = Dimension::Radius(d);
    let (points, _) = drawn(&document, &dim);
    let hook: Vec<f64> = points
        .iter()
        .filter(|p| (p.y - 1.5).abs() < 1e-6)
        .map(|p| p.x)
        .collect();
    let (lo, hi) = (
        hook.iter().cloned().fold(f64::MAX, f64::min),
        hook.iter().cloned().fold(f64::MIN, f64::max),
    );
    assert!(
        (lo - 3.0).abs() < 1e-6 && (hi - 3.18).abs() < 1e-6,
        "hook from the stored point one arrow toward the text: {lo}..{hi}"
    );
    let s = style(&document);
    let position = dimension_text_pos_f64(&dim, Some(&s), 0.18, 1.0, None);
    let half = dimension_text_cells(&dim, Some(&s)).unwrap() * 0.18 * CELL_WIDTH * 0.5;
    assert!(
        (position.x - half - 0.09 - 3.18).abs() < 1e-6,
        "text starts a gap past the hook end, got {}",
        position.x - half
    );
}

// Text stored well past the second extension line of a horizontal
// dimension: the dimension line runs out to the text, not just two arrow
// lengths.
#[test]
fn outside_text_on_the_axis_pulls_the_line_out_to_it() {
    let document = document();
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.8, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 2.0, 0.0);
    d.base.text_middle_point = Vector3::new(2.5, 2.0, 0.0);
    let (points, _) = drawn(&document, &Dimension::Linear(d));
    let furthest = points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    assert!(
        furthest > 2.0 && furthest < 2.5,
        "line reaches the text's near edge, got {furthest}"
    );
}

// Horizontal text stored above a vertical dimension: the dimension line
// runs up to the text's height and hooks one arrow toward it.
#[test]
fn horizontal_outside_text_gets_a_leg_and_a_hook() {
    let document = document();
    let top = 0.3;
    let mut d = vertical(top);
    d.base.text_middle_point = Vector3::new(-0.5 + 0.9, 1.0, 0.0);
    let (points, _) = drawn(&document, &Dimension::Linear(d));
    assert!(
        points
            .iter()
            .any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x + 0.5).abs() < 1e-6),
        "the dimension line reaches the text height at the extension line"
    );
    assert!(
        points
            .iter()
            .any(|p| (p.y - 1.0).abs() < 1e-6 && (p.x - (-0.5 + 0.18)).abs() < 1e-6),
        "a hook one arrow long turns toward the text"
    );
}

// Radius text outside the arc: no line to the centre, a leader from the arc
// point to a hook the text sits against, arrowhead body pointing outward.
#[test]
fn radius_text_outside_draws_a_hooked_leader_only() {
    let document = document();
    let mut d = DimensionRadius::default();
    d.angle_vertex = Vector3::new(0.0, 0.0, 0.0);
    d.definition_point = Vector3::new(1.0, 0.0, 0.0);
    d.base.text_middle_point = Vector3::new(3.0, 0.8, 0.0);
    let (points, fills) = drawn(&document, &Dimension::Radius(d));
    assert!(
        !points.iter().any(|p| p.length() < 1e-6),
        "no dimension line reaches the centre"
    );
    let hook: Vec<_> = points.iter().filter(|p| (p.y - 0.8).abs() < 1e-6).collect();
    assert!(
        hook.len() >= 2,
        "a horizontal hook at the text height: {hook:?}"
    );
    assert!(
        points.iter().any(|p| p.x > 1.05 && p.x < 1.3 && p.y > 0.0),
        "arrowhead body lies outside the arc along the leader"
    );
    assert_eq!(fills, 1);
}

#[test]
fn moved_diameter_leader_leaves_the_rim_once() {
    let mut doc = document();
    let style = doc.dim_styles.get_mut("Standard").unwrap();
    style.dimtmove = 1;
    style.dimtofl = false;
    let tip = Vector3::new(1.0, 0.0, 0.0);
    let mut d = DimensionDiameter::new(tip, Vector3::new(-1.0, 0.0, 0.0));
    d.base.text_user_positioned = true;
    d.base.text_middle_point = Vector3::new(3.0, 2.0, 0.0);
    let (points, fills) = drawn(&doc, &Dimension::Diameter(d));
    // Count full leader runs, excluding the short strokes outlining the arrow.
    let starts = points
        .windows(2)
        .filter(|pair| pair[0].distance(DVec3::X) < 1e-6 && pair[1].distance(DVec3::X) > 0.2)
        .count();
    assert_eq!(starts, 1, "only the hooked leader leaves the arrow tip");
    assert_eq!(fills, 1);
}

// Diameter text outside the circle without DIMTOFL: a single leader from
// the near side, one arrowhead, and no line across the circle.
#[test]
fn diameter_text_outside_draws_one_leader_and_no_chord() {
    let document = document();
    let mut d = DimensionDiameter::default();
    d.angle_vertex = Vector3::new(-1.0, 0.0, 0.0);
    d.definition_point = Vector3::new(1.0, 0.0, 0.0);
    d.base.text_middle_point = Vector3::new(3.0, 0.8, 0.0);
    let (points, fills) = drawn(&document, &Dimension::Diameter(d));
    assert!(
        !points.iter().any(|p| p.x < -0.5),
        "nothing is drawn on the far side of the circle"
    );
    assert_eq!(fills, 1, "one arrowhead");
    assert!(
        points.iter().any(|p| (p.y - 0.8).abs() < 1e-6),
        "hook at the text height"
    );
}
