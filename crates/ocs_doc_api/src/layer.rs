//! Shared layer-name normalization rules.
//!
//! Layer names in OpenCADStudio/acadrust are case-insensitive, leading/trailing
//! whitespace is ignored, and empty names are invalid. The host (`normalize_name`
//! in `acadrust::tables`) implements the canonical rule set; this module re-exports
//! a compatible, dependency-free function for the wire side and for backends that
//! do not link against `acadrust` directly.

/// Normalize a raw layer name into its case-insensitive lookup key.
///
/// This mirrors `acadrust::tables::normalize_name` used by the host:
/// - trims leading and trailing ASCII whitespace
/// - converts the result to uppercase
/// - returns `""` if the name is empty after trimming
pub fn normalize_layer_name(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// A normalized layer name is non-empty and matches the host lookup key.
pub fn is_valid_layer_name(name: &str) -> bool {
    !normalize_layer_name(name).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_trims_and_uppercases() {
        assert_eq!(normalize_layer_name("  Walls  "), "WALLS");
        assert_eq!(normalize_layer_name("walls"), "WALLS");
        assert_eq!(normalize_layer_name("WALLS"), "WALLS");
    }

    #[test]
    fn empty_and_whitespace_names_are_invalid() {
        assert_eq!(normalize_layer_name(""), "");
        assert_eq!(normalize_layer_name("   "), "");
        assert!(!is_valid_layer_name(""));
        assert!(!is_valid_layer_name("   "));
    }
}
