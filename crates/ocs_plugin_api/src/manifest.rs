//! Plugin identity and capability declaration.

/// Host plugin API version. Bump when the host runtime surface breaks
/// compatibility. v2 added `HostApi::start_interactive`. v3 changes
/// `document()` / `document_mut()` to local cached copies for out-of-process
/// plugins and appends `document_reader` / `document_view` at the end of the
/// vtable so API v2 plugins keep working.
pub const API_VERSION: u32 = 3;

/// Oldest plugin API major the current host still loads. This keeps previously
/// compiled cdylibs usable as long as their vtable layout is a prefix of the
/// current `HostApi` trait.
pub const API_VERSION_MIN_SUPPORTED: u32 = 2;

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

/// True when a plugin built against `plugin_major` can be loaded by this host.
/// The host supports majors from `API_VERSION_MIN_SUPPORTED` up to
/// `API_VERSION`.
pub fn host_accepts_plugin_version(plugin_major: u32) -> bool {
    plugin_major >= API_VERSION_MIN_SUPPORTED && plugin_major <= API_VERSION
}

#[cfg(test)]
mod tests {
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
