//! The add-on catalog as the host reads it: the registry entries, the
//! release a registry entry offers, and the packages found on disk. The
//! plugin host (`plugin::external`) fills these; the Plugin Manager shows
//! them.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// An add-on package found on disk (not necessarily loaded or compatible).
#[derive(Debug, Clone)]
pub struct ExternalPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// GitHub source in `owner/repo` form. New marketplace installs persist
    /// this beside the manifest so their README remains discoverable even when
    /// an older `plugin.toml` does not declare `repository`.
    pub repository: Option<String>,
    pub api_version: u32,
    /// Full codec (`acadrust`) git source used by the plugin.
    pub acadrust_source: Option<String>,
    /// Whether `[opencad]` declares `acadrust_source`.
    pub acadrust_declared: bool,
    /// Full `rustc --version` output used by the plugin.
    pub rustc_version: Option<String>,
    /// Whether `[opencad]` declares `rustc_version`.
    pub rustc_declared: bool,
    pub ribbon_order: i32,
    pub command_prefixes: Vec<String>,
    /// The package directory under the plugins folder.
    pub dir: PathBuf,
    /// True when the package ships inside the application rather than the
    /// per-user plugin directory.
    pub bundled: bool,
    /// Whether a native library for this platform sits beside `plugin.toml`.
    pub lib_present: bool,
    /// Explicit library filename declared as `library` in `plugin.toml`. When
    /// set, only that exact file is accepted as the package's native library
    /// instead of the first file with the platform extension (an additive,
    /// optional key — older packages are unaffected).
    pub library: Option<String>,
}

/// One entry in the curated plugin registry (`plugins/registry.json`).
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub repo: String,
    pub name: String,
    pub description: String,
    /// Optional hex SHA-256 digests keyed by asset filename, release tag, or `*`.
    pub sha256: BTreeMap<String, String>,
    /// Optional signature metadata string carried by the registry entry.
    pub signature: Option<String>,
}

/// Pending first-load approval or re-approval after hash mismatch for a
/// third-party native plugin package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPluginApproval {
    pub id: String,
    pub name: String,
    pub version: String,
    pub repository: Option<String>,
    pub library: String,
    pub sha256: String,
    pub previous_sha256: Option<String>,
}

/// Installable release metadata shown by the marketplace before download.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub api_version: u32,
    /// Full codec (`acadrust`) git source used by the release.
    pub acadrust_source: Option<String>,
    /// Whether `[opencad]` declares `acadrust_source`.
    pub acadrust_declared: bool,
    /// Whether the release matches this host.
    pub acadrust_compatible: bool,
    /// Full `rustc --version` output used by the release.
    pub rustc_version: Option<String>,
    /// Whether `[opencad]` declares `rustc_version`.
    pub rustc_declared: bool,
    /// Whether the release's rustc matches this host.
    pub rustc_compatible: bool,
}
