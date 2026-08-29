//! Phase-2 external plugin discovery.
//!
//! Scans the per-user plugins directory for installed add-on packages and
//! reads their `plugin.toml` so the host can list them and gate them on the
//! API version — *before* any native code is loaded. Actually loading the
//! `cdylib` is a separate step; this module only inspects what is on disk.
//!
//! Layout (mirrors the spec in `docs/plugin-architecture.md`):
//! ```text
//! <config>/OpenCADStudio/plugins/
//!   <plugin-id>/
//!     plugin.toml
//!     <lib<name>.so | .dll | .dylib>
//! ```

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::path::PathBuf;

/// One entry in the curated plugin registry (`plugins/registry.json`).
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub repo: String,
    pub name: String,
    pub description: String,
}

/// Installable release metadata shown by the marketplace before download.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub api_version: u32,
    /// Full `acadrust` git source used by the release.
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
    /// Full `acadrust` git source used by the plugin.
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
    /// Whether a native library for this platform sits beside `plugin.toml`.
    pub lib_present: bool,
}

impl ExternalPlugin {
    /// True when the package's API version is supported by this host.
    pub fn api_compatible(&self) -> bool {
        ocs_plugin_api::manifest::host_accepts_plugin_version(self.api_version)
    }

    /// Returns whether the package's dependency fingerprint matches the host.
    pub fn acadrust_compatible(&self) -> bool {
        if !ocs_plugin_api::version_info::uses_acadrust_gate(self.api_version) {
            return true;
        }
        if !self.acadrust_declared {
            return true;
        }
        match self.acadrust_source.as_deref() {
            None | Some("") => false,
            Some(source) => ocs_plugin_api::version_info::acadrust_sources_compatible(
                source,
                ocs_plugin_api::version_info::host_acadrust_source(),
            ),
        }
    }

    /// Returns whether the package's rustc matches the host.
    pub fn rustc_compatible(&self) -> bool {
        if !ocs_plugin_api::version_info::uses_acadrust_gate(self.api_version) {
            return true;
        }
        if !self.rustc_declared {
            return true;
        }
        match self.rustc_version.as_deref() {
            None | Some("") => false,
            Some(version) => ocs_plugin_api::version_info::rustc_versions_compatible(
                version,
                ocs_plugin_api::version_info::host_rustc_version(),
            ),
        }
    }

    /// Returns whether the package can be loaded.
    #[allow(dead_code)] // plugin-host surface (issue #100); not yet wired
    pub fn loadable(&self) -> bool {
        self.api_compatible()
            && self.acadrust_compatible()
            && self.rustc_compatible()
            && self.lib_present
    }
}

/// `<config>/OpenCADStudio/plugins`, matching the settings/recent-files store.
/// Overridable via `OCS_PLUGINS_DIR` for tests.
pub fn plugins_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OCS_PLUGINS_DIR") {
        return Some(PathBuf::from(p));
    }
    let base: PathBuf = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)?
    } else if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push("Library");
        p.push("Application Support");
        p
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d)
    } else {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push(".config");
        p
    };
    let mut p = base;
    p.push("OpenCADStudio");
    p.push("plugins");
    Some(p)
}

/// Delete an installed package's folder. It stays loaded for the current
/// session (the library is resident); the removal takes effect on next start.
#[cfg(not(target_arch = "wasm32"))]
pub fn uninstall(id: &str) -> Result<(), String> {
    let dir = plugins_dir()
        .ok_or("cannot locate the plugins folder")?
        .join(id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Native dynamic-library extension for the current platform (no dot).
fn lib_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Discover every package under the plugins directory, sorted by `ribbon_order`
/// then id. Missing directory → empty list (not an error).
pub fn discover() -> Vec<ExternalPlugin> {
    let Some(root) = plugins_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let toml_path = dir.join("plugin.toml");
        let Ok(text) = std::fs::read_to_string(&toml_path) else {
            continue;
        };
        if let Some(mut p) = parse_plugin_toml(&text) {
            if p.repository.is_none() {
                p.repository = std::fs::read_to_string(dir.join(".source_repo"))
                    .ok()
                    .and_then(|repo| normalize_repository(&repo));
            }
            p.lib_present = lib_present_in(&dir);
            p.dir = dir;
            found.push(p);
        }
    }
    found.sort_by(|a, b| a.ribbon_order.cmp(&b.ribbon_order).then(a.id.cmp(&b.id)));
    found
}

/// True when a file with this platform's dynamic-library extension exists in
/// `dir` (any name — the package owns its lib naming).
fn lib_present_in(dir: &std::path::Path) -> bool {
    let ext = lib_extension();
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some(ext))
        })
        .unwrap_or(false)
}

/// Minimal `plugin.toml` reader for the documented `[plugin]` / `[opencad]`
/// keys. Deliberately small (string / integer / string-array values) so the
/// host doesn't pull in a full TOML parser for a fixed, host-defined schema.
/// Returns `None` when the required `id` is missing. `dir` / `lib_present` are
/// filled in by the caller.
pub(crate) fn parse_plugin_toml(text: &str) -> Option<ExternalPlugin> {
    let mut id = None;
    let mut name = String::new();
    let mut version = String::new();
    let mut description = String::new();
    let mut repository = None;
    let mut api_version: u32 = 0;
    let mut acadrust_source: Option<String> = None;
    let mut acadrust_declared = false;
    let mut rustc_version: Option<String> = None;
    let mut rustc_declared = false;
    let mut ribbon_order: i32 = 0;
    let mut command_prefixes: Vec<String> = Vec::new();
    let mut section = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            section = header
                .find(']')
                .map(|end| header[..end].trim());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "id" => id = Some(unquote(value)),
            "name" => name = unquote(value),
            "version" => version = unquote(value),
            "description" => description = unquote(value),
            "repository" => repository = normalize_repository(&unquote(value)),
            "api_version" => api_version = value.parse().unwrap_or(0),
            "acadrust_source" if section == Some("opencad") => {
                acadrust_declared = true;
                let v = unquote(value);
                acadrust_source = if v.is_empty() { None } else { Some(v) };
            }
            "rustc_version" if section == Some("opencad") => {
                rustc_declared = true;
                let v = unquote(value);
                rustc_version = if v.is_empty() { None } else { Some(v) };
            }
            "ribbon_order" => ribbon_order = value.parse().unwrap_or(0),
            "command_prefixes" => command_prefixes = parse_string_array(value),
            _ => {}
        }
    }

    Some(ExternalPlugin {
        id: id?,
        name,
        version,
        description,
        repository,
        api_version,
        acadrust_source,
        acadrust_declared,
        rustc_version,
        rustc_declared,
        ribbon_order,
        command_prefixes,
        dir: PathBuf::new(),
        lib_present: false,
    })
}

/// Convert a plugin source URL or shorthand to the marketplace's canonical
/// `owner/repo` form.
pub(crate) fn normalize_repository(value: &str) -> Option<String> {
    let repo = value
        .trim()
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/")
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || name.is_empty() {
        None
    } else {
        Some(format!("{owner}/{name}"))
    }
}

/// Strip surrounding single or double quotes from a TOML scalar.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parse `["a", "b"]` into `["a", "b"]`. Tolerant of spacing and missing
/// brackets; ignores empty entries.
fn parse_string_array(s: &str) -> Vec<String> {
    s.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(unquote)
        .filter(|e| !e.is_empty())
        .collect()
}

// ── Runtime loading (desktop only) ──────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use loader::{shutdown_plugins, with_manager};

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
pub(crate) use loader::{load_at_startup, loaded_ids};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use loader::remove_plugin;

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, allow(dead_code))]
mod loader {
    use super::lib_extension;
    use crate::plugin::v4_support;
    use ocs_plugin_api::process::PluginManager;
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    // Process-wide plugin manager. Drop kills every runner process asynchronously
    // so host shutdown is never delayed by a plugin.
    thread_local! {
        static MANAGER: RefCell<Option<PluginManager>> = const { RefCell::new(None) };
    }

    /// Discover packages and spawn every API-compatible one as a separate
    /// process. Call once at startup. Returns per-id results so the host can
    /// report load failures.
    pub(crate) fn load_at_startup(
        app: &mut crate::app::OpenCADStudio,
    ) -> Vec<(String, Result<(), String>)> {
        let discovered = super::discover();
        let mut manager = PluginManager::new();
        manager.set_notification_handler(v4_support::notification_handler());
        let mut out = Vec::new();
        for d in &discovered {
            if !d.api_compatible() {
                continue;
            }
            if !d.lib_present {
                continue;
            }
            if ocs_plugin_api::version_info::uses_acadrust_gate(d.api_version)
                && d.acadrust_declared
                && d.acadrust_source.is_none()
            {
                eprintln!(
                    "[plugin] {} declares acadrust metadata but has no fingerprint; cannot verify compatibility",
                    d.id
                );
            }
            if !d.acadrust_compatible() {
                let host_src = ocs_plugin_api::version_info::host_acadrust_source();
                let plugin_hash = d
                    .acadrust_source
                    .as_deref()
                    .and_then(ocs_plugin_api::version_info::acadrust_source_hash)
                    .unwrap_or("unknown");
                let host_hash = ocs_plugin_api::version_info::acadrust_source_hash(host_src)
                    .unwrap_or("unknown");
                out.push((
                    d.id.clone(),
                    Err(format!(
                        "Plugin built for acadrust @{plugin_hash}, but this host uses @{host_hash}"
                    )),
                ));
                continue;
            }
            if !d.rustc_compatible() {
                let host_rustc = ocs_plugin_api::version_info::host_rustc_version();
                let plugin_rustc = d.rustc_version.as_deref().unwrap_or("unknown");
                out.push((
                    d.id.clone(),
                    Err(format!(
                        "Plugin built with {plugin_rustc}, host requires {host_rustc} - rebuild required"
                    )),
                ));
                continue;
            }
            let Some(path) = lib_file(&d.dir) else {
                out.push((
                    d.id.clone(),
                    Err("no native library in package".to_string()),
                ));
                continue;
            };
            let mut host = crate::app::plugin_host::HostSession::new(app, 0);
            match manager.load(&path, &mut host) {
                Ok(id) => out.push((id, Ok(()))),
                Err(e) => out.push((d.id.clone(), Err(e.to_string()))),
            }
        }
        MANAGER.with(|m| *m.borrow_mut() = Some(manager));
        out
    }

    /// Ids of the plugins currently loaded in the process store.
    pub fn loaded_ids() -> Vec<String> {
        MANAGER.with(|m| m.borrow().as_ref().map(|mgr| mgr.ids()).unwrap_or_default())
    }

    /// Run `f` with a reference to the loaded plugin manager.
    pub fn with_manager<R>(f: impl FnOnce(&PluginManager) -> R) -> R {
        MANAGER.with(|m| {
            let guard = m.borrow();
            if let Some(manager) = guard.as_ref() {
                return f(manager);
            }
            drop(guard);
            let empty = PluginManager::new();
            f(&empty)
        })
    }

    /// Eagerly shut down all plugin runner processes.
    pub fn shutdown_plugins() {
        MANAGER.with(|m| {
            if let Some(mut manager) = m.borrow_mut().take() {
                manager.shutdown_all();
            }
        });
    }

    /// Shut down the loaded plugin with `id` and remove it from the manager so
    /// its files can be deleted on Windows. Returns true if the plugin was
    /// loaded and has been removed.
    pub fn remove_plugin(id: &str) -> bool {
        MANAGER.with(|m| {
            if let Some(manager) = m.borrow_mut().as_mut() {
                if manager.ids().iter().any(|loaded| loaded == id) {
                    manager.remove(id)
                } else {
                    true
                }
            } else {
                true
            }
        })
    }

    /// Path to the native library beside `plugin.toml`, if any.
    fn lib_file(dir: &Path) -> Option<PathBuf> {
        let ext = lib_extension();
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let p = e.path();
            (p.extension().and_then(|s| s.to_str()) == Some(ext)).then_some(p)
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::plugin::v4_support;

    #[test]
    fn api_v2_plugin_from_template_is_compatible() {
        let toml = r#"
[plugin]
id = "opencad.my_plugin"
name = "My Plugin"
version = "0.1.0"
description = "Template plugin"
repository = "https://github.com/example/opencad-my-plugin.git"

[opencad]
api_version = 2
ribbon_order = 60
command_prefixes = ["MP_"]
xdata_apps = ["MYPLUGIN_RECORD"]
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert_eq!(p.api_version, 2);
        assert_eq!(p.repository.as_deref(), Some("example/opencad-my-plugin"));
        assert!(p.command_prefixes.contains(&"MP_".to_string()));
        assert!(p.api_compatible(), "V2 plugins must be accepted by the V4 host");
    }

    #[test]
    fn missing_id_is_rejected() {
        assert!(parse_plugin_toml("name = \"x\"").is_none());
    }

    #[test]
    fn incompatible_api_flagged() {
        let p = parse_plugin_toml("id=\"a\"\napi_version = 9999").unwrap();
        assert!(!p.api_compatible());
        assert!(!p.loadable());
    }

    #[test]
    fn undeclared_rustc_falls_back_to_api_gate() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(!p.rustc_declared);
        assert!(p.rustc_version.is_none());
        assert!(p.rustc_compatible(), "undeclared rustc is treated as compatible");
    }

    #[test]
    fn declared_empty_rustc_version_is_incompatible() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
rustc_version = ""
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(p.rustc_declared);
        assert!(p.rustc_version.is_none());
        assert!(!p.rustc_compatible(), "declared but empty rustc is incompatible");
        assert!(!p.loadable());
    }

    #[test]
    fn rustc_version_outside_opencad_is_ignored() {
        let toml = r#"
[plugin]
id = "opencad.test"
api_version = 4
rustc_version = "rustc 1.98.0"
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(!p.rustc_declared);
        assert!(p.rustc_version.is_none());
    }

    #[test]
    fn rustc_mismatch_detected() {
        let _host = ocs_plugin_api::version_info::host_rustc_version();
        let other = "rustc 0.0.0-fake (not matching anything)";
        let toml = format!(
            r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
rustc_version = "{other}"
"#
        );
        let p = parse_plugin_toml(&toml).expect("parsed");
        assert!(p.rustc_declared);
        assert!(!p.rustc_compatible(), "mismatched rustc should be incompatible");
        assert!(!p.loadable());
    }

    #[test]
    fn rustc_match_detected() {
        let host = ocs_plugin_api::version_info::host_rustc_version();
        let toml = format!(
            r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
rustc_version = "{host}"
"#
        );
        let p = parse_plugin_toml(&toml).expect("parsed");
        assert!(p.rustc_declared);
        assert!(p.rustc_compatible(), "matching rustc should be compatible");
    }

    #[test]
    fn rustc_gate_only_applies_to_api_v4_and_newer() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 2

[opencad]
rustc_version = "rustc 0.0.0-fake"
"#;
        let mut p = parse_plugin_toml(toml).expect("parsed");
        p.lib_present = true;
        assert!(p.rustc_declared);
        assert!(
            p.rustc_compatible(),
            "API v2 plugin should bypass rustc gate"
        );
        assert!(p.loadable());
    }

    #[test]
    fn undeclared_acadrust_falls_back_to_api_gate() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(!p.acadrust_declared);
        assert!(p.acadrust_source.is_none());
        assert!(p.acadrust_compatible(), "undeclared acadrust is treated as compatible");
    }

    #[test]
    fn declared_empty_acadrust_source_is_incompatible() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
acadrust_source = ""
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(p.acadrust_declared);
        assert!(p.acadrust_source.is_none());
        assert!(!p.acadrust_compatible(), "declared but empty source is incompatible");
        assert!(!p.loadable());
    }

    #[test]
    fn acadrust_source_outside_opencad_is_ignored() {
        let toml = r#"
[plugin]
id = "opencad.test"
api_version = 4
acadrust_source = "0123456789012345678901234567890123456789"
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert!(!p.acadrust_declared);
        assert!(p.acadrust_source.is_none());
    }

    #[test]
    fn acadrust_mismatch_detected() {
        let host = ocs_plugin_api::version_info::host_acadrust_source();
        let other = if host.contains("94df2c3") {
            "git+https://github.com/HakanSeven12/cadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb"
        } else {
            "git+https://github.com/HakanSeven12/cadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd"
        };
        let toml = format!(
            r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
acadrust_source = "{other}"
"#
        );
        let p = parse_plugin_toml(&toml).expect("parsed");
        assert!(p.acadrust_declared);
        assert!(!p.acadrust_compatible(), "mismatched acadrust fingerprint should be incompatible");
        assert!(!p.loadable());
    }

    #[test]
    fn acadrust_match_detected() {
        let host = ocs_plugin_api::version_info::host_acadrust_source();
        let toml = format!(
            r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 4

[opencad]
acadrust_source = "{host}"
"#
        );
        let p = parse_plugin_toml(&toml).expect("parsed");
        assert!(p.acadrust_declared);
        assert!(p.acadrust_compatible(), "matching acadrust fingerprint should be compatible");
    }

    #[test]
    fn acadrust_gate_only_applies_to_api_v4_and_newer() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 2

[opencad]
acadrust_source = "git+https://github.com/HakanSeven12/cadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb"
"#;
        let mut p = parse_plugin_toml(toml).expect("parsed");
        p.lib_present = true;
        assert!(p.acadrust_declared);
        assert!(
            p.acadrust_compatible(),
            "API v2 plugin should bypass acadrust gate"
        );
        assert!(p.loadable());
    }

    /// Integration smoke test for the out-of-process plugin path.
    /// Set `OCS_TEST_PLUGIN` to the built cdylib path and make sure the
    /// `OpenCADStudio` binary is built; the test uses it as the runner host.
    #[test]
    fn spawn_and_dispatch_test_plugin() {
        let path = match std::env::var_os("OCS_TEST_PLUGIN") {
            Some(p) => std::path::PathBuf::from(p),
            None => return,
        };
        if !path.exists() {
            eprintln!("OCS_TEST_PLUGIN does not exist: {}", path.display());
            return;
        }
        let host_exe = std::path::PathBuf::from(
            std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
                .unwrap_or_else(|| std::env::current_exe().unwrap().into_os_string()),
        );
        assert!(
            host_exe.exists(),
            "host exe not found: {}",
            host_exe.display()
        );
        std::env::set_var("OCS_PLUGIN_RUNNER_EXE", &host_exe);

        let mut app = crate::app::OpenCADStudio::new_for_test();
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
                &path,
                &mut host,
                v4_support::notification_handler(),
            )
            .expect("spawn test plugin");
        assert_eq!(process.id(), "opencad.my_plugin");
        let mut started = false;
        let handled = process
            .dispatch(&mut host, "MP_HELLO", &mut |_id| {
                started = true;
            })
            .expect("dispatch MP_HELLO");
        assert!(handled, "plugin should handle MP_HELLO");
        assert!(!started, "MP_HELLO is not interactive");
    }
}
