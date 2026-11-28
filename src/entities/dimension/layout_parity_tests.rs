// Tests for layout_parity.
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

