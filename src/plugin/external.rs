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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

impl ExternalPlugin {
    /// True when the package's API version is supported by this host.
    pub fn api_compatible(&self) -> bool {
        ocs_plugin_api::manifest::host_accepts_plugin_version(self.api_version)
    }

    /// Returns whether the package's dependency fingerprint matches the host.
    pub fn acadrust_compatible(&self) -> bool {
        acadrust_source_compatible(
            self.api_version,
            self.acadrust_declared,
            self.acadrust_source.as_deref(),
        )
    }

    /// Returns whether the package's rustc matches the host.
    pub fn rustc_compatible(&self) -> bool {
        if !ocs_plugin_api::version_info::uses_acadrust_gate(self.api_version) {
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

pub(crate) fn acadrust_source_compatible(
    api_version: u32,
    declared: bool,
    source: Option<&str>,
) -> bool {
    !ocs_plugin_api::version_info::uses_acadrust_gate(api_version)
        || declared
            && source.is_some_and(|source| {
                !source.is_empty()
                    && ocs_plugin_api::version_info::acadrust_sources_compatible(
                        source,
                        ocs_plugin_api::version_info::host_acadrust_source(),
                    )
            })
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

/// Read-only plugins shipped with the application. The environment override
/// makes development and packaging tests independent of a particular bundle
/// layout.
pub fn bundled_plugins_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OCS_BUNDLED_PLUGINS_DIR") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    #[cfg(target_os = "macos")]
    {
        return Some(exe.parent()?.parent()?.join("Resources").join("plugins"));
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(exe.parent()?.join("plugins"))
    }
}

/// Delete an installed package's folder. It stays loaded for the current
/// session (the library is resident); the removal takes effect on next start.
pub(crate) fn is_valid_plugin_id(id: &str) -> bool {
    if id.is_empty()
        || id.len() > 64
        || id == "."
        || id == ".."
        || id.contains("..")
        || id.ends_with('.')
    {
        return false;
    }
    let bytes = id.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return false;
    }
    let stem = id.split('.').next().unwrap_or(id).to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn uninstall(id: &str) -> Result<(), String> {
    if !is_valid_plugin_id(id) {
        return Err(format!("invalid plugin id `{id}`"));
    }
    let root = plugins_dir().ok_or("cannot locate the plugins folder")?;
    uninstall_in_root(&root, id)
}

#[cfg(not(target_arch = "wasm32"))]
fn uninstall_in_root(user_root: &std::path::Path, id: &str) -> Result<(), String> {
    if !is_valid_plugin_id(id) {
        return Err(format!("invalid plugin id `{id}`"));
    }
    if !user_root.exists() {
        return Ok(());
    }
    let root_canon = std::fs::canonicalize(user_root).map_err(|e| e.to_string())?;
    let mut discovered = std::collections::BTreeMap::new();
    discover_root(&root_canon, false, &mut discovered);
    let candidate = discovered
        .get(id)
        .map(|plugin| plugin.dir.clone())
        .unwrap_or_else(|| root_canon.join(id));
    match std::fs::symlink_metadata(&candidate) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "refusing to uninstall symlinked plugin directory for `{id}`"
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }
    let dir_canon = std::fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
    if dir_canon == root_canon || dir_canon.parent() != Some(root_canon.as_path()) {
        return Err(format!(
            "plugin directory for `{id}` escapes plugins folder"
        ));
    }
    if dir_canon.is_dir() {
        std::fs::remove_dir_all(&dir_canon).map_err(|e| e.to_string())?;
    }
    let _ = revoke_lock_entry_in_root(&root_canon, id);
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

/// Discover bundled and per-user packages, sorted by `ribbon_order` then id.
/// A per-user package with the same id explicitly overrides the bundled copy.
/// Missing directories are ignored.
pub fn discover() -> Vec<ExternalPlugin> {
    discover_from_roots(bundled_plugins_dir().as_deref(), plugins_dir().as_deref())
}

fn discover_from_roots(
    bundled_root: Option<&Path>,
    user_root: Option<&Path>,
) -> Vec<ExternalPlugin> {
    let mut found = BTreeMap::new();
    if let Some(root) = bundled_root {
        discover_root(root, true, &mut found);
    }
    if let Some(root) = user_root {
        discover_root(root, false, &mut found);
    }
    let mut found: Vec<_> = found.into_values().collect();
    found.sort_by(|a, b| a.ribbon_order.cmp(&b.ribbon_order).then(a.id.cmp(&b.id)));
    found
}

fn discover_root(root: &Path, bundled: bool, found: &mut BTreeMap<String, ExternalPlugin>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
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
            p.bundled = bundled;
            found.insert(p.id.clone(), p);
        }
    }
}

/// True when a regular non-symlink file with this platform's dynamic-library
/// extension exists in `dir`.
fn lib_present_in(dir: &std::path::Path) -> bool {
    lib_file(dir).is_some()
}

/// Path to the first non-symlink native library beside `plugin.toml`, if any.
fn lib_file(dir: &Path) -> Option<PathBuf> {
    let ext = lib_extension();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|s| s.to_str()) == Some(ext)
                && std::fs::symlink_metadata(p)
                    .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.file_type().is_file())
        })
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// The package's native library: the exact `library` file named by
/// `plugin.toml` when declared, otherwise the first regular file with this
/// platform's extension. Refuses symlinks and multi-segment paths.
pub(crate) fn lib_file_for(plugin: &ExternalPlugin) -> Option<PathBuf> {
    if let Some(name) = plugin.library.as_deref() {
        let mut comps = Path::new(name).components();
        let Some(std::path::Component::Normal(file_name)) = comps.next() else {
            return None;
        };
        if comps.next().is_some() {
            return None;
        }
        let path = plugin.dir.join(file_name);
        let matches_ext = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s == lib_extension());
        let is_regular_file = std::fs::symlink_metadata(&path)
            .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.file_type().is_file());
        return if matches_ext && is_regular_file {
            Some(path)
        } else {
            None
        };
    }
    lib_file(&plugin.dir)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginLockEntry {
    pub sha256: String,
    pub library: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_tag: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginLockfile {
    pub version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginLockEntry>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for PluginLockfile {
    fn default() -> Self {
        Self {
            version: 1,
            plugins: BTreeMap::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginTrustState {
    Bundled,
    Approved { sha256: String },
    Unapproved { sha256: String, library: String },
    HashMismatch {
        expected: String,
        actual: String,
        library: String,
    },
    Unverifiable(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_valid_sha256_hex(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::Digest;
    crate::io::recovery::hex_digest(sha2::Sha256::digest(bytes))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    use std::io::Read;

    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(format!(
            "refusing to hash symlinked or non-regular plugin binary `{}`",
            path.display()
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(crate::io::recovery::hex_digest(hasher.finalize()))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn lockfile_path_in(plugins_root: &Path) -> PathBuf {
    plugins_root.join("plugin.lock.json")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn lockfile_path() -> Option<PathBuf> {
    plugins_dir().map(|root| lockfile_path_in(&root))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_lockfile_from(path: &Path) -> Result<PluginLockfile, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.file_type().is_file() {
                return Err("refusing to read symlinked plugin.lock.json".to_string());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PluginLockfile::default()),
        Err(e) => return Err(e.to_string()),
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lock: PluginLockfile =
        serde_json::from_str(&text).map_err(|e| format!("invalid plugin.lock.json: {e}"))?;
    if lock.version != 1 {
        return Err(format!(
            "unsupported plugin.lock.json version {}",
            lock.version
        ));
    }
    for (id, entry) in &mut lock.plugins {
        if !is_valid_plugin_id(id) {
            return Err(format!("invalid plugin id `{id}` in plugin.lock.json"));
        }
        if !is_valid_sha256_hex(&entry.sha256) {
            return Err(format!("invalid sha256 digest for `{id}` in plugin.lock.json"));
        }
        let mut comps = Path::new(&entry.library).components();
        if !matches!(
            (comps.next(), comps.next()),
            (Some(std::path::Component::Normal(_)), None)
        ) {
            return Err(format!(
                "invalid library filename `{}` for `{id}` in plugin.lock.json",
                entry.library
            ));
        }
        entry.sha256 = entry.sha256.to_ascii_lowercase();
    }
    Ok(lock)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_lockfile_to(path: &Path, lock: &PluginLockfile) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| "plugin.lock.json path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err("refusing to overwrite symlinked plugin.lock.json".to_string());
        }
    }
    let json = serde_json::to_vec_pretty(lock).map_err(|e| e.to_string())?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let nonce_hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let tmp_path = parent.join(format!(
        "plugin.lock.json.tmp-{}-{nonce_hex}",
        std::process::id()
    ));
    let write_res = (|| -> Result<(), String> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp_path).map_err(|e| e.to_string())?;
        file.write_all(&json).map_err(|e| e.to_string())?;
        file.write_all(b"\n").map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if write_res.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_res
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn record_lock_entry_in_root(
    plugins_root: &Path,
    id: &str,
    library: &str,
    sha256: &str,
    version: Option<&str>,
    repository: Option<&str>,
    release_tag: Option<&str>,
) -> Result<PluginLockEntry, String> {
    if !is_valid_plugin_id(id) {
        return Err(format!("invalid plugin id `{id}`"));
    }
    if !is_valid_sha256_hex(sha256) {
        return Err(format!("invalid sha256 digest for `{id}`"));
    }
    let mut comps = Path::new(library).components();
    if !matches!(
        (comps.next(), comps.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err(format!("invalid library filename `{library}`"));
    }
    let lock_path = lockfile_path_in(plugins_root);
    let mut lock = load_lockfile_from(&lock_path)?;
    let entry = PluginLockEntry {
        sha256: sha256.to_ascii_lowercase(),
        library: library.to_string(),
        version: version.map(str::to_string),
        repository: repository.map(str::to_string),
        release_tag: release_tag.map(str::to_string),
    };
    lock.plugins.insert(id.to_string(), entry.clone());
    save_lockfile_to(&lock_path, &lock)?;
    Ok(entry)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn revoke_lock_entry_in_root(plugins_root: &Path, id: &str) -> Result<(), String> {
    let lock_path = lockfile_path_in(plugins_root);
    let mut lock = load_lockfile_from(&lock_path)?;
    if lock.plugins.remove(id).is_some() {
        save_lockfile_to(&lock_path, &lock)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn evaluate_plugin_trust(
    plugin: &ExternalPlugin,
    lockfile: &Result<PluginLockfile, String>,
) -> PluginTrustState {
    if plugin.bundled {
        return PluginTrustState::Bundled;
    }
    let lock = match lockfile {
        Ok(lock) => lock,
        Err(error) => return PluginTrustState::Unverifiable(error.clone()),
    };
    let Some(lib_path) = lib_file_for(plugin) else {
        return PluginTrustState::Unverifiable("no valid native library in package".to_string());
    };
    let library = lib_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let actual = match sha256_file(&lib_path) {
        Ok(digest) => digest,
        Err(error) => return PluginTrustState::Unverifiable(error),
    };
    match lock.plugins.get(&plugin.id) {
        None => PluginTrustState::Unapproved {
            sha256: actual,
            library,
        },
        Some(entry) => {
            let expected = entry.sha256.to_ascii_lowercase();
            let same_digest =
                crate::rest::constant_time_eq(expected.as_bytes(), actual.as_bytes());
            if same_digest && entry.library == library {
                PluginTrustState::Approved { sha256: actual }
            } else {
                PluginTrustState::HashMismatch {
                    expected,
                    actual,
                    library,
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn evaluate_plugin_trust_on_disk(plugin: &ExternalPlugin) -> PluginTrustState {
    let lockfile = lockfile_path()
        .ok_or_else(|| "cannot locate plugins folder for plugin.lock.json".to_string())
        .and_then(|path| load_lockfile_from(&path));
    evaluate_plugin_trust(plugin, &lockfile)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn approve_plugin_in_root(
    plugins_root: &Path,
    plugin: &ExternalPlugin,
    release_tag: Option<&str>,
) -> Result<PluginLockEntry, String> {
    let lib_path = lib_file_for(plugin)
        .ok_or_else(|| format!("no valid native library in package `{}`", plugin.id))?;
    let library = lib_path
        .file_name()
        .ok_or_else(|| "plugin library has no filename".to_string())?
        .to_string_lossy()
        .into_owned();
    let sha256 = sha256_file(&lib_path)?;
    record_lock_entry_in_root(
        plugins_root,
        &plugin.id,
        &library,
        &sha256,
        (!plugin.version.is_empty()).then_some(plugin.version.as_str()),
        plugin.repository.as_deref(),
        release_tag,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn approve_plugin(plugin: &ExternalPlugin) -> Result<PluginLockEntry, String> {
    let root = plugins_dir().ok_or("cannot locate the plugins folder")?;
    approve_plugin_in_root(&root, plugin, None)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn collect_pending_approvals(plugins: &[ExternalPlugin]) -> Vec<PendingPluginApproval> {
    let lockfile = lockfile_path()
        .ok_or_else(|| "cannot locate plugins folder for plugin.lock.json".to_string())
        .and_then(|path| load_lockfile_from(&path));
    plugins
        .iter()
        .filter(|plugin| {
            !plugin.bundled
                && plugin.lib_present
                && plugin.api_compatible()
                && plugin.acadrust_compatible()
                && plugin.rustc_compatible()
        })
        .filter_map(|plugin| match evaluate_plugin_trust(plugin, &lockfile) {
            PluginTrustState::Unapproved { sha256, library } => Some(PendingPluginApproval {
                id: plugin.id.clone(),
                name: if plugin.name.is_empty() {
                    plugin.id.clone()
                } else {
                    plugin.name.clone()
                },
                version: plugin.version.clone(),
                repository: plugin.repository.clone(),
                library,
                sha256,
                previous_sha256: None,
            }),
            PluginTrustState::HashMismatch {
                expected,
                actual,
                library,
            } => Some(PendingPluginApproval {
                id: plugin.id.clone(),
                name: if plugin.name.is_empty() {
                    plugin.id.clone()
                } else {
                    plugin.name.clone()
                },
                version: plugin.version.clone(),
                repository: plugin.repository.clone(),
                library,
                sha256: actual,
                previous_sha256: Some(expected),
            }),
            PluginTrustState::Bundled
            | PluginTrustState::Approved { .. }
            | PluginTrustState::Unverifiable(_) => None,
        })
        .collect()
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
    let mut library = None;
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
            section = header.find(']').map(|end| header[..end].trim());
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
            "library" => {
                let v = unquote(value);
                library = if v.is_empty() { None } else { Some(v) };
            }
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

    let id = id.filter(|id| is_valid_plugin_id(id))?;
    Some(ExternalPlugin {
        id,
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
        bundled: false,
        lib_present: false,
        library,
    })
}

/// Convert a plugin source URL or shorthand to the marketplace's canonical
/// `owner/repo` form.
fn is_valid_github_slug_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn normalize_repository(value: &str) -> Option<String> {
    let raw = value.trim();
    let repo = raw
        .strip_prefix("https://github.com/")
        .or_else(|| raw.strip_prefix("http://github.com/"))
        .unwrap_or(raw)
        .trim_end_matches('/');
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if is_valid_github_slug_segment(owner) && is_valid_github_slug_segment(name) {
        Some(format!("{owner}/{name}"))
    } else {
        None
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

#[cfg(test)]
pub(crate) use loader::PROCESS_TESTS_LOCK;
#[cfg(all(not(target_arch = "wasm32"), not(test)))]
pub(crate) use loader::{shutdown_plugins, with_manager};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use loader::{shutdown_plugins, with_fresh_installed_manager, with_manager};

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
pub(crate) use loader::load_at_startup;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use loader::{load_single_approved, loaded_ids, remove_plugin};
#[cfg(test)]
pub(crate) use loader::{remove_plugin_for_tests, spawn_into_manager_for_tests};
pub use crate::plugin_catalog::{ExternalPlugin, PendingPluginApproval, RegistryEntry, ReleaseInfo};

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, allow(dead_code))]
mod loader {
    #[cfg(test)]
    pub(super) use super::lib_file_for;
    use crate::plugin::v4_support;
    use ocs_plugin_api::process::PluginManager;

    // Plugin manager. THREAD-LOCAL and REENTRANT (RefCell immutable borrows
    // nest): every with_manager caller runs on the app's thread — dispatch
    // (whose nested inline serving of plugin write frames re-enters with_manager
    // through the boundary broadcasts), the drains, the grips pump. The V4
    // reader threads that surface plugin notifications never touch the manager:
    // the API v13 `ChannelReady` attach ack goes through the QUEUE and is
    // processed by the app thread at its next message boundary (v4_support::
    // drain_channel_ready_acks) — which is what the queue exists for. A
    // process-GLOBAL Mutex here was tried in v13 and SELF-DEADLOCKED the first
    // plugin-write boundary (AppHangB1, the consumer's CVI_ALIGN_BYPI repro):
    // the outer with_manager(manager.dispatch) held the mutex while the
    // plugin's nested write frames were served inline, and the boundary
    // publish re-entered with_manager on the same thread — a non-reentrant
    // lock waiting on itself. RefCell borrows are the never-freeze shape here.
    thread_local! {
        static MANAGER: std::cell::RefCell<Option<PluginManager>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Discover packages and spawn every API-compatible one as a separate
    /// process. Call once at startup. Returns per-id results so the host can
    /// report load failures.
    pub(crate) fn load_at_startup(
        app: &mut crate::app::OpenCADStudio,
    ) -> Vec<(String, Result<(), String>)> {
        let discovered = super::discover();
        let lockfile = super::lockfile_path()
            .ok_or_else(|| "cannot locate plugins folder for plugin.lock.json".to_string())
            .and_then(|path| super::load_lockfile_from(&path));
        let mut manager = PluginManager::new();
        manager.set_notification_handler(v4_support::notification_handler());
        let mut out = Vec::new();
        for d in &discovered {
            if !d.api_compatible() {
                // LOUD (v13.1): an API-skipped plugin used to vanish silently —
                // the machine then answered every one of its commands with
                // "unknown command" and nothing named the cause. The plugin is
                // still in the plugin manager UI's list of packages; this line
                // is the only place the reason reaches the user.
                out.push((
                    d.id.clone(),
                    Err(format!(
                        "plugin declares api_version {}, host accepts {}..{}; \
                         update the plugin (pins the matching host rev)",
                        d.api_version,
                        ocs_plugin_api::manifest::API_VERSION_MIN_SUPPORTED,
                        ocs_plugin_api::manifest::API_VERSION
                    )),
                ));
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
                    "[plugin] {} declares codec metadata but has no fingerprint; cannot verify compatibility",
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
                        "Plugin built for codec @{plugin_hash}, but this host uses @{host_hash}"
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
            match super::evaluate_plugin_trust(d, &lockfile) {
                super::PluginTrustState::Bundled | super::PluginTrustState::Approved { .. } => {}
                super::PluginTrustState::Unapproved { sha256, .. } => {
                    out.push((
                        d.id.clone(),
                        Err(format!(
                            "plugin_untrusted: awaiting first-load approval (sha256: {sha256})"
                        )),
                    ));
                    continue;
                }
                super::PluginTrustState::HashMismatch {
                    expected, actual, ..
                } => {
                    out.push((
                        d.id.clone(),
                        Err(format!(
                            "plugin_hash_mismatch: native library changed on disk (expected {expected}, actual {actual})"
                        )),
                    ));
                    continue;
                }
                super::PluginTrustState::Unverifiable(error) => {
                    out.push((
                        d.id.clone(),
                        Err(format!(
                            "plugin_untrusted: cannot verify plugin.lock.json ({error})"
                        )),
                    ));
                    continue;
                }
            }
            let Some(path) = super::lib_file_for(d) else {
                out.push((
                    d.id.clone(),
                    Err("no native library in package".to_string()),
                ));
                continue;
            };
            let mut host = crate::app::plugin_host::HostSession::new(app, 0);
            match manager.load_with_expected_id(&path, Some(&d.id), &mut host) {
                Ok(id) => out.push((id, Ok(()))),
                Err(e) => {
                    // Also on stderr: a headless or automation session never
                    // sees the command-line error, and a failed load would
                    // otherwise look like a plugin that never answered.
                    eprintln!("[plugin] {} FAILED to load: {e}", d.id);
                    out.push((d.id.clone(), Err(e.to_string())));
                }
            }
        }
        MANAGER.with(|m| *m.borrow_mut() = Some(manager));
        out
    }

    pub(crate) fn load_single_approved(
        app: &mut crate::app::OpenCADStudio,
        plugin: &super::ExternalPlugin,
    ) -> Result<String, String> {
        if !plugin.api_compatible() {
            return Err(format!("plugin `{}` has incompatible API version", plugin.id));
        }
        if !plugin.acadrust_compatible() || !plugin.rustc_compatible() {
            return Err(format!("plugin `{}` ABI is incompatible with this host", plugin.id));
        }
        match super::evaluate_plugin_trust_on_disk(plugin) {
            super::PluginTrustState::Bundled | super::PluginTrustState::Approved { .. } => {}
            super::PluginTrustState::Unapproved { .. } => {
                return Err("plugin is not approved in plugin.lock.json".to_string());
            }
            super::PluginTrustState::HashMismatch { .. } => {
                return Err("plugin binary hash does not match plugin.lock.json".to_string());
            }
            super::PluginTrustState::Unverifiable(err) => {
                return Err(format!("cannot verify plugin trust: {err}"));
            }
        }
        let path = super::lib_file_for(plugin)
            .ok_or_else(|| "no valid native library in package".to_string())?;
        let mut manager = MANAGER.with(|m| m.borrow_mut().take()).unwrap_or_default();
        manager.set_notification_handler(v4_support::notification_handler());
        let load_res = if manager.ids().iter().any(|id| id == &plugin.id) {
            Ok(plugin.id.clone())
        } else {
            let mut host = crate::app::plugin_host::HostSession::new(app, 0);
            manager
                .load_with_expected_id(&path, Some(&plugin.id), &mut host)
                .map_err(|e| e.to_string())
        };
        MANAGER.with(|m| *m.borrow_mut() = Some(manager));
        load_res
    }

    /// Ids of the plugins currently loaded in the process store.
    pub fn loaded_ids() -> Vec<String> {
        MANAGER.with(|m| m.borrow().as_ref().map(|mgr| mgr.ids()).unwrap_or_default())
    }

    /// Run `f` with a reference to the loaded plugin manager. REENTRANT
    /// while holding an immutable borrow: the dispatch path re-enters
    /// (nested inline serving → boundary broadcasts); nested immutable
    /// borrows of the RefCell are exactly what makes that safe.
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

    /// Run `f` with an EMPTY manager installed in the process store (test
    /// setup for the declaration-law tests: nothing is loaded, so only the
    /// boot-window semantics can let an id pass the owner gate). Without an
    /// installed manager `with_manager` serves a throwaway empty manager per
    /// call, which cannot carry a boot marker across two calls. The previous
    /// store is restored afterwards.
    #[cfg(test)]
    pub(crate) fn with_fresh_installed_manager<R>(f: impl FnOnce() -> R) -> R {
        let previous = MANAGER.with(|m| m.borrow_mut().take());
        MANAGER.with(|m| *m.borrow_mut() = Some(PluginManager::new()));
        let result = f();
        MANAGER.with(|m| *m.borrow_mut() = previous);
        result
    }

    /// Serializes the process-spawning tests (spawn/dispatch/notification-attach)
    /// against each other: they share the process-global ChannelReady attach
    /// queue and runner bookkeeping; concurrent boots would test
    /// interference, not determinism. Grab at the top of each such test.
    #[cfg(test)]
    pub(crate) static PROCESS_TESTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Spawn one plugin and insert it into the process-wide manager — the
    /// single-plugin twin of the boot-time load path, so tests exercise the
    /// SAME live machinery a real boot uses (broadcasts, dispatches, the API
    /// v13 notification attach). The caller owns cleanup: `remove_plugin(id)`
    /// after the assertions.
    #[cfg(test)]
    pub(crate) fn spawn_into_manager_for_tests(
        cdylib_path: &std::path::Path,
        host: &mut dyn ocs_plugin_api::host::HostApi,
        handler: ocs_plugin_api::process::NotificationHandler,
    ) -> Result<
        (
            String,
            std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
        ),
        String,
    > {
        MANAGER.with(|m| {
            let mut guard = m.borrow_mut();
            let mut manager = guard.take().unwrap_or_default();
            manager.set_notification_handler(handler);
            let result = manager
                .load(cdylib_path, host)
                .map(|id| {
                    let process = manager
                        .grip_endpoint(&id)
                        .map(|(process, _, _)| process)
                        .expect("just loaded");
                    (id, process)
                })
                .map_err(|e| e.to_string());
            *guard = Some(manager);
            result
        })
    }

    /// Remove a loaded plugin from the process-wide manager (test cleanup
    /// after [`spawn_into_manager_for_tests`]); its process shuts down first.
    #[cfg(test)]
    pub(crate) fn remove_plugin_for_tests(id: &str) -> bool {
        remove_plugin(id)
    }

    /// Eagerly shut down all plugin runner processes.
    pub fn shutdown_plugins() {
        if let Some(mut manager) = MANAGER.with(|m| m.borrow_mut().take()) {
            manager.shutdown_all();
        }
    }

    /// Shut down the loaded plugin with `id` and remove it from the manager so
    /// its files can be deleted on Windows. Returns true if the plugin was
    /// loaded and has been removed.
    pub fn remove_plugin(id: &str) -> bool {
        MANAGER.with(|m| {
            let mut guard = m.borrow_mut();
            if let Some(manager) = guard.as_mut() {
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

}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::plugin::v4_support;

    #[test]
    fn api_v14_plugin_from_template_is_compatible_and_pre_v14_rejected() {
        let toml = r#"
[plugin]
id = "opencad.my_plugin"
name = "My Plugin"
version = "0.1.0"
description = "Template plugin"
repository = "https://github.com/example/opencad-my-plugin.git"

[opencad]
api_version = 14
ribbon_order = 60
command_prefixes = ["MP_"]
xdata_apps = ["MYPLUGIN_RECORD"]
"#;
        let p = parse_plugin_toml(toml).expect("parsed");
        assert_eq!(p.api_version, 14);
        assert_eq!(p.repository.as_deref(), Some("example/opencad-my-plugin"));
        assert!(p.command_prefixes.contains(&"MP_".to_string()));
        assert!(
            p.api_compatible(),
            "V14+ self-hosted plugins must be accepted by the current host"
        );
        for old in [2, 3, 13] {
            let old_toml = format!("id=\"old\"\napi_version = {old}");
            let old_p = parse_plugin_toml(&old_toml).expect("parsed");
            assert!(
                !old_p.api_compatible(),
                "pre-V14 plugins (v{old}) must be rejected"
            );
        }
    }

    #[test]
    fn missing_id_is_rejected() {
        assert!(parse_plugin_toml("name = \"x\"").is_none());
    }

    #[test]
    fn discovers_bundled_plugins_and_allows_a_user_override() {
        let base = std::env::temp_dir().join(format!(
            "ocs-bundled-plugin-discovery-{}",
            std::process::id()
        ));
        let bundled = base.join("bundled").join("opencad.python");
        let user = base.join("user").join("opencad.python");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        let manifest = |version: &str| {
            format!(
                "[plugin]\nid=\"opencad.python\"\nversion=\"{version}\"\n[opencad]\napi_version=7\n"
            )
        };
        std::fs::write(bundled.join("plugin.toml"), manifest("bundled")).unwrap();
        std::fs::write(user.join("plugin.toml"), manifest("user")).unwrap();
        std::fs::write(
            bundled.join(format!("plugin.{}", lib_extension())),
            b"fixture",
        )
        .unwrap();
        std::fs::write(user.join(format!("plugin.{}", lib_extension())), b"fixture").unwrap();

        let bundled_only = discover_from_roots(Some(&base.join("bundled")), None);
        assert_eq!(bundled_only.len(), 1);
        assert!(bundled_only[0].bundled);
        assert_eq!(bundled_only[0].version, "bundled");

        let overridden = discover_from_roots(Some(&base.join("bundled")), Some(&base.join("user")));
        assert_eq!(overridden.len(), 1);
        assert!(!overridden[0].bundled);
        assert_eq!(overridden[0].version, "user");
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn incompatible_api_flagged() {
        let p = parse_plugin_toml("id=\"a\"\napi_version = 9999").unwrap();
        assert!(!p.api_compatible());
        assert!(!p.loadable());
    }

    #[test]
    fn undeclared_rustc_is_incompatible() {
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
        assert!(!p.rustc_compatible());
        assert!(!p.loadable());
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
        assert!(
            !p.rustc_compatible(),
            "declared but empty rustc is incompatible"
        );
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
        assert!(
            !p.rustc_compatible(),
            "mismatched rustc should be incompatible"
        );
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
api_version = 3

[opencad]
rustc_version = "rustc 0.0.0-fake"
"#;
        let mut p = parse_plugin_toml(toml).expect("parsed");
        p.lib_present = true;
        assert!(p.rustc_declared);
        assert!(
            p.rustc_compatible(),
            "API v3 helper check still treats rustc gate as v4+"
        );
        assert!(
            !p.loadable(),
            "API v3 plugin must be rejected by the v14+ minimum gate"
        );
    }

    #[test]
    fn undeclared_acadrust_is_incompatible_for_gated_api() {
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
        assert!(!p.acadrust_compatible());
        assert!(!p.loadable());
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
        assert!(
            !p.acadrust_compatible(),
            "declared but empty source is incompatible"
        );
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
            "git+https://github.com/HakanSeven12/opencadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb"
        } else {
            "git+https://github.com/HakanSeven12/opencadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd"
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
        assert!(
            !p.acadrust_compatible(),
            "mismatched codec fingerprint should be incompatible"
        );
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
        assert!(
            p.acadrust_compatible(),
            "matching codec fingerprint should be compatible"
        );
    }

    #[test]
    fn acadrust_gate_only_applies_to_api_v4_and_newer() {
        let toml = r#"
[plugin]
id = "opencad.test"
name = "Test"
version = "0.1.0"
api_version = 3

[opencad]
acadrust_source = "git+https://github.com/HakanSeven12/opencadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb"
"#;
        let mut p = parse_plugin_toml(toml).expect("parsed");
        p.lib_present = true;
        assert!(p.acadrust_declared);
        assert!(
            p.acadrust_compatible(),
            "API v3 helper check still treats codec gate as v4+"
        );
        assert!(
            !p.loadable(),
            "API v3 plugin must be rejected by the v14+ minimum gate"
        );
    }

    /// Integration smoke test for the out-of-process plugin path.
    /// Set `OCS_TEST_PLUGIN` to the built cdylib path and make sure the
    /// `OpenCADStudio` binary is built; the test uses it as the runner host.
    #[test]
    fn with_manager_reentry_survives_the_first_write_boundary() {
        // The FIRST-WRITE-BOUNDARY hang class (v13 AppHangB1, the consumer's
        // CVI_ALIGN_BYPI repro): the dispatch path holds the manager access
        // for the whole call, and the plugin's inline-served write frames
        // re-enter `with_manager` at their boundary broadcasts. This guards
        // the REENTRANCY directly; a watchdog thread turns a regression
        // into a bounded failure instead of another AppHangB1 (the suite
        // must never hang).
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&done);
        let worker = std::thread::spawn(move || {
            // The dispatch shape: an outer with_manager whose closure —
            // while the manager access is held — triggers the boundary
            // broadcasts that themselves go through with_manager, exactly
            // what a plugin's inline-served add_entity does.
            with_manager(|manager| {
                let _ = manager.ids();
                crate::plugin::v4_support::publish_document_changed(93);
                crate::plugin::v4_support::publish_document_changed_regions(
                    1,
                    93,
                    Vec::new(),
                    Vec::new(),
                );
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        });
        for _ in 0..100 {
            if done.load(std::sync::atomic::Ordering::SeqCst) {
                worker.join().expect("worker");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!(
            "with_manager self-deadlocked on re-entry — the first-write-boundary \
             AppHangB1 class is back (a non-reentrant manager access)"
        );
    }

    #[test]
    fn spawn_and_dispatch_test_plugin() {
        let _guard = PROCESS_TESTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Process-spawning tests tolerate a busy machine: raise the plugin
        // call timeout for the whole test binary (never lowers any floor).
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "10");
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
            None,
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

        // The FIRST-WRITE-BOUNDARY shape (the v13 AppHangB1 class): the
        // nested write frames of a dispatched command are served inline
        // under the manager access the dispatch holds; the boundary publish
        // must re-enter cleanly. Bounded: the call's own timeout would fire
        // on a regression, never a hang.
        let start = std::time::Instant::now();
        let wrote = process
            .dispatch(&mut host, "MP_WRITE", &mut |_| {})
            .unwrap_or_else(|e| panic!("MP_WRITE failed: {e}"));
        assert!(wrote, "MP_WRITE must dispatch");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "the first write boundary spent {:?} — regression shape",
            start.elapsed()
        );
    }

    /// Deterministic boot (API v12 hardening): plugin commands must dispatch
    /// on N consecutive FRESH spawns — the consumer's real-host validation
    /// saw a boot-dependent "unknown command" flake where both plugins were
    /// listed as loaded yet dispatch answered unhandled. Each iteration is a
    /// full fresh boot: a new app, a new runner process, a fresh handshake.
    #[test]
    fn plugin_dispatch_is_deterministic_across_consecutive_fresh_spawns() {
        let _guard = PROCESS_TESTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Process-spawning tests tolerate a busy machine: raise the plugin
        // call timeout for the whole test binary (never lowers any floor).
        std::env::set_var("OCS_PLUGIN_CALL_TIMEOUT_SECS", "10");
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

        const FRESH_BOOTS: usize = 3;
        for boot in 1..=FRESH_BOOTS {
            let mut app = crate::app::OpenCADStudio::new_for_test();
            let mut host = crate::app::plugin_host::HostSession::new(&mut app, 0);
            let process = ocs_plugin_api::process::PluginProcess::spawn(
                &path,
                &mut host,
                v4_support::notification_handler(),
                None,
            )
            .unwrap_or_else(|e| panic!("fresh boot {boot}: spawn failed: {e}"));
            let handled = process
                .dispatch(&mut host, "MP_HELLO", &mut |_| {})
                .unwrap_or_else(|e| panic!("fresh boot {boot}: dispatch failed: {e}"));
            assert!(
                handled,
                "fresh boot {boot}/{FRESH_BOOTS}: MP_HELLO must dispatch — a boot-dependent \
                 dispatch flake is a release blocker"
            );
            process.shutdown();
        }
    }

    struct ScratchDir(std::path::PathBuf);
    impl ScratchDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ocs_ext_{label}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn plugin_id_validation_rejects_traversal_and_reserved_names() {
        assert!(is_valid_plugin_id("opencad.my_plugin-1"));
        assert!(is_valid_plugin_id("A1.b"));
        for bad in [
            "",
            ".",
            "..",
            "../evil",
            "a/b",
            "a\\b",
            "C:evil",
            "foo\0bar",
            ".hidden",
            "foo.",
            "foo..bar",
            "con",
            "CON.plugin",
            "prn",
            "aux",
            "nul",
            "com1",
            "COM9.extra",
            "lpt1",
            "LPT9",
        ] {
            assert!(!is_valid_plugin_id(bad), "expected rejection for {bad:?}");
        }
        let too_long = "a".repeat(65);
        assert!(!is_valid_plugin_id(&too_long));
    }

    #[test]
    fn parse_plugin_toml_rejects_invalid_and_traversal_ids() {
        for bad_id in ["../escape", "foo/bar", "CON", ""] {
            let toml = format!("id = \"{bad_id}\"\napi_version = 14\n");
            assert!(
                parse_plugin_toml(&toml).is_none(),
                "expected rejection for id {bad_id:?}"
            );
        }
    }

    #[test]
    fn lib_file_for_rejects_traversal_and_nested_library_paths() {
        let dir = ScratchDir::new("lib_file_for_guard");
        let ext = lib_extension();
        let nested_dir = dir.0.join("nested");
        std::fs::create_dir_all(&nested_dir).unwrap();
        std::fs::write(nested_dir.join(format!("sub.{ext}")), b"nested").unwrap();
        let valid_name = format!("good.{ext}");
        std::fs::write(dir.0.join(&valid_name), b"good").unwrap();

        let base = ExternalPlugin {
            id: "opencad.test".into(),
            name: String::new(),
            version: String::new(),
            description: String::new(),
            repository: None,
            library: Some(valid_name),
            api_version: 14,
            acadrust_source: None,
            acadrust_declared: false,
            rustc_version: None,
            rustc_declared: false,
            ribbon_order: 0,
            command_prefixes: Vec::new(),
            dir: dir.0.clone(),
            lib_present: true,
            bundled: false,
        };
        assert!(loader::lib_file_for(&base).is_some());

        for bad_lib in [
            format!("../escape.{ext}"),
            format!("nested/sub.{ext}"),
            format!("/abs/lib.{ext}"),
            format!("./good.{ext}"),
        ] {
            let mut tampered = base.clone();
            tampered.library = Some(bad_lib.clone());
            assert!(
                loader::lib_file_for(&tampered).is_none(),
                "expected rejection for library path {bad_lib:?}"
            );
        }
    }

    #[test]
    fn uninstall_in_root_rejects_traversal_and_removes_discovered_dir() {
        let sandbox = ScratchDir::new("uninstall_guard");
        let plugins_root = sandbox.0.join("plugins");
        let victim = sandbox.0.join("victim");
        std::fs::create_dir_all(&plugins_root).unwrap();
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), b"keep").unwrap();

        assert!(uninstall_in_root(&plugins_root, "../victim").is_err());
        assert!(victim.exists());

        #[cfg(unix)]
        {
            let link = plugins_root.join("opencad.linked");
            std::os::unix::fs::symlink(&victim, &link).unwrap();
            assert!(uninstall_in_root(&plugins_root, "opencad.linked").is_err());
            assert!(victim.join("keep.txt").exists());
            let _ = std::fs::remove_file(&link);
        }

        let custom_dir = plugins_root.join("custom-folder");
        std::fs::create_dir_all(&custom_dir).unwrap();
        std::fs::write(
            custom_dir.join("plugin.toml"),
            "id = \"opencad.custom\"\napi_version = 14\n",
        )
        .unwrap();
        uninstall_in_root(&plugins_root, "opencad.custom").unwrap();
        assert!(!custom_dir.exists());
    }

    #[test]
    fn normalize_repository_rejects_malformed_and_preserves_git_suffix_names() {
        assert_eq!(
            normalize_repository("https://github.com/owner/rust"),
            Some("owner/rust".to_string())
        );
        assert_eq!(
            normalize_repository("https://github.com/owner/demo.git/"),
            Some("owner/demo".to_string())
        );
        for bad in [
            "../evil",
            "owner/..",
            "owner/repo?x=1",
            "owner/repo#frag",
            "https://evil.com/owner/repo",
            "owner/repo/extra",
        ] {
            assert_eq!(
                normalize_repository(bad),
                None,
                "expected None for {bad:?}"
            );
        }
    }

    #[test]
    fn bundled_plugin_is_pre_trusted_while_user_override_requires_lockfile() {
        let sandbox = ScratchDir::new("bundled_trust");
        let plugins_root = sandbox.0.join("user");
        let pkg_dir = sandbox.0.join("bundled/opencad.python");
        std::fs::create_dir_all(&plugins_root).unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let lib_name = format!("py.{}", lib_extension());
        std::fs::write(pkg_dir.join(&lib_name), b"bundled-binary").unwrap();

        let bundled = ExternalPlugin {
            id: "opencad.python".into(),
            name: "Python".into(),
            version: "1.0.0".into(),
            description: String::new(),
            repository: None,
            api_version: ocs_plugin_api::manifest::API_VERSION,
            acadrust_source: Some(ocs_plugin_api::version_info::host_acadrust_source().to_string()),
            acadrust_declared: true,
            rustc_version: Some(ocs_plugin_api::version_info::host_rustc_version().to_string()),
            rustc_declared: true,
            ribbon_order: 0,
            command_prefixes: Vec::new(),
            dir: pkg_dir.clone(),
            bundled: true,
            lib_present: true,
            library: Some(lib_name.clone()),
        };

        let lock = load_lockfile_from(&lockfile_path_in(&plugins_root));
        assert_eq!(evaluate_plugin_trust(&bundled, &lock), PluginTrustState::Bundled);

        let mut user_override = bundled.clone();
        user_override.bundled = false;
        assert!(matches!(
            evaluate_plugin_trust(&user_override, &lock),
            PluginTrustState::Unapproved { .. }
        ));
    }

    #[test]
    fn lockfile_round_trip_detects_approval_tampering_and_corrupt_lockfile() {
        let sandbox = ScratchDir::new("lock_round_trip");
        let plugins_root = sandbox.0.join("plugins");
        let pkg_dir = plugins_root.join("opencad.demo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let lib_name = format!("demo.{}", lib_extension());
        let lib_path = pkg_dir.join(&lib_name);
        std::fs::write(&lib_path, b"plugin-binary-v1").unwrap();

        let plugin = ExternalPlugin {
            id: "opencad.demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            description: String::new(),
            repository: Some("owner/demo".into()),
            api_version: ocs_plugin_api::manifest::API_VERSION,
            acadrust_source: Some(ocs_plugin_api::version_info::host_acadrust_source().to_string()),
            acadrust_declared: true,
            rustc_version: Some(ocs_plugin_api::version_info::host_rustc_version().to_string()),
            rustc_declared: true,
            ribbon_order: 0,
            command_prefixes: Vec::new(),
            dir: pkg_dir.clone(),
            bundled: false,
            lib_present: true,
            library: Some(lib_name.clone()),
        };

        let lock_path = lockfile_path_in(&plugins_root);
        let initial_lock = load_lockfile_from(&lock_path);
        assert!(matches!(
            evaluate_plugin_trust(&plugin, &initial_lock),
            PluginTrustState::Unapproved { .. }
        ));

        let entry = approve_plugin_in_root(&plugins_root, &plugin, Some("v1.0.0")).unwrap();
        let approved_lock = load_lockfile_from(&lock_path);
        assert_eq!(
            evaluate_plugin_trust(&plugin, &approved_lock),
            PluginTrustState::Approved {
                sha256: entry.sha256.clone()
            }
        );

        // Modifying 1 byte triggers HashMismatch with exact digests.
        std::fs::write(&lib_path, b"plugin-binary-v2").unwrap();
        let tampered_digest = sha256_bytes(b"plugin-binary-v2");
        assert_eq!(
            evaluate_plugin_trust(&plugin, &approved_lock),
            PluginTrustState::HashMismatch {
                expected: entry.sha256.clone(),
                actual: tampered_digest.clone(),
                library: lib_name.clone(),
            }
        );

        // Re-approving updates the lockfile to the new digest.
        let reapproved = approve_plugin_in_root(&plugins_root, &plugin, Some("v1.0.1")).unwrap();
        assert_eq!(reapproved.sha256, tampered_digest);
        let refreshed_lock = load_lockfile_from(&lock_path);
        assert_eq!(
            evaluate_plugin_trust(&plugin, &refreshed_lock),
            PluginTrustState::Approved {
                sha256: tampered_digest
            }
        );

        // Corrupt plugin.lock.json fails closed as Unverifiable.
        std::fs::write(&lock_path, b"{not valid json").unwrap();
        let corrupt = load_lockfile_from(&lock_path);
        assert!(matches!(
            evaluate_plugin_trust(&plugin, &corrupt),
            PluginTrustState::Unverifiable(_)
        ));
    }

    #[test]
    fn uninstall_in_root_revokes_lockfile_entry() {
        let sandbox = ScratchDir::new("revoke_lock");
        let plugins_root = sandbox.0.join("plugins");
        let pkg_dir = plugins_root.join("opencad.temp");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let lib_name = format!("temp.{}", lib_extension());
        std::fs::write(pkg_dir.join(&lib_name), b"bin").unwrap();
        std::fs::write(pkg_dir.join("plugin.toml"), "id = \"opencad.temp\"\napi_version = 14\n")
            .unwrap();
        record_lock_entry_in_root(
            &plugins_root,
            "opencad.temp",
            &lib_name,
            &sha256_bytes(b"bin"),
            None,
            None,
            None,
        )
        .unwrap();

        uninstall_in_root(&plugins_root, "opencad.temp").unwrap();
        let lock = load_lockfile_from(&lockfile_path_in(&plugins_root)).unwrap();
        assert!(!lock.plugins.contains_key("opencad.temp"));
    }
}
