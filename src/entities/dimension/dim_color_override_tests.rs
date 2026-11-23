// Tests for dim_color_overrid.
use super::*;
use crate::t;

use super::dim_color_index;
use crate::entities::dim_override::{DIMCLRD, DIMCLRE, DIMCLRT};
use document::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

// An ACAD/DSTYLE record holding the overrides the Properties panel writes.
fn xdata(overrides: &[(i16, i16)]) -> ExtendedData {
    let mut values = vec![
        XDataValue::String("DSTYLE".to_string()),
        XDataValue::ControlString("{".to_string()),
    ];
    for (code, aci) in overrides {
        values.push(XDataValue::Integer16(*code));
        values.push(XDataValue::Integer16(*aci));
    }
    values.push(XDataValue::ControlString("}".to_string()));
    let mut record = ExtendedDataRecord::new("ACAD");
    for value in values {
        record.add_value(value);
    }
    let mut xd = ExtendedData::new();
    xd.add_record(record);
    xd
}

// Every dim colour honours its per-object override, not just DIMCLRD.
// Text and extension-line colours edited on the Properties panel used to
// show there and render with the style/layer colour anyway. (#898)
#[test]
fn override_wins_over_style_for_every_dim_colour() {
    let xd = xdata(&[(DIMCLRD, 1), (DIMCLRE, 3), (DIMCLRT, 5)]);
    assert_eq!(dim_color_index(&xd, DIMCLRD, 256), 1);
    assert_eq!(dim_color_index(&xd, DIMCLRE, 256), 3);
    assert_eq!(dim_color_index(&xd, DIMCLRT, 256), 5);
}

// Only the overridden variable changes; the rest still inherit the style.
#[test]
fn absent_override_falls_back_to_the_style_value() {
    let xd = xdata(&[(DIMCLRT, 5)]);
    assert_eq!(dim_color_index(&xd, DIMCLRT, 256), 5);
    assert_eq!(dim_color_index(&xd, DIMCLRD, 7), 7);
    assert_eq!(dim_color_index(&xd, DIMCLRE, 256), 256);
}

// An entity with no override record at all is unaffected.
#[test]
fn no_record_uses_the_style_value() {
    let xd = ExtendedData::new();
    assert_eq!(dim_color_index(&xd, DIMCLRT, 2), 2);
}

// ByLayer (256) and ByBlock (0) stay meaningful as override values, so an
// explicit "back to ByLayer" edit is not mistaken for "no override".
#[test]
fn bylayer_override_is_kept() {
    let xd = xdata(&[(DIMCLRT, 256)]);
    assert_eq!(dim_color_index(&xd, DIMCLRT, 5), 256);
}
