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

/// The marker `recovered_name` appends (and strips when it is already
/// there, so it never stacks across crash cycles).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
const RECOVERED_MARKER: &str = " (recovered)";

/// The name a restored drawing carries as a tab and download: the drawing's
/// stem plus " (recovered)" and its own extension. The extension split
/// follows the WORKER convention (`name.rsplit('.').next()`, the whole name
/// when there is no dot): a dot-file like ".dxf" is a DXF name to the parse
/// worker, and `Path::extension` would misread it as extensionless —
/// restoring DXF bytes under a ".dwg" name jams the restore door. A name
/// with no extension at all defaults to DXF, matching how autosave
/// serializes untitled tabs (`source_is_dxf` → `dwg_source_version.is_none()`).
/// An existing trailing marker is stripped first, so "(recovered)" stacks
/// at most once no matter how many crash cycles re-snapshotted a previously
/// recovered name.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn recovered_name(drawing: &str) -> String {
    // A hostile stored name must not smuggle path separators into the tab
    // title / download name — through the head OR the extension ("a.b/c"
    // keeps "/c" after the last dot): reduce the WHOLE input to its final
    // path component first, then split stem/extension.
    let name = drawing.rsplit(['/', '\\']).next().unwrap_or_default();
    let (head, ext) = match name.rsplit_once('.') {
        Some((head, ext)) if !ext.is_empty() => (head, ext.to_string()),
        // No dot (or a trailing one): the whole component is the stem.
        _ => (name, "dxf".to_string()),
    };
    // An empty stem (".dxf" — a DXF name under the worker's rsplit
    // convention) keeps the whole component as the stem so the extension
    // does not flip, and the result stays idempotent because the marker
    // lives in the stem.
    let mut stem = if head.is_empty() { name } else { head };
    // Idempotent: strip every trailing marker, then append exactly one.
    while let Some(stripped) = stem.strip_suffix(RECOVERED_MARKER) {
        stem = stripped;
    }
    format!("{stem}{RECOVERED_MARKER}.{ext}")
}

/// The open plan for a "Restore all" batch: one `(recovered tab name,
/// snapshot key)` pair per pending snapshot, in the pending list's order —
/// the same order the one-at-a-time prompts would have offered them
/// (newest first), and the order the batch's reads are fired in. The keys
/// travel unchanged: each snapshot is consumed only by its own successful
/// open, exactly like a single restore.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub(crate) fn restore_all_plan(entries: &[WebAutosaveEntry]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|entry| (recovered_name(&entry.name), entry.name.clone()))
        .collect()
}

/// Overwrite (or create) the snapshot for `name`. The optional thumbnail is
/// written alongside as `.thumb`; passing `None` removes any stale one.
#[cfg(target_arch = "wasm32")]
pub async fn snapshot(name: &str, bytes: &[u8], thumbnail: Option<&[u8]>) -> Result<(), String> {
    let directory = autosave_directory(true).await?;
    let mut names = read_index(&directory).await?;
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
    // A corrupt index must abort the rewrite (not reset it to `names`),
    // or this remove would orphan every other row's snapshot.
    let mut names = read_index(&directory).await?;
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
    let directory = autosave_directory(false).await?;
    let mut entries = Vec::new();
    let mut dead: Vec<String> = Vec::new();
    // An unreadable index fails the whole boot scan: guessing an empty list
    // would cap-evict and remove snapshots the index can no longer name.
    for name in read_index(&directory).await? {
        let file = match super::web_recent::get_file(&directory, &snapshot_key(&name)).await {
            Ok(file) => file,
            // Crash between the index and snapshot writes: the row points
            // at nothing recoverable.
            Err(_) => {
                dead.push(name);
                continue;
            }
        };
        if file.size() == 0.0 {
            dead.push(name);
            continue;
        }
        entries.push(WebAutosaveEntry {
            name,
            size: file.size() as u64,
            modified_ms: file.last_modified(),
        });
    }
    // Dead rows are junk no other path removes: they never appear in an
    // offer, so the user cannot discard them and the cap cannot evict them.
    for name in dead {
        let _ = remove(&name).await;
    }
    let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
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
async fn read_index(directory: &web_sys::FileSystemDirectoryHandle) -> Result<Vec<String>, String> {
    let bytes = match super::web_recent::read_entry(directory, INDEX_KEY).await {
        Ok(bytes) => bytes,
        // A missing index is a fresh store.
        Err(_) => return Ok(Vec::new()),
    };
    // An empty (freshly created or truncated) index is an empty store.
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    // A non-empty file that fails to parse is real data we cannot name:
    // returning it as empty would let the next write reset the index and
    // silently orphan every other row's snapshot. Surface the failure so
    // callers skip the round/boot and leave the file untouched.
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
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
    fn recovered_names_never_smuggle_separators_or_drift() {
        // Fuzz round: the extension could smuggle a path ("a.b/c") and the
        // empty-stem fallback reinstated the unsanitized input, breaking
        // idempotence across crash cycles.
        let once = recovered_name("a.b/c");
        assert_eq!(
            once, "c (recovered).dxf",
            "final path component, no separator survives"
        );
        assert_eq!(recovered_name(&once), once, "idempotent");
        let dotfile = recovered_name("/.dwg");
        assert_eq!(dotfile, ".dwg (recovered).dwg");
        assert_eq!(recovered_name(&dotfile), dotfile, "idempotent");
    }

    #[test]
    fn recovered_names_keep_a_parseable_extension() {
        assert_eq!(recovered_name("plan.dwg"), "plan (recovered).dwg");
        assert_eq!(recovered_name("sketch.dxf"), "sketch (recovered).dxf");
        // Extensionless names (untitled tabs) restore as DXF: that is what
        // autosave serializes them as (`source_is_dxf` → no DWG source
        // version), and the parse worker dispatches on the final extension.
        assert_eq!(recovered_name("Drawing1"), "Drawing1 (recovered).dxf");
        // Multi-dot names keep everything but the final extension in the stem.
        assert_eq!(recovered_name("a.b.dwg"), "a.b (recovered).dwg");
        // A dot-file like ".dxf" is a DXF name under the worker's
        // `rsplit('.')` convention — the extension must not flip to dwg.
        assert_eq!(recovered_name(".dxf"), ".dxf (recovered).dxf");
        assert_eq!(recovered_name(".dwg"), ".dwg (recovered).dwg");
        assert_eq!(recovered_name(""), " (recovered).dxf");
    }

    #[test]
    fn the_recovered_marker_never_stacks() {
        // Restoring sets the tab name to the recovered name; a later
        // autosave round snapshots under it. One marker per drawing:
        assert_eq!(
            recovered_name("plan (recovered).dwg"),
            "plan (recovered).dwg"
        );
        // Idempotent even for names that already stacked (old cycles).
        assert_eq!(
            recovered_name("plan (recovered) (recovered).dwg"),
            "plan (recovered).dwg"
        );
        // Extensionless stacked names de-stack too, then gain ".dxf".
        assert_eq!(recovered_name("plan (recovered)"), "plan (recovered).dxf");
        // A marker embedded in the middle (not a suffix) is left alone.
        assert_eq!(
            recovered_name("(recovered) plan.dwg"),
            "(recovered) plan (recovered).dwg"
        );
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

    #[test]
    fn restore_all_keeps_the_pending_order_and_maps_only_the_names() {
        let entries = vec![
            entry("new.dwg", 300.0),
            entry("mid.dwg", 200.0),
            entry("plan (recovered).dxf", 100.0),
        ];
        let plan = restore_all_plan(&entries);
        assert_eq!(
            plan.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "new (recovered).dwg",
                "mid (recovered).dwg",
                "plan (recovered).dxf"
            ]
        );
        // Keys ride along unchanged — consumption stays keyed by the exact
        // snapshot identity, never by the display name.
        assert_eq!(
            plan.iter().map(|(_, key)| key.as_str()).collect::<Vec<_>>(),
            ["new.dwg", "mid.dwg", "plan (recovered).dxf"]
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
