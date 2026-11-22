//! Shared support for integration tests (`tests/*.rs`).
//!
//! This lives in a `common/` directory, not `tests/common.rs`, so Cargo does
//! not build it as a separate, empty test target. It is declared once by
//! `tests/integration.rs` and reached from suites as `crate::common::…`.
//!
//! Helpers here may only use the public API and the test-only dev-dependencies.
//! Fixture selection is explicit and environment-driven, never a developer
//! path, and a missing optional fixture skips with a reason instead of failing.

use std::path::{Path, PathBuf};

/// A temporary directory whose lifetime is tied to the test.
///
/// Always prefer this to `std::env::temp_dir().join(process::id().to_string())`:
/// the latter leaks files and collides between threads and machines.
pub fn scratch_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create a temporary directory")
}

/// Path to a fixture checked into `tests/fixtures/`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Path to an optional, machine-specific fixture named by an environment variable.
///
/// Returns `None` and prints a SKIP reason when the variable is unset or the
/// file is missing. Use for large or licensed corpora that cannot be checked
/// in. Use for large or licensed corpora that cannot be checked in.
pub fn optional_env_fixture(var: &str, what: &str) -> Option<PathBuf> {
    let value = std::env::var(var).ok()?;
    let path = PathBuf::from(value);
    if !path.is_file() {
        eprintln!("SKIP: {what} not found at {} (set {var})", path.display());
        return None;
    }
    Some(path)
}

/// Read an optional, machine-specific fixture, skipping with a reason if it is
/// not configured. Use this instead of a hardcoded developer path.
pub fn read_optional_env_fixture(var: &str, what: &str) -> Option<Vec<u8>> {
    let path = optional_env_fixture(var, what)?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
}

/// Print a skip reason. Used by tests gated on external tools or hardware.
pub fn skip(what: &str) {
    eprintln!("SKIP: {what}");
}

/// Assert that `haystack` contains `needle`, with a readable message.
pub fn assert_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        haystack.contains(needle),
        "{context}: expected to find {needle:?} in:\n{haystack}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_paths_are_absolute_and_under_the_repository() {
        let path = fixture("plot/page-setups-metric.dxf");
        assert!(path.is_absolute());
        assert!(path.ends_with("tests/fixtures/plot/page-setups-metric.dxf"));
    }

    #[test]
    fn scratch_directories_are_unique_and_removed_on_drop() {
        let first = scratch_dir();
        let second = scratch_dir();
        pretty_assertions::assert_ne!(first.path(), second.path());
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
    }
}
