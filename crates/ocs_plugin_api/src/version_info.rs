//! Embedded version metadata generated at build time.
//!
//! At compile time `build.rs` writes a small JSON blob to
//! `OUT_DIR/version_info.json` containing host version, `ocs_plugin_api`
//! version, `acadrust` version and source, API version bounds, and a build
//! timestamp. The blob is embedded via `include_str!` and is available without
//! enabling the `host` feature.
//!
//! The acadrust source string is used to detect binary-incompatible plugin
//! builds. For API v4 and later the host compares the plugin's resolved
//! `acadrust` source with its own via [`acadrust_sources_compatible`]. Two
//! sources are considered compatible only when they resolve to the same 40
//! character git commit hash.
//!
//! The rustc version string is also recorded because Rust has no stable ABI:
//! a plugin built with a different compiler than the host can pass simple calls
//! and then crash the runner on compound types. For API v4 and later the host
//! can compare the plugin's declared rustc with its own via
//! [`rustc_versions_compatible`].

use std::sync::OnceLock;

/// The embedded version info as a JSON string.
pub const EMBEDDED_VERSION_INFO_JSON: &str =
    include_str!(concat!(env!("OUT_DIR"), "/version_info.json"));

/// Convenience accessor for the embedded version info JSON.
pub fn get_embedded_version_info_json() -> &'static str {
    EMBEDDED_VERSION_INFO_JSON
}

#[derive(Debug, Clone)]
struct VersionInfo {
    acadrust_source: String,
    rustc_version: String,
}

pub const ACADRUST_GATE_API_VERSION: u32 = 4;

/// Returns whether this API version uses the dependency gate.
pub fn uses_acadrust_gate(api_version: u32) -> bool {
    api_version >= ACADRUST_GATE_API_VERSION
}

/// Extracts a string from the generated compact JSON.
fn json_string_value(json: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{key}":""#);
    let start = json.find(&pattern)? + pattern.len();
    let end = json[start..].find('"')?;
    Some(json[start..start + end].to_string())
}

fn parsed_version_info() -> &'static VersionInfo {
    static INFO: OnceLock<VersionInfo> = OnceLock::new();
    INFO.get_or_init(|| VersionInfo {
        acadrust_source: json_string_value(EMBEDDED_VERSION_INFO_JSON, "acadrust_source")
            .unwrap_or_default(),
        rustc_version: json_string_value(EMBEDDED_VERSION_INFO_JSON, "rustc_version")
            .unwrap_or_default(),
    })
}

/// Returns the host's full `acadrust` Cargo source.
pub fn host_acadrust_source() -> &'static str {
    &parsed_version_info().acadrust_source
}

/// Returns the host's `rustc --version` output.
pub fn host_rustc_version() -> &'static str {
    &parsed_version_info().rustc_version
}

/// Returns whether two `rustc --version` outputs are considered compatible.
///
/// The full version line (e.g. "rustc 1.98.0 (hash date)") is compared
/// verbatim after normalising whitespace. This is intentionally strict: Rust
/// has no stable ABI, so even the same nominal compiler built in different
/// environments can produce incompatible layouts.
pub fn rustc_versions_compatible(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.split_whitespace().collect::<String>()
        .eq_ignore_ascii_case(&b.split_whitespace().collect::<String>())
}

/// Extracts the full git commit hash from a Cargo source.
pub fn acadrust_source_hash(source: &str) -> Option<&str> {
    let hash = source.rsplit('#').next()?;
    if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

/// Returns whether two Cargo sources use the same commit.
pub fn acadrust_sources_compatible(a: &str, b: &str) -> bool {
    match (acadrust_source_hash(a), acadrust_source_hash(b)) {
        (Some(ha), Some(hb)) => ha.eq_ignore_ascii_case(hb),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_version_info_is_valid_json() {
        let json = get_embedded_version_info_json();
        assert!(!json.is_empty());
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        assert!(value.get("ocs_version").is_some());
        assert!(value.get("acadrust_source").is_some());
        assert!(value.get("rustc_version").is_some());
    }

    #[test]
    fn embedded_version_info_has_expected_keys() {
        let value: serde_json::Value =
            serde_json::from_str(get_embedded_version_info_json()).expect("valid JSON");
        for key in [
            "ocs_version",
            "ocs_plugin_api_version",
            "acadrust_version",
            "acadrust_source",
            "rustc_version",
            "api_version",
            "api_version_min_supported",
            "build_timestamp",
        ] {
            assert!(value.get(key).is_some(), "missing version info key {}", key);
        }
    }

    #[test]
    fn embedded_version_info_matches_package_versions() {
        let value: serde_json::Value =
            serde_json::from_str(get_embedded_version_info_json()).expect("valid JSON");
        assert_eq!(
            value["ocs_plugin_api_version"].as_str(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(value["api_version"].as_i64(), Some(4));
        assert_eq!(value["api_version_min_supported"].as_i64(), Some(2));

        // acadrust_version should have a patch component (e.g. "0.4.0").
        let acadrust = value["acadrust_version"].as_str().expect("acadrust_version string");
        assert_eq!(acadrust.split('.').count(), 3);
    }

    #[test]
    fn host_acadrust_source_is_non_empty() {
        assert!(!host_acadrust_source().is_empty());
    }

    #[test]
    fn host_rustc_version_is_non_empty() {
        assert!(!host_rustc_version().is_empty());
    }

    #[test]
    fn rustc_version_comparison_is_strict() {
        assert!(rustc_versions_compatible(
            "rustc 1.98.0 (abc123 2026-01-01)",
            "rustc 1.98.0 (abc123 2026-01-01)"
        ));
        assert!(!rustc_versions_compatible(
            "rustc 1.96.0 (def456 2025-11-01)",
            "rustc 1.98.0 (abc123 2026-01-01)"
        ));
    }

    #[test]
    fn rustc_version_comparison_normalises_whitespace() {
        assert!(rustc_versions_compatible(
            "rustc  1.98.0  (abc123  2026-01-01)",
            "rustc 1.98.0 (abc123 2026-01-01)"
        ));
    }

    #[test]
    fn rustc_version_comparison_rejects_empty() {
        assert!(!rustc_versions_compatible("", "rustc 1.98.0"));
        assert!(!rustc_versions_compatible("rustc 1.98.0", ""));
        assert!(!rustc_versions_compatible("", ""));
    }

    #[test]
    fn dependency_gate_starts_at_api_v4() {
        assert!(!uses_acadrust_gate(3));
        assert!(uses_acadrust_gate(4));
        assert!(uses_acadrust_gate(5));
    }

    #[test]
    fn extracts_full_hash_from_cargo_source() {
        let src = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd";
        assert_eq!(
            acadrust_source_hash(src),
            Some("94df2c3f87fa051b16ffc3923f80e9247c85c5fd")
        );
    }

    #[test]
    fn rejects_malformed_or_missing_hash() {
        assert!(acadrust_source_hash("").is_none());
        assert!(acadrust_source_hash("registry+https://crates.io").is_none());
        assert!(acadrust_source_hash("git+https://github.com/foo/bar.git#short").is_none());
        assert!(
            acadrust_source_hash("git+https://github.com/foo/bar.git#gggggggggggggggggggggggggggggggggggggggg")
                .is_none()
        );
    }

    #[test]
    fn source_comparison_matches_full_hashes() {
        let a = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd";
        let b = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd";
        assert!(acadrust_sources_compatible(a, b));
    }

    #[test]
    fn source_comparison_detects_mismatch() {
        let a = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd";
        let b = "git+https://github.com/HakanSeven12/cadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb";
        assert!(!acadrust_sources_compatible(a, b));
    }

    #[test]
    fn source_comparison_case_insensitive() {
        let a = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd";
        let b = "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94DF2C3F87FA051B16FFC3923F80E9247C85C5FD";
        assert!(acadrust_sources_compatible(a, b));
    }
}
