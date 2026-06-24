//! Phase-2 plugin marketplace (desktop): install external add-ons from a linked
//! GitHub repository's Releases.
//!
//! Flow: the user links an `owner/repo`; the host reads its releases, offers the
//! ones carrying a binary for this platform, and on install downloads that
//! binary plus `plugin.toml` into the plugins folder. The API-version gate runs
//! at install time (and again at load). Security (signatures, sandboxing) is
//! intentionally out of scope here — the user vouches for the repos they link.

#![cfg(not(target_arch = "wasm32"))]

use std::io::Read as _;

use super::external;
use super::external::RegistryEntry;

/// The curated registry, read from the OpenCADStudio repo's `main` branch.
const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/HakanSeven12/OpenCADStudio/main/plugins/registry.json";

/// Fetch the curated plugin registry (`plugins/registry.json`).
pub fn fetch_registry() -> Result<Vec<RegistryEntry>, String> {
    let body = download_string(REGISTRY_URL)?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let arr = json.as_array().ok_or("registry is not a JSON array")?;
    Ok(arr
        .iter()
        .filter_map(|e| {
            let repo = e["repo"].as_str()?.to_string();
            Some(RegistryEntry {
                repo,
                name: e["name"].as_str().unwrap_or_default().to_string(),
                description: e["description"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// One release of a linked repo.
#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

impl Release {
    /// The platform-matching native library asset, if the release has one.
    fn lib_asset(&self) -> Option<&Asset> {
        let ext = external_lib_ext();
        let suffix = format!("{}.{ext}", platform_suffix());
        self.assets
            .iter()
            .find(|a| a.name.ends_with(&suffix))
            .or_else(|| {
                self.assets
                    .iter()
                    .find(|a| a.name.ends_with(&format!(".{ext}")))
            })
    }

    fn toml_asset(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == "plugin.toml")
    }

    /// True when this release ships an installable package for this platform.
    pub fn installable(&self) -> bool {
        self.lib_asset().is_some() && self.toml_asset().is_some()
    }
}

/// `os-arch` tag used in release asset names, e.g. `linux-x86_64`.
fn platform_suffix() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!("{os}-{arch}")
}

/// Native dynamic-library extension for this platform (no dot).
fn external_lib_ext() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into()
}

const UA: &str = concat!("OpenCADStudio/", env!("CARGO_PKG_VERSION"));

/// Fetch the releases of `owner/repo` from the GitHub API.
pub fn fetch_releases(repo: &str) -> Result<Vec<Release>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let body = agent()
        .get(&url)
        .header("User-Agent", UA)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let arr = json.as_array().ok_or("unexpected releases response")?;
    let mut out = Vec::new();
    for r in arr {
        let tag = r["tag_name"].as_str().unwrap_or_default().to_string();
        let assets = r["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|asset| {
                        let name = asset["name"].as_str()?.to_string();
                        let url = asset["browser_download_url"].as_str()?.to_string();
                        Some(Asset { name, url })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !tag.is_empty() {
            out.push(Release { tag, assets });
        }
    }
    Ok(out)
}

fn download_string(url: &str) -> Result<String, String> {
    agent()
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    agent()
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .as_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Download and install a release's package into the plugins folder. Verifies
/// the API version from the package's `plugin.toml` first. Returns the plugin
/// id on success.
pub fn install(release: &Release) -> Result<String, String> {
    let lib = release.lib_asset().ok_or("no library for this platform")?;
    let toml = release.toml_asset().ok_or("release has no plugin.toml")?;

    let toml_text = download_string(&toml.url)?;
    let manifest = external::parse_plugin_toml(&toml_text).ok_or("plugin.toml is missing an id")?;
    if !ocs_plugin_api::host_accepts_plugin_version(manifest.api_version) {
        return Err(format!(
            "API version {} is incompatible (host supports {}-{})",
            manifest.api_version,
            ocs_plugin_api::API_VERSION_MIN_SUPPORTED,
            ocs_plugin_api::API_VERSION
        ));
    }

    let dir = external::plugins_dir()
        .ok_or("cannot locate the plugins folder")?
        .join(&manifest.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let bytes = download_bytes(&lib.url)?;
    // Clean upgrade / reinstall: drop any previously-installed native library
    // with a different name so the loader doesn't pick a stale one. The
    // currently-resident library (if loaded) keeps running until restart.
    let ext = external_lib_ext();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for old in rd.flatten() {
            let p = old.path();
            let is_lib = p.extension().and_then(|s| s.to_str()) == Some(ext);
            if is_lib && p.file_name().and_then(|s| s.to_str()) != Some(lib.name.as_str()) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    std::fs::write(dir.join(&lib.name), bytes).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("plugin.toml"), toml_text).map_err(|e| e.to_string())?;
    Ok(manifest.id)
}
