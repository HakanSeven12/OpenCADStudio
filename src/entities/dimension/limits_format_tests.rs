// Tests for limits_forma.
use super::*;
use crate::t;

use super::*;

/// A horizontal linear dimension measuring 10 units along X.
fn horizontal() -> Dimension {
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 5.0, 0.0);
    Dimension::Linear(d)
}

fn style() -> DimStyle {
    let mut s = DimStyle::standard();
    s.dimdec = 3;
    s.dimtdec = 3;
    // Keep trailing zeros so the expectations read like the drawing.
    s.dimzin = 0;
    s.dimtzin = 0;
    s.dimtp = 0.02;
    s.dimtm = 0.05;
    s
}

// the source application's DIMLIM shows the upper limit over the lower one and nothing
// else. The renderer used to keep the nominal and hang the stack beside
// it, so a drawing with limits-only styles grew an extra number on every
// regenerated dimension.
#[test]
fn limits_replace_the_nominal() {
    let mut s = style();
    s.dimlim = true;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S10.020^9.950;".to_string(), None))
    );
}

// Stacked limits keep a common number of decimals: with DIMTZIN 8 the
// zero in 10.020 stays because 9.965 has nothing to drop, while a pair
// that both end in zero shortens together.
#[test]
fn limits_drop_trailing_zeros_only_together() {
    let mut s = style();
    s.dimlim = true;
    s.dimtzin = 8;
    s.dimtm = 0.035;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S10.020^9.965;".to_string(), None))
    );
    s.dimtm = 0.08;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S10.02^9.92;".to_string(), None))
    );
}

// The offsets apply to the number the reader sees, which for a paper
// space dimension is the DIMLFAC-scaled distance (and DIMRND-rounded).
#[test]
fn limits_are_offsets_from_the_scaled_measurement() {
    let mut s = style();
    s.dimlim = true;
    s.dimlfac = 7.5;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S75.020^74.950;".to_string(), None))
    );
    s.dimrnd = 0.5;
    s.dimtm = 0.0;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S75.020^75.000;".to_string(), None))
    );
}

// the source application keeps the stack when both offsets are zero: production
// drawings show `\S11.469^11.469;` for such dimensions.
#[test]
fn limits_without_offsets_still_stack() {
    let mut s = style();
    s.dimlim = true;
    s.dimtp = 0.0;
    s.dimtm = 0.0;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some((r"\S10.000^10.000;".to_string(), None))
    );
}

#[test]
fn limits_take_precedence_over_deviation() {
    let mut s = style();
    s.dimlim = true;
    s.dimtol = true;
    let (_, suffix) = dimension_text_parts(&horizontal(), Some(&s)).unwrap();
    assert_eq!(suffix, None);
}

#[test]
fn deviation_keeps_the_nominal_and_a_separate_suffix() {
    let mut s = style();
    s.dimtol = true;
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some(("10.000".to_string(), Some(r"\S+0.020^-0.050;".to_string())))
    );
}

// A per-entity DSTYLE override turning limits off (group 72 = 0) is how
// the reference callouts in a limits-style drawing show a plain value.
#[test]
fn limits_off_shows_the_plain_value() {
    let s = style();
    assert_eq!(
        dimension_text_parts(&horizontal(), Some(&s)),
        Some(("10.000".to_string(), None))
    );
}

#[test]
fn user_text_substitutes_the_stack_for_the_measurement() {
    let mut s = style();
    s.dimlim = true;
    let mut dim = horizontal();
    dim.base_mut().text = "<> REF".to_string();
    let (value, _) = dimension_text_parts(&dim, Some(&s)).unwrap();
    assert_eq!(value, r"\S10.020^9.950; REF");
}

// Fit decisions size the text by its visible cells: a stack is as wide as
// its wider half, drawn at DIMTFAC, not as wide as its control codes.
#[test]
fn text_cells_measure_the_wider_stack_half() {
    assert_eq!(text_cells("10.000", 0.5), 6.0);
    assert_eq!(text_cells(r"R\S1.020^0.98;", 0.5), 1.0 + 5.0 * 0.5);
    assert_eq!(text_cells(r"\S10.020^9.950; REF", 1.0), 6.0 + 4.0);
    assert_eq!(
        text_cells(r"\S1.020^0.980;\PCutouts", 1.0),
        7.0,
        "widest line, not the sum"
    );
    assert_eq!(text_lines(r"4X <>\PNominal"), 2);
    assert_eq!(
        text_cells(r"{\fArial|b0|i1|c0|p34;Cutouts}", 1.0),
        7.0,
        "format codes and braces take no room"
    );
    assert_eq!(text_cells(r"\H0.7x;10\L Ref\l", 1.0), 6.0);
}

// A three-point angular dimension whose first extension point coincides
// with the vertex measures nothing; the stored angle keeps the text.
#[test]
fn degenerate_angular_dimension_reports_its_stored_angle() {
    use document::entities::DimensionAngular3Pt;
    let mut d = DimensionAngular3Pt::default();
    d.angle_vertex = Vector3::new(6.0, 4.85, 0.0);
    d.first_point = d.angle_vertex;
    d.second_point = Vector3::new(7.0, 4.85, 0.0);
    d.definition_point = Vector3::new(8.0, 5.4, 0.0);
    d.base.actual_measurement = std::f64::consts::FRAC_PI_4;
    let dim = Dimension::Angular3Pt(d);
    let mut s = style();
    s.dimadec = 0;
    assert_eq!(
        dimension_text_parts(&dim, Some(&s)),
        Some(("45°".to_string(), None))
    );
    s.dimlim = true;
    s.dimtp = 0.1;
    s.dimtm = 0.1;
    let (value, _) = dimension_text_parts(&dim, Some(&s)).unwrap();
    assert!(value.starts_with(r"\S45"), "{value}");
}

// the source application rounds a printed value half away from zero; Rust's formatter
// rounds the binary value, which sits a hair below the decimal tie.
#[test]
fn decimal_text_rounds_half_away_from_zero() {
    assert_eq!(decimal_text(0.4255, 3), "0.426");
    assert_eq!(decimal_text(0.4085, 3), "0.409");
    assert_eq!(decimal_text(-0.4255, 3), "-0.426");
    assert_eq!(decimal_text(2.5, 0), "3");
    assert_eq!(decimal_text(0.1234, 3), "0.123");
    assert_eq!(decimal_text(0.0000000000004, 12), "0.000000000000");
}

// Alternate units multiply the displayed (DIMLFAC-scaled) value and stack
// their limits like the primary ones.
#[test]
fn alternate_limits_stack_from_the_scaled_value() {
    let mut s = style();
    s.dimlim = true;
    s.dimlfac = 2.0;
    s.dimalt = true;
    s.dimaltf = 25.4;
    s.dimaltd = 2;
    s.dimalttd = 2;
    s.dimaltz = 0;
    s.dimalttz = 0;
    s.dimtp = 0.02;
    s.dimtm = 0.05;
    let (value, _) = dimension_text_parts(&horizontal(), Some(&s)).unwrap();
    assert_eq!(value, r"\S20.020^19.950; [\S508.51^506.73;]");
}

#[test]
fn a_limits_stack_is_two_halves_tall() {
    let mut s = style();
    assert_eq!(dimension_text_block_height(Some(&s), 2.0), 2.0);
    s.dimlim = true;
    s.dimtfac = 0.5;
    let expected = 2.0
        * 0.5
        * f64::from(
            crate::entities::text_support::STACK_RAISE
                + crate::entities::text_support::STACK_HALF_SCALE,
        );
    assert_eq!(dimension_text_block_height(Some(&s), 2.0), expected);
}
