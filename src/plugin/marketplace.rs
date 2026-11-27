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
pub(crate) const REGISTRY_URL: &str = concat!(
    "https://raw.githubusercontent.com/HakanSeven12/OpenCADStudio/v",
    env!("OCS_APP_VERSION"),
    "/plugins/registry.json"
);

const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_BUNDLE_ENTRIES: usize = 256;
const MAX_BUNDLE_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_UNCOMPRESSED_TOTAL: u64 = 256 * 1024 * 1024;
const MAX_BUNDLE_COMPRESSION_RATIO: u64 = 100;

fn normalize_repo_arg(repo: &str) -> Result<String, String> {
    external::normalize_repository(repo).ok_or_else(|| format!("invalid repository slug `{repo}`"))
}

fn is_valid_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag != "."
        && tag != ".."
        && !tag.contains("..")
        && tag.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(b, b'/' | b'\\' | b'?' | b'#' | b'%' | b'<' | b'>' | b'"' | b'\'')
        })
}

fn is_valid_asset_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return false;
    }
    let mut comps = Path::new(name).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

fn is_allowed_github_url(url: &str, expected_repo: Option<&str>) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if url
        .bytes()
        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return false;
    }
    let Some((authority, path_and_query)) = rest.split_once('/') else {
        return false;
    };
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if let Some(repo) = expected_repo {
        if authority == "github.com" {
            let expected_prefix = format!("{repo}/releases/download/");
            return path_and_query.starts_with(&expected_prefix);
        }
    }
    matches!(
        authority,
        "github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "raw.githubusercontent.com"
    )
}

/// Fetch the curated plugin registry (`plugins/registry.json`).
fn parse_registry_sha256(value: &serde_json::Value) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut map = std::collections::BTreeMap::new();
    if value.is_null() {
        return Ok(map);
    }
    if let Some(hex) = value.as_str() {
        let trimmed = hex.trim();
        if !external::is_valid_sha256_hex(trimmed) {
            return Err(format!("invalid registry sha256 `{trimmed}`"));
        }
        map.insert("*".to_string(), trimmed.to_ascii_lowercase());
        return Ok(map);
    }
    if let Some(obj) = value.as_object() {
        for (key, val) in obj {
            let Some(hex) = val.as_str() else {
                return Err(format!("registry sha256 for `{key}` must be a hex string"));
            };
            let trimmed = hex.trim();
            if !external::is_valid_sha256_hex(trimmed) {
                return Err(format!("invalid registry sha256 `{trimmed}` for `{key}`"));
            }
            map.insert(key.clone(), trimmed.to_ascii_lowercase());
        }
        return Ok(map);
    }
    Err("registry sha256 must be a string or map".to_string())
}

pub fn fetch_registry() -> Result<Vec<RegistryEntry>, String> {
    let body = download_string(REGISTRY_URL)?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let arr = json.as_array().ok_or("registry is not a JSON array")?;
    let mut entries = Vec::new();
    for e in arr {
        let Some(raw_repo) = e["repo"].as_str() else {
            continue;
        };
        let Some(repo) = external::normalize_repository(raw_repo) else {
            continue;
        };
        let sha256 = parse_registry_sha256(&e["sha256"])?;
        let signature = e["signature"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        entries.push(RegistryEntry {
            repo,
            name: e["name"].as_str().unwrap_or_default().to_string(),
            description: e["description"].as_str().unwrap_or_default().to_string(),
            sha256,
            signature,
        });
    }
    Ok(entries)
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

    /// Bundles of further plugins for this platform: `<anything>-<platform>.zip`
    /// holding one `plugin.toml` + library per plugin, at its root or one
    /// folder per plugin.
    fn bundle_assets(&self) -> impl Iterator<Item = &Asset> {
        let suffix = format!("-{}.zip", platform_suffix());
        self.assets
            .iter()
            .filter(move |a| a.name.ends_with(&suffix))
    }

    /// True when this release ships an installable package for this platform.
    pub fn installable(&self) -> bool {
        (self.lib_asset().is_some() && self.toml_asset().is_some())
            || self.bundle_assets().next().is_some()
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

const UA: &str = concat!("OpenCADStudio/", env!("OCS_APP_VERSION"));

/// Fetch releases without consuming GitHub API quota. The public Atom feed
/// supplies tags and the expanded-assets endpoint supplies download links.
/// Fall back to the API only if GitHub changes either public representation.
pub fn fetch_releases(repo: &str) -> Result<Vec<Release>, String> {
    let repo = normalize_repo_arg(repo)?;
    match fetch_releases_public(&repo) {
        Ok(releases) => Ok(releases),
        Err(public_error) => fetch_releases_api(&repo).map_err(|api_error| {
            format!(
                "public release metadata failed: {public_error}; \
                 GitHub API fallback failed: {api_error}"
            )
        }),
    }
}

fn fetch_releases_public(repo: &str) -> Result<Vec<Release>, String> {
    let repo = normalize_repo_arg(repo)?;
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
            is_valid_release_tag(tag).then(|| tag.to_string())
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
                let name = href.rsplit('/').next()?;
                if !is_valid_asset_name(name) {
                    return None;
                }
                let url = format!("https://github.com{href}");
                if !is_allowed_github_url(&url, Some(&repo)) {
                    return None;
                }
                Some(Asset {
                    name: name.to_string(),
                    url,
                })
            })
            .collect::<Vec<_>>();
        assets.dedup_by(|left, right| left.url == right.url);
        releases.push(Release { tag, assets });
    }
    Ok(releases)
}

fn fetch_releases_api(repo: &str) -> Result<Vec<Release>, String> {
    let repo = normalize_repo_arg(repo)?;
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let body = download_string_with_accept(&url, Some("application/vnd.github+json"))?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let arr = json.as_array().ok_or("unexpected releases response")?;
    let mut out = Vec::new();
    for r in arr {
        let tag = r["tag_name"].as_str().unwrap_or_default().to_string();
        if !is_valid_release_tag(&tag) {
            continue;
        }
        let assets = r["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|asset| {
                        let name = asset["name"].as_str()?;
                        let url = asset["browser_download_url"].as_str()?;
                        if !is_valid_asset_name(name) || !is_allowed_github_url(url, Some(&repo)) {
                            return None;
                        }
                        Some(Asset {
                            name: name.to_string(),
                            url: url.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Release { tag, assets });
    }
    Ok(out)
}

/// Fetch installable releases and read each bundled manifest so compatibility
/// is known before the Plugin Manager offers an Install action.
pub fn fetch_release_info(repo: &str) -> Result<Vec<ReleaseInfo>, String> {
    let repo = normalize_repo_arg(repo)?;
    let releases = fetch_releases(&repo)?;
    let mut info = Vec::new();
    let mut last_error = None;
    for release in releases.into_iter().filter(Release::installable) {
        let result = (|| {
            let manifest_asset = release
                .toml_asset()
                .ok_or_else(|| format!("release {} has no plugin.toml", release.tag))?;
            if !is_allowed_github_url(&manifest_asset.url, Some(&repo)) {
                return Err(format!(
                    "release {} has disallowed plugin.toml URL",
                    release.tag
                ));
            }
            let manifest_text = download_string(&manifest_asset.url)?;
            let manifest = external::parse_plugin_toml(&manifest_text)
                .ok_or_else(|| format!("release {} plugin.toml is missing an id", release.tag))?;
            let acadrust_source = manifest.acadrust_source.clone();
            let acadrust_declared = manifest.acadrust_declared;
            let acadrust_compatible = external::acadrust_source_compatible(
                manifest.api_version,
                acadrust_declared,
                acadrust_source.as_deref(),
            );
            let rustc_version = manifest.rustc_version.clone();
            let rustc_declared = manifest.rustc_declared;
            let rustc_compatible =
                if !ocs_plugin_api::version_info::uses_acadrust_gate(manifest.api_version) {
                    true
                } else {
                    match rustc_version.as_deref() {
                        None | Some("") => false,
                        Some(version) => ocs_plugin_api::version_info::rustc_versions_compatible(
                            version,
                            ocs_plugin_api::version_info::host_rustc_version(),
                        ),
                    }
                };
            Ok::<_, String>(ReleaseInfo {
                tag: release.tag,
                api_version: manifest.api_version,
                acadrust_source,
                acadrust_declared,
                acadrust_compatible,
                rustc_version,
                rustc_declared,
                rustc_compatible,
            })
        })();
        match result {
            Ok(release) => info.push(release),
            Err(error) => last_error = Some(error),
        }
    }

    // No release for this platform is an answer (the card files under
    // Incompatible), not an error; a release that failed to read is one.
    match last_error {
        Some(error) if info.is_empty() => Err(error),
        _ => Ok(info),
    }
}

/// Fetch the repository README from its default branch as raw Markdown.
pub fn fetch_readme(repo: &str) -> Result<String, String> {
    let repo = normalize_repo_arg(repo)?;
    // GitHub's raw host is not subject to the unauthenticated REST API's
    // 60-request/hour limit. `HEAD` resolves the repository's default branch.
    let raw_url = format!("https://raw.githubusercontent.com/{repo}/HEAD/README.md");
    if let Ok(readme) = download_string(&raw_url) {
        return Ok(readme);
    }

    let url = format!("https://api.github.com/repos/{repo}/readme");
    download_string_with_accept(&url, Some("application/vnd.github.raw+json"))
}

fn download_string_with_accept(url: &str, accept: Option<&str>) -> Result<String, String> {
    let a = agent();
    let mut req = a.get(url).header("User-Agent", UA);
    if let Some(accept) = accept {
        req = req.header("Accept", accept);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_METADATA_BYTES {
        return Err(format!(
            "metadata response exceeded {MAX_METADATA_BYTES} byte limit"
        ));
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn download_string(url: &str) -> Result<String, String> {
    download_string_with_accept(url, None)
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    // A package runs to tens of megabytes; the catalog's 15 s would cut a
    // slow download off midway.
    crate::network::agent(std::time::Duration::from_secs(600))
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .as_reader()
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(format!(
            "package download exceeded {MAX_PACKAGE_BYTES} byte limit"
        ));
    }
    Ok(buf)
}

fn parse_sha256_list_for_asset(text: &str, asset_name: &str) -> Option<String> {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hex) = parts.next() else {
            continue;
        };
        if !external::is_valid_sha256_hex(hex) {
            continue;
        }
        match parts.next() {
            None => return Some(hex.to_ascii_lowercase()),
            Some(name) => {
                let clean = name.trim_start_matches('*');
                let base = Path::new(clean)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(clean);
                if clean == asset_name || base == asset_name {
                    return Some(hex.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

fn verify_asset_sha256(
    asset_name: &str,
    release: &Release,
    repository: &str,
    bytes: &[u8],
    expected_sha256: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<(), String> {
    let actual = external::sha256_bytes(bytes);
    if let Some(map) = expected_sha256 {
        if let Some(expected) = map
            .get(asset_name)
            .or_else(|| map.get(&release.tag))
            .or_else(|| map.get("*"))
        {
            if !external::is_valid_sha256_hex(expected) {
                return Err(format!("invalid expected sha256 for `{asset_name}`"));
            }
            let expected_lower = expected.to_ascii_lowercase();
            if !crate::rest::constant_time_eq(expected_lower.as_bytes(), actual.as_bytes()) {
                return Err(format!(
                    "SHA-256 mismatch for `{asset_name}`: expected {expected_lower}, got {actual}"
                ));
            }
        }
    }
    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{asset_name}.sha256"))
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|a| a.name == "SHA256SUMS" || a.name == "SHA256SUMS.txt")
        });
    if let Some(checksum) = checksum_asset {
        if !is_allowed_github_url(&checksum.url, Some(repository)) {
            return Err(format!("checksum asset `{}` has disallowed URL", checksum.name));
        }
        let body = download_string(&checksum.url)?;
        if let Some(expected) = parse_sha256_list_for_asset(&body, asset_name) {
            if !crate::rest::constant_time_eq(expected.as_bytes(), actual.as_bytes()) {
                return Err(format!(
                    "release checksum mismatch for `{asset_name}`: expected {expected}, got {actual}"
                ));
            }
        }
    }
    Ok(())
}

/// Download and install a release's packages into the plugins folder: its
/// library + `plugin.toml`, and every plugin in its platform bundles. Each
/// `plugin.toml` passes the API-version (and, before v14, ABI) checks before
/// anything is written. Returns the installed plugin ids, comma-separated.
pub fn install(release: &Release, repository: &str) -> Result<String, String> {
    install_verified(release, repository, None)
}

/// Download, verify optional registry/release SHA-256 checksums, install
/// packages into the plugins folder, and record the library hash in `plugin.lock.json`.
pub fn install_verified(
    release: &Release,
    repository: &str,
    expected_sha256: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<String, String> {
    let repository = normalize_repo_arg(repository)?;
    let mut packages = Vec::new();
    if let (Some(lib), Some(toml)) = (release.lib_asset(), release.toml_asset()) {
        if !is_valid_asset_name(&lib.name)
            || !is_allowed_github_url(&lib.url, Some(&repository))
            || !is_allowed_github_url(&toml.url, Some(&repository))
        {
            return Err("release contains invalid asset name or disallowed URL".to_string());
        }
        let toml_text = download_string(&toml.url)?;
        let id = checked_manifest(&toml_text)?.id;
        let lib_bytes = download_bytes(&lib.url)?;
        verify_asset_sha256(&lib.name, release, &repository, &lib_bytes, expected_sha256)?;
        packages.push(Package {
            id,
            toml_text,
            files: vec![(PathBuf::from(&lib.name), lib_bytes)],
        });
    }
    for bundle in release.bundle_assets() {
        if !is_allowed_github_url(&bundle.url, Some(&repository)) {
            return Err("release bundle has disallowed URL".to_string());
        }
        let bundle_bytes = download_bytes(&bundle.url)?;
        verify_asset_sha256(
            &bundle.name,
            release,
            &repository,
            &bundle_bytes,
            expected_sha256,
        )?;
        packages.extend(bundle_packages(&bundle_bytes)?);
    }
    if packages.is_empty() {
        return Err("no package for this platform".to_string());
    }
    let mut ids = Vec::new();
    for package in packages {
        ids.push(write_package_with_tag(
            package,
            &repository,
            Some(&release.tag),
        )?);
    }
    Ok(ids.join(", "))
}

/// One plugin ready to write: its `plugin.toml` and its other files, by path
/// inside the package folder.
struct Package {
    id: String,
    toml_text: String,
    files: Vec<(PathBuf, Vec<u8>)>,
}

/// The packages in a bundle zip: each folder holding a `plugin.toml` (the
/// root included) with the files below it.
fn has_only_normal_components(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|comp| matches!(comp, std::path::Component::Normal(_)))
}

fn bundle_packages(bytes: &[u8]) -> Result<Vec<Package>, String> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(format!(
            "bundle exceeded {MAX_PACKAGE_BYTES} byte compressed limit"
        ));
    }
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("bundle: {e}"))?;
    if archive.len() > MAX_BUNDLE_ENTRIES {
        return Err(format!(
            "bundle contains {} entries (limit {MAX_BUNDLE_ENTRIES})",
            archive.len()
        ));
    }
    let archive_ratio_budget = (bytes.len() as u64)
        .max(1024)
        .saturating_mul(MAX_BUNDLE_COMPRESSION_RATIO);
    let mut total_uncompressed: u64 = 0;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|e| format!("bundle: {e}"))?;
        if file
            .unix_mode()
            .is_some_and(|mode| (mode & 0o170000) == 0o120000)
        {
            return Err("bundle contains a symlink entry".to_string());
        }
        let Some(path) = file.enclosed_name().filter(|_| file.is_file()) else {
            continue;
        };
        if !has_only_normal_components(&path) {
            return Err(format!("bundle entry `{}` has unsafe path components", path.display()));
        }
        let compressed_size = file.compressed_size();
        let entry_ratio_budget = compressed_size
            .max(1024)
            .saturating_mul(MAX_BUNDLE_COMPRESSION_RATIO);
        let remaining = MAX_UNCOMPRESSED_TOTAL.saturating_sub(total_uncompressed);
        let entry_cap = MAX_BUNDLE_ENTRY_BYTES.min(remaining);
        let mut data = Vec::new();
        file.take(entry_cap + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("bundle: {e}"))?;
        let data_len = data.len() as u64;
        if data_len > MAX_BUNDLE_ENTRY_BYTES {
            return Err(format!(
                "bundle entry `{}` exceeded {MAX_BUNDLE_ENTRY_BYTES} byte limit",
                path.display()
            ));
        }
        total_uncompressed = total_uncompressed.saturating_add(data_len);
        if total_uncompressed > MAX_UNCOMPRESSED_TOTAL {
            return Err(format!(
                "bundle uncompressed size exceeded {MAX_UNCOMPRESSED_TOTAL} byte limit"
            ));
        }
        if data_len > entry_ratio_budget || total_uncompressed > archive_ratio_budget {
            return Err(format!(
                "bundle compression ratio exceeded {MAX_BUNDLE_COMPRESSION_RATIO}:1"
            ));
        }
        files.push((path, data));
    }
    let roots: Vec<PathBuf> = files
        .iter()
        .filter(|(path, _)| path.file_name().is_some_and(|name| name == "plugin.toml"))
        .map(|(path, _)| path.parent().unwrap_or(Path::new("")).to_path_buf())
        .collect();
    if roots.is_empty() {
        return Err("bundle has no plugin.toml".to_string());
    }
    let mut packages = Vec::new();
    for root in &roots {
        let toml_path = root.join("plugin.toml");
        let toml_text = files
            .iter()
            .find(|(path, _)| *path == toml_path)
            .and_then(|(_, data)| String::from_utf8(data.clone()).ok())
            .ok_or("bundle: plugin.toml is not UTF-8")?;
        let id = checked_manifest(&toml_text)?.id;
        // A file belongs to the deepest package folder above it.
        let owned = files
            .iter()
            .filter(|(path, _)| *path != toml_path)
            .filter(|(path, _)| {
                roots
                    .iter()
                    .filter(|other| path.starts_with(other))
                    .max_by_key(|other| other.components().count())
                    == Some(root)
            })
            .map(|(path, data)| {
                let inner = path.strip_prefix(root).unwrap_or(path).to_path_buf();
                (inner, data.clone())
            })
            .collect();
        packages.push(Package {
            id,
            toml_text,
            files: owned,
        });
    }
    Ok(packages)
}

/// Parse `plugin.toml` and refuse a plugin this host cannot load.
fn checked_manifest(toml_text: &str) -> Result<external::ExternalPlugin, String> {
    let manifest = external::parse_plugin_toml(toml_text)
        .ok_or("plugin.toml is missing or has an invalid id")?;
    if !external::is_valid_plugin_id(&manifest.id) {
        return Err(format!("invalid plugin id `{}`", manifest.id));
    }
    if let Some(lib) = manifest.library.as_deref() {
        let mut comps = Path::new(lib).components();
        if !matches!(
            (comps.next(), comps.next()),
            (Some(std::path::Component::Normal(_)), None)
        ) {
            return Err(format!("invalid plugin library filename `{lib}`"));
        }
    }
    if !ocs_plugin_api::manifest::host_accepts_plugin_version(manifest.api_version) {
        return Err(format!(
            "API version {} is incompatible (host supports {}-{})",
            manifest.api_version,
            ocs_plugin_api::manifest::API_VERSION_MIN_SUPPORTED,
            ocs_plugin_api::manifest::effective_max_api_version()
        ));
    }

    if ocs_plugin_api::version_info::uses_acadrust_gate(manifest.api_version) {
        let Some(source) = manifest.acadrust_source.as_deref() else {
            return Err("Release has no codec source; cannot verify ABI compatibility".to_string());
        };
        if source.is_empty() {
            return Err("Release has no codec source; cannot verify ABI compatibility".to_string());
        }
        if !ocs_plugin_api::version_info::acadrust_sources_compatible(
            source,
            ocs_plugin_api::version_info::host_acadrust_source(),
        ) {
            let host_src = ocs_plugin_api::version_info::host_acadrust_source();
            let plugin_hash =
                ocs_plugin_api::version_info::acadrust_source_hash(source).unwrap_or("unknown");
            let host_hash =
                ocs_plugin_api::version_info::acadrust_source_hash(host_src).unwrap_or("unknown");
            return Err(format!(
                "Plugin built for codec @{plugin_hash}, but this host uses @{host_hash}"
            ));
        }
    }

    if ocs_plugin_api::version_info::uses_acadrust_gate(manifest.api_version) {
        let Some(version) = manifest.rustc_version.as_deref() else {
            return Err(
                "Release has no rustc version; cannot verify ABI compatibility".to_string(),
            );
        };
        if version.is_empty() {
            return Err(
                "Release has no rustc version; cannot verify ABI compatibility".to_string(),
            );
        }
        if !ocs_plugin_api::version_info::rustc_versions_compatible(
            version,
            ocs_plugin_api::version_info::host_rustc_version(),
        ) {
            let host_rustc = ocs_plugin_api::version_info::host_rustc_version();
            return Err(format!(
                "Plugin built with {version}, host requires {host_rustc} - rebuild required"
            ));
        }
    }

    Ok(manifest)
}

/// Write one package into `plugins/<id>/`; its library replaces the resident
/// one even while that is loaded.
fn write_package_with_tag(
    package: Package,
    repository: &str,
    release_tag: Option<&str>,
) -> Result<String, String> {
    let plugins_root = external::plugins_dir().ok_or("cannot locate the plugins folder")?;
    std::fs::create_dir_all(&plugins_root).map_err(|e| e.to_string())?;
    write_package_in_root_with_tag(&plugins_root, package, repository, release_tag)
}

fn ensure_contained_write_target(dir_canon: &Path, rel: &Path) -> Result<PathBuf, String> {
    if !has_only_normal_components(rel) {
        return Err(format!("unsafe package file path `{}`", rel.display()));
    }
    let mut cursor = dir_canon.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for comp in &comps[..comps.len().saturating_sub(1)] {
        cursor.push(comp.as_os_str());
        match std::fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                    return Err(format!(
                        "unsafe non-directory or symlink segment `{}`",
                        cursor.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&cursor).map_err(|err| err.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let target = dir_canon.join(rel);
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to overwrite symlink `{}` inside package",
                target.display()
            ));
        }
    }
    if let Some(parent) = target.parent() {
        let parent_canon = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
        if !parent_canon.starts_with(dir_canon) {
            return Err("package write target escaped plugin directory".to_string());
        }
    }
    Ok(target)
}

#[cfg(test)]
fn write_package_in_root(
    plugins_root: &Path,
    package: Package,
    repository: &str,
) -> Result<String, String> {
    write_package_in_root_with_tag(plugins_root, package, repository, None)
}

fn write_package_in_root_with_tag(
    plugins_root: &Path,
    package: Package,
    repository: &str,
    release_tag: Option<&str>,
) -> Result<String, String> {
    if !external::is_valid_plugin_id(&package.id) {
        return Err(format!("invalid plugin id `{}`", package.id));
    }
    let repository = normalize_repo_arg(repository)?;
    std::fs::create_dir_all(plugins_root).map_err(|e| e.to_string())?;
    let root_canon = std::fs::canonicalize(plugins_root).map_err(|e| e.to_string())?;
    let dir = root_canon.join(&package.id);
    if let Ok(meta) = std::fs::symlink_metadata(&dir) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to write through symlinked plugin directory `{}`",
                package.id
            ));
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir_canon = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    if dir_canon == root_canon || dir_canon.parent() != Some(root_canon.as_path()) {
        return Err(format!(
            "plugin directory for `{}` escapes plugins folder",
            package.id
        ));
    }
    let ext = external_lib_ext();
    let is_library = |path: &Path| {
        let mut comps = path.components();
        matches!(
            (comps.next(), comps.next()),
            (Some(std::path::Component::Normal(_)), None)
        ) && path.extension().is_some_and(|e| e == ext)
    };
    let (lib_name, lib_bytes) = package
        .files
        .iter()
        .find(|(path, _)| is_library(path))
        .ok_or_else(|| format!("{}: no library for this platform", package.id))?;
    replace_library(&dir_canon, &lib_name.to_string_lossy(), ext, lib_bytes)?;
    for (path, data) in package.files.iter().filter(|(path, _)| !is_library(path)) {
        let target = ensure_contained_write_target(&dir_canon, path)?;
        std::fs::write(target, data).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir_canon.join("plugin.toml"), &package.toml_text)
        .map_err(|e| e.to_string())?;
    std::fs::write(dir_canon.join(".source_repo"), format!("{repository}\n"))
        .map_err(|e| e.to_string())?;
    let parsed_version = external::parse_plugin_toml(&package.toml_text)
        .map(|m| m.version)
        .filter(|v| !v.is_empty());
    let lib_file_name = lib_name
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| lib_name.to_string_lossy().into_owned());
    let lib_sha256 = external::sha256_bytes(lib_bytes);
    external::record_lock_entry_in_root(
        &root_canon,
        &package.id,
        &lib_file_name,
        &lib_sha256,
        parsed_version.as_deref(),
        Some(&repository),
        release_tag,
    )?;
    Ok(package.id)
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

    fn valid_test_toml(id: &str, extra: &str) -> String {
        format!(
            "[plugin]\nid = \"{id}\"\napi_version = 14\n{extra}[opencad]\nacadrust_source = \"{}\"\nrustc_version = \"{}\"\n",
            ocs_plugin_api::version_info::host_acadrust_source(),
            ocs_plugin_api::version_info::host_rustc_version(),
        )
    }

    #[test]
    fn registry_url_is_pinned_to_app_version_tag() {
        assert!(!REGISTRY_URL.contains("/main/"));
        assert!(REGISTRY_URL.contains(concat!("/v", env!("OCS_APP_VERSION"), "/plugins/registry.json")));
    }

    #[test]
    fn checked_manifest_rejects_traversal_id_and_library_path() {
        assert!(checked_manifest(&valid_test_toml("../escape", "")).is_err());
        assert!(checked_manifest(&valid_test_toml("opencad.ok", "library = \"../bad.so\"\n")).is_err());
        assert!(checked_manifest(&valid_test_toml("opencad.ok", "library = \"sub/bad.so\"\n")).is_err());
        assert!(checked_manifest(&valid_test_toml("opencad.ok", "library = \"good.so\"\n")).is_ok());
    }

    #[test]
    fn write_package_in_root_blocks_traversal_and_writes_normalized_repo() {
        let dir = scratch("write_pkg");
        let plugins_root = dir.join("plugins");
        let ext = external_lib_ext();
        let bad_pkg = Package {
            id: "../outside".into(),
            toml_text: valid_test_toml("../outside", ""),
            files: vec![(PathBuf::from(format!("lib.{ext}")), b"BIN".to_vec())],
        };
        assert!(write_package_in_root(&plugins_root, bad_pkg, "owner/repo").is_err());

        let escaping_file_pkg = Package {
            id: "opencad.escape".into(),
            toml_text: valid_test_toml("opencad.escape", ""),
            files: vec![
                (PathBuf::from(format!("lib.{ext}")), b"BIN".to_vec()),
                (PathBuf::from("../pwned.txt"), b"BAD".to_vec()),
            ],
        };
        assert!(write_package_in_root(&plugins_root, escaping_file_pkg, "owner/repo").is_err());
        assert!(!dir.join("pwned.txt").exists());

        let ok_pkg = Package {
            id: "opencad.demo".into(),
            toml_text: valid_test_toml("opencad.demo", ""),
            files: vec![
                (PathBuf::from(format!("demo.{ext}")), b"BIN".to_vec()),
                (PathBuf::from("assets/info.txt"), b"OK".to_vec()),
            ],
        };
        assert_eq!(
            write_package_in_root(&plugins_root, ok_pkg, "https://github.com/owner/rust").unwrap(),
            "opencad.demo"
        );
        assert_eq!(
            std::fs::read_to_string(plugins_root.join("opencad.demo/.source_repo")).unwrap(),
            "owner/rust\n"
        );
        assert_eq!(
            std::fs::read(plugins_root.join("opencad.demo/assets/info.txt")).unwrap(),
            b"OK"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundle_packages_rejects_excessive_entries_and_compression_bombs() {
        use std::io::Write;
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let mut too_many = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut too_many));
            for i in 0..=MAX_BUNDLE_ENTRIES {
                zw.start_file(format!("f_{i}.txt"), opts).unwrap();
                zw.write_all(b"x").unwrap();
            }
            zw.finish().unwrap();
        }
        assert!(bundle_packages(&too_many).is_err());

        let mut bomb = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut bomb));
            zw.start_file("plugin.toml", opts).unwrap();
            zw.write_all(valid_test_toml("opencad.bomb", "").as_bytes())
                .unwrap();
            zw.start_file("payload.bin", opts).unwrap();
            zw.write_all(&vec![0u8; 512 * 1024]).unwrap();
            zw.finish().unwrap();
        }
        let err = bundle_packages(&bomb).err().expect("compression bomb must be rejected");
        assert!(err.contains("ratio"), "unexpected error: {err}");
    }

    #[test]
    fn install_rejects_disallowed_urls_and_invalid_repos() {
        let rel = Release {
            tag: "v1.0.0".into(),
            assets: vec![
                Asset {
                    name: format!("plugin.{}", external_lib_ext()),
                    url: "https://evil.example/evil.so".into(),
                },
                Asset {
                    name: "plugin.toml".into(),
                    url: "https://github.com/owner/repo/releases/download/v1.0.0/plugin.toml".into(),
                },
            ],
        };
        assert!(install(&rel, "owner/repo").is_err());
        assert!(fetch_releases("../bad").is_err());
        assert!(fetch_readme("https://evil.com/owner/repo").is_err());
    }

    #[test]
    fn write_package_in_root_records_sha256_in_plugin_lock() {
        let dir = scratch("write_lock");
        let plugins_root = dir.join("plugins");
        let ext = external_lib_ext();
        let lib_name = format!("demo.{ext}");
        let pkg = Package {
            id: "opencad.lockdemo".into(),
            toml_text: valid_test_toml("opencad.lockdemo", "version = \"1.2.3\"\n"),
            files: vec![(PathBuf::from(&lib_name), b"LOCKED-BIN".to_vec())],
        };
        write_package_in_root_with_tag(&plugins_root, pkg, "owner/demo", Some("v1.2.3")).unwrap();

        let lock_path = crate::plugin::external::lockfile_path_in(&plugins_root);
        let lock = crate::plugin::external::load_lockfile_from(&lock_path);
        let entry = lock.as_ref().unwrap().plugins.get("opencad.lockdemo").unwrap();
        assert_eq!(entry.library, lib_name);
        assert_eq!(entry.sha256, external::sha256_bytes(b"LOCKED-BIN"));
        assert_eq!(entry.version.as_deref(), Some("1.2.3"));
        assert_eq!(entry.release_tag.as_deref(), Some("v1.2.3"));
        let discovered_pkg = crate::plugin::external::ExternalPlugin {
            id: "opencad.lockdemo".into(),
            name: "Lock Demo".into(),
            version: "1.2.3".into(),
            description: String::new(),
            repository: Some("owner/demo".into()),
            api_version: ocs_plugin_api::manifest::API_VERSION,
            acadrust_source: Some(ocs_plugin_api::version_info::host_acadrust_source().to_string()),
            acadrust_declared: true,
            rustc_version: Some(ocs_plugin_api::version_info::host_rustc_version().to_string()),
            rustc_declared: true,
            ribbon_order: 0,
            command_prefixes: Vec::new(),
            dir: plugins_root.join("opencad.lockdemo"),
            bundled: false,
            lib_present: true,
            library: Some(lib_name),
        };
        assert_eq!(
            crate::plugin::external::evaluate_plugin_trust(&discovered_pkg, &lock),
            crate::plugin::external::PluginTrustState::Approved {
                sha256: external::sha256_bytes(b"LOCKED-BIN"),
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_asset_sha256_rejects_mismatched_registry_digest() {
        let rel = Release {
            tag: "v1.0.0".into(),
            assets: Vec::new(),
        };
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            "demo.so".to_string(),
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        );
        assert!(verify_asset_sha256("demo.so", &rel, "owner/repo", b"actual", Some(&map)).is_err());

        let mut good = std::collections::BTreeMap::new();
        good.insert("demo.so".to_string(), external::sha256_bytes(b"actual"));
        assert!(verify_asset_sha256("demo.so", &rel, "owner/repo", b"actual", Some(&good)).is_ok());
    }

    #[test]
    fn parse_registry_sha256_rejects_invalid_hex_and_accepts_valid_metadata() {
        let valid_hex = external::sha256_bytes(b"ok");
        assert!(parse_registry_sha256(&serde_json::json!(valid_hex)).is_ok());
        assert!(parse_registry_sha256(&serde_json::json!({"v1.0": valid_hex})).is_ok());
        assert!(parse_registry_sha256(&serde_json::json!("not-hex")).is_err());
        assert!(parse_registry_sha256(&serde_json::json!({"v1.0": "short"})).is_err());
    }
}
