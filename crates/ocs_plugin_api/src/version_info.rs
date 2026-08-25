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
    })
}

/// Returns the host's full `acadrust` Cargo source.
pub fn host_acadrust_source() -> &'static str {
    &parsed_version_info().acadrust_source
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
