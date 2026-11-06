//! Comparing a shown property value with a typed one: numbers as numbers,
//! text ignoring case, and name patterns.
//!
//! Patterns, for `=` and `!=` on text: `*` any run, `?` one character, `#`
//! one digit, `FAL*,AJTO*` any of these, `~FAL*` everything else. A value
//! that reads as a number is never a pattern, and an exact match always
//! counts, so a colour shown as `255,0,0` still equals `255,0,0`.

use super::Op;
use crate::entities::common::parse_f64;

/// Two numbers are the same when they differ only by rounding: a typed `10`
/// matches a length shown as `10.0000`.
pub fn numbers_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// `=` without patterns: numbers as numbers, anything else as text ignoring
/// case.
pub fn values_equal(actual: &str, wanted: &str) -> bool {
    match (parse_f64(actual), parse_f64(wanted)) {
        (Some(a), Some(b)) => numbers_equal(a, b),
        _ => actual.eq_ignore_ascii_case(wanted),
    }
}

/// Whether `wanted` is a pattern rather than a plain value.
pub fn is_pattern(wanted: &str) -> bool {
    parse_f64(wanted).is_none()
        && (wanted.starts_with('~') || wanted.contains(['*', '?', '#', ',']))
}

/// `=` with patterns.
pub fn matches_value(actual: &str, wanted: &str) -> bool {
    if values_equal(actual, wanted) {
        return true;
    }
    if !is_pattern(wanted) {
        return false;
    }
    let (negate, list) = match wanted.strip_prefix('~') {
        Some(rest) => (true, rest),
        None => (false, wanted),
    };
    let hit = list
        .split(',')
        .map(str::trim)
        .filter(|alternative| !alternative.is_empty())
        .any(|alternative| {
            values_equal(actual, alternative)
                || crate::io::xref_model::wildcard_match_with_digits(actual, alternative)
        });
    hit != negate
}

/// The operator's test on a property value that exists.
pub fn test(op: Op, actual: &str, wanted: &str) -> bool {
    let ordered = |test: fn(f64, f64) -> bool| match (parse_f64(actual), parse_f64(wanted)) {
        (Some(a), Some(b)) => test(a, b),
        _ => false,
    };
    match op {
        Op::Any => true,
        Op::Eq => matches_value(actual, wanted),
        Op::Ne => !matches_value(actual, wanted),
        Op::Gt => ordered(|a, b| a > b && !numbers_equal(a, b)),
        Op::Ge => ordered(|a, b| a > b || numbers_equal(a, b)),
        Op::Lt => ordered(|a, b| a < b && !numbers_equal(a, b)),
        Op::Le => ordered(|a, b| a < b || numbers_equal(a, b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_compare_as_numbers_and_text_ignores_case() {
        assert!(values_equal("10.0000", "10"));
        assert!(!values_equal("10.0000", "10.5"));
        assert!(values_equal("Walls", "WALLS"));
        assert!(test(Op::Ge, "10.0000", "10") && test(Op::Le, "10.0000", "10"));
        assert!(!test(Op::Gt, "10.0000", "10") && !test(Op::Lt, "10.0000", "10"));
        assert!(test(Op::Gt, "12.5", "10") && test(Op::Lt, "3", "10"));
        assert!(
            !test(Op::Gt, "ByLayer", "10"),
            "text never orders against a number"
        );
    }

    #[test]
    fn patterns_on_text() {
        assert!(test(Op::Eq, "FAL-KULSO", "FAL*"));
        assert!(test(Op::Eq, "fal-1", "FAL-?"));
        assert!(test(Op::Eq, "A1", "A#") && !test(Op::Eq, "AB", "A#"));
        assert!(test(Op::Eq, "AJTO-1", "FAL*,AJTO*"));
        assert!(test(Op::Eq, "AJTO-1", "~FAL*") && !test(Op::Eq, "FAL-1", "~FAL*"));
        assert!(test(Op::Ne, "AJTO-1", "FAL*"));
    }

    #[test]
    fn an_exact_value_with_pattern_characters_still_matches() {
        assert!(test(Op::Eq, "255,0,0", "255,0,0"));
        assert!(!test(Op::Eq, "255,0,0", "255,0,1"));
        assert!(test(Op::Eq, "*U12", "*U12"), "an anonymous block name");
        assert!(!is_pattern("10"));
    }
}
