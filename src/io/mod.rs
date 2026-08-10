// I/O module — open, save, and export CAD documents.
//
// All file reading/writing goes through acadrust.
// Default save format: DWG (AC1032 / R2018+).

pub mod file_association;
#[cfg(not(target_arch = "wasm32"))]
pub mod edit_lock;
pub mod obj;
#[cfg(not(target_arch = "wasm32"))]
pub mod single_instance;
pub mod pdf_export;
pub mod plot_style;
pub mod print_to_printer;
pub mod recovery;
pub mod step;
pub mod stl;
pub mod xref;
pub mod linetypes;
pub mod patterns;
pub mod update_check;
pub mod paper_sizes;
pub mod thumbnail;
#[cfg(target_arch = "wasm32")]
mod web_worker;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web_recent;

use crate::scene::DerivedCaches;
use acadrust::entities::EntityType;
use acadrust::io::dwg::DwgReader;
use acadrust::{
    CadDocument, DwgReadOptions, DwgWriter, DxfReader, DxfReaderConfiguration, DxfWriter,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

/// Thread-safe state shared by the native loader and the open overlay.
///
/// `basis_points` is monotonic in 0..=10000. `completed/total` describe the
/// current sub-stage and let diagnostics distinguish real progress from a
/// cosmetic timer.
#[derive(Debug)]
pub struct OpenProgressState {
    pub phase: AtomicU8,
    pub basis_points: AtomicU16,
    pub completed: AtomicU32,
    pub total: AtomicU32,
}

impl OpenProgressState {
    pub fn new(phase: u8) -> Self {
        Self {
            phase: AtomicU8::new(phase),
            basis_points: AtomicU16::new(0),
            completed: AtomicU32::new(0),
            total: AtomicU32::new(1),
        }
    }

    pub fn set(&self, phase: u8, basis_points: u16, completed: usize, total: usize) {
        self.completed
            .store(completed.min(u32::MAX as usize) as u32, Ordering::Relaxed);
        self.total.store(
            total.max(1).min(u32::MAX as usize) as u32,
            Ordering::Relaxed,
        );
        self.basis_points
            .fetch_max(basis_points.min(10000), Ordering::Relaxed);
        self.phase.store(phase, Ordering::Release);
    }

    pub fn set_fraction(&self, phase: u8, base: u16, span: u16, completed: usize, total: usize) {
        let denominator = total.max(1) as u64;
        let value = base as u64 + (completed.min(total.max(1)) as u64 * span as u64 / denominator);
        self.set(phase, value.min(10000) as u16, completed, total);
    }
}

pub fn open_phase_name(phase: u8) -> &'static str {
    match phase {
        crate::app::OPEN_PHASE_READING => "reading",
        crate::app::OPEN_PHASE_PARSING => "parsing",
        crate::app::OPEN_PHASE_XREF => "references",
        crate::app::OPEN_PHASE_CACHING => "derived-caches",
        crate::app::OPEN_PHASE_FINALIZING => "finalizing",
        _ => "unknown",
    }
}

fn recovery_fingerprint_needed(caches: &DerivedCaches) -> bool {
    let parser_issue = caches.read_stats.as_ref().is_some_and(|stats| {
        stats.recovered()
            || stats.skipped_source_records > 0
            || !stats.stream_completed
    });
    let reference_issue = caches.xrefs.iter().any(|item| {
        matches!(
            item.status,
            crate::io::xref::XrefStatus::Recovered | crate::io::xref::XrefStatus::Failed
        )
    });
    parser_issue || caches.corrupt_dropped > 0 || caches.xref_dropped > 0 || reference_issue
}

#[derive(Debug, Clone)]
pub struct OpenLoadError {
    pub message: String,
    pub source_sha256: Option<String>,
    pub read_stats: Option<acadrust::ReadStats>,
    pub recovery_available: bool,
}

impl OpenLoadError {
    fn new(message: impl Into<String>, source_sha256: Option<String>) -> Self {
        Self {
            message: message.into(),
            source_sha256,
            read_stats: None,
            recovery_available: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recovery_prompt(
        message: impl Into<String>,
        read_stats: Option<acadrust::ReadStats>,
    ) -> Self {
        Self {
            message: message.into(),
            source_sha256: None,
            read_stats,
            recovery_available: true,
        }
    }
}

impl std::fmt::Display for OpenLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<String> for OpenLoadError {
    fn from(message: String) -> Self {
        Self::new(message, None)
    }
}

impl From<&str> for OpenLoadError {
    fn from(message: &str) -> Self {
        Self::new(message, None)
    }
}

// ── Open ──────────────────────────────────────────────────────────────────

/// Show the file picker and return the chosen path plus its size in bytes.
/// Returning size up-front lets the loading overlay display "47.3 MB" before
/// the parser thread starts.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_open_path() -> Option<(PathBuf, u64)> {
    let handle = crate::sys::file_dialog()
        .set_title("Open CAD file")
        .add_filter("CAD Files", &["dwg", "dxf", "bak", "sv$", "DWG", "DXF", "BAK"])
        .add_filter("DWG Files", &["dwg", "DWG"])
        .add_filter("DXF Files", &["dxf", "DXF"])
        .add_filter("Backup / Autosave", &["bak", "sv$", "BAK"])
        .add_filter("All Files", &["*"])
        .pick_file()
        .await?;
    let path = crate::sys::handle_path(&handle);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Some((path, size))
}

/// Pick the drawing whose layers become translation targets. A standards file
/// is a drawing too, so `.dws` and `.dwt` sit alongside the ordinary formats
/// rather than needing anything of their own. (#624)
pub async fn pick_layer_standard_path() -> Option<PathBuf> {
    let handle = crate::sys::file_dialog()
        .set_title("Load layer standard")
        .add_filter(
            "Drawings and standards",
            &["dwg", "dws", "dwt", "dxf", "DWG", "DWS", "DWT", "DXF"],
        )
        .add_filter("All Files", &["*"])
        .pick_file()
        .await?;
    Some(crate::sys::handle_path(&handle))
}

/// Pick where a set of layer mappings is written, or read back from.
pub async fn pick_layer_mapping_path(save: bool) -> Option<PathBuf> {
    let dialog = crate::sys::file_dialog()
        .set_title(if save {
            "Save layer mappings"
        } else {
            "Load layer mappings"
        })
        .add_filter("Layer mappings", &["ocslmap"])
        .add_filter("All Files", &["*"]);
    let handle = if save {
        dialog.set_file_name("layers.ocslmap").save_file().await?
    } else {
        dialog.pick_file().await?
    };
    Some(crate::sys::handle_path(&handle))
}

/// Load a CAD file from a known path. Parsing and cache building run on a
/// dedicated OS thread so the async executor stays free for rendering during
/// the load. Writes phase markers into `phase` so the UI can show
/// "Parsing entities…" / "Building caches…" / "Finalizing…" while the loader
/// thread runs.
#[cfg(not(target_arch = "wasm32"))]
pub async fn open_path_with_phase(
    path: PathBuf,
    progress: Arc<OpenProgressState>,
    model_bg: [f32; 4],
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError> {
    open_path_with_phase_attempt(path, progress, model_bg, OpenAttempt::Strict).await
}

#[cfg(target_arch = "wasm32")]
pub async fn open_path_with_phase(
    _path: PathBuf,
    _progress: Arc<OpenProgressState>,
    _model_bg: [f32; 4],
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError> {
    Err(OpenLoadError::from(
        "filesystem path opening is unavailable on this target",
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn recover_path_with_phase(
    path: PathBuf,
    progress: Arc<OpenProgressState>,
    model_bg: [f32; 4],
    initial_error: String,
    initial_stats: Option<acadrust::ReadStats>,
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError> {
    open_path_with_phase_attempt(
        path,
        progress,
        model_bg,
        OpenAttempt::Recovery(initial_error, initial_stats),
    )
    .await
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
enum OpenAttempt {
    Strict,
    Recovery(String, Option<acadrust::ReadStats>),
}

#[cfg(not(target_arch = "wasm32"))]
struct OpenAttemptFailure {
    message: String,
    read_stats: Option<acadrust::ReadStats>,
    recoverable: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<String> for OpenAttemptFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            read_stats: None,
            recoverable: false,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_path_with_phase_attempt(
    path: PathBuf,
    progress: Arc<OpenProgressState>,
    model_bg: [f32; 4],
    attempt: OpenAttempt,
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let path2 = path.clone();
    let progress2 = progress.clone();
    let recovery_available = matches!(attempt, OpenAttempt::Strict);
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-file-open".to_string())
        .spawn(move || {
            let initial_fingerprint = crate::io::edit_lock::FileFingerprint::capture(&path2).ok();
            let attempted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (|| -> Result<_, OpenAttemptFailure> {
        use iced::time::Instant;
                progress2.set(crate::app::OPEN_PHASE_PARSING, 200, 0, 1000);
        let t_parse = Instant::now();
        let parser_progress = {
                    let progress = Arc::clone(&progress2);
                    let callback: Arc<dyn Fn(u16) + Send + Sync> = Arc::new(move |value| {
                        progress.set_fraction(
                            crate::app::OPEN_PHASE_PARSING,
                            200,
                            5600,
                            value as usize,
                            1000,
                        );
                    });
                    callback
                };
                std::fs::File::open(&path2).map_err(|error| OpenAttemptFailure {
                    message: format!("failed to open drawing: {error}"),
                    read_stats: None,
                    recoverable: false,
                })?;
                let outcome = load_file_for_open(&path2, Some(parser_progress), &attempt)?;
                let read_stats = outcome.stats;
                let mut doc = outcome.document;
        let parse_ms = t_parse.elapsed().as_millis() as u32;
                progress2.set(crate::app::OPEN_PHASE_PARSING, 5800, 1000, 1000);
        let t_purge = Instant::now();
        let dropped = purge_corrupt_entities(&mut doc);
        let purge_ms = t_purge.elapsed().as_millis() as u32;
                if matches!(attempt, OpenAttempt::Strict) && dropped > 0 {
                    return Err(OpenAttemptFailure {
                        message: format!(
                            "normal read found {dropped} structurally invalid drawing records"
                        ),
                        read_stats: Some(read_stats),
                        recoverable: true,
                    });
                }
                progress2.set(crate::app::OPEN_PHASE_XREF, 6000, 0, 1);
                let t_xref = Instant::now();
                let (xref_infos, xref_dropped) = if let Some(base_dir) = path2.parent() {
                    let xref_progress = {
                        let progress = Arc::clone(&progress2);
                        let callback: Arc<dyn Fn(usize, usize) + Send + Sync> =
                            Arc::new(move |completed, total| {
                                progress.set_fraction(
                                    crate::app::OPEN_PHASE_XREF,
                                    6000,
                                    1400,
                                    completed,
                                    total,
                                );
                            });
                        callback
                    };
                    crate::io::xref::resolve_xrefs_with_progress(
                        &mut doc,
                        base_dir,
                        Some(xref_progress),
                    )
                } else {
                    (Vec::new(), 0)
                };
                let xref_ms = t_xref.elapsed().as_millis() as u32;
                progress2.set(crate::app::OPEN_PHASE_CACHING, 7400, 0, 10000);
        let t_caches = Instant::now();
        let cache_progress = |value: u16| {
                    progress2.set_fraction(
                        crate::app::OPEN_PHASE_CACHING,
                        7400,
                        2200,
                        value as usize,
                        10000,
                    );
                };
                let mut caches = crate::scene::build_derived_caches_with_progress(
                    &doc,
                    &cache_progress,
                    path2.parent(),
                );
        caches.timings = crate::scene::OpenTimings {
            parse_ms,
            purge_ms,
            caches_ms: t_caches.elapsed().as_millis() as u32,
                    xref_ms,
        };
        caches.corrupt_dropped = dropped;
                caches.read_stats = Some(read_stats);
                caches.xref_dropped = xref_dropped;
                caches.xrefs = xref_infos;
                if recovery_fingerprint_needed(&caches) {
                    caches.source_sha256 = stable_sha256_file(
                        &path2,
                        initial_fingerprint.as_ref(),
                    );
                }
                progress2.set(crate::app::OPEN_PHASE_FINALIZING, 9600, 0, 1);
                let (prepared_doc, prepared_geometry) =
                    crate::scene::prepare_open_geometry(doc, &caches, model_bg);
                doc = prepared_doc;
                caches.prepared_geometry = Some(prepared_geometry);
                progress2.set(crate::app::OPEN_PHASE_FINALIZING, 9950, 1, 1);
                    Ok((doc, caches))
                })()
            }));
            let result = match attempted {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(failure)) => Err(if recovery_available && failure.recoverable {
                    OpenLoadError::recovery_prompt(failure.message, failure.read_stats)
                } else {
                    OpenLoadError {
                        message: failure.message,
                        source_sha256: stable_sha256_file(
                            &path2,
                            initial_fingerprint.as_ref(),
                        ),
                        read_stats: failure.read_stats,
                        recovery_available: false,
                    }
                }),
                Err(payload) => {
                    let message = format!(
                        "file-open worker panicked: {}",
                        panic_message(payload.as_ref())
                    );
                    Err(OpenLoadError {
                        message,
                        source_sha256: stable_sha256_file(
                            &path2,
                            initial_fingerprint.as_ref(),
                        ),
                        read_stats: None,
                        recovery_available: false,
                    })
                }
            };
            let _ = sender.send(result);
    })
        .map_err(|error| OpenLoadError::from(format!("failed to start parser thread: {error}")))?;
    let (doc, caches) = receiver
        .await
        .map_err(|_| OpenLoadError::from("parser thread stopped without a result"))??;
    Ok((name, path, doc, caches))
}

#[cfg(not(target_arch = "wasm32"))]
fn stable_sha256_file(
    path: &Path,
    initial: Option<&crate::io::edit_lock::FileFingerprint>,
) -> Option<String> {
    let initial = initial?;
    let before = crate::io::edit_lock::FileFingerprint::capture(path).ok()?;
    if &before != initial {
        return None;
    }
    let digest = crate::io::recovery::sha256_file(path).ok()?;
    let after = crate::io::edit_lock::FileFingerprint::capture(path).ok()?;
    (after == before).then_some(digest)
}

#[cfg(not(target_arch = "wasm32"))]
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic payload");
    message.chars().take(500).collect()
}

/// Web file open: show the browser picker, read the chosen file's bytes, parse
/// it, and build the derived caches — producing the same payload as the native
/// `open_path_with_phase` so it can feed the existing `Message::FileOpened`
/// handler. There is no filesystem path on the web, so a name-only `PathBuf`
/// stands in for the document path.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
pub struct WebOpenOutcome {
    pub name: String,
    pub size_bytes: u64,
    pub result: Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError>,
    pub recovery_bytes: Option<Arc<[u8]>>,
    pub cache_bytes: Option<Arc<[u8]>>,
    pub record_recent: bool,
}

#[cfg(target_arch = "wasm32")]
pub async fn pick_and_load_web(
    progress: Arc<OpenProgressState>,
) -> WebOpenOutcome {
    let Some(handle) = crate::sys::file_dialog()
        .set_title("Open CAD file")
        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
        .add_filter("All Files", &["*"])
        .pick_file()
        .await
    else {
        return WebOpenOutcome {
            name: "Opening…".to_string(),
            size_bytes: 0,
            result: Err(OpenLoadError::from("Cancelled")),
            recovery_bytes: None,
            cache_bytes: None,
            record_recent: false,
        };
    };
    let name = handle.file_name();
    progress.set(crate::app::OPEN_PHASE_READING, 500, 1, 2);
    let bytes: Arc<[u8]> = Arc::from(handle.read().await);
    let size_bytes = bytes.len() as u64;
    let result = load_web_bytes(&name, &bytes, progress.clone(), false, "", None).await;
    let keep_for_recovery = result
        .as_ref()
        .err()
        .is_some_and(|error| error.recovery_available);
    let cache_bytes = result.is_ok().then(|| Arc::clone(&bytes));
    WebOpenOutcome {
        name,
        size_bytes,
        result,
        recovery_bytes: keep_for_recovery.then(|| Arc::clone(&bytes)),
        cache_bytes,
        record_recent: false,
    }
}

/// Reopen a browser-private recent copy without showing the file picker.
#[cfg(target_arch = "wasm32")]
pub async fn open_recent_web(
    path: PathBuf,
    progress: Arc<OpenProgressState>,
) -> WebOpenOutcome {
    open_recent_web_attempt(path, progress, false, String::new()).await
}

#[cfg(target_arch = "wasm32")]
pub async fn recover_web_bytes(
    name: String,
    bytes: Arc<[u8]>,
    progress: Arc<OpenProgressState>,
    initial_error: String,
    initial_stats: Option<acadrust::ReadStats>,
) -> WebOpenOutcome {
    let size_bytes = bytes.len() as u64;
    let result = load_web_bytes(
        &name,
        &bytes,
        progress,
        true,
        &initial_error,
        initial_stats,
    )
    .await;
    WebOpenOutcome {
        name,
        size_bytes,
        result,
        recovery_bytes: None,
        cache_bytes: None,
        record_recent: false,
    }
}

#[cfg(target_arch = "wasm32")]
async fn open_recent_web_attempt(
    path: PathBuf,
    progress: Arc<OpenProgressState>,
    recovery_mode: bool,
    initial_error: String,
) -> WebOpenOutcome {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let bytes = match web_recent::read(&name).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return WebOpenOutcome {
                name: name.clone(),
                size_bytes: 0,
                result: Err(OpenLoadError::from(format!(
                    "Recent copy unavailable for \"{name}\": {error}"
                ))),
                recovery_bytes: None,
                cache_bytes: None,
                record_recent: false,
            };
        }
    };
    progress.set(
        crate::app::OPEN_PHASE_READING,
        1000,
        bytes.len(),
        bytes.len(),
    );
    let result = load_web_bytes(
        &name,
        &bytes,
        progress,
        recovery_mode,
        &initial_error,
        None,
    )
    .await;
    let keep_for_recovery = result
        .as_ref()
        .err()
        .is_some_and(|error| error.recovery_available);
    let record_recent = result.is_ok();
    WebOpenOutcome {
        name: name.clone(),
        size_bytes: bytes.len() as u64,
        result,
        recovery_bytes: keep_for_recovery.then(|| Arc::from(bytes)),
        cache_bytes: None,
        record_recent,
    }
}

#[cfg(target_arch = "wasm32")]
async fn load_web_bytes(
    name: &str,
    bytes: &[u8],
    progress: Arc<OpenProgressState>,
    recovery_mode: bool,
    initial_error: &str,
    mut initial_stats: Option<acadrust::ReadStats>,
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), OpenLoadError> {
    progress.set(crate::app::OPEN_PHASE_PARSING, 1000, 0, 1);
    let (outcome, mut source_sha256) = match web_worker::parse_document(
        name,
        bytes,
        recovery_mode,
        initial_error,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            let recoverable_parse_error = error.recovery_available;
            let mut source_sha256 = error.source_sha256;
            if recovery_mode && source_sha256.is_none() {
                source_sha256 = web_worker::sha256_document(bytes).await.ok();
            }
            let read_stats = merge_read_stats(error.read_stats, initial_stats.take());
            let message = if recovery_mode && !error.message.contains("initial read failed:") {
                format!(
                    "initial read failed: {initial_error}; recovery read failed: {}",
                    error.message
                )
            } else {
                error.message
            };
            return Err(OpenLoadError {
                message: format!("Web parser worker: {message}"),
                recovery_available: !recovery_mode && recoverable_parse_error,
                source_sha256,
                read_stats,
            });
        }
    };
    let mut outcome = outcome;
    if let Some(initial_stats) = initial_stats.take() {
        merge_read_diagnostics(&mut outcome.stats, initial_stats);
    }
    let mut doc = outcome.document;
    if name.to_ascii_lowercase().ends_with(".dxf") {
        fix_dxf_dimension_rotations(&mut doc);
        fix_dxf_layout_plot_settings(&mut doc);
    }
    fix_viewport_status_flags(&mut doc);
    fix_current_style_names(&mut doc);
    progress.set(crate::app::OPEN_PHASE_CACHING, 7000, 0, 1);
    let dropped = purge_corrupt_entities(&mut doc);
    if !recovery_mode && dropped > 0 {
        return Err(OpenLoadError {
            message: format!(
                "normal read found {dropped} structurally invalid drawing records"
            ),
            source_sha256: None,
            read_stats: Some(outcome.stats),
            recovery_available: true,
        });
    }
    let mut caches = crate::scene::build_derived_caches(&doc);
    caches.corrupt_dropped = dropped;
    caches.read_stats = Some(outcome.stats);
    if source_sha256.is_none() && recovery_fingerprint_needed(&caches) {
        source_sha256 = web_worker::sha256_document(bytes).await.ok();
    }
    caches.source_sha256 = source_sha256;
    progress.set(crate::app::OPEN_PHASE_FINALIZING, 9900, 1, 1);
    let path = PathBuf::from(name);
    Ok((name.to_string(), path, doc, caches))
}

/// Parse a CAD document from in-memory bytes, choosing the format from
/// `name`'s extension. Used by the web build, where files arrive as bytes from
/// a browser file picker (no filesystem path). Shares the post-load fixups with
/// [`load_file`]; raster-image path resolution is skipped (there is no sibling
/// directory to search on the web).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_bytes(name: &str, bytes: Vec<u8>) -> Result<CadDocument, String> {
    use std::io::Cursor;
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    match ext.as_str() {
        "dwg" => {
            let mut doc = DwgReader::from_stream(Cursor::new(bytes))
                .read()
                .map_err(|e| e.to_string())?;
            fix_viewport_status_flags(&mut doc);
            fix_current_style_names(&mut doc);
            Ok(doc)
        }
        "dxf" => {
            let mut doc = DxfReader::from_reader(Cursor::new(bytes))
                .map_err(|e| e.to_string())?
                .read()
                .map_err(|e| e.to_string())?;
            fix_dxf_dimension_rotations(&mut doc);
            fix_dxf_layout_plot_settings(&mut doc);
            fix_viewport_status_flags(&mut doc);
            fix_current_style_names(&mut doc);
            Ok(doc)
        }
        _ => Err(format!("Unsupported file format: .{ext}")),
    }
}

/// Load a DWG or DXF file directly from a path (auto-detect by extension).
/// Peek at a file's leading bytes to tell a DWG (version tag "AC10xx") from a
/// DXF. Used for `.bak` copies, whose extension hides the real format.
fn sniff_dwg_or_dxf(path: &Path) -> String {
    use std::io::Read;
    let mut buf = [0u8; 6];
    if let Ok(mut f) = std::fs::File::open(path) {
        let _ = f.read(&mut buf);
    }
    if buf.starts_with(b"AC10") {
        "dwg".to_string()
    } else {
        "dxf".to_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_file(path: &Path) -> Result<CadDocument, String> {
    load_file_with_progress(path, None).map(|outcome| outcome.document)
}

/// Opening a drawing by path is a desktop affair. In the browser a file
/// arrives through the page rather than from a filesystem the app can reach,
/// so there is nothing behind a path to open.
///
/// The function still exists there so the features that read a *second*
/// drawing — importing one as a block, taking layer standards from one — go on
/// compiling and say why they cannot run, instead of each having to know that
/// the web has no files.
#[cfg(target_arch = "wasm32")]
pub fn load_file(_path: &Path) -> Result<CadDocument, String> {
    Err(crate::t!("Opening a drawing by path is not available in the browser.").into_owned())
}

pub(crate) fn load_file_with_progress(
    path: &Path,
    _progress: Option<Arc<dyn Fn(u16) + Send + Sync>>,
) -> Result<acadrust::ReadOutcome, String> {
    let outcome = read_file_attempt(path, _progress, false).map_err(|failure| failure.message)?;
    if !outcome.stats.has_usable_drawing_data() {
        return Err("initial read returned no source drawing records".to_string());
    }
    finalize_loaded_outcome(path, outcome)
}

#[cfg(not(target_arch = "wasm32"))]
fn load_file_for_open(
    path: &Path,
    progress: Option<Arc<dyn Fn(u16) + Send + Sync>>,
    attempt: &OpenAttempt,
) -> Result<acadrust::ReadOutcome, OpenAttemptFailure> {
    let outcome = match attempt {
        OpenAttempt::Strict => {
            let outcome = read_file_attempt(path, progress, false).map_err(|failure| {
                OpenAttemptFailure {
                    message: failure.message,
                    read_stats: None,
                    recoverable: failure.recoverable,
                }
            })?;
            if !outcome.stats.has_usable_drawing_data() {
                return Err(OpenAttemptFailure {
                    message: "initial read returned no source drawing records".to_string(),
                    read_stats: Some(outcome.stats),
                    recoverable: true,
                });
            }
            if outcome.stats.recovered()
                || outcome.stats.skipped_source_records > 0
                || !outcome.stats.stream_completed
            {
                let message = outcome
                    .stats
                    .diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.message.clone())
                    .unwrap_or_else(|| {
                        "normal read detected recoverable drawing errors".to_string()
                    });
                return Err(OpenAttemptFailure {
                    message,
                    read_stats: Some(outcome.stats),
                    recoverable: true,
                });
            }
            outcome
        }
        OpenAttempt::Recovery(initial_error, initial_stats) => {
            let mut outcome = read_file_attempt(path, progress, true).map_err(|failure| {
                OpenAttemptFailure {
                    message: format!(
                        "initial read failed: {initial_error}; recovery read failed: {}",
                        failure.message
                    ),
                    read_stats: initial_stats.clone(),
                    recoverable: false,
                }
            })?;
            if !outcome.stats.has_usable_drawing_data() {
                if let Some(initial_stats) = initial_stats.clone() {
                    merge_read_diagnostics(&mut outcome.stats, initial_stats);
                }
                return Err(OpenAttemptFailure {
                    message: format!(
                        "initial read failed: {initial_error}; recovery found no usable drawing data"
                    ),
                    read_stats: Some(outcome.stats),
                    recoverable: false,
                });
            }
            outcome.document.notifications.notify(
                acadrust::notification::NotificationType::Error,
                format!("Initial read failed; recovery mode continued: {initial_error}"),
            );
            acadrust::push_read_diagnostic(
                &mut outcome.stats.diagnostics,
                acadrust::ReadDiagnostic::new(
                    "strict-read-failed",
                    acadrust::ReadStage::RecordStream,
                    initial_error.clone(),
                ),
            );
            outcome.stats.recovered_errors = outcome.stats.recovered_errors.saturating_add(1);
            if let Some(initial_stats) = initial_stats.clone() {
                merge_read_diagnostics(&mut outcome.stats, initial_stats);
            }
            outcome
        }
    };
    finalize_loaded_outcome(path, outcome).map_err(OpenAttemptFailure::from)
}

struct ReaderFailure {
    message: String,
    #[cfg(not(target_arch = "wasm32"))]
    recoverable: bool,
}

impl ReaderFailure {
    fn from_reader(error: acadrust::DxfError) -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            recoverable: recoverable_reader_error(&error),
            message: error.to_string(),
        }
    }

    fn terminal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            #[cfg(not(target_arch = "wasm32"))]
            recoverable: false,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn recoverable_reader_error(error: &acadrust::DxfError) -> bool {
    matches!(
        error,
        acadrust::DxfError::Compression(_)
            | acadrust::DxfError::Parse(_)
            | acadrust::DxfError::InvalidDxfCode(_)
            | acadrust::DxfError::InvalidHandle(_)
            | acadrust::DxfError::ObjectNotFound(_)
            | acadrust::DxfError::InvalidEntityType(_)
            | acadrust::DxfError::ChecksumMismatch { .. }
            | acadrust::DxfError::InvalidHeader(_)
            | acadrust::DxfError::InvalidFormat(_)
            | acadrust::DxfError::InvalidSentinel(_)
            | acadrust::DxfError::Decompression(_)
            | acadrust::DxfError::Encoding(_)
    )
}

fn merge_read_diagnostics(
    target: &mut acadrust::ReadStats,
    source: acadrust::ReadStats,
) {
    for diagnostic in source.diagnostics {
        if !target.diagnostics.contains(&diagnostic) {
            acadrust::push_read_diagnostic(&mut target.diagnostics, diagnostic);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn merge_read_stats(
    primary: Option<acadrust::ReadStats>,
    fallback: Option<acadrust::ReadStats>,
) -> Option<acadrust::ReadStats> {
    match (primary, fallback) {
        (Some(mut primary), Some(fallback)) => {
            merge_read_diagnostics(&mut primary, fallback);
            Some(primary)
        }
        (Some(primary), None) => Some(primary),
        (None, fallback) => fallback,
    }
}

fn read_file_attempt(
    path: &Path,
    progress: Option<Arc<dyn Fn(u16) + Send + Sync>>,
    failsafe: bool,
) -> Result<acadrust::ReadOutcome, ReaderFailure> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // A `.bak` backup or `.sv$` autosave holds a verbatim DWG/DXF copy — detect
    // the real format from the file's leading bytes, not the extension.
    let effective = if ext == "bak" || ext == "sv$" {
        sniff_dwg_or_dxf(path)
    } else {
        ext.clone()
    };

    match effective.as_str() {
        "dwg" => read_dwg_path(path, progress, failsafe),
        "dxf" => read_dxf_path(path, failsafe),
        _ => Err(ReaderFailure::terminal(format!(
            "Unsupported file format: .{ext}"
        ))),
    }
}

fn finalize_loaded_outcome(
    path: &Path,
    mut outcome: acadrust::ReadOutcome,
) -> Result<acadrust::ReadOutcome, String> {
    let doc = &mut outcome.document;
    normalize_block_origins(doc);
    if outcome.stats.source_format == Some(acadrust::SourceFormat::Dxf) {
        fix_dxf_dimension_rotations(doc);
        fix_dxf_layout_plot_settings(doc);
    }
    fix_viewport_status_flags(doc);
    fix_current_style_names(doc);
    resolve_raster_image_paths(doc, path.parent());
    doc.source_path = Some(path.to_string_lossy().into_owned());
    Ok(outcome)
}

fn read_dwg_path(
    path: &Path,
    progress: Option<Arc<dyn Fn(u16) + Send + Sync>>,
    failsafe: bool,
) -> Result<acadrust::ReadOutcome, ReaderFailure> {
    let options = if failsafe {
        DwgReadOptions::failsafe()
    } else {
        DwgReadOptions::default()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let mut reader = {
        let mut reader = DwgReader::from_mmap(path).map_err(ReaderFailure::from_reader)?;
        reader.options = options;
        reader
    };
    #[cfg(target_arch = "wasm32")]
    let mut reader = DwgReader::from_file_with_options(path, options)
        .map_err(ReaderFailure::from_reader)?;
    if let Some(progress) = progress {
        reader.set_progress_callback(progress);
    }
    reader
        .read_with_stats()
        .map_err(ReaderFailure::from_reader)
}

fn read_dxf_path(path: &Path, failsafe: bool) -> Result<acadrust::ReadOutcome, ReaderFailure> {
    DxfReader::from_file(path)
        .map_err(ReaderFailure::from_reader)?
        .with_configuration(DxfReaderConfiguration {
            failsafe,
            ..DxfReaderConfiguration::default()
        })
        .read_with_stats()
        .map_err(ReaderFailure::from_reader)
}

/// Canonicalise legacy block-table origins to the post-R10 representation:
/// block contents are local around zero and INSERT carries placement.
///
/// DWG exposes the legacy point on `BlockRecord::base_point`; DXF exposes it
/// on the structural BLOCK entity. Rendering, picking, exploding and nested
/// insertion can then share the ordinary zero-origin transform without each
/// path having to reinterpret this compatibility field.
fn normalize_block_origins(doc: &mut CadDocument) {
    use acadrust::types::Vector3;
    use acadrust::EntityType;

    let blocks: Vec<_> = doc
        .block_records
        .iter()
        .filter_map(|record| {
            let marker_point = match doc.get_entity(record.block_entity_handle) {
                Some(EntityType::Block(block)) => block.base_point,
                _ => Vector3::ZERO,
            };
            let base_point = if marker_point.length() > 1e-12 {
                marker_point
            } else {
                record.base_point
            };
            (base_point.length() > 1e-12).then(|| {
                (
                    record.name.clone(),
                    record.block_entity_handle,
                    record.entity_handles.clone(),
                    base_point,
                )
            })
        })
        .collect();

    for (name, marker_handle, handles, base_point) in blocks {
        let offset = base_point * -1.0;
        for handle in handles {
            let Some(entity) = doc.get_entity_mut(handle) else {
                continue;
            };
            match entity {
                EntityType::Block(block) => block.base_point = Vector3::ZERO,
                EntityType::BlockEnd(_) => {}
                _ => entity.translate(offset),
            }
        }
        if let Some(EntityType::Block(block)) = doc.get_entity_mut(marker_handle) {
            block.base_point = Vector3::ZERO;
        }
        if let Some(record) = doc.block_records.get_mut(&name) {
            record.base_point = Vector3::ZERO;
        }
    }
}

/// A RasterImage entity stores its file path on the linked ImageDefinition,
/// often as the original author's absolute path (e.g. a Windows / VMware share)
/// that doesn't exist on this machine. Resolve each image to a usable path:
/// the stored path if it exists, otherwise the same file name next to the
/// drawing — and write the result onto the entity so the renderer finds it.
fn resolve_raster_image_paths(doc: &mut CadDocument, base_dir: Option<&Path>) {
    use acadrust::objects::ObjectType;
    use acadrust::EntityType;
    use std::collections::HashMap;

    let defs: HashMap<acadrust::Handle, String> = doc
        .objects
        .iter()
        .filter_map(|(h, o)| match o {
            ObjectType::ImageDefinition(d) => Some((*h, d.file_name.clone())),
            _ => None,
        })
        .collect();

    for e in doc.entities_mut() {
        if let EntityType::RasterImage(img) = e {
            let raw = if !img.file_path.trim().is_empty() {
                img.file_path.clone()
            } else {
                img.definition_handle
                    .and_then(|h| defs.get(&h).cloned())
                    .unwrap_or_default()
            };
            if raw.trim().is_empty() {
                continue;
            }
            if let Some(resolved) = resolve_image_file(&raw, base_dir) {
                img.file_path = resolved;
            } else {
                // At least surface the stored path so the renderer can try it.
                img.file_path = raw;
            }
        }
    }

    // Underlay definitions (PDF/DWF/DGN) store their file the same way raster
    // images do — resolve them with the same fallbacks so a drawing shipped
    // next to its PDF finds it even when the stored relative path is stale.
    for o in doc.objects.values_mut() {
        if let ObjectType::UnderlayDefinition(def) = o {
            if def.file_path.trim().is_empty() {
                continue;
            }
            if let Some(resolved) = resolve_image_file(&def.file_path, base_dir) {
                def.file_path = resolved;
            }
        }
    }
}

/// Resolve a (possibly foreign / absolute) image path to an existing file:
/// as stored, then relative to the drawing folder, then just the file name
/// next to the drawing.
pub(crate) fn resolve_image_file(raw: &str, base_dir: Option<&Path>) -> Option<String> {
    if Path::new(raw).is_file() {
        return Some(raw.to_string());
    }
    let base_dir = base_dir?;
    let joined = base_dir.join(raw);
    if joined.is_file() {
        return Some(joined.to_string_lossy().into_owned());
    }
    let name = raw.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(raw);
    let cand = base_dir.join(name);
    if cand.is_file() {
        return Some(cand.to_string_lossy().into_owned());
    }
    None
}

// ── Save ──────────────────────────────────────────────────────────────────

pub const DEFAULT_SAVE_FORMAT: &str = "DWG 2018";

pub const SAVE_FORMAT_OPTIONS: &[&str] = &[
    "DWG 2018", "DWG 2013", "DWG 2010", "DWG 2007", "DWG 2004", "DWG 2000", "DWG R14", "DXF 2018",
    "DXF 2013", "DXF 2010", "DXF 2007", "DXF 2004", "DXF 2000", "DXF R14",
];

pub fn canonical_save_format(format: &str) -> &'static str {
    SAVE_FORMAT_OPTIONS
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(format))
        .unwrap_or(DEFAULT_SAVE_FORMAT)
}

pub fn source_is_dxf(path: Option<&Path>, document: &CadDocument) -> bool {
    match path
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("dxf") => true,
        Some("dwg") => false,
        _ => document.dwg_source_version.is_none(),
    }
}

/// Parse a format string like "DWG 2013" or "DXF 2007" into
/// `(extension, DxfVersion)`.  Falls back to ("dwg", AC1032) for unknown strings.
pub fn parse_save_format(format: &str) -> (&'static str, acadrust::DxfVersion) {
    use acadrust::DxfVersion;
    let f = format.to_ascii_uppercase();
    let is_dxf = f.starts_with("DXF");
    let ext = if is_dxf { "dxf" } else { "dwg" };
    let version = if f.contains("2013") {
        DxfVersion::AC1027
    } else if f.contains("2010") {
        DxfVersion::AC1024
    } else if f.contains("2007") {
        DxfVersion::AC1021
    } else if f.contains("2004") {
        DxfVersion::AC1018
    } else if f.contains("2000") {
        DxfVersion::AC1015
    } else if f.contains("R14") {
        DxfVersion::AC1014
    } else {
        DxfVersion::AC1032
    }; // 2018
    (ext, version)
}

/// Reverse of [`parse_save_format`]: the Save-dialog format string for a
/// version + DXF/DWG choice (e.g. `AC1018, is_dxf=false` -> `"DWG 2004"`).
/// Used to default the Save-As dropdown to the loaded file's version so a
/// round-trip preserves it instead of silently offering "DWG 2018".
pub fn format_for_version(version: acadrust::DxfVersion, is_dxf: bool) -> String {
    use acadrust::DxfVersion::*;
    let year = match version {
        AC1032 => "2018",
        AC1027 => "2013",
        AC1024 => "2010",
        AC1021 => "2007",
        AC1018 => "2004",
        AC1015 => "2000",
        AC1014 => "R14",
        _ => "2018",
    };
    format!("{} {}", if is_dxf { "DXF" } else { "DWG" }, year)
}

/// Count of unsupported "raw passthrough" objects/entities — AEC / application
/// objects with no native representation, kept only as verbatim source-version
/// bytes — that would be DROPPED when saving `doc` to `target_version` (or to
/// DXF). Returns 0 for a same-version DWG save, where they round-trip verbatim.
/// Used to warn the user before a lossy Save-As.
pub fn dropped_on_save_count(
    doc: &acadrust::CadDocument,
    target_version: acadrust::DxfVersion,
    is_dxf: bool,
) -> usize {
    if !is_dxf && doc.dwg_source_version == Some(target_version) {
        return 0;
    }

    let mut n = doc
        .objects
        .values()
        .filter(|object| match object {
            acadrust::objects::ObjectType::Unknown {
                raw_dxf_codes,
                raw_dwg_data,
                raw_dwg_version,
                ..
            } => {
                if is_dxf {
                    raw_dxf_codes.is_none()
                } else {
                    raw_dwg_data.is_none()
                        || raw_dwg_version.is_some_and(|source| source != target_version)
                }
            }
            _ => false,
        })
        .count();
    for e in doc.entities() {
        let dropped = match e {
            acadrust::EntityType::Unknown(entity) => {
                if is_dxf {
                    entity.raw_dxf_codes.is_none()
                } else {
                    entity.raw_dwg_data.is_none()
                        || entity
                            .dwg_source_version
                            .is_some_and(|source| source != target_version)
                }
            }
            _ => false,
        };
        if dropped {
            n += 1;
        }
    }
    n
}

/// Before overwriting `path`, copy the existing file to a sibling `<name>.bak`
/// so a faulty or accidental save can be recovered (#205). Best-effort: a
/// failed backup never blocks the save itself.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn write_backup(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("bak"));
    }
}

/// Structured native-save failure. The UI needs the OS error category after
/// the worker completes so file-sharing violations can offer recovery actions
/// instead of being flattened into an opaque command-line string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFailure {
    pub message: String,
    pub file_in_use: bool,
    pub externally_modified: bool,
}

impl SaveFailure {
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file_in_use: false,
            externally_modified: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn file_in_use(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file_in_use: true,
            externally_modified: false,
        }
    }

    fn replacing(path: &Path, error: std::io::Error) -> Self {
        Self {
            message: format!("replace {}: {error}", path.display()),
            file_in_use: replace_error_is_file_in_use(&error),
            externally_modified: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn externally_modified(path: &Path) -> Self {
        Self {
            message: format!(
                "{} changed on disk after it was opened",
                path.display()
            ),
            file_in_use: false,
            externally_modified: true,
        }
    }
}

impl std::fmt::Display for SaveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SaveFailure {}

fn replace_error_is_file_in_use(error: &std::io::Error) -> bool {
    #[cfg(target_os = "windows")]
    {
        // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION. ReplaceFileW returns
        // 32 for the common case where AutoCAD holds the DWG open (#498).
        windows_replace_error_is_file_in_use(error.raw_os_error())
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Advisory locks normally do not block rename on Unix, but filesystems
        // may still report EBUSY or ETXTBSY for an active destination.
        matches!(error.raw_os_error(), Some(16 | 26))
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_replace_error_is_file_in_use(raw_os_error: Option<i32>) -> bool {
    matches!(raw_os_error, Some(32 | 33))
}

#[cfg(test)]
mod save_failure_tests {
    use super::windows_replace_error_is_file_in_use;

    #[test]
    fn issue_498_recognizes_windows_file_sharing_errors() {
        assert!(windows_replace_error_is_file_in_use(Some(32)));
        assert!(windows_replace_error_is_file_in_use(Some(33)));
        assert!(!windows_replace_error_is_file_in_use(Some(5)));
        assert!(!windows_replace_error_is_file_in_use(None));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn external_change_prevents_atomic_replace() {
        let path = std::env::temp_dir().join(format!(
            "ocs_external_change_{}_{}.dwg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();
        let expected = super::edit_lock::FileFingerprint::capture(&path).unwrap();
        std::fs::write(&path, b"new").unwrap();

        let error = super::save_owned_as_version_atomic(
            acadrust::CadDocument::new(),
            &path,
            acadrust::DxfVersion::AC1032,
            false,
            Some(expected),
        )
        .unwrap_err();

        assert!(error.externally_modified);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let _ = std::fs::remove_file(path);
    }
}

// ── Plot Style Table ──────────────────────────────────────────────────────

/// Show a file-open dialog and load the selected CTB or STB file.
pub async fn pick_plot_style() -> Option<plot_style::PlotStyleTable> {
    let mut dialog = crate::sys::file_dialog()
        .set_title("Load Plot Style Table")
        .add_filter("Plot Style Tables", &["ctb", "CTB"])
        .add_filter("CTB Files", &["ctb", "CTB"])
        .add_filter("All Files", &["*"]);
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(dir) = plot_style::ensure_plot_styles_dir() {
        dialog = dialog.set_directory(dir);
    }
    let handle = dialog.pick_file().await?;
    plot_style::PlotStyleTable::load(&crate::sys::handle_path(&handle)).ok()
}

// ── Image file picker ─────────────────────────────────────────────────────

/// Show a file-open dialog for raster images and decode the selected file.
/// Returns `(path, pixel_width, pixel_height)` or an error string.
pub async fn pick_image_file() -> Result<(PathBuf, u32, u32), String> {
    let handle = crate::sys::file_dialog()
        .set_title("Select Image File")
        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "tiff", "tif"])
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("All Files", &["*"])
        .pick_file()
        .await
        .ok_or_else(|| "Cancelled".to_string())?;
    let path = crate::sys::handle_path(&handle);
    let img = image::open(&path).map_err(|e| e.to_string())?;
    let (w, h) = image::GenericImageView::dimensions(&img);
    Ok((path, w, h))
}

/// Save `doc` to `path` with the given DXF version, overriding `doc.version`.
/// Format is auto-detected from the extension (dwg / dxf).
pub fn save_as_version(
    doc: &CadDocument,
    path: &Path,
    version: acadrust::DxfVersion,
) -> Result<(), String> {
    let clone_started = iced::time::Instant::now();
    let snapshot = doc.clone();
    let clone_ms = clone_started.elapsed().as_secs_f64() * 1000.0;
    save_owned_as_version_inner(snapshot, path, version, false, clone_ms, |_| Ok(()))
        .map_err(|error| error.to_string())
}

/// Save an owned document snapshot. Preparation, serialization, compression and
/// disk I/O can therefore run on a worker without borrowing live editor state.
/// Output is written beside the destination and atomically renamed only after a
/// complete file exists, so a failed save cannot truncate the previous drawing.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_owned_as_version_atomic(
    doc: CadDocument,
    path: &Path,
    version: acadrust::DxfVersion,
    backup: bool,
    expected_fingerprint: Option<edit_lock::FileFingerprint>,
) -> Result<(), SaveFailure> {
    save_owned_as_version_inner(doc, path, version, backup, 0.0, move |path| {
        let Some(expected) = expected_fingerprint else {
            return Ok(());
        };
        match edit_lock::FileFingerprint::capture(path) {
            Ok(current) if current == expected => Ok(()),
            _ => Err(SaveFailure::externally_modified(path)),
        }
    })
}

fn save_owned_as_version_inner<F>(
    mut doc: CadDocument,
    path: &Path,
    version: acadrust::DxfVersion,
    backup: bool,
    clone_ms: f64,
    before_replace: F,
) -> Result<(), SaveFailure>
where
    F: FnOnce(&Path) -> Result<(), SaveFailure>,
{
    let perf = crate::perf::enabled();
    let total_started = iced::time::Instant::now();
    doc.version = version;
    let styles_started = iced::time::Instant::now();
    sync_current_styles_on_save(&mut doc);
    let styles_ms = styles_started.elapsed().as_secs_f64() * 1000.0;
    let dimensions_started = iced::time::Instant::now();
    crate::modules::draw::modify::explode::bake_dimension_blocks(&mut doc);
    let dimensions_ms = dimensions_started.elapsed().as_secs_f64() * 1000.0;
    let temp_path = save_temp_path(path);
    let ext = temp_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let write_started = iced::time::Instant::now();
    let result = match ext.as_str() {
        "dxf" => DxfWriter::new(&doc)
            .write_to_file(&temp_path)
            .map_err(|e| SaveFailure::other(e.to_string())),
        _ => DwgWriter::write_to_file(&temp_path, &doc)
            .map_err(|e| SaveFailure::other(e.to_string())),
    };
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = before_replace(path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    if backup {
        write_backup(path);
    }
    if let Err(error) = replace_save_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(SaveFailure::replacing(path, error));
    }
    if perf {
        crate::perf_record!(
            "[perf] save total={:.1}ms clone={:.1} styles={:.1} dimensions={:.1} write={:.1} entities={} objects={} path={}",
            total_started.elapsed().as_secs_f64() * 1000.0,
            clone_ms,
            styles_ms,
            dimensions_ms,
            write_started.elapsed().as_secs_f64() * 1000.0,
            doc.entities().count(),
            doc.objects.len(),
            path.display(),
        );
    }
    Ok(())
}

fn save_temp_path(path: &Path) -> PathBuf {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".to_string());
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dwg".to_string());
    let name = format!(
        ".{stem}.ocs-save-{}-{serial}.{extension}",
        std::process::id()
    );
    path.parent().unwrap_or_else(|| Path::new(".")).join(name)
}

#[cfg(not(target_os = "windows"))]
fn replace_save_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(target_os = "windows")]
fn replace_save_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return std::fs::rename(temp_path, path);
    }
    use std::os::windows::ffi::OsStrExt;
    let replaced: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement: Vec<u16> = temp_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Save using the document's existing version.
pub fn save(doc: &CadDocument, path: &Path) -> Result<(), String> {
    save_as_version(doc, path, doc.version)
}

/// Serialize a document to an in-memory byte buffer, format chosen by `ext`
/// (`dxf` → DXF, anything else → DWG) at the given DXF version (overriding
/// `doc.version`). Used by the web build, which hands the bytes to a browser
/// download instead of writing a path — the byte-buffer counterpart to
/// [`save_as_version`].
#[allow(dead_code)] // web/wasm build only; unused in the native target
pub fn save_to_bytes(
    doc: &CadDocument,
    ext: &str,
    version: acadrust::DxfVersion,
) -> Result<Vec<u8>, String> {
    let perf = crate::perf::enabled();
    let total_started = iced::time::Instant::now();
    let clone_started = iced::time::Instant::now();
    let mut doc = doc.clone();
    let clone_ms = clone_started.elapsed().as_secs_f64() * 1000.0;
    doc.version = version;
    let styles_started = iced::time::Instant::now();
    sync_current_styles_on_save(&mut doc);
    let styles_ms = styles_started.elapsed().as_secs_f64() * 1000.0;
    let dimensions_started = iced::time::Instant::now();
    crate::modules::draw::modify::explode::bake_dimension_blocks(&mut doc);
    let dimensions_ms = dimensions_started.elapsed().as_secs_f64() * 1000.0;
    let write_started = iced::time::Instant::now();
    let result = match ext.to_lowercase().as_str() {
        "dxf" => DxfWriter::new(&doc).write_to_vec().map_err(|e| e.to_string()),
        _ => {
            let mut buf = std::io::Cursor::new(Vec::new());
            DwgWriter::write_to_writer(&mut buf, &doc).map_err(|e| e.to_string())?;
            Ok(buf.into_inner())
        }
    };
    if perf {
        let bytes = result.as_ref().map_or(0, Vec::len);
        crate::perf_record!(
            "[perf] save-bytes total={:.1}ms clone={:.1} styles={:.1} dimensions={:.1} write={:.1} bytes={} entities={} objects={}",
            total_started.elapsed().as_secs_f64() * 1000.0,
            clone_ms,
            styles_ms,
            dimensions_ms,
            write_started.elapsed().as_secs_f64() * 1000.0,
            bytes,
            doc.entities().count(),
            doc.objects.len(),
        );
    }
    result
}


// ── Post-load fixups ──────────────────────────────────────────────────────

// Resolve the current text / dimension / multiline style from the handle the
// DWG header stores into the *name* the app reads. DXF stores these as names
// directly ($TEXTSTYLE / $DIMSTYLE / $CMLSTYLE), but DWG only stores handles,
// so without this the current-style markers (and any code keyed on the name)
// fall back to "Standard". Only overrides when the handle resolves, leaving the
// DXF-provided names intact.
fn fix_current_style_names(doc: &mut CadDocument) {
    use acadrust::objects::ObjectType;

    let h = doc.header.current_text_style_handle;
    if h.is_valid() {
        if let Some(name) = doc.text_styles.iter().find(|s| s.handle == h).map(|s| s.name.clone()) {
            doc.header.current_text_style_name = name;
        }
    }
    let h = doc.header.current_dimstyle_handle;
    if h.is_valid() {
        if let Some(name) = doc.dim_styles.iter().find(|s| s.handle == h).map(|s| s.name.clone()) {
            doc.header.current_dimstyle_name = name;
        }
    }
    let h = doc.header.current_multiline_style_handle;
    if h.is_valid() {
        if let Some(name) = doc.objects.values().find_map(|o| match o {
            ObjectType::MLineStyle(s) if s.handle == h => Some(s.name.clone()),
            _ => None,
        }) {
            doc.header.multiline_style = name;
        }
    }

    // Current table / multileader style. DXF carries these as $CTABLESTYLE /
    // $CMLEADERSTYLE header vars (already read). DWG has no header field for
    // them — they live in the variable dictionary as DICTIONARYVAR entries
    // keyed "CTABLESTYLE" / "CMLEADERSTYLE". Resolve from there when present;
    // a missing entry simply leaves the existing value untouched.
    if let Some(v) = vardict_value(doc, "CTABLESTYLE") {
        if !v.is_empty() {
            doc.header.current_table_style_name = v;
        }
    }
    if let Some(v) = vardict_value(doc, "CMLEADERSTYLE") {
        if !v.is_empty() {
            doc.header.current_mleader_style_name = v;
        }
    }
}

/// Find the handle of the `DictionaryVariable` registered under `name` in any
/// of the document's dictionaries (the variable dictionary).
fn vardict_handle(doc: &CadDocument, name: &str) -> Option<acadrust::Handle> {
    use acadrust::objects::ObjectType;
    doc.objects.values().find_map(|o| {
        let entries = match o {
            ObjectType::Dictionary(d) => &d.entries,
            ObjectType::DictionaryWithDefault(d) => &d.entries,
            _ => return None,
        };
        entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)
    })
}

/// Look up a system-variable value stored in the document's variable
/// dictionary.
fn vardict_value(doc: &CadDocument, name: &str) -> Option<String> {
    use acadrust::objects::ObjectType;
    let handle = vardict_handle(doc, name)?;
    match doc.objects.get(&handle) {
        Some(ObjectType::DictionaryVariable(v)) => Some(v.value.clone()),
        _ => None,
    }
}

/// Write a drawing variable, creating the variable dictionary and record when
/// needed so new drawings preserve the value too.
pub(crate) fn set_drawing_variable(doc: &mut CadDocument, name: &str, value: &str) {
    use acadrust::objects::{Dictionary, DictionaryVariable, ObjectType};
    if let Some(h) = vardict_handle(doc, name) {
        if let Some(ObjectType::DictionaryVariable(v)) = doc.objects.get_mut(&h) {
            v.value = value.to_string();
        }
        return;
    }

    let root = crate::scene::annotative::root_named_dict_handle(doc);
    let variable_dictionary = crate::scene::annotative::as_dict(doc, root)
        .and_then(|dictionary| dictionary.get("AcDbVariableDictionary"))
        .filter(|handle| {
            matches!(doc.objects.get(handle), Some(ObjectType::Dictionary(_)))
        })
        .unwrap_or_else(|| {
            let handle = doc.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = root;
            doc.objects
                .insert(handle, ObjectType::Dictionary(dictionary));
            if let Some(ObjectType::Dictionary(root_dictionary)) = doc.objects.get_mut(&root) {
                root_dictionary.add_entry("AcDbVariableDictionary", handle);
            }
            handle
        });

    let handle = doc.allocate_handle();
    let mut variable = DictionaryVariable::new(name, value);
    variable.handle = handle;
    variable.owner_handle = variable_dictionary;
    doc.objects
        .insert(handle, ObjectType::DictionaryVariable(variable));
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&variable_dictionary) {
        dictionary.add_entry(name, handle);
    }
}

/// The layout tab that was active when the drawing was saved — the `CTAB`
/// system variable (stored in the variable dictionary as a `DICTIONARYVAR`).
/// `None` when the file recorded no current tab.
pub fn saved_active_layout(doc: &CadDocument) -> Option<String> {
    vardict_value(doc, "CTAB").filter(|s| !s.is_empty())
}

/// Record `name` as the active layout tab (`CTAB`) so the next save round-trips
/// which space was open. Updates the existing `CTAB` variable in place, or
/// creates one under the root named-object dictionary when the drawing never
/// carried it (e.g. a document authored here from scratch) — otherwise the exact
/// paper layout would be lost and reopening fell back to the first paper tab.
pub fn set_saved_active_layout(doc: &mut CadDocument, name: &str) {
    set_drawing_variable(doc, "CTAB", name);
}

/// Materialise the current-style choices into their format-specific storage
/// before saving, treating the current-style *names* as the single source of
/// truth:
///
/// * text / dim / multiline live in the DWG header as **handles** — the writer
///   keeps a stored handle if it's still valid and otherwise falls back to
///   "Standard", so a Set Current change (which only updates the name) would be
///   lost. Re-resolve the handle from the name here so the change persists.
/// * table / multileader live in the **variable dictionary** (DICTIONARYVAR).
///
/// DXF additionally writes its own header vars from the names, so this keeps
/// every representation consistent.
fn sync_current_styles_on_save(doc: &mut CadDocument) {
    use acadrust::objects::ObjectType;

    let th = doc
        .text_styles
        .get(&doc.header.current_text_style_name)
        .map(|s| s.handle);
    if let Some(h) = th {
        doc.header.current_text_style_handle = h;
    }
    let dh = doc
        .dim_styles
        .get(&doc.header.current_dimstyle_name)
        .map(|s| s.handle);
    if let Some(h) = dh {
        doc.header.current_dimstyle_handle = h;
    }
    let mname = doc.header.multiline_style.clone();
    let mh = doc.objects.values().find_map(|o| match o {
        ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&mname) => Some(s.handle),
        _ => None,
    });
    if let Some(h) = mh {
        doc.header.current_multiline_style_handle = h;
    }

    let table = doc.header.current_table_style_name.clone();
    let mleader = doc.header.current_mleader_style_name.clone();
    set_drawing_variable(doc, "CTABLESTYLE", &table);
    set_drawing_variable(doc, "CMLEADERSTYLE", &mleader);
    let annotation = doc.header.current_annotation_scale.clone();
    set_drawing_variable(doc, "CANNOSCALE", &annotation);
}

// ── Corrupt-entity guard ──────────────────────────────────────────────────
//
// acadrust's DWG parser occasionally desynchronises on certain files and
// produces entities with garbage fields: non-unit normals (components in
// 1e200+), nonsensical vertex counts (e.g. 100000), or infinite/NaN
// coordinates.  Tessellating such entities triggers huge allocations and
// numerical blow-ups in the wire pipeline.
//
// `purge_corrupt_entities` scans the document and removes any entity that
// fails a cheap sanity check, returning the number dropped so the caller can
// surface it to the UI / log.

fn finite_unit_normal(n: &acadrust::types::Vector3) -> bool {
    let (x, y, z) = (n.x, n.y, n.z);
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return false;
    }
    let mag2 = x * x + y * y + z * z;
    // Accept anything within ~10% of unit length. Real files sometimes
    // store slightly denormalised normals from rounding.
    (mag2 - 1.0).abs() < 0.21
}

fn finite_coord(v: f64) -> bool {
    v.is_finite() && v.abs() < 1.0e12
}

fn finite_vec3(v: &acadrust::types::Vector3) -> bool {
    finite_coord(v.x) && finite_coord(v.y) && finite_coord(v.z)
}

/// Returns true if the entity looks like parser garbage and should be dropped.
pub(crate) fn is_entity_corrupt(e: &EntityType) -> bool {
    use acadrust::entities::EntityType as E;
    // Reject polylines at or above this vertex count. Even valid drawings
    // rarely use this many — and parser desync produces exactly-100_000-vertex
    // junk records.
    const MAX_VERTS: usize = 100_000;
    match e {
        E::LwPolyline(p) => {
            !finite_unit_normal(&p.normal)
                || p.vertices.len() >= MAX_VERTS
                || !finite_coord(p.elevation)
                || p.elevation.abs() > 1.0e10
                || !finite_coord(p.thickness)
                || p.thickness.abs() > 1.0e10
                || p.vertices
                    .iter()
                    .any(|v| !finite_coord(v.location.x) || !finite_coord(v.location.y))
        }
        E::Polyline2D(p) => {
            !finite_unit_normal(&p.normal)
                || p.vertices.len() >= MAX_VERTS
                || !finite_coord(p.elevation)
                || p.elevation.abs() > 1.0e10
                || !finite_coord(p.thickness)
                || p.thickness.abs() > 1.0e10
                || p.vertices.iter().any(|v| !finite_vec3(&v.location))
        }
        E::Polyline3D(p) => {
            p.vertices.len() >= MAX_VERTS
                || p.vertices.iter().any(|v| !finite_vec3(&v.position))
        }
        E::Polyline(p) => {
            p.vertices.len() >= MAX_VERTS
                || p.vertices.iter().any(|v| !finite_vec3(&v.location))
        }
        E::Line(l) => !finite_vec3(&l.start) || !finite_vec3(&l.end),
        E::Circle(c) => {
            !finite_vec3(&c.center)
                || !finite_coord(c.radius)
                // Reject zero- or near-zero circles: they tessellate into a
                // degenerate curve the tessellator cannot sample.
                || c.radius.abs() < 1.0e-10
                || c.radius.abs() > 1.0e10
        }
        E::Arc(a) => {
            !finite_vec3(&a.center)
                || !finite_coord(a.radius)
                || !a.start_angle.is_finite()
                || !a.end_angle.is_finite()
                // Same degenerate-curve guard as Circle.
                || a.radius.abs() < 1.0e-10
                || a.radius.abs() > 1.0e10
                // Zero-sweep arc (start_angle == end_angle, modulo 2π) collapses
                // to a single point in WCS — a three-point circle fit on
                // coincident vertices recurses unboundedly in parameter_division.
                || (a.end_angle - a.start_angle).abs() < 1.0e-9
                // Near-zero sweep is the same trap with a wider mouth: a tiny but
                // non-zero sweep (e.g. 1.6e-6 rad) still places start/mid/end
                // within the coincidence tolerance, so sampling
                // recurses and allocates until OOM. Gate on arc *length*
                // (radius × sweep), not sweep alone, so a legitimately large-
                // radius small-sweep arc (still a visible curve) survives while
                // sub-precision arcs are dropped.
                || a.radius.abs() * (a.end_angle - a.start_angle).abs() < 1.0e-6
                // Near-collinear sample points: even when the arc *length* clears
                // the floor above, a small sweep over a modest radius leaves
                // start/mid/end almost on one line (a 35-unit, 6.5e-7-rad arc has
                // arc length 2.3e-5 — past the gate — yet bows off its chord by
                // only ~2e-12). A three-point circle fit then returns a
                // near-infinite radius and `parameter_division` subdivides without
                // bound. Gate on the sagitta (chord height = r·(1−cos(sweep/2))),
                // the true measure of how far the arc departs a straight line and
                // of the fit's conditioning.
                || a.radius.abs()
                    * (1.0 - ((a.end_angle - a.start_angle).abs() * 0.5).cos())
                    < 1.0e-6
                || !finite_unit_normal(&a.normal)
        }
        E::Ellipse(e) => {
            !finite_vec3(&e.center)
                || !finite_vec3(&e.major_axis)
                || !e.start_parameter.is_finite()
                || !e.end_parameter.is_finite()
                || (e.end_parameter - e.start_parameter).abs() < 1.0e-9
                || {
                    let m2 = e.major_axis.x * e.major_axis.x
                        + e.major_axis.y * e.major_axis.y
                        + e.major_axis.z * e.major_axis.z;
                    !m2.is_finite() || m2 < 1.0e-20 || m2 > 1.0e20
                }
                || !e.minor_axis_ratio.is_finite()
                || e.minor_axis_ratio.abs() < 1.0e-10
        }
        E::Spline(s) => {
            // Parser desync emits exactly-100_000-control-point splines with a
            // garbage knot vector. Building a NURBS from one and
            // tessellating it runs `parameter_division` into an unbounded
            // allocation — single-threaded, 32 GB+ — long before the drawing
            // finishes loading. Reject the desync signature plus any spline
            // the kernel can't build: non-finite control points, or a knot vector
            // that's non-finite, non-monotonic, or the wrong length
            // (the kernel requires `knots.len() == ctrl.len() + degree + 1`).
            let n = s.control_points.len();
            let degree_bad = s.degree < 1;
            let deg = s.degree.max(0) as usize;
            let knots_bad = !s.knots.is_empty()
                && (s.knots.iter().any(|k| !k.is_finite())
                    || s.knots.windows(2).any(|w| w[1] < w[0])
                    || s.knots.len() != n + deg + 1);
            // Degenerate: every control point collapses onto (nearly) the same
            // point, so the curve has zero length. A three-point fit
            // `parameter_division` never converges on it and the tessellation
            // hangs — a periodic 9-point spline pinned at the origin is the seen
            // case. Reject when the control-point extent is sub-precision.
            let degenerate_extent = n >= 2 && {
                let (mut mn, mut mx) = ([f64::MAX; 3], [f64::MIN; 3]);
                for p in &s.control_points {
                    mn[0] = mn[0].min(p.x);
                    mx[0] = mx[0].max(p.x);
                    mn[1] = mn[1].min(p.y);
                    mx[1] = mx[1].max(p.y);
                    mn[2] = mn[2].min(p.z);
                    mx[2] = mx[2].max(p.z);
                }
                (mx[0] - mn[0]).max(mx[1] - mn[1]).max(mx[2] - mn[2]) < 1.0e-6
            };
            n >= MAX_VERTS
                || degree_bad
                || s.control_points.iter().any(|p| !finite_vec3(p))
                || knots_bad
                || degenerate_extent
        }
        _ => false,
    }
}

pub fn purge_corrupt_entities(doc: &mut CadDocument) -> usize {
    use crate::par::prelude::*;
    // Detection is pure and read-only; the per-vertex finite/extent checks on
    // large polylines dominate, so fan the scan out across cores. Gather
    // entity references in one pass, test in parallel, then remove serially
    // (`remove_entity` needs `&mut doc`).
    let entities: Vec<&EntityType> = doc.entities().collect();
    let bad: Vec<acadrust::Handle> = entities
        .par_iter()
        .filter(|e| is_entity_corrupt(e))
        .map(|e| e.common().handle)
        .collect();
    let n = bad.len();
    for h in bad {
        doc.remove_entity(h);
    }
    n
}

/// acadrust's ViewportStatusFlags::from_bits() maps bit 0 → is_on and bit 15 → locked,
/// but the real DXF/DWG spec uses bit 15 (0x8000) → viewport on and bit 14 (0x4000) → locked.
/// Files from AutoCAD and other tools always set bit 15 for active viewports, leaving bit 0
/// clear, so acadrust reads every such viewport as off.  Correct that here after loading.
fn fix_viewport_status_flags(doc: &mut CadDocument) {
    for entity in doc.entities_mut() {
        if let EntityType::Viewport(vp) = entity {
            let bits = vp.status.to_bits();
            // If bit 0 is not set but bit 15 is, this is an external-format viewport:
            // treat bit 15 as "on" and bit 14 as "locked".
            if (bits & 0x0001) == 0 && (bits & 0x8000) != 0 {
                vp.status.is_on = true;
                vp.status.locked = (bits & 0x4000) != 0;
            }
        }
    }
}

/// The acadrust DXF reader stores several rotation fields directly from DXF
/// group code 50 in degrees, while DWG and our own creation code store radians.
/// Apply to_radians() on load so tessellation can call cos/sin uniformly.
fn fix_dxf_dimension_rotations(doc: &mut CadDocument) {
    for entity in doc.entities_mut() {
        match entity {
            // Dimension angles (rotation / text / oblique) are converted
            // degrees->radians inside the acadrust DXF reader now, so a
            // dimension arm here would double-convert.
            EntityType::AttributeDefinition(a) => {
                a.rotation = a.rotation.to_radians();
            }
            EntityType::AttributeEntity(a) => {
                a.rotation = a.rotation.to_radians();
            }
            EntityType::Shape(s) => {
                s.rotation = s.rotation.to_radians();
            }
            _ => {}
        }
    }
}

/// Recover integer-valued AcDbPlotSettings fields that acadrust can leave at
/// their defaults when a DXF writer right-aligns the value with leading spaces.
/// The raw pairs are preserved on Layout, so trim and parse those authoritative
/// values after loading. In particular, losing code 73 turns a 90°/270° sheet
/// back to 0° and makes a landscape layout render as portrait (#505).
fn fix_dxf_layout_plot_settings(doc: &mut CadDocument) {
    use acadrust::objects::{ObjectType, PlotFlags};

    for object in doc.objects.values_mut() {
        let ObjectType::Layout(layout) = object else {
            continue;
        };
        let Some(codes) = layout.raw_plot_settings_codes.as_ref() else {
            continue;
        };

        for (code, value) in codes {
            match *code {
                70 => {
                    if let Ok(value) = value.trim().parse::<i32>() {
                        layout.plot_flags = PlotFlags::from_bits(value);
                    }
                }
                72 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.plot_paper_units = value;
                    }
                }
                73 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.plot_rotation = value;
                    }
                }
                74 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.plot_type = value;
                    }
                }
                75 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.plot_scale_type = value;
                    }
                }
                76 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.shade_plot_mode = value;
                    }
                }
                77 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.shade_plot_resolution = value;
                    }
                }
                78 => {
                    if let Ok(value) = value.trim().parse::<i16>() {
                        layout.shade_plot_dpi = value;
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod layer_roundtrip_tests {
    use super::*;
    use acadrust::tables::layer::Layer as DocLayer;

    // Add `count` new layers the way the UI does (allocate_handle, then add),
    // round-trip through `ext`, and return whether every one survived.
    fn roundtrip_layers(ext: &str, count: usize) -> bool {
        let mut doc = CadDocument::new();
        crate::io::linetypes::populate_document(&mut doc);
        let names: Vec<String> = (0..count).map(|n| format!("Layer{}", n + 1)).collect();
        for name in &names {
            let mut dl = DocLayer::new(name);
            dl.handle = doc.allocate_handle();
            doc.layers.add(dl).unwrap();
        }
        let path = std::env::temp_dir().join(format!("ocs_layer_rt_{count}.{ext}"));
        save_as_version(&doc, &path, acadrust::DxfVersion::AC1032).expect("save");
        let loaded = load_file(&path).expect("load");
        let _ = std::fs::remove_file(&path);
        names.iter().all(|n| loaded.layers.contains(n))
    }

    #[test]
    fn dwg_preserves_new_layer() {
        assert!(roundtrip_layers("dwg", 1), "DWG dropped the new layer (issue #67)");
    }

    #[test]
    fn dxf_preserves_new_layer() {
        assert!(roundtrip_layers("dxf", 1), "DXF dropped the new layer");
    }

    // Each new layer must get a distinct handle, or they collide and all but
    // the last are dropped on a handle-based DWG save (issue #67).
    #[test]
    fn dwg_preserves_multiple_new_layers() {
        assert!(roundtrip_layers("dwg", 3), "DWG dropped colliding new layers (issue #67)");
    }

    // #252: an entity added (as a plugin does) on a layer that no LAYER command
    // ever created must keep that layer across a DWG save — `Scene::add_entity`
    // auto-registers it so the writer resolves a real handle instead of NULL
    // (which reopens as layer "0").
    #[test]
    fn dwg_preserves_entity_layer_auto_registered_on_add() {
        use acadrust::entities::Point;
        use acadrust::EntityType;

        let mut scene = crate::scene::Scene::new();
        crate::io::linetypes::populate_document(&mut scene.document);

        let mut pt = Point::new();
        pt.common.layer = "PLUGIN-LAYER".to_string();
        let h = scene.add_entity(EntityType::Point(pt));
        assert!(!h.is_null(), "entity was not added");

        let path = std::env::temp_dir().join("ocs_entity_layer_rt.dwg");
        save_as_version(&scene.document, &path, acadrust::DxfVersion::AC1032).expect("save");
        let loaded = load_file(&path).expect("load");
        let _ = std::fs::remove_file(&path);

        assert!(
            loaded.layers.contains("PLUGIN-LAYER"),
            "layer table dropped the auto-registered layer (#252)"
        );
        let ent = loaded
            .get_entity(h)
            .or_else(|| loaded.entities().find(|e| matches!(e, EntityType::Point(_))))
            .expect("point entity missing after round-trip");
        assert_eq!(
            ent.common().layer,
            "PLUGIN-LAYER",
            "entity collapsed to layer 0 on DWG save (#252)"
        );
    }
}

#[cfg(test)]
mod corrupt_guard_tests {
    use super::*;
    use acadrust::entities::{Arc, EntityType, Spline};
    use acadrust::types::Vector3;

    // A near-zero-sweep arc: sweep 1.56e-6 rad on a 3.9e-3 radius. The angles
    // are individually finite and the radius is in range, so the old
    // (end-start) < 1e-9 check passed it through — but start/mid/end land
    // within the coincidence tolerance and sampling allocates
    // until OOM. The arc-length floor must reject it.
    #[test]
    fn rejects_near_degenerate_arc() {
        let mut a = Arc::new();
        a.center = Vector3::new(2880.84, 891.83, 0.0);
        a.radius = 0.0038974142851181423;
        a.start_angle = 1.0401656235942365;
        a.end_angle = 1.0401671831670538;
        a.normal = Vector3::new(0.0, 0.0, 1.0);
        assert!(is_entity_corrupt(&EntityType::Arc(a)));
    }

    // A 35-unit-radius arc sweeping 6.5e-7 rad has arc length 2.3e-5 — past the
    // arc-length floor — yet its start/mid/end bow off the chord by only ~2e-12,
    // so a three-point circle fit blows up and sampling hangs. The
    // sagitta floor must reject it where the arc-length floor alone does not.
    #[test]
    fn rejects_near_collinear_arc() {
        let mut a = Arc::new();
        a.center = Vector3::new(551435.3071786845, 4051623.7156955916, 0.0);
        a.radius = 35.0;
        a.start_angle = 5.823361856481176;
        a.end_angle = 5.823362506017916;
        a.normal = Vector3::new(0.0, 0.0, 1.0);
        // Sanity: arc length clears the old gate, proving the sagitta gate is
        // what catches this one.
        assert!(a.radius * (a.end_angle - a.start_angle).abs() > 1.0e-6);
        assert!(is_entity_corrupt(&EntityType::Arc(a)));
    }

    // A large-radius small-sweep arc is still a visible curve and must survive:
    // radius 1e6 × sweep 1e-4 ≈ 100 units of arc.
    #[test]
    fn keeps_large_radius_small_sweep_arc() {
        let mut a = Arc::new();
        a.radius = 1.0e6;
        a.start_angle = 0.0;
        a.end_angle = 1.0e-4;
        a.normal = Vector3::new(0.0, 0.0, 1.0);
        assert!(!is_entity_corrupt(&EntityType::Arc(a)));
    }

    // Parser desync emits 100_000-control-point splines; building a kernel
    // NURBS from one and tessellating it OOMs. The control-point cap rejects it.
    #[test]
    fn rejects_desync_spline() {
        let pts = vec![Vector3::new(0.0, 0.0, 0.0); 100_000];
        let s = Spline::from_control_points(3, pts);
        assert!(is_entity_corrupt(&EntityType::Spline(s)));
    }

    // A periodic spline whose control points all collapse onto (nearly) one
    // point has zero length; sampling never converges and the
    // tessellation hangs. The control-point extent floor must reject it.
    #[test]
    fn rejects_degenerate_point_spline() {
        let pts = vec![Vector3::new(1e-12, -1e-12, 0.0); 9];
        let s = Spline::from_control_points(3, pts);
        assert!(is_entity_corrupt(&EntityType::Spline(s)));
    }

    // A normal cubic spline (4 control points, valid clamped knots) survives.
    #[test]
    fn keeps_valid_spline() {
        let pts = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
            Vector3::new(2.0, -1.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ];
        let s = Spline::from_control_points(3, pts);
        assert!(!is_entity_corrupt(&EntityType::Spline(s)));
    }
}
