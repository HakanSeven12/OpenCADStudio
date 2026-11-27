//! Unit suffixes on number literals (`3.6m`, `90deg`, `2'9"`).

use super::value::AngleMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Unit {
    /// Millimetres per one of this unit.
    Length(f64),
    /// Radians per one of this unit.
    Angle(f64),
}

/// The unit a suffix names, case-insensitive. `'` and `"` are feet and inches.
pub(super) fn lookup(suffix: &str) -> Option<Unit> {
    use std::f64::consts::PI;
    Some(match suffix.to_ascii_lowercase().as_str() {
        "mm" => Unit::Length(1.0),
        "cm" => Unit::Length(10.0),
        "m" => Unit::Length(1000.0),
        "km" => Unit::Length(1_000_000.0),
        "in" | "\"" => Unit::Length(25.4),
        "ft" | "'" => Unit::Length(304.8),
        "deg" => Unit::Angle(PI / 180.0),
        "rad" => Unit::Angle(1.0),
        "grad" => Unit::Angle(PI / 200.0),
        _ => return None,
    })
}

/// `value` in `unit`, expressed in drawing units (lengths) or in the current
/// angle mode (angles).
pub(super) fn apply(value: f64, unit: Unit, base_unit: f64, mode: AngleMode) -> f64 {
    match unit {
        Unit::Length(mm) => value * mm / base_unit,
        Unit::Angle(rad) => mode.radians_to_mode(value * rad),
    }
}
