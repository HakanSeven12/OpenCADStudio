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
use std::path::{Path, PathBuf};

use super::external;
use super::external::{RegistryEntry, ReleaseInfo};

/// The curated registry, read from the OpenCADStudio repo's `main` branch.
pub(crate) const REGISTRY_URL: &str =
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
    crate::network::agent(std::time::Duration::from_secs(15))
}

const UA: &str = concat!("OpenCADStudio/", env!("CARGO_PKG_VERSION"));

/// Fetch releases without consuming GitHub API quota. The public Atom feed
/// supplies tags and the expanded-assets endpoint supplies download links.
/// Fall back to the API only if GitHub changes either public representation.
pub fn fetch_releases(repo: &str) -> Result<Vec<Release>, String> {
    match fetch_releases_public(repo) {
        Ok(releases) => Ok(releases),
        Err(public_error) => fetch_releases_api(repo).map_err(|api_error| {
            format!(
                "public release metadata failed: {public_error}; \
                 GitHub API fallback failed: {api_error}"
            )
        }),
    }
}

fn fetch_releases_public(repo: &str) -> Result<Vec<Release>, String> {
    let atom_url = format!("https://github.com/{repo}/releases.atom");
    let atom = download_string(&atom_url)?;
    let tag_marker = format!("https://github.com/{repo}/releases/tag/");
    let mut tags = atom
        .lines()
        .filter_map(|line| {
            let start = line.find(&tag_marker)? + tag_marker.len();
            let rest = &line[start..];
            let end = rest.find('"')?;
            let tag = &rest[..end];
            (!tag.is_empty()).then(|| tag.to_string())
        })
        .collect::<Vec<_>>();
    tags.dedup();
    if tags.is_empty() {
        return Err("release feed contains no tags".to_string());
    }

    let mut releases = Vec::new();
    for tag in tags {
        let assets_url = format!("https://github.com/{repo}/releases/expanded_assets/{tag}");
        let html = download_string(&assets_url)?;
        let asset_prefix = format!("/{repo}/releases/download/{tag}/");
        let mut assets = html
            .split("href=\"")
            .skip(1)
            .filter_map(|tail| {
                let end = tail.find('"')?;
                let href = &tail[..end];
                if !href.starts_with(&asset_prefix) {
                    return None;
                }
                let name = href.rsplit('/').next()?.to_string();
                Some(Asset {
                    name,
                    url: format!("https://github.com{href}"),
                })
            })
            .collect::<Vec<_>>();
        assets.dedup_by(|left, right| left.url == right.url);
        releases.push(Release { tag, assets });
    }
    Ok(releases)
}

fn fetch_releases_api(repo: &str) -> Result<Vec<Release>, String> {
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

/// Fetch installable releases and read each bundled manifest so compatibility
/// is known before the Plugin Manager offers an Install action.
pub fn fetch_release_info(repo: &str) -> Result<Vec<ReleaseInfo>, String> {
    let releases = fetch_releases(repo)?;
    let mut info = Vec::new();
    let mut last_error = None;
    for release in releases.into_iter().filter(Release::installable) {
        let result = (|| {
            let manifest_asset = release
                .toml_asset()
                .ok_or_else(|| format!("release {} has no plugin.toml", release.tag))?;
            let manifest_text = download_string(&manifest_asset.url)?;
            let manifest = external::parse_plugin_toml(&manifest_text)
                .ok_or_else(|| format!("release {} plugin.toml is missing an id", release.tag))?;
            let acadrust_source = manifest.acadrust_source.clone();
            let acadrust_declared = manifest.acadrust_declared;
            let acadrust_compatible = if !ocs_plugin_api::version_info::uses_acadrust_gate(
                manifest.api_version,
            ) {
                true
            } else if !acadrust_declared {
                true
            } else {
                match acadrust_source.as_deref() {
                    None | Some("") => false,
                    Some(source) => ocs_plugin_api::version_info::acadrust_sources_compatible(
                        source,
                        ocs_plugin_api::version_info::host_acadrust_source(),
                    ),
                }
            };
            Ok::<_, String>(ReleaseInfo {
                tag: release.tag,
                api_version: manifest.api_version,
                acadrust_source,
                acadrust_declared,
                acadrust_compatible,
            })
        })();
        match result {
            Ok(release) => info.push(release),
            Err(error) => last_error = Some(error),
        }
    }

    // Once a repo declares a fingerprint, require it on later API versions.
    let any_declared = info.iter().any(|r| r.acadrust_declared);
    if any_declared {
        for r in &mut info {
            if !r.acadrust_declared
                && ocs_plugin_api::version_info::uses_acadrust_gate(r.api_version)
            {
                r.acadrust_compatible = false;
            }
        }
    }

    if info.is_empty() {
        Err(last_error.unwrap_or_else(|| "no installable releases found".to_string()))
    } else {
        Ok(info)
    }
}

/// Fetch the repository README from its default branch as raw Markdown.
pub fn fetch_readme(repo: &str) -> Result<String, String> {
    // GitHub's raw host is not subject to the unauthenticated REST API's
    // 60-request/hour limit. `HEAD` resolves the repository's default branch.
    let raw_url = format!("https://raw.githubusercontent.com/{repo}/HEAD/README.md");
    if let Ok(readme) = download_string(&raw_url) {
        return Ok(readme);
    }

    let url = format!("https://api.github.com/repos/{repo}/readme");
    agent()
        .get(&url)
        .header("User-Agent", UA)
        .header("Accept", "application/vnd.github.raw+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
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
/// the API version and, when present, the `acadrust` fingerprint from the
/// package's `plugin.toml` first. Returns the plugin id on success.
pub fn install(release: &Release, repository: &str) -> Result<String, String> {
    let lib = release.lib_asset().ok_or("no library for this platform")?;
    let toml = release.toml_asset().ok_or("release has no plugin.toml")?;

    let toml_text = download_string(&toml.url)?;
    let manifest = external::parse_plugin_toml(&toml_text).ok_or("plugin.toml is missing an id")?;
    if !ocs_plugin_api::manifest::host_accepts_plugin_version(manifest.api_version) {
        return Err(format!(
            "API version {} is incompatible (host supports {}-{})",
            manifest.api_version,
            ocs_plugin_api::manifest::API_VERSION_MIN_SUPPORTED,
            ocs_plugin_api::manifest::effective_max_api_version()
        ));
    }

    if ocs_plugin_api::version_info::uses_acadrust_gate(manifest.api_version)
        && manifest.acadrust_declared
    {
        let Some(source) = manifest.acadrust_source.as_deref() else {
            return Err(
                "Release declares acadrust metadata but has no source; cannot verify ABI compatibility".to_string(),
            );
        };
        if source.is_empty() {
            return Err(
                "Release declares acadrust metadata but has no source; cannot verify ABI compatibility".to_string(),
            );
        }
        if !ocs_plugin_api::version_info::acadrust_sources_compatible(
            source,
            ocs_plugin_api::version_info::host_acadrust_source(),
        ) {
            let host_src = ocs_plugin_api::version_info::host_acadrust_source();
            let plugin_hash = ocs_plugin_api::version_info::acadrust_source_hash(source)
                .unwrap_or("unknown");
            let host_hash = ocs_plugin_api::version_info::acadrust_source_hash(host_src)
                .unwrap_or("unknown");
            return Err(format!(
                "Plugin built for acadrust @{plugin_hash}, but this host uses @{host_hash}"
            ));
        }
    }

    let dir = external::plugins_dir()
        .ok_or("cannot locate the plugins folder")?
        .join(&manifest.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let bytes = download_bytes(&lib.url)?;
    replace_library(&dir, &lib.name, external_lib_ext(), &bytes)?;
    std::fs::write(dir.join("plugin.toml"), toml_text).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(".source_repo"), format!("{repository}\n"))
        .map_err(|e| e.to_string())?;
    Ok(manifest.id)
}

/// Install `bytes` as the package's native library at `dir/lib_name`, upgrading
/// in place even when the previous version is currently loaded.
///
/// A loaded `cdylib` is memory-mapped by its runner process, so on Windows its
/// file cannot be truncated or removed — a plain overwrite fails with "being
/// used by another process". It *can*, however, be renamed (the loader opens it
/// with `FILE_SHARE_DELETE`). So we move every existing library in the package
/// aside to a `.old` stash and then write the new one under its own name. The
/// stash keeps serving the running session; the loader picks up the freshly
/// written library on the next start. Stale stashes are swept here on the next
/// upgrade, once the process holding them has exited and released the lock.
///
/// This also subsumes the old "drop differently-named libraries" cleanup:
/// stashing every resident `.<ext>` file means neither a same-named nor a
/// legacy-named binary is left behind for the loader to pick up.
fn replace_library(dir: &Path, lib_name: &str, ext: &str, bytes: &[u8]) -> Result<(), String> {
    // Sweep stashes left by previous upgrades. A still-locked one (its session
    // is somehow still alive) just fails to remove and is retried next time.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("old") {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    // Move every resident library out of the way so a loaded (locked) binary
    // doesn't block the write, whatever its filename.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some(ext) {
                stash_aside(&p);
            }
        }
    }
    std::fs::write(dir.join(lib_name), bytes).map_err(|e| e.to_string())
}

/// Rename a resident (possibly loaded, hence locked) library out of the way so
/// a new one can take its place. Stashes are named `<lib>.<n>.old` — the `.old`
/// extension keeps them clear of the loader's `.<ext>` scan, and the numeric
/// slot lets several in-session upgrades coexist without colliding. Best-effort:
/// rename works on a loaded DLL; if it can't, fall back to a direct remove, and
/// if that fails too the caller's write surfaces the real lock error.
fn stash_aside(path: &Path) {
    for n in 0..64 {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{n}.old"));
        let stash = PathBuf::from(name);
        // Reuse a slot only if we can clear whatever occupies it.
        if stash.exists() && std::fs::remove_file(&stash).is_err() {
            continue;
        }
        if std::fs::rename(path, &stash).is_ok() {
            return;
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocs_mkt_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Happy path: the new library lands under its name, the old one is stashed,
    /// and a second upgrade sweeps the (now-unlocked) stash instead of piling up.
    #[test]
    fn replace_library_swaps_and_sweeps() {
        let dir = scratch("swap");
        let lib = "opencad.demo-x86_64.dll";
        std::fs::write(dir.join(lib), b"V1").unwrap();

        replace_library(&dir, lib, "dll", b"V2").unwrap();
        assert_eq!(std::fs::read(dir.join(lib)).unwrap(), b"V2");
        let stashes = |d: &Path| {
            std::fs::read_dir(d)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("old"))
                .collect::<Vec<_>>()
        };
        let s = stashes(&dir);
        assert_eq!(s.len(), 1, "exactly one stash after first upgrade");
        assert_eq!(
            std::fs::read(&s[0]).unwrap(),
            b"V1",
            "stash holds the old lib"
        );

        replace_library(&dir, lib, "dll", b"V3").unwrap();
        assert_eq!(std::fs::read(dir.join(lib)).unwrap(), b"V3");
        assert_eq!(
            stashes(&dir).len(),
            1,
            "second upgrade sweeps the old stash, doesn't accumulate",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A differently-named legacy library is stashed aside too, so the loader
    /// won't find a stale `.<ext>` beside the new one.
    #[test]
    fn replace_library_clears_legacy_named_lib() {
        let dir = scratch("legacy");
        std::fs::write(dir.join("old_name_plugin.dll"), b"LEGACY").unwrap();
        replace_library(&dir, "new.name-x86_64.dll", "dll", b"NEW").unwrap();

        let live_libs: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dll"))
            .collect();
        assert_eq!(live_libs.len(), 1, "only the new lib carries the .dll ext");
        assert!(live_libs[0].ends_with("new.name-x86_64.dll"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The reported bug, reproduced faithfully: a currently-loaded DLL is held
    /// open the way the Windows loader holds it (`FILE_SHARE_READ | DELETE`, no
    /// write share). A plain overwrite fails — that's the lock the user hit —
    /// yet `replace_library` upgrades in place anyway.
    #[cfg(windows)]
    #[test]
    fn replace_library_upgrades_a_locked_binary() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_DELETE: u32 = 0x4;

        let dir = scratch("locked");
        let lib = "opencad.landsurvey-windows-x86_64.dll";
        let path = dir.join(lib);
        std::fs::write(&path, b"RESIDENT").unwrap();

        // Simulate the loaded DLL: the loader's handle shares read+delete but
        // not write, exactly like a memory-mapped image section.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();

        assert!(
            std::fs::write(&path, b"UPGRADE").is_err(),
            "precondition: a locked binary can't be overwritten in place",
        );

        replace_library(&dir, lib, "dll", b"UPGRADE").expect("upgrade under lock");
        assert_eq!(std::fs::read(&path).unwrap(), b"UPGRADE");

        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
