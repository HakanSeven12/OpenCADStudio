//! The drawing unit names and conversions of `units.rs` against the one table
//! of INSUNITS codes (`insunits_to_mm`, `insunits_name`).

use super::*;
use crate::entities::units::{insunits_name, insunits_to_mm};

/// Each INSUNITS code that has a size converts to each other one, by the
/// ratio of their sizes.
#[test]
fn each_unit_with_a_size_converts_to_each_other_one() {
    for from in 1..=24 {
        for to in 1..=24 {
            let expected = insunits_to_mm(from).unwrap() / insunits_to_mm(to).unwrap();
            assert_eq!(
                conversion_factor(from, to),
                Some(expected),
                "INSUNITS {from} to {to}"
            );
        }
    }
}

/// Units that the picker does not offer: known values.
#[test]
fn units_outside_the_picker_convert_by_their_size() {
    let close = |from: i16, to: i16, expected: f64| {
        let factor = conversion_factor(from, to).unwrap_or(f64::NAN);
        assert!(
            (factor - expected).abs() <= 1e-12 * expected,
            "INSUNITS {from} to {to}: {factor}, expected {expected}"
        );
    };
    close(14, 4, 100.0); // decimeters to millimetres
    close(21, 6, 1_200.0 / 3_937.0); // US survey feet to metres
    close(21, 1, 1_200_000.0 / 3_937.0 / 25.4); // US survey feet to inches
    close(6, 13, 1_000_000.0); // metres to microns
    close(2, 1, 12.0); // feet to inches, as before
}

/// A drawing with no unit, or a code that is no unit, converts to nothing.
#[test]
fn unitless_and_unknown_codes_do_not_convert() {
    assert_eq!(conversion_factor(0, 4), None);
    assert_eq!(conversion_factor(4, 0), None);
    assert_eq!(conversion_factor(25, 4), None);
    assert_eq!(conversion_factor(4, -1), None);
}

/// The picker's names are the names of the one table, and a unit that the
/// picker does not offer has its name too, not a placeholder.
#[test]
fn unit_names_agree_with_the_full_table() {
    for (code, name) in all() {
        assert_eq!(name, insunits_name(code), "INSUNITS {code}");
    }
    for code in 0..=24 {
        assert_eq!(label(code), insunits_name(code), "INSUNITS {code}");
    }
    assert_eq!(label(21), "US Survey Feet");
    assert_eq!(label(14), "Decimeters");
}

/// Each unit of the picker has a size, except Unitless.
#[test]
fn each_unit_of_the_picker_has_a_size() {
    for (code, name) in all() {
        assert_eq!(insunits_to_mm(code).is_some(), code != 0, "{name}");
    }
}
