// I/O module — open, save, and export CAD documents.
//
// All file reading/writing goes through acadrust.
// Default save format: DWG (AC1032 / R2018+).

pub mod file_association;
#[cfg(target_os = "windows")]
pub mod hang_watchdog;
pub mod obj;
#[cfg(not(target_arch = "wasm32"))]
pub mod single_instance;
pub mod pdf_export;
pub mod plot_style;
pub mod print_to_printer;
pub mod step;
pub mod stl;
pub mod xref;
pub mod linetypes;
pub mod patterns;
pub mod update_check;
pub mod paper_sizes;
pub mod thumbnail;

use crate::scene::DerivedCaches;
use acadrust::entities::EntityType;
use acadrust::io::dwg::DwgReader;
use acadrust::{CadDocument, DwgWriter, DxfReader, DxfWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

// Phase tags written into the shared atomic so the UI overlay can display a
// human-readable label. Kept in sync with the constants in `crate::app`.
const PHASE_PARSING: u8 = 1;
const PHASE_CACHING: u8 = 2;
const PHASE_FINALIZING: u8 = 3;

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

/// Load a CAD file from a known path. Parsing and cache building run on a
/// dedicated OS thread so the async executor stays free for rendering during
/// the load. Writes phase markers into `phase` so the UI can show
/// "Parsing entities…" / "Building caches…" / "Finalizing…" while the loader
/// thread runs.
pub async fn open_path_with_phase(
    path: PathBuf,
    phase: Arc<AtomicU8>,
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let path2 = path.clone();
    let phase2 = phase.clone();
    let (doc, caches) = std::thread::spawn(move || -> Result<_, String> {
        use iced::time::Instant;
        phase2.store(PHASE_PARSING, Ordering::Relaxed);
        let t_parse = Instant::now();
        let mut doc = load_file(&path2)?;
        let parse_ms = t_parse.elapsed().as_millis() as u32;
        let t_purge = Instant::now();
        let dropped = purge_corrupt_entities(&mut doc);
        let purge_ms = t_purge.elapsed().as_millis() as u32;
        phase2.store(PHASE_CACHING, Ordering::Relaxed);
        let t_caches = Instant::now();
        let mut caches = crate::scene::build_derived_caches(&doc);
        caches.timings = crate::scene::OpenTimings {
            parse_ms,
            purge_ms,
            caches_ms: t_caches.elapsed().as_millis() as u32,
        };
        caches.corrupt_dropped = dropped;
        phase2.store(PHASE_FINALIZING, Ordering::Relaxed);
        Ok((doc, caches))
    })
    .join()
    .map_err(|_| "parser thread panicked".to_string())??;
    Ok((name, path, doc, caches))
}

/// Web file open: show the browser picker, read the chosen file's bytes, parse
/// it, and build the derived caches — producing the same payload as the native
/// `open_path_with_phase` so it can feed the existing `Message::FileOpened`
/// handler. There is no filesystem path on the web, so a name-only `PathBuf`
/// stands in for the document path.
#[cfg(target_arch = "wasm32")]
pub async fn pick_and_load_web(
) -> Result<(String, PathBuf, CadDocument, DerivedCaches), String> {
    let handle = crate::sys::file_dialog()
        .set_title("Open CAD file")
        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
        .add_filter("All Files", &["*"])
        .pick_file()
        .await
        .ok_or_else(|| "Cancelled".to_string())?;
    let name = handle.file_name();
    let bytes = handle.read().await;
    let mut doc = load_bytes(&name, bytes)?;
    let dropped = purge_corrupt_entities(&mut doc);
    let mut caches = crate::scene::build_derived_caches(&doc);
    caches.corrupt_dropped = dropped;
    let path = PathBuf::from(&name);
    Ok((name, path, doc, caches))
}

/// Parse a CAD document from in-memory bytes, choosing the format from
/// `name`'s extension. Used by the web build, where files arrive as bytes from
/// a browser file picker (no filesystem path). Shares the post-load fixups with
/// [`load_file`]; raster-image path resolution is skipped (there is no sibling
/// directory to search on the web).
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

pub fn load_file(path: &Path) -> Result<CadDocument, String> {
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
        "dwg" => {
            let mut doc = DwgReader::from_file(path)
                .map_err(|e| e.to_string())?
                .read()
                .map_err(|e| e.to_string())?;
            fix_viewport_status_flags(&mut doc);
            fix_current_style_names(&mut doc);
            resolve_raster_image_paths(&mut doc, path.parent());
            Ok(doc)
        }
        "dxf" => {
            let mut doc = DxfReader::from_file(path)
                .map_err(|e| e.to_string())?
                .read()
                .map_err(|e| e.to_string())?;
            fix_dxf_dimension_rotations(&mut doc);
            fix_viewport_status_flags(&mut doc);
            fix_current_style_names(&mut doc);
            resolve_raster_image_paths(&mut doc, path.parent());
            Ok(doc)
        }
        _ => Err(format!("Unsupported file format: .{ext}")),
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
}

/// Resolve a (possibly foreign / absolute) image path to an existing file:
/// as stored, then relative to the drawing folder, then just the file name
/// next to the drawing.
fn resolve_image_file(raw: &str, base_dir: Option<&Path>) -> Option<String> {
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
    if !is_dxf && target_version == doc.version {
        return 0;
    }
    let mut n = doc
        .objects
        .values()
        .filter(|o| {
            matches!(
                o,
                acadrust::objects::ObjectType::Unknown { raw_dwg_data: Some(_), .. }
            )
        })
        .count();
    for e in doc.entities() {
        if matches!(
            e,
            acadrust::EntityType::Unknown(_)
                | acadrust::EntityType::SectionSymbol(_)
                | acadrust::EntityType::ViewBorder(_)
        ) {
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

// ── Plot Style Table ──────────────────────────────────────────────────────

/// Show a file-open dialog and load the selected CTB or STB file.
pub async fn pick_plot_style() -> Option<plot_style::PlotStyleTable> {
    let handle = crate::sys::file_dialog()
        .set_title("Load Plot Style Table")
        .add_filter("Plot Style Tables", &["ctb", "stb", "CTB", "STB"])
        .add_filter("CTB Files", &["ctb", "CTB"])
        .add_filter("STB Files", &["stb", "STB"])
        .add_filter("All Files", &["*"])
        .pick_file()
        .await?;
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
    let mut doc = doc.clone();
    doc.version = version;
    sync_current_styles_on_save(&mut doc);
    crate::modules::draw::modify::explode::bake_dimension_blocks(&mut doc);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "dxf" => DxfWriter::new(&doc)
            .write_to_file(path)
            .map_err(|e| e.to_string()),
        _ => DwgWriter::write_to_file(path, &doc).map_err(|e| e.to_string()),
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
    let mut doc = doc.clone();
    doc.version = version;
    sync_current_styles_on_save(&mut doc);
    crate::modules::draw::modify::explode::bake_dimension_blocks(&mut doc);
    match ext.to_lowercase().as_str() {
        "dxf" => DxfWriter::new(&doc).write_to_vec().map_err(|e| e.to_string()),
        _ => {
            let mut buf = std::io::Cursor::new(Vec::new());
            DwgWriter::write_to_writer(&mut buf, &doc).map_err(|e| e.to_string())?;
            Ok(buf.into_inner())
        }
    }
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

/// Write a value into an existing variable-dictionary entry. No-op when the
/// entry is absent (e.g. a brand-new document with no variable dictionary).
fn set_vardict_value(doc: &mut CadDocument, name: &str, value: &str) {
    use acadrust::objects::ObjectType;
    if let Some(h) = vardict_handle(doc, name) {
        if let Some(ObjectType::DictionaryVariable(v)) = doc.objects.get_mut(&h) {
            v.value = value.to_string();
        }
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
    use acadrust::objects::{DictionaryVariable, ObjectType};
    if let Some(h) = vardict_handle(doc, "CTAB") {
        if let Some(ObjectType::DictionaryVariable(v)) = doc.objects.get_mut(&h) {
            v.value = name.to_string();
        }
        return;
    }
    // Attach a new CTAB entry to the root named-object dictionary. Resolve it
    // robustly (or synthesise one) so the current-tab record persists even on a
    // from-scratch document, or a foreign DWG whose header root pointer is
    // unresolvable. See `annotative::root_named_dict_handle`.
    let root = crate::scene::annotative::root_named_dict_handle(doc);
    let handle = doc.allocate_handle();
    let mut var = DictionaryVariable::new("CTAB", name);
    var.handle = handle;
    var.owner_handle = root;
    doc.objects.insert(handle, ObjectType::DictionaryVariable(var));
    if let Some(ObjectType::Dictionary(rd)) = doc.objects.get_mut(&root) {
        rd.entries.push(("CTAB".to_string(), handle));
    }
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
    set_vardict_value(doc, "CTABLESTYLE", &table);
    set_vardict_value(doc, "CMLEADERSTYLE", &mleader);
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
                // degenerate truck curve that crashes parameter_division.
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
                // to a single point in WCS — truck's circle_arc on three
                // coincident vertices recurses unboundedly in parameter_division.
                || (a.end_angle - a.start_angle).abs() < 1.0e-9
                // Near-zero sweep is the same trap with a wider mouth: a tiny but
                // non-zero sweep (e.g. 1.6e-6 rad) still places start/mid/end
                // within truck's coincidence tolerance, so parameter_division
                // recurses and allocates until OOM. Gate on arc *length*
                // (radius × sweep), not sweep alone, so a legitimately large-
                // radius small-sweep arc (still a visible curve) survives while
                // sub-precision arcs are dropped.
                || a.radius.abs() * (a.end_angle - a.start_angle).abs() < 1.0e-6
                // Near-collinear sample points: even when the arc *length* clears
                // the floor above, a small sweep over a modest radius leaves
                // start/mid/end almost on one line (a 35-unit, 6.5e-7-rad arc has
                // arc length 2.3e-5 — past the gate — yet bows off its chord by
                // only ~2e-12). truck's 3-point `circle_arc` fit then returns a
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
            // garbage knot vector. Building a truck NURBS/B-spline from one and
            // tessellating it runs `parameter_division` into an unbounded
            // allocation — single-threaded, 32 GB+ — long before the drawing
            // finishes loading. Reject the desync signature plus any spline
            // truck can't build: non-finite control points, or a knot vector
            // that's non-finite, non-monotonic, or the wrong length
            // (truck requires `knots.len() == ctrl.len() + degree + 1`).
            let n = s.control_points.len();
            let degree_bad = s.degree < 1;
            let deg = s.degree.max(0) as usize;
            let knots_bad = !s.knots.is_empty()
                && (s.knots.iter().any(|k| !k.is_finite())
                    || s.knots.windows(2).any(|w| w[1] < w[0])
                    || s.knots.len() != n + deg + 1);
            // Degenerate: every control point collapses onto (nearly) the same
            // point, so the curve has zero length. truck's `circle_arc` /
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
    // within truck's coincidence tolerance and parameter_division allocates
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
    // so truck's 3-point circle fit blows up and parameter_division hangs. The
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

    // Parser desync emits 100_000-control-point splines; building a truck
    // NURBS from one and tessellating it OOMs. The control-point cap rejects it.
    #[test]
    fn rejects_desync_spline() {
        let pts = vec![Vector3::new(0.0, 0.0, 0.0); 100_000];
        let s = Spline::from_control_points(3, pts);
        assert!(is_entity_corrupt(&EntityType::Spline(s)));
    }

    // A periodic spline whose control points all collapse onto (nearly) one
    // point has zero length; truck's parameter_division never converges and the
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
