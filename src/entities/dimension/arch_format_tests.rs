// Tests for arch_forma.
use super::*;
use crate::t;

use super::{format_architectural, format_engineering, format_fractional};

// A 3ft object that measures 35.99" (float noise) must carry the rounded
// fraction up through inches into feet — not print "2'-11 1"". (Regression
// for the fraction that reduced to 1/1 and leaked out as a bare "1".)
#[test]
fn arch_carries_fraction_up_to_feet() {
    assert_eq!(format_architectural(35.99, 4, 2, 1), "3'-0\"");
    assert_eq!(format_architectural(36.0, 4, 2, 1), "3'-0\"");
    // An exact 15/16 must still render as a fraction, no spurious carry.
    assert_eq!(format_architectural(35.9375, 4, 2, 1), "2'-11 15/16\"");
}

// Carry that stops at inches (11.999" → 12" → 1'-0", not "0'-11 1"").
#[test]
fn arch_carries_fraction_up_to_inches() {
    assert_eq!(format_architectural(11.999, 4, 2, 1), "1'-0\"");
}

#[test]
fn arch_normal_values_unchanged() {
    assert_eq!(format_architectural(30.5, 4, 2, 1), "2'-6 1/2\"");
    assert_eq!(format_architectural(0.0, 4, 2, 1), "0'-0\"");
    assert_eq!(format_architectural(-30.25, 4, 2, 1), "-2'-6 1/4\"");
}

// Same carry bug lived in the plain fractional formatter.
#[test]
fn fractional_carries_up() {
    assert_eq!(format_fractional(35.9999, 4, 2), "36");
    assert_eq!(format_fractional(11.999, 4, 2), "12");
    assert_eq!(format_fractional(6.5, 4, 2), "6 1/2");
}

// Engineering units round to DIMDEC before the feet/inches split, so a
// remainder that rounds up to 12" carries instead of printing "2'-12.00"".
#[test]
fn engineering_carries_inches_up_to_feet() {
    assert_eq!(format_engineering(35.999, 2), "3'-0.00\"");
    assert_eq!(format_engineering(11.999, 2), "1'-0.00\"");
    assert_eq!(format_engineering(11.9, 0), "1'-0\"");
}

#[test]
fn engineering_normal_values_unchanged() {
    assert_eq!(format_engineering(30.5, 2), "2'-6.50\"");
    assert_eq!(format_engineering(36.0, 2), "3'-0.00\"");
    assert_eq!(format_engineering(0.0, 2), "0'-0.00\"");
    assert_eq!(format_engineering(-30.25, 2), "-2'-6.25\"");
}

// A magnitude that rounds away to zero must not keep a lone minus sign.
#[test]
fn engineering_negative_zero_has_no_sign() {
    assert_eq!(format_engineering(-0.001, 2), "0'-0.00\"");
}
