// Tests for angular_uni.
use super::*;
use crate::t;

use super::format_angular_value;

fn style(dimaunit: i16, dimadec: i16) -> DimStyle {
    let mut s = DimStyle::standard();
    s.dimaunit = dimaunit;
    s.dimadec = dimadec;
    s
}

// DIMAUNIT 4 is Surveyor's units. It used to fall through to the decimal
// branch, so an angular dimension in a drawing authored with surveyor
// units read as plain degrees while AUNITS=4 elsewhere wrote a bearing.
#[test]
fn surveyor_units_write_a_bearing() {
    assert_eq!(
        format_angular_value(45.0, Some(&style(4, 0))),
        "N 45\u{b0}0'0\" E"
    );
    assert_eq!(
        format_angular_value(135.0, Some(&style(4, 0))),
        "N 45\u{b0}0'0\" W"
    );
    assert_eq!(
        format_angular_value(200.0, Some(&style(4, 0))),
        "S 70\u{b0}0'0\" W"
    );
    assert_eq!(
        format_angular_value(300.0, Some(&style(4, 0))),
        "S 30\u{b0}0'0\" E"
    );
    // A hair under a cardinal must not quote a quarter-turn bearing.
    assert_eq!(format_angular_value(89.99999, Some(&style(4, 0))), "N");
    assert_eq!(format_angular_value(90.0, Some(&style(4, 0))), "N");
    assert_eq!(format_angular_value(0.0, Some(&style(4, 0))), "E");
}

// The other unit formats are untouched.
#[test]
fn other_angular_units_unchanged() {
    assert_eq!(
        format_angular_value(45.5, Some(&style(1, 0))),
        "45\u{b0}30'0\""
    );
    assert_eq!(format_angular_value(90.0, Some(&style(2, 1))), "100.0g");
}
