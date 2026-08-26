//! Plugin identity, API version constants, and compatibility rules.
//!
//! This module defines the host ABI major version (`API_VERSION`), the oldest
//! major the host still loads (`API_VERSION_MIN_SUPPORTED`), and the runtime
//! `OCS_PLUGIN_MAX_API_VERSION` gate that lets operators disable newer API
//! majors without rebuilding.
//!
//! Compatibility rule: a plugin compiled against major `N` runs on any host
//! whose major is `>= N`, because new vtable entries and enum variants are
//! appended at the end. `host_accepts_plugin_version` enforces the supported
//! range after applying the runtime cap.
//!
//! See `ARCHITECTURE.md` in the crate root for the full versioning policy.

/// Host plugin API version. Bump when the host runtime surface breaks
/// compatibility. v2 added `HostApi::start_interactive`. v3 changes
/// `document()` / `document_mut()` to local cached copies for out-of-process
/// plugins and appends `document_reader` / `document_view` at the end of the
/// vtable so API v2 plugins keep working. v4 adds full-duplex notifications on
/// a multiplexed socket while leaving the V2/V3 ABI and protocol untouched.
pub const API_VERSION: u32 = 4;

/// Oldest plugin API major the current host still loads. This keeps previously
/// compiled cdylibs usable as long as their vtable layout is a prefix of the
/// current `HostApi` trait.
pub const API_VERSION_MIN_SUPPORTED: u32 = 2;

/// Environment variable that caps the API major accepted by the host at
/// runtime. Set to `3` to disable V4 plugins while keeping V2/V3 plugins
/// working; set to `2` for V2-only mode. The same mechanism will apply to
/// future API majors.
pub const MAX_API_VERSION_ENV: &str = "OCS_PLUGIN_MAX_API_VERSION";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u32,
}

impl ApiVersion {
    pub const CURRENT: Self = Self { major: API_VERSION };

    /// True when this plugin version can run on `host`. A plugin is compatible
    /// with any host whose API major is the same or newer (new host methods are
    /// appended at the end of the vtable, so old plugins ignore them).
    pub fn is_compatible_with(&self, host: ApiVersion) -> bool {
        self.major <= host.major
    }
}

/// The effective maximum API major this host accepts, honoring
/// [`MAX_API_VERSION_ENV`]. This lets operators disable a new API major at
/// runtime without rebuilding.
pub fn effective_max_api_version() -> u32 {
    std::env::var(MAX_API_VERSION_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|max: u32| max.clamp(API_VERSION_MIN_SUPPORTED, API_VERSION))
        .unwrap_or(API_VERSION)
}

/// True when a plugin built against `plugin_major` can be loaded by this host.
/// The host supports majors from `API_VERSION_MIN_SUPPORTED` up to
/// [`effective_max_api_version`], which can be lowered at runtime via
/// `OCS_PLUGIN_MAX_API_VERSION`.
pub fn host_accepts_plugin_version(plugin_major: u32) -> bool {
    plugin_major >= API_VERSION_MIN_SUPPORTED && plugin_major <= effective_max_api_version()
}

#[cfg(test)]
mod tests {
    use crate::test_lock::ENV_LOCK;

    use super::*;

    #[test]
    fn current_matches_const() {
        assert_eq!(ApiVersion::CURRENT.major, API_VERSION);
    }

    #[test]
    fn same_major_is_compatible() {
        assert!(ApiVersion::CURRENT.is_compatible_with(ApiVersion::CURRENT));
        assert!(!ApiVersion::CURRENT.is_compatible_with(ApiVersion {
            major: API_VERSION - 1,
        }));
        // Forward compatibility: a plugin compiled today runs on a future host
        // that only appends new vtable entries.
        assert!(ApiVersion::CURRENT.is_compatible_with(ApiVersion {
            major: API_VERSION + 1,
        }));
    }

    #[test]
    fn api_v2_plugin_runs_on_api_v3_host() {
        assert!(ApiVersion { major: 2 }.is_compatible_with(ApiVersion { major: 3 }));
    }

    #[test]
    fn effective_max_defaults_to_api_version() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(MAX_API_VERSION_ENV);
        assert_eq!(effective_max_api_version(), API_VERSION);
    }

    #[test]
    fn effective_max_can_disable_v4() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(MAX_API_VERSION_ENV, "3");
        assert_eq!(effective_max_api_version(), 3);
        assert!(!host_accepts_plugin_version(4));
        assert!(host_accepts_plugin_version(3));
        assert!(host_accepts_plugin_version(2));
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn effective_max_v2_only_mode() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(MAX_API_VERSION_ENV, "2");
        assert_eq!(effective_max_api_version(), 2);
        assert!(!host_accepts_plugin_version(4));
        assert!(!host_accepts_plugin_version(3));
        assert!(host_accepts_plugin_version(2));
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn effective_max_clamps_to_supported_range() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(MAX_API_VERSION_ENV, "99");
        assert_eq!(effective_max_api_version(), API_VERSION);
        std::env::set_var(MAX_API_VERSION_ENV, "1");
        assert_eq!(effective_max_api_version(), API_VERSION_MIN_SUPPORTED);
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn host_accepts_supported_range() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(MAX_API_VERSION_ENV);
        assert!(host_accepts_plugin_version(API_VERSION_MIN_SUPPORTED));
        assert!(host_accepts_plugin_version(API_VERSION));
        assert!(!host_accepts_plugin_version(API_VERSION + 1));
        assert!(!host_accepts_plugin_version(API_VERSION_MIN_SUPPORTED - 1));
    }
}

/// Static metadata every plugin supplies at registration time.
/// Keep fields in sync with `plugin.toml` beside the package.
#[derive(Clone, Copy, Debug)]
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub api_version: ApiVersion,
    /// Sort key for add-on ribbon tabs (lower = further left among plugins).
    pub ribbon_order: i32,
    pub xdata_apps: &'static [&'static str],
    pub command_prefixes: &'static [&'static str],
}
