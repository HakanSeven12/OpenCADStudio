// Community font repository — detect missing SHX fonts and fetch them from
// the project's `fonts/` folder on GitHub.
//
// DWG files reference SHX fonts by bare name (`vnarial.shx`, `romans.shx`, …).
// When the file doesn't resolve on disk the renderer substitutes an embedded
// LFF stroke font, which mangles drawings authored with fonts it cannot stand
// in for. This module detects those references and downloads the real files
// from the community folder, where anyone can contribute (see
// `fonts/README.md` in the repository root).

use std::path::{Path, PathBuf};

use acadrust::CadDocument;

/// The community folder's GitHub contents API (lists name + download URL).
#[cfg(not(target_arch = "wasm32"))]
const REPO_API_URL: &str =
    "https://api.github.com/repos/HakanSeven12/OpenCADStudio/contents/fonts";
#[cfg(not(target_arch = "wasm32"))]
const MAX_FONT_BYTES: u64 = 16 * 1024 * 1024;

/// Local store for fonts downloaded from the community repository. Text
/// resolution searches here after the drawing folder.
#[cfg(not(target_arch = "wasm32"))]
pub fn fonts_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|dir| dir.join("fonts"))
}

/// Web builds have no filesystem to cache fonts into.
#[cfg(target_arch = "wasm32")]
pub fn fonts_dir() -> Option<PathBuf> {
    None
}

/// Where missing fonts are fetched from. The community repository is the
/// default; a custom source is any HTTP folder - a company file server, a
/// private GitHub repo's raw folder - where each font lives as
/// `{base}/{file_name}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSource {
    Community,
    Custom(String),
}

impl FontSource {
    /// Empty / whitespace input selects the community repository.
    pub fn from_url(url: &str) -> Self {
        match url.trim() {
            "" => FontSource::Community,
            trimmed => FontSource::Custom(trimmed.trim_end_matches('/').to_string()),
        }
    }
}

/// One file listed in the community folder.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoFont {
    pub name: String,
    pub download_url: String,
}

/// Referenced-but-unresolvable `.shx` font files across the document's text
/// styles. Names are deduplicated case-insensitively and reduced to the bare
/// file name (`C:\Fonts\VNArial.shx` → `VNArial.shx`).
pub fn missing_shx_fonts(doc: &CadDocument) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    let base = doc
        .source_path
        .as_deref()
        .map(Path::new)
        .and_then(|p| p.parent());
    for style in doc.text_styles.iter() {
        // The primary font and the big font (Asian double-byte glyphs) are
        // both plain .shx files the style depends on.
        for file in [style.font_file.trim(), style.big_font_file.trim()] {
            if file.is_empty() || !file.to_ascii_lowercase().ends_with(".shx") {
                continue;
            }
            let resolved = crate::io::resolve_image_file(file, base).is_some()
                || local_font_file(file).is_some();
            if resolved {
                continue;
            }
            let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_string();
            if !missing.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
                missing.push(name);
            }
        }
    }
    missing.sort_by_key(|name| name.to_ascii_lowercase());
    missing
}

/// A font file previously downloaded into the local fonts directory, matched
/// case-insensitively by file name.
pub fn local_font_file(name: &str) -> Option<PathBuf> {
    let file = name.rsplit(['/', '\\']).next()?;
    let dir = fonts_dir()?;
    let direct = dir.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    let lower = file.to_ascii_lowercase();
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase() == lower)
        .map(|entry| entry.path())
}

/// List the community folder through the GitHub contents API.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_repo_fonts() -> Result<Vec<RepoFont>, String> {
    let agent = crate::network::agent(std::time::Duration::from_secs(15));
    let mut response = agent
        .get(REPO_API_URL)
        .header("User-Agent", concat!("OpenCADStudio/", env!("OCS_APP_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Could not reach the font repository: {e}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Could not read the font repository listing: {e}"))?;
    parse_contents(&body)
}

/// Parse the GitHub contents-API response:
/// `[{"name":"romans.shx","download_url":"https://…"}, …]`.
#[cfg(not(target_arch = "wasm32"))]
fn parse_contents(body: &str) -> Result<Vec<RepoFont>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Unexpected font listing: {e}"))?;
    let items = value
        .as_array()
        .ok_or_else(|| "Unexpected font-repository response.".to_string())?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let url = item.get("download_url")?.as_str()?.to_string();
            (!name.is_empty() && !url.is_empty()).then_some(RepoFont {
                name,
                download_url: url,
            })
        })
        .collect())
}

/// Download every `missing` font that exists in the community folder into the
/// local fonts directory. Returns the (requested, saved-path) pairs that were
/// actually fetched; names absent from the repository are skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn download_fonts(
    missing: &[String],
    source: &FontSource,
) -> Result<Vec<(String, PathBuf)>, String> {
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let repo = match source {
        FontSource::Community => Some(fetch_repo_fonts()?),
        FontSource::Custom(_) => None,
    };
    let dir = fonts_dir().ok_or_else(|| "Could not resolve the fonts folder.".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let mut downloaded = Vec::new();
    for want in missing {
        let file = want.rsplit(['/', '\\']).next().unwrap_or(want);
        let url = match (source, repo.as_ref()) {
            // Community: download URLs come from the listing, matched by name.
            (FontSource::Community, Some(repo)) => {
                let lower = file.to_ascii_lowercase();
                repo.iter()
                    .find(|f| f.name.to_ascii_lowercase() == lower)
                    .map(|f| f.download_url.clone())
            }
            // Custom folder: the font is simply `{base}/{file_name}`; a miss
            // (404) is skipped without failing the batch.
            (FontSource::Custom(base), _) => {
                Some(format!("{base}/{}", percent_encode_path(file)))
            }
            (FontSource::Community, None) => None,
        };
        let Some(url) = url else { continue };
        let agent = crate::network::agent(std::time::Duration::from_secs(30));
        let mut response = agent
            .get(&url)
            .header("User-Agent", concat!("OpenCADStudio/", env!("OCS_APP_VERSION")))
            .call()
            .map_err(|e| format!("Could not download {file}: {e}"))?;
        if response.status() != 200 {
            continue;
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_FONT_BYTES)
            .read_to_vec()
            .map_err(|e| format!("Could not download {file}: {e}"))?;
        if bytes.is_empty() {
            continue;
        }
        let dest = dir.join(file);
        std::fs::write(&dest, &bytes)
            .map_err(|e| format!("Could not save {}: {e}", dest.display()))?;
        downloaded.push((file.to_string(), dest));
    }
    Ok(downloaded)
}

/// The web build has no network client, and no fonts folder to save into.
#[cfg(target_arch = "wasm32")]
pub fn download_fonts(
    missing: &[String],
    _source: &FontSource,
) -> Result<Vec<(String, PathBuf)>, String> {
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    Err("Downloading fonts is not available in the web app.".to_string())
}

/// Escape path characters that would break a URL (spaces and non-ASCII file
/// names are common in font sets).
#[cfg(not(target_arch = "wasm32"))]
fn percent_encode_path(file: &str) -> String {
    let mut out = String::with_capacity(file.len());
    for byte in file.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_github_contents_listing() {
        let body = r#"[
            {"name":"romans.shx","path":"fonts/romans.shx",
             "download_url":"https://raw.githubusercontent.com/x/y/main/fonts/romans.shx","type":"file"},
            {"name":"subset","path":"fonts/subset","type":"dir"},
            {"name":"no-url.shx","path":"fonts/no-url.shx","type":"file"}
        ]"#;
        let fonts = parse_contents(body).unwrap();
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "romans.shx");
        assert!(fonts[0].download_url.ends_with("/fonts/romans.shx"));
    }

    #[test]
    fn custom_source_urls_are_trimmed_and_encoded() {
        assert_eq!(FontSource::from_url(""), FontSource::Community);
        assert_eq!(
            FontSource::from_url("  https://intra/fonts/  "),
            FontSource::Custom("https://intra/fonts".into())
        );
        assert_eq!(percent_encode_path("vn 30f.shx"), "vn%2030f.shx");
    }

    #[test]
    fn rejects_a_non_listing_response() {
        assert!(parse_contents("{\"message\":\"Not Found\"}").is_err());
    }

    #[test]
    fn missing_fonts_deduplicate_by_name_case_insensitively() {
        // Two styles referencing the same missing font with different casing
        // and one whose file resolves next to the drawing must yield one hit.
        let dir = std::env::temp_dir().join("ocs_font_repo_tests");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("exists.shx"), b"stub").unwrap();
        let mut doc = CadDocument::new();
        let mk = |name: &str, file: &str| {
            let mut style = acadrust::TextStyle::new(name);
            style.font_file = file.into();
            style
        };
        let _ = doc.text_styles.add(mk("A", "MISSING1.shx"));
        let _ = doc.text_styles.add(mk("B", "missing1.SHX"));
        let _ = doc.text_styles.add(mk("C", "exists.shx"));
        doc.source_path = Some(dir.join("drawing.dwg").to_string_lossy().into_owned());
        let missing = missing_shx_fonts(&doc);
        assert_eq!(missing, vec!["MISSING1.shx".to_string()]);
        let _ = std::fs::remove_file(dir.join("exists.shx"));
    }
}
