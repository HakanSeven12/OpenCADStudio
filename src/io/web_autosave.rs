//! Crash-recovery autosave snapshots for the web build.
//!
//! The native editor writes a `.sv$` copy beside each dirty drawing; the
//! browser has no writable paths, so the same SAVETIME tick instead serializes
//! every dirty tab into a dedicated OPFS directory (`opencadstudio-autosave`,
//! separate from the recents cache). Snapshots are keyed by the drawing's
//! display name — one snapshot per name, overwritten each round — and are
//! offered for restore at boot when newer than the drawing's last explicit
//! save (the OPFS recent copy of the same name).
//!
//! Losing that comparison never deletes a snapshot: the recent copy is
//! refreshed by every OPEN as well as every save, so a snapshot that just
//! went stale may still be the only survivor of a crash right after a
//! re-open. Accumulation is bounded instead — the boot scan keeps the
//! newest [`SNAPSHOT_CAP`] snapshots and evicts only the oldest ones beyond
//! that cap.
//!
//! OPFS entry names carry only the FNV hash of the drawing name (like
//! `web_recent`), so a small JSON index maps hashes back to names. The index
//! is written before its snapshot: a crash between the two writes leaves a
//! name without bytes — a missing file or a zero-byte one — which `list`
//! filters out, rather than an orphan snapshot nobody can name.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;

#[cfg(target_arch = "wasm32")]
const AUTOSAVE_DIRECTORY: &str = "opencadstudio-autosave";
#[cfg(target_arch = "wasm32")]
const INDEX_KEY: &str = "index.json";

/// One snapshot as `list`/`scan` report it. `modified_ms` is the OPFS file's
/// `lastModified` (milliseconds since the Unix epoch), used to decide whether
/// the snapshot outranks the drawing's last explicit save.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
#[derive(Debug, Clone)]
pub struct WebAutosaveEntry {
    pub name: String,
    pub size: u64,
    pub modified_ms: f64,
}

/// A snapshot is worth offering when it is strictly newer than the last
/// explicit save, or when no saved copy exists at all (the drawing was never
/// saved — or its recent copy was evicted — so the snapshot is all there is).
/// The comparison decides *offering* only, never survival: the recent copy
/// is refreshed by every open, so "older than the recent copy" is not proof
/// that a snapshot recovers nothing.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn newer_than_saved(autosave_ms: f64, saved_ms: Option<f64>) -> bool {
    match saved_ms {
        Some(saved_ms) => autosave_ms > saved_ms,
        None => true,
    }
}

/// Upper bound on stored snapshots; the boot scan evicts the oldest ones
/// beyond it. Without a bound, snapshots superseded by a save (or by a
/// mere re-open) would pile up forever, since age no longer deletes them.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) const SNAPSHOT_CAP: usize = 16;

/// Split `entries` into `(kept, evicted)` so at most `cap` snapshots remain,
/// both sides ordered by `modified_ms` (newest first within each side; the
/// evicted names overall run oldest-first, so an interrupted scan deletes
/// the least valuable snapshots first). This is the only deletion criterion
/// the boot scan applies — being older than the recent copy is not one.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn cap_evictions(
    mut entries: Vec<WebAutosaveEntry>,
    cap: usize,
) -> (Vec<WebAutosaveEntry>, Vec<String>) {
    entries.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms));
    let overflow = entries.len().saturating_sub(cap);
    let mut evicted: Vec<String> = entries
        .split_off(entries.len() - overflow)
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    evicted.reverse();
    (entries, evicted)
}

/// The name a restored drawing carries as a tab and download: the drawing's
/// stem plus " (recovered)" and its own extension (DWG when the stored name
/// has none, matching how autosave serializes untitled tabs). The extension
/// must survive intact because the web parser picks the format from it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn recovered_name(drawing: &str) -> String {
    let path = std::path::Path::new(drawing);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| drawing.to_string());
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .filter(|ext| !ext.is_empty())
        .unwrap_or_else(|| "dwg".to_string());
    format!("{stem} (recovered).{ext}")
}

/// Overwrite (or create) the snapshot for `name`. The optional thumbnail is
/// written alongside as `.thumb`; passing `None` removes any stale one.
#[cfg(target_arch = "wasm32")]
pub async fn snapshot(name: &str, bytes: &[u8], thumbnail: Option<&[u8]>) -> Result<(), String> {
    let directory = autosave_directory(true).await?;
    let mut names = read_index(&directory).await;
    if !names.iter().any(|stored| stored == name) {
        names.push(name.to_string());
        write_index(&directory, &names).await?;
    }
    super::web_recent::write_entry(&directory, &snapshot_key(name), bytes).await?;
    match thumbnail {
        Some(thumbnail) => {
            super::web_recent::write_entry(&directory, &thumbnail_key(name), thumbnail).await?;
        }
        None => {
            let _ = JsFuture::from(directory.remove_entry(&thumbnail_key(name))).await;
        }
    }
    Ok(())
}

/// All live snapshots. Index rows whose file is gone (crash between writes,
/// partial eviction) and zero-byte snapshots (the crash window after the
/// index write but before the snapshot write) are skipped rather than
/// reported — an empty file recovers nothing and would only route the user
/// into the corrupt-file recovery flow.
#[cfg(target_arch = "wasm32")]
pub async fn list() -> Result<Vec<WebAutosaveEntry>, String> {
    let directory = autosave_directory(false).await?;
    let mut entries = Vec::new();
    for name in read_index(&directory).await {
        let Ok(file) = super::web_recent::get_file(&directory, &snapshot_key(&name)).await else {
            continue;
        };
        if file.size() == 0.0 {
            continue;
        }
        entries.push(WebAutosaveEntry {
            name,
            size: file.size() as u64,
            modified_ms: file.last_modified(),
        });
    }
    Ok(entries)
}

#[cfg(target_arch = "wasm32")]
pub async fn read(name: &str) -> Result<Vec<u8>, String> {
    let directory = autosave_directory(false).await?;
    super::web_recent::read_entry(&directory, &snapshot_key(name)).await
}

#[cfg(target_arch = "wasm32")]
pub async fn remove(name: &str) -> Result<(), String> {
    let directory = autosave_directory(false).await?;
    let drawing = JsFuture::from(directory.remove_entry(&snapshot_key(name)))
        .await
        .map_err(super::web_recent::js_error);
    let _ = JsFuture::from(directory.remove_entry(&thumbnail_key(name))).await;
    let mut names = read_index(&directory).await;
    let before = names.len();
    names.retain(|stored| stored != name);
    if names.len() != before {
        // Best-effort: a stale row only costs a skipped stat in `list`.
        let _ = write_index(&directory, &names).await;
    }
    drawing.map(|_| ())
}

/// Boot scan: offer the snapshots worth recovering — strictly newer than the
/// drawing's last explicit copy, newest first. Age never deletes: the recent
/// copy is refreshed by every OPEN as well as every save, so a snapshot that
/// lost the comparison may still be the only survivor of a crash right after
/// a re-open. Accumulation is bounded instead — beyond [`SNAPSHOT_CAP`]
/// snapshots the oldest are evicted, which is the only deletion here.
#[cfg(target_arch = "wasm32")]
pub async fn scan() -> Result<Vec<WebAutosaveEntry>, String> {
    let (kept, evicted) = cap_evictions(list().await?, SNAPSHOT_CAP);
    for name in evicted {
        let _ = remove(&name).await;
    }
    let mut recoverable = Vec::new();
    for entry in kept {
        let saved_ms = super::web_recent::modified_ms(&entry.name).await;
        if newer_than_saved(entry.modified_ms, saved_ms) {
            recoverable.push(entry);
        }
    }
    // `cap_evictions` already ordered the survivors newest-first.
    Ok(recoverable)
}

#[cfg(target_arch = "wasm32")]
fn snapshot_key(name: &str) -> String {
    format!("{}.sv$", super::web_recent::name_hash(name))
}

#[cfg(target_arch = "wasm32")]
fn thumbnail_key(name: &str) -> String {
    format!("{}.thumb", super::web_recent::name_hash(name))
}

#[cfg(target_arch = "wasm32")]
async fn read_index(directory: &web_sys::FileSystemDirectoryHandle) -> Vec<String> {
    super::web_recent::read_entry(directory, INDEX_KEY)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
async fn write_index(
    directory: &web_sys::FileSystemDirectoryHandle,
    names: &[String],
) -> Result<(), String> {
    let bytes = serde_json::to_vec(names).map_err(|error| error.to_string())?;
    super::web_recent::write_entry(directory, INDEX_KEY, &bytes).await
}

#[cfg(target_arch = "wasm32")]
async fn autosave_directory(create: bool) -> Result<web_sys::FileSystemDirectoryHandle, String> {
    let window = web_sys::window().ok_or_else(|| "browser window unavailable".to_string())?;
    let root = JsFuture::from(window.navigator().storage().get_directory())
        .await
        .map_err(super::web_recent::js_error)?
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .map_err(super::web_recent::js_error)?;
    let options = web_sys::FileSystemGetDirectoryOptions::new();
    options.set_create(create);
    JsFuture::from(root.get_directory_handle_with_options(AUTOSAVE_DIRECTORY, &options))
        .await
        .map_err(super::web_recent::js_error)?
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .map_err(super::web_recent::js_error)
}

/// Pure logic only — the OPFS side needs a browser and is exercised by hand
/// (and by the boot scan on every web start).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_must_outrank_the_last_save_to_be_offered() {
        assert!(newer_than_saved(2000.0, Some(1000.0)));
        // Equal timestamps are the same save round — not newer.
        assert!(!newer_than_saved(1000.0, Some(1000.0)));
        assert!(!newer_than_saved(500.0, Some(1000.0)));
        // No saved copy at all: the snapshot is the only survivor.
        assert!(newer_than_saved(1.0, None));
    }

    #[test]
    fn recovered_names_keep_a_parseable_extension() {
        assert_eq!(recovered_name("plan.dwg"), "plan (recovered).dwg");
        assert_eq!(recovered_name("sketch.dxf"), "sketch (recovered).dxf");
        assert_eq!(recovered_name("Drawing1"), "Drawing1 (recovered).dwg");
        // Multi-dot names keep everything but the final extension in the stem.
        assert_eq!(recovered_name("a.b.dwg"), "a.b (recovered).dwg");
        assert_eq!(recovered_name(""), " (recovered).dwg");
    }

    #[test]
    fn under_the_cap_nothing_is_evicted_and_order_is_newest_first() {
        let (kept, evicted) = cap_evictions(
            vec![
                entry("old.dwg", 100.0),
                entry("new.dwg", 300.0),
                entry("mid.dwg", 200.0),
            ],
            SNAPSHOT_CAP,
        );
        assert!(evicted.is_empty());
        assert_eq!(
            kept.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["new.dwg", "mid.dwg", "old.dwg"]
        );
    }

    #[test]
    fn over_the_cap_exactly_the_oldest_overflow_is_evicted() {
        let entries = (0..SNAPSHOT_CAP + 4)
            .map(|i| entry(&format!("d{i}.dwg"), i as f64))
            .collect();
        let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
        assert_eq!(kept.len(), SNAPSHOT_CAP);
        // Newest kept first, oldest evicted first.
        assert_eq!(kept.first().map(|e| e.name.as_str()), Some("d19.dwg"));
        assert_eq!(
            evicted.iter().map(|name| name.as_str()).collect::<Vec<_>>(),
            ["d0.dwg", "d1.dwg", "d2.dwg", "d3.dwg"]
        );
    }

    fn entry(name: &str, modified_ms: f64) -> WebAutosaveEntry {
        WebAutosaveEntry {
            name: name.to_string(),
            size: 16,
            modified_ms,
        }
    }
}
