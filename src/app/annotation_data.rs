use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use codec::entities::{table::CellRange, TableBuilder};
use codec::objects::{ClassObject, ClassObjectData, DataLink, DataLinkCustomData, ObjectType};
use codec::types::{Handle, Vector3};
use codec::{CadDocument, EntityType};
use iced::Task;

use super::{Message, ModalKind, OpenCADStudio};
use crate::command::CadCommand;
use crate::ui::window::annotation_data::{
    DataExtractionField, DataExtractionState, DataLinkChoice, DataLinkField, ExtractionBegin,
    ExtractionPage, ExtractionSource, LinkPathType, LinkRange, TableInsertField, TableInsertion,
    TableSource,
};

#[derive(Clone)]
struct ExtractionRecord {
    object_type: String,
    handle: String,
    layer: String,
    color: String,
    linetype: String,
    details: String,
}

// ── In-session picked-file registries (web) ───────────────────────────────
//
// The browser exposes no reusable filesystem path: a picked file's bytes exist
// only while the app keeps them. Both registries below hold those bytes (and
// what was parsed from them) keyed by the reference the drawing or wizard
// carries, following the image-model / PDF-raster pattern. Native builds never
// fill them — the real path stays readable — but the lookup and matching
// logic compiles everywhere so it stays unit-testable.

fn picked_sources() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn extraction_documents() -> &'static Mutex<HashMap<String, Arc<CadDocument>>> {
    static DOCUMENTS: OnceLock<Mutex<HashMap<String, Arc<CadDocument>>>> = OnceLock::new();
    DOCUMENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lower-cased trailing component of a path-style reference. Links and wizard
/// sources carry whatever the author stored, so the file name — not the full
/// path — is the only component a re-picked file can be expected to match.
fn source_name_key(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Do two source references name the same file? Compares trailing components
/// case-insensitively (Windows paths are case-insensitive where the link was
/// authored, and the web has no path beyond the name at all).
fn same_source_name(a: &str, b: &str) -> bool {
    let (a, b) = (source_name_key(a), source_name_key(b));
    !a.is_empty() && a == b
}

/// Keep the bytes of a spreadsheet or settings file chosen through a picker.
/// Re-opening the link in-session re-reads these instead of demanding a new
/// pick; re-picking the same file overwrites the entry.
// Only the web registers sources (native re-reads the real file, so a file
// edited after picking is picked up); tests exercise the registry directly.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
/// Returns false when the registration REPLACED different bytes under the
/// same key — the browser hands back bare file names, so two distinct files
/// named alike collide and every reference to that name would silently read
/// the newer pick. Callers surface that to the user.
pub(crate) fn register_picked_source(path: &str, bytes: Arc<Vec<u8>>) -> bool {
    let mut cache = picked_sources()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(old) = cache.get(path) else {
        cache.insert(path.to_string(), bytes);
        return true;
    };
    if old.as_slice() == bytes.as_slice() {
        return true;
    }
    // Refuse AND keep the old bytes: overwriting would silently retarget
    // every reference reading this name. The name is remembered so the
    // browse handlers can tell the user why their pick did nothing.
    take_last_collision().replace(file_name_of(path));
    false
}

/// The name whose re-pick was refused because different bytes are already
/// registered under it — the browse handler reports and clears it.
fn last_collision() -> &'static Mutex<Option<String>> {
    static LAST: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

fn take_last_collision() -> std::sync::MutexGuard<'static, Option<String>> {
    last_collision()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Bytes registered for this reference — exact key first, then any entry that
/// names the same file (the web fallback for links whose stored path is the
/// original author's, while the user picked only the bare file).
pub(crate) fn picked_source_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    let cache = picked_sources()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(bytes) = cache.get(path) {
        return Some(Arc::clone(bytes));
    }
    cache
        .iter()
        .find(|(key, _)| same_source_name(key, path))
        .map(|(_, bytes)| Arc::clone(bytes))
}

/// Keep a source drawing parsed at pick time, keyed by the name the wizard
/// lists. The extraction pipeline reads these on the web instead of calling
/// the path-based `load_file`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn register_extraction_document(name: &str, document: CadDocument) {
    extraction_documents()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(name.to_string(), Arc::new(document));
}

/// The parsed drawing registered under this source reference, matched by exact
/// key or by file name.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn extraction_document(path: &str) -> Option<Arc<CadDocument>> {
    let cache = extraction_documents()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(document) = cache.get(path) {
        return Some(Arc::clone(document));
    }
    cache
        .iter()
        .find(|(key, _)| same_source_name(key, path))
        .map(|(_, document)| Arc::clone(document))
}

impl ExtractionRecord {
    fn value(&self, key: &str) -> String {
        match key {
            "type" => self.object_type.clone(),
            "handle" => self.handle.clone(),
            "layer" => self.layer.clone(),
            "color" => self.color.clone(),
            "linetype" => self.linetype.clone(),
            "details" => self.details.clone(),
            _ => String::new(),
        }
    }
}

fn table_styles(doc: &CadDocument) -> Vec<String> {
    let mut styles: Vec<String> = doc
        .objects
        .iter()
        .filter_map(|(_, object)| match object {
            ObjectType::TableStyle(style) => Some(style.name.clone()),
            _ => None,
        })
        .collect();
    styles.sort_by_key(|name| name.to_ascii_lowercase());
    styles.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if styles.is_empty() {
        styles.push("Standard".into());
    }
    styles
}

fn link_display_name(link: &DataLink) -> String {
    if !link.description.trim().is_empty() && link.description != link.connection_string {
        return link.description.clone();
    }
    Path::new(&link.connection_string)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Data Link".into())
}

fn resolve_link_path(doc: &CadDocument, link: &DataLink) -> PathBuf {
    let stored = PathBuf::from(&link.connection_string);
    if stored.is_absolute() || link.path_option == 1 {
        return stored;
    }
    doc.source_path
        .as_deref()
        .map(Path::new)
        .and_then(Path::parent)
        .map(|parent| parent.join(&stored))
        .unwrap_or(stored)
}

fn data_link_choices(doc: &CadDocument) -> Vec<DataLinkChoice> {
    let mut links = doc
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::ClassObject(object) => match &object.data {
                ClassObjectData::DataLink(link) => {
                    let path = resolve_link_path(doc, link);
                    let path_text = path.to_string_lossy().into_owned();
                    // The web has no filesystem: a link is valid when its
                    // picked bytes are still registered in-session.
                    let valid = path.is_file() || picked_source_bytes(&path_text).is_some();
                    Some(DataLinkChoice {
                        handle: *handle,
                        name: link_display_name(link),
                        path: path_text,
                        valid,
                    })
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    links.sort_by_key(|link| link.name.to_ascii_lowercase());
    links
}

fn data_link<'a>(doc: &'a CadDocument, handle: Handle) -> Option<&'a DataLink> {
    match doc.objects.get(&handle) {
        Some(ObjectType::ClassObject(object)) => match &object.data {
            ClassObjectData::DataLink(link) => Some(link),
            _ => None,
        },
        _ => None,
    }
}

fn data_link_mut<'a>(doc: &'a mut CadDocument, handle: Handle) -> Option<&'a mut DataLink> {
    match doc.objects.get_mut(&handle) {
        Some(ObjectType::ClassObject(object)) => match &mut object.data {
            ClassObjectData::DataLink(link) => Some(link),
            _ => None,
        },
        _ => None,
    }
}

fn column_index(text: &str) -> Option<usize> {
    let mut value = 0usize;
    let mut any = false;
    for byte in text.trim().bytes() {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        any = true;
        value = value
            .checked_mul(26)?
            .checked_add((byte.to_ascii_uppercase() - b'A' + 1) as usize)?;
    }
    any.then_some(value.saturating_sub(1))
}

fn cell_ref(text: &str) -> Option<(usize, usize)> {
    let split = text.find(|c: char| c.is_ascii_digit())?;
    let column = column_index(&text[..split])?;
    let row = text[split..].trim().parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, column))
}

fn crop_range(rows: Vec<Vec<String>>, range: &str) -> Result<Vec<Vec<String>>, String> {
    let (start, end) = range.split_once(':').unwrap_or((range, range));
    let (Some((r0, c0)), Some((r1, c1))) = (cell_ref(start), cell_ref(end)) else {
        return Err(crate::t!("Enter a valid cell range such as A1:D10.").into_owned());
    };
    let (r0, r1) = (r0.min(r1), r0.max(r1));
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    // The row axis is bounded by the file (skip/take over real rows); the
    // column axis was not — `A1:ZZZZZZ1` parses fine and then collected one
    // String per column (~308 M slots per row, OOM). Columns past the widest
    // row are empty in every row, so clamp the window to the data width.
    let widest = rows.iter().map(Vec::len).max().unwrap_or(0);
    let c1 = c1.min(widest.saturating_sub(1));
    Ok(rows
        .into_iter()
        .skip(r0)
        .take(r1 - r0 + 1)
        .map(|row| {
            (c0..=c1)
                .map(|column| row.get(column).cloned().unwrap_or_default())
                .collect()
        })
        .collect())
}

fn workbook_metadata(path: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "csv" || ext == "txt" {
        return Ok((Vec::new(), Vec::new()));
    }
    if let Some(bytes) = picked_source_bytes(&path.to_string_lossy()) {
        return workbook_metadata_bytes(&bytes);
    }
    if cfg!(target_arch = "wasm32") {
        return Err(
            crate::t!("Pick the file again to load its contents in the browser.").into_owned(),
        );
    }
    use calamine::{open_workbook_auto, Reader};
    let workbook = open_workbook_auto(path).map_err(|error| error.to_string())?;
    let sheets = workbook.sheet_names();
    let names = workbook
        .defined_names()
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    Ok((sheets, names))
}

/// Sheet and defined-name catalog read straight from bytes (a picked file on
/// the web, where no path exists to reopen).
fn workbook_metadata_bytes(bytes: &[u8]) -> Result<(Vec<String>, Vec<String>), String> {
    use calamine::{open_workbook_auto_from_rs, Reader};
    let workbook =
        open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let sheets = workbook.sheet_names();
    let names = workbook
        .defined_names()
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    Ok((sheets, names))
}

fn named_range_target(formula: &str) -> Option<(String, String)> {
    let formula = formula.trim().trim_start_matches('=');
    let (sheet, range) = formula.rsplit_once('!')?;
    let sheet = sheet.trim().trim_matches('\'').replace("''", "'");
    let range = range.replace('$', "");
    (!sheet.is_empty() && !range.is_empty()).then_some((sheet, range))
}

/// Read the text of a CSV/TXT (or settings) source: registered picked bytes
/// first, then the native filesystem. On the web an unregistered path has
/// nothing behind it, so it asks for the pick instead of failing on I/O.
fn read_text_source(path: &Path) -> Result<String, String> {
    if let Some(bytes) = picked_source_bytes(&path.to_string_lossy()) {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    if cfg!(target_arch = "wasm32") {
        return Err(
            crate::t!("Pick the file again to load its contents in the browser.").into_owned(),
        );
    }
    std::fs::read_to_string(path).map_err(|error| error.to_string())
}

/// Sheet selection + range resolution shared by the path- and bytes-based
/// workbook readers. Returns the raw rows and the cell range to crop to, if
/// any.
fn workbook_rows<RS: std::io::Read + std::io::Seek>(
    workbook: &mut calamine::Sheets<RS>,
    sheet: Option<&str>,
    range_kind: LinkRange,
    range_value: Option<&str>,
) -> Result<(Vec<Vec<String>>, Option<String>), String> {
    use calamine::Reader;
    let (selected_sheet, cell_range) = if range_kind == LinkRange::NamedRange {
        let name = range_value
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| crate::t!("Select a named range.").into_owned())?;
        let formula = workbook
            .defined_names()
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, formula)| formula.clone())
            .ok_or_else(|| crate::t!("The named range was not found.").into_owned())?;
        named_range_target(&formula).ok_or_else(|| {
            crate::t!("The named range does not refer to a worksheet cell range.").into_owned()
        })?
    } else {
        let selected_sheet = sheet
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| workbook.sheet_names().first().cloned())
            .ok_or_else(|| crate::t!("The spreadsheet contains no worksheets.").into_owned())?;
        let range = (range_kind == LinkRange::CellRange)
            .then_some(range_value)
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_default();
        (selected_sheet, range)
    };
    let data = workbook
        .worksheet_range(&selected_sheet)
        .map_err(|error| error.to_string())?;
    Ok((
        data.rows()
            .map(|row| row.iter().map(ToString::to_string).collect())
            .collect(),
        (!cell_range.is_empty()).then_some(cell_range),
    ))
}

pub(crate) fn read_tabular_file(
    path: &Path,
    sheet: Option<&str>,
    range_kind: LinkRange,
    range_value: Option<&str>,
) -> Result<Vec<Vec<String>>, String> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (rows, cell_range) = if ext == "csv" || ext == "txt" {
        if range_kind == LinkRange::NamedRange {
            return Err(crate::t!("Named ranges require a spreadsheet file.").into_owned());
        }
        (
            crate::app::commands::display::parse_csv_table(&read_text_source(path)?),
            (range_kind == LinkRange::CellRange)
                .then_some(range_value)
                .flatten()
                .map(str::to_string),
        )
    } else if let Some(bytes) = picked_source_bytes(&path.to_string_lossy()) {
        use calamine::open_workbook_auto_from_rs;
        let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes.as_slice()))
            .map_err(|error| error.to_string())?;
        workbook_rows(&mut workbook, sheet, range_kind, range_value)?
    } else if cfg!(target_arch = "wasm32") {
        return Err(
            crate::t!("Pick the file again to load its contents in the browser.").into_owned(),
        );
    } else {
        use calamine::open_workbook_auto;
        let mut workbook = open_workbook_auto(path).map_err(|error| error.to_string())?;
        workbook_rows(&mut workbook, sheet, range_kind, range_value)?
    };
    let rows = match cell_range
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        Some(value) => crop_range(rows, value)?,
        None => rows,
    };
    if rows.is_empty() || rows.iter().all(Vec::is_empty) {
        Err(crate::t!("The selected data source is empty.").into_owned())
    } else {
        Ok(rows)
    }
}

fn table_style_handle(doc: &CadDocument, name: &str) -> Option<Handle> {
    doc.objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(name) => Some(*handle),
            _ => None,
        })
}

fn build_table(
    rows: &[Vec<String>],
    style: Option<Handle>,
    title: Option<&str>,
) -> codec::entities::Table {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let title_rows = usize::from(title.is_some_and(|value| !value.trim().is_empty()));
    let mut table = TableBuilder::new(rows.len().max(1) + title_rows, columns)
        .at(Vector3::new(0.0, 0.0, 0.0))
        .row_height(0.5)
        .column_width(2.0)
        .build();
    table.table_style_handle = style;
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        table.set_cell_text(0, 0, title);
        if columns > 1 {
            table.merge_cells(CellRange::new(0, 0, 0, columns - 1));
        }
    }
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, value) in row.iter().enumerate() {
            table.set_cell_text(row_index + title_rows, column_index, value);
        }
    }
    table
}

fn link_selection(link: &DataLink) -> (Option<String>, LinkRange, Option<String>) {
    let mut sheet = None;
    let mut kind = LinkRange::EntireSheet;
    let mut range = None;
    for item in &link.custom_data {
        if let Some(value) = item.value.strip_prefix("sheet:") {
            sheet = (!value.is_empty()).then(|| value.to_string());
        } else if let Some(value) = item.value.strip_prefix("range-kind:") {
            kind = if value.eq_ignore_ascii_case("named") {
                LinkRange::NamedRange
            } else if value.eq_ignore_ascii_case("cell") {
                LinkRange::CellRange
            } else {
                LinkRange::EntireSheet
            };
        } else if let Some(value) = item.value.strip_prefix("range:") {
            range = (!value.is_empty()).then(|| value.to_string());
            if kind == LinkRange::EntireSheet {
                kind = LinkRange::CellRange;
            }
        }
    }
    (sheet, kind, range)
}

fn link_custom_data(sheet: &str, kind: LinkRange, range: Option<&str>) -> Vec<DataLinkCustomData> {
    let mut values = Vec::new();
    if !sheet.trim().is_empty() {
        values.push(DataLinkCustomData {
            target: Handle::NULL,
            value: format!("sheet:{}", sheet.trim()),
        });
    }
    if kind != LinkRange::EntireSheet {
        values.push(DataLinkCustomData {
            target: Handle::NULL,
            value: format!(
                "range-kind:{}",
                if kind == LinkRange::NamedRange {
                    "named"
                } else {
                    "cell"
                }
            ),
        });
        if let Some(range) = range.filter(|value| !value.trim().is_empty()) {
            values.push(DataLinkCustomData {
                target: Handle::NULL,
                value: format!("range:{}", range.trim()),
            });
        }
    }
    values
}

fn read_link_rows(doc: &CadDocument, link: &DataLink) -> Result<Vec<Vec<String>>, String> {
    let (sheet, kind, range) = link_selection(link);
    read_tabular_file(
        &resolve_link_path(doc, link),
        sheet.as_deref(),
        kind,
        range.as_deref(),
    )
}

pub(crate) fn read_data_link(
    doc: &CadDocument,
    handle: Handle,
) -> Result<Vec<Vec<String>>, String> {
    let link = data_link(doc, handle)
        .ok_or_else(|| crate::t!("The data link no longer exists.").into_owned())?;
    read_link_rows(doc, link)
}

pub(crate) fn data_link_write_path(doc: &CadDocument, handle: Handle) -> Result<PathBuf, String> {
    let link = data_link(doc, handle)
        .ok_or_else(|| crate::t!("The data link no longer exists.").into_owned())?;
    if link.option & 1 == 0 {
        return Err(crate::t!("Writing to this data link is disabled.").into_owned());
    }
    let path = resolve_link_path(doc, link);
    let writable = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("csv") || value.eq_ignore_ascii_case("txt")
        });
    if !writable {
        return Err(
            crate::t!("Writing linked data is supported for CSV and text sources.").into_owned(),
        );
    }
    Ok(path)
}

fn entity_details(entity: &EntityType) -> String {
    match entity {
        EntityType::Line(value) => format!(
            "({:.3},{:.3},{:.3})-({:.3},{:.3},{:.3})",
            value.start.x, value.start.y, value.start.z, value.end.x, value.end.y, value.end.z
        ),
        EntityType::Circle(value) => format!(
            "C({:.3},{:.3},{:.3}) R={:.3}",
            value.center.x, value.center.y, value.center.z, value.radius
        ),
        EntityType::Arc(value) => format!(
            "C({:.3},{:.3},{:.3}) R={:.3} {:.1}°-{:.1}°",
            value.center.x,
            value.center.y,
            value.center.z,
            value.radius,
            value.start_angle.to_degrees(),
            value.end_angle.to_degrees()
        ),
        EntityType::Text(value) => value.value.clone(),
        EntityType::MText(value) => value.value.clone(),
        EntityType::Insert(value) => format!(
            "{} @({:.3},{:.3},{:.3})",
            value.block_name, value.insert_point.x, value.insert_point.y, value.insert_point.z
        ),
        EntityType::LwPolyline(value) => format!("{} vertices", value.vertices.len()),
        EntityType::Polyline(value) => format!("{} vertices", value.vertices.len()),
        EntityType::Polyline2D(value) => format!("{} vertices", value.vertices.len()),
        EntityType::Polyline3D(value) => format!("{} vertices", value.vertices.len()),
        EntityType::Hatch(value) => value.pattern.name.clone(),
        EntityType::Dimension(value) => format!("{:.3}", value.base().actual_measurement),
        EntityType::Spline(value) => format!("{} control points", value.control_points.len()),
        _ => String::new(),
    }
}

fn collect_records(
    doc: &CadDocument,
    allowed_handles: Option<&BTreeSet<Handle>>,
    allowed_types: &BTreeSet<String>,
    out: &mut Vec<ExtractionRecord>,
) {
    for entity in doc.entities() {
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            continue;
        }
        if allowed_handles.is_some_and(|handles| !handles.contains(&entity.common().handle)) {
            continue;
        }
        let object_type = crate::entities::names::dxf_name(entity).to_string();
        if !allowed_types.is_empty() && !allowed_types.contains(&object_type) {
            continue;
        }
        out.push(ExtractionRecord {
            object_type,
            handle: format!("{:X}", entity.common().handle.value()),
            layer: entity.common().layer.clone(),
            color: entity.common().color.to_string(),
            linetype: entity.common().linetype.clone(),
            details: entity_details(entity),
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_drawing_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("dwg") || value.eq_ignore_ascii_case("dxf"))
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_drawing_paths(
    path: &Path,
    recursive: bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if path.is_file() {
        if is_drawing_file(path) {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !path.is_dir() {
        return Err(
            crate::t!("Data source not found: %{path}", path = path.display()).into_owned(),
        );
    }
    for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let child = entry.path();
        if child.is_file() && is_drawing_file(&child) {
            out.push(child);
        } else if recursive && child.is_dir() {
            collect_drawing_paths(&child, true, out)?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn extraction_drawing_paths(state: &DataExtractionState) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for source in &state.source_files {
        collect_drawing_paths(Path::new(source), state.include_subfolders, &mut paths)?;
    }
    paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    paths.dedup_by(|left, right| left == right);
    Ok(paths)
}

/// Visit the source drawings the extraction pipeline reads beyond the current
/// one. Native loads them from disk one at a time (folders walked
/// recursively), so only one foreign drawing is in memory at once; the web
/// reads the documents parsed at pick time from the in-session registry —
/// every listed source must have been picked, or it asks for the pick again.
fn for_each_extraction_document(
    state: &DataExtractionState,
    mut visit: impl FnMut(&CadDocument),
) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    for source in &state.source_files {
        let document = extraction_document(source).ok_or_else(|| {
            crate::tf!("Drawing not loaded in the browser: {name}.", name = source).into_owned()
        })?;
        visit(&document);
    }
    #[cfg(not(target_arch = "wasm32"))]
    for source in extraction_drawing_paths(state)? {
        let document = crate::io::load_file(&source)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        visit(&document);
    }
    Ok(())
}

fn collect_type_counts(
    doc: &CadDocument,
    allowed_handles: Option<&BTreeSet<Handle>>,
    counts: &mut BTreeMap<String, usize>,
) {
    for entity in doc.entities() {
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            continue;
        }
        if allowed_handles.is_some_and(|handles| !handles.contains(&entity.common().handle)) {
            continue;
        }
        *counts
            .entry(crate::entities::names::dxf_name(entity).to_string())
            .or_default() += 1;
    }
}

fn extraction_preview(
    state: &DataExtractionState,
    current: &CadDocument,
) -> Result<Vec<Vec<String>>, String> {
    let allowed_types = state
        .objects
        .iter()
        .filter(|value| value.checked)
        .map(|value| value.name.clone())
        .collect::<BTreeSet<_>>();
    let selected = state
        .selection_handles
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut records = Vec::new();
    match state.source {
        ExtractionSource::CurrentDrawing => {
            collect_records(current, None, &allowed_types, &mut records)
        }
        ExtractionSource::CurrentSelection => {
            collect_records(current, Some(&selected), &allowed_types, &mut records)
        }
        ExtractionSource::DrawingsAndFolders => {
            if state.include_current {
                collect_records(current, None, &allowed_types, &mut records);
            }
            for_each_extraction_document(state, |document| {
                collect_records(document, None, &allowed_types, &mut records);
            })?;
        }
    }
    let props = state
        .properties
        .iter()
        .filter(|value| value.checked)
        .collect::<Vec<_>>();
    if props.is_empty() {
        return Err(crate::t!("Select at least one property.").into_owned());
    }
    let mut header = Vec::new();
    if state.show_name {
        header.push("Name".to_string());
    }
    header.extend(
        props
            .iter()
            .filter(|value| !(state.show_name && value.key == "type"))
            .map(|value| value.name.clone()),
    );
    if state.show_count {
        header.push("Count".to_string());
    }
    let mut rows = Vec::new();
    if state.combine_identical {
        let mut grouped: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        for record in records {
            let mut values = Vec::new();
            if state.show_name {
                values.push(record.object_type.clone());
            }
            values.extend(
                props
                    .iter()
                    .filter(|value| !(state.show_name && value.key == "type"))
                    .map(|value| record.value(&value.key)),
            );
            *grouped.entry(values).or_default() += 1;
        }
        for (mut values, count) in grouped {
            if state.show_count {
                values.push(count.to_string());
            }
            rows.push(values);
        }
    } else {
        for record in records {
            let mut values = Vec::new();
            if state.show_name {
                values.push(record.object_type.clone());
            }
            values.extend(
                props
                    .iter()
                    .filter(|value| !(state.show_name && value.key == "type"))
                    .map(|value| record.value(&value.key)),
            );
            if state.show_count {
                values.push("1".into());
            }
            rows.push(values);
        }
    }
    let mut output = vec![header];
    output.extend(rows);
    Ok(output)
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn rows_to_csv(rows: &[Vec<String>]) -> String {
    let mut csv = String::new();
    for row in rows {
        csv.push_str(
            &row.iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    csv
}

fn save_extraction_settings(state: &DataExtractionState) -> Result<(), String> {
    if state.settings_path.trim().is_empty() {
        return Ok(());
    }
    let object_types = state
        .objects
        .iter()
        .filter(|value| value.checked)
        .map(|value| value.name.clone())
        .collect::<Vec<_>>();
    let properties = state
        .properties
        .iter()
        .filter(|value| value.checked)
        .map(|value| value.key.clone())
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "version": 1,
        "source": match state.source {
            ExtractionSource::CurrentDrawing => "currentDrawing",
            ExtractionSource::CurrentSelection => "currentSelection",
            ExtractionSource::DrawingsAndFolders => "drawingsAndFolders",
        },
        "includeCurrent": state.include_current,
        "includeSubfolders": state.include_subfolders,
        "sourceFiles": state.source_files,
        "objectTypes": object_types,
        "properties": properties,
        "combineIdentical": state.combine_identical,
        "showCount": state.show_count,
        "showName": state.show_name,
        "outputTable": state.output_table,
        "outputFile": state.output_file,
        "outputPath": state.output_path,
        "tableStyle": state.table_style,
        "tableTitle": state.table_title,
    });
    let text = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    #[cfg(target_arch = "wasm32")]
    {
        // No filesystem to write to: hand the settings to the browser as a
        // download, mirroring the native write of the same JSON.
        let name = Path::new(state.settings_path.trim())
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "extraction.dxex".into());
        crate::sys::download_bytes(&name, text.as_bytes());
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::write(&state.settings_path, text).map_err(|error| error.to_string())
    }
}

fn load_extraction_settings(state: &mut DataExtractionState) -> Result<(), String> {
    let text = read_text_source(Path::new(state.settings_path.trim()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    state.source = match value["source"].as_str() {
        Some("currentSelection") => ExtractionSource::CurrentSelection,
        Some("drawingsAndFolders") => ExtractionSource::DrawingsAndFolders,
        _ => ExtractionSource::CurrentDrawing,
    };
    state.include_current = value["includeCurrent"]
        .as_bool()
        .unwrap_or(state.include_current);
    state.include_subfolders = value["includeSubfolders"]
        .as_bool()
        .unwrap_or(state.include_subfolders);
    if let Some(files) = value["sourceFiles"].as_array() {
        state.source_files = files
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
    }
    let object_types = value["objectTypes"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for item in &mut state.objects {
        item.checked = object_types.is_empty() || object_types.contains(item.name.as_str());
    }
    let properties = value["properties"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for item in &mut state.properties {
        item.checked = properties.is_empty() || properties.contains(item.key.as_str());
    }
    state.combine_identical = value["combineIdentical"]
        .as_bool()
        .unwrap_or(state.combine_identical);
    state.show_count = value["showCount"].as_bool().unwrap_or(state.show_count);
    state.show_name = value["showName"].as_bool().unwrap_or(state.show_name);
    state.output_table = value["outputTable"].as_bool().unwrap_or(state.output_table);
    state.output_file = value["outputFile"].as_bool().unwrap_or(state.output_file);
    state.output_path = value["outputPath"]
        .as_str()
        .unwrap_or(&state.output_path)
        .to_string();
    state.table_style = value["tableStyle"]
        .as_str()
        .unwrap_or(&state.table_style)
        .to_string();
    state.table_title = value["tableTitle"]
        .as_str()
        .unwrap_or(&state.table_title)
        .to_string();
    Ok(())
}

impl OpenCADStudio {
    fn reset_annotation_modal(&mut self, modal: ModalKind) {
        self.active_modal = Some(modal);
        self.reset_modal_geometry();
    }

    pub(super) fn open_table_insert(&mut self) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let styles = table_styles(doc);
        let current = doc.header.current_table_style_name.clone();
        let links = data_link_choices(doc);
        self.table_insert.styles = styles;
        self.table_insert.style = current;
        self.table_insert.links = links;
        self.table_insert.link = self.table_insert.links.first().cloned();
        self.table_insert.error.clear();
        self.reset_annotation_modal(ModalKind::InsertTable);
    }

    pub(super) fn open_data_link_manager(&mut self, from_table: bool) {
        self.data_link_parent_table = from_table;
        self.refresh_data_link_manager(None);
        self.data_link_manager.editing = false;
        self.data_link_manager.status.clear();
        self.reset_annotation_modal(ModalKind::DataLinkManager);
    }

    fn refresh_data_link_manager(&mut self, selected: Option<Handle>) {
        let doc = &self.tabs[self.active_tab].scene.document;
        let links = data_link_choices(doc);
        let selected = selected
            .and_then(|handle| links.iter().find(|value| value.handle == handle).cloned())
            .or_else(|| {
                self.data_link_manager.selected.as_ref().and_then(|old| {
                    links
                        .iter()
                        .find(|value| value.handle == old.handle)
                        .cloned()
                })
            })
            .or_else(|| links.first().cloned());
        let preview = selected
            .as_ref()
            .and_then(|choice| data_link(doc, choice.handle))
            .and_then(|link| read_link_rows(doc, link).ok())
            .unwrap_or_default();
        self.data_link_manager.links = links;
        self.data_link_manager.selected = selected;
        self.data_link_manager.preview = preview;
    }

    pub(super) fn open_data_extraction(&mut self) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let selection_handles = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle)
            .collect::<Vec<_>>();
        let mut counts = BTreeMap::<String, usize>::new();
        for entity in doc.entities() {
            if !matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                *counts
                    .entry(crate::entities::names::dxf_name(entity).to_string())
                    .or_default() += 1;
            }
        }
        let mut state = DataExtractionState::default();
        state.selection_handles = selection_handles;
        state.objects = counts
            .into_iter()
            .map(
                |(name, count)| crate::ui::window::annotation_data::ExtractionObject {
                    name,
                    checked: true,
                    count,
                },
            )
            .collect();
        state.table_styles = table_styles(doc);
        state.table_style = doc.header.current_table_style_name.clone();
        self.data_extraction = state;
        self.reset_annotation_modal(ModalKind::DataExtraction);
    }

    pub(super) fn on_table_insert_style(&mut self, style: String) -> Task<Message> {
        self.table_insert.style = style;
        Task::none()
    }

    pub(super) fn on_table_insert_field(&mut self, field: TableInsertField) -> Task<Message> {
        match field {
            TableInsertField::Style(value) => self.table_insert.style = value,
            TableInsertField::Source(value) => self.table_insert.source = value,
            TableInsertField::Link(value) => self.table_insert.link = Some(value),
            TableInsertField::Insertion(value) => self.table_insert.insertion = value,
            TableInsertField::Columns(value) => self.table_insert.columns = value,
            TableInsertField::DataRows(value) => self.table_insert.data_rows = value,
            TableInsertField::ColumnWidth(value) => self.table_insert.column_width = value,
            TableInsertField::RowHeight(value) => self.table_insert.row_height = value,
            TableInsertField::Preview(value) => self.table_insert.preview = value,
            TableInsertField::FirstRowStyle(value) => self.table_insert.first_row_style = value,
            TableInsertField::SecondRowStyle(value) => self.table_insert.second_row_style = value,
            TableInsertField::OtherRowStyle(value) => self.table_insert.other_row_style = value,
        }
        self.table_insert.error.clear();
        Task::none()
    }

    pub(super) fn on_table_insert_apply(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if self.table_insert.source == TableSource::DataExtraction {
            self.open_data_extraction();
            return Task::none();
        }
        if self.table_insert.source == TableSource::DataLink {
            let Some(choice) = self.table_insert.link.clone() else {
                self.table_insert.error = crate::t!("Select a data link.").into_owned();
                return Task::none();
            };
            let doc = &self.tabs[i].scene.document;
            let Some(link) = data_link(doc, choice.handle) else {
                self.table_insert.error =
                    crate::t!("The selected data link no longer exists.").into_owned();
                return Task::none();
            };
            let rows = match read_link_rows(doc, link) {
                Ok(rows) => rows,
                Err(error) => {
                    self.table_insert.error = error;
                    return Task::none();
                }
            };
            let table = build_table(
                &rows,
                table_style_handle(doc, &self.table_insert.style),
                None,
            );
            let command = crate::modules::annotate::data_link::DataLinkPlaceCommand::existing(
                table,
                choice.handle,
            );
            self.active_modal = None;
            self.reset_modal_geometry();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Task::none();
        }
        let parse_usize = |value: &str| {
            value
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
        };
        let parse_number = |value: &str| {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)
        };
        let (Some(columns), Some(data_rows), Some(column_width), Some(row_height)) = (
            parse_usize(&self.table_insert.columns),
            parse_usize(&self.table_insert.data_rows),
            parse_number(&self.table_insert.column_width),
            parse_number(&self.table_insert.row_height),
        ) else {
            self.table_insert.error =
                crate::t!("Columns, rows, width, and height must be positive values.").into_owned();
            return Task::none();
        };
        let doc = &self.tabs[i].scene.document;
        let style = doc
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::TableStyle(style)
                    if style.name.eq_ignore_ascii_case(&self.table_insert.style) =>
                {
                    Some((*handle, style))
                }
                _ => None,
            });
        let multiplier = self.tabs[i].scene.creation_annotation_multiplier();
        let command = crate::modules::annotate::table_cmd::TableCommand::configured(
            style,
            multiplier,
            columns,
            data_rows,
            column_width,
            row_height,
            self.table_insert.insertion == TableInsertion::Window,
            self.table_insert.preview,
            [
                &self.table_insert.first_row_style,
                &self.table_insert.second_row_style,
                &self.table_insert.other_row_style,
            ],
        );
        self.active_modal = None;
        self.reset_modal_geometry();
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    pub(super) fn on_data_link_new(&mut self) -> Task<Message> {
        let mut n = 1usize;
        loop {
            let name = format!("Data Link {n}");
            if !self
                .data_link_manager
                .links
                .iter()
                .any(|link| link.name.eq_ignore_ascii_case(&name))
            {
                self.data_link_manager.name = name;
                break;
            }
            n += 1;
        }
        self.data_link_manager.path.clear();
        self.data_link_manager.path_type = LinkPathType::Full;
        self.data_link_manager.editing_handle = None;
        self.data_link_manager.editing = true;
        self.data_link_manager.range_kind = LinkRange::EntireSheet;
        self.data_link_manager.range.clear();
        self.data_link_manager.sheets.clear();
        self.data_link_manager.sheet.clear();
        self.data_link_manager.named_ranges.clear();
        self.data_link_manager.allow_write = false;
        self.data_link_manager.use_source_formatting = true;
        self.data_link_manager.update_source_formatting = true;
        self.data_link_manager.insert_table = false;
        self.data_link_manager.more_options = false;
        self.data_link_manager.status.clear();
        Task::none()
    }

    pub(super) fn on_data_link_select(&mut self, handle: Handle) -> Task<Message> {
        self.refresh_data_link_manager(Some(handle));
        Task::none()
    }

    pub(super) fn on_data_link_edit(&mut self) -> Task<Message> {
        let Some(choice) = self.data_link_manager.selected.clone() else {
            return Task::none();
        };
        let Some(link) = data_link(&self.tabs[self.active_tab].scene.document, choice.handle)
        else {
            return Task::none();
        };
        self.data_link_manager.editing = true;
        self.data_link_manager.editing_handle = Some(choice.handle);
        self.data_link_manager.name = link_display_name(link);
        let path = resolve_link_path(&self.tabs[self.active_tab].scene.document, link);
        self.data_link_manager.path = path.to_string_lossy().into_owned();
        self.data_link_manager.path_type = match link.path_option {
            2 => LinkPathType::Relative,
            3 => LinkPathType::FileName,
            _ => LinkPathType::Full,
        };
        let (sheet, range_kind, range) = link_selection(link);
        let (sheets, named_ranges) = workbook_metadata(&path).unwrap_or_default();
        self.data_link_manager.sheets = sheets;
        self.data_link_manager.named_ranges = named_ranges;
        self.data_link_manager.sheet = sheet
            .filter(|value| {
                self.data_link_manager
                    .sheets
                    .iter()
                    .any(|candidate| candidate == value)
            })
            .or_else(|| self.data_link_manager.sheets.first().cloned())
            .unwrap_or_default();
        self.data_link_manager.range = range.unwrap_or_default();
        self.data_link_manager.range_kind = range_kind;
        self.data_link_manager.allow_write = link.option & 1 != 0;
        self.data_link_manager.use_source_formatting = link.flags & 1 != 0;
        self.data_link_manager.update_source_formatting = link.flags & 2 != 0;
        self.data_link_manager.status.clear();
        Task::none()
    }

    pub(super) fn on_data_link_field(&mut self, field: DataLinkField) -> Task<Message> {
        match field {
            DataLinkField::Name(value) => self.data_link_manager.name = value,
            DataLinkField::Path(value) => self.data_link_manager.path = value,
            DataLinkField::PathType(value) => self.data_link_manager.path_type = value,
            DataLinkField::RangeKind(value) => self.data_link_manager.range_kind = value,
            DataLinkField::Range(value) => self.data_link_manager.range = value,
            DataLinkField::Sheet(value) => self.data_link_manager.sheet = value,
            DataLinkField::AllowWrite(value) => self.data_link_manager.allow_write = value,
            DataLinkField::UseSourceFormatting(value) => {
                self.data_link_manager.use_source_formatting = value
            }
            DataLinkField::UpdateSourceFormatting(value) => {
                self.data_link_manager.update_source_formatting = value
            }
            DataLinkField::InsertTable(value) => self.data_link_manager.insert_table = value,
            DataLinkField::MoreOptions(value) => self.data_link_manager.more_options = value,
        }
        self.data_link_manager.status.clear();
        Task::none()
    }

    /// Web DATALINK: the picked .csv's bytes arrived. They are registered in
    /// the picked-source registry — so `DATALINKUPDATE` re-reads them
    /// in-session instead of failing on the absent filesystem — and fed to
    /// the same table-insert path the native typed-path command uses.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_datalink_csv_picked(
        &mut self,
        picked: Option<(String, Vec<u8>)>,
    ) -> Task<Message> {
        let Some((name, bytes)) = picked else {
            return Task::none();
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if !register_picked_source(&name, Arc::new(bytes)) {
            if let Some(name) = take_last_collision().take() {
                self.command_line.push_warning(
                    crate::tf!(
                        "\"{name}\" is already loaded with different contents; the existing file was kept."
                    )
                    .as_ref(),
                );
            }
            return Task::none();
        }
        self.place_data_link_table(&text, &name);
        Task::none()
    }

    pub(super) fn on_data_link_browse(&mut self) -> Task<Message> {
        Task::perform(
            async {
                let Some(handle) = crate::sys::file_dialog()
                    .set_title(crate::t!("Choose a Spreadsheet File").as_ref())
                    .add_filter(
                        crate::t!("Spreadsheet Files").as_ref(),
                        &["csv", "txt", "xls", "xlsx", "xlsb", "ods"],
                    )
                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                    .pick_file()
                    .await
                else {
                    return None;
                };
                let path = crate::sys::handle_path(&handle);
                // The browser forgets the file the moment the pick resolves:
                // keep its bytes so preview, save, and later re-opens read
                // them instead of asking for the picker again.
                #[cfg(target_arch = "wasm32")]
                {
                    let fresh = register_picked_source(
                        &path.to_string_lossy(),
                        Arc::new(handle.read().await),
                    );
                    if !fresh {
                        return None;
                    }
                }
                Some(path)
            },
            Message::DataLinkBrowseResult,
        )
    }

    pub(super) fn on_data_link_browse_result(&mut self, path: Option<PathBuf>) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        if let Some(name) = take_last_collision().take() {
            self.command_line.push_warning(
                crate::tf!(
                    "\"{name}\" is already loaded with different contents; the existing file was kept."
                )
                .as_ref(),
            );
        }
        if let Some(path) = path {
            self.data_link_manager.path = path.to_string_lossy().into_owned();
            let (sheets, named_ranges) = workbook_metadata(&path).unwrap_or_default();
            self.data_link_manager.sheets = sheets;
            self.data_link_manager.named_ranges = named_ranges;
            self.data_link_manager.sheet = self
                .data_link_manager
                .sheets
                .first()
                .cloned()
                .unwrap_or_default();
            if self.data_link_manager.range_kind == LinkRange::NamedRange {
                self.data_link_manager.range = self
                    .data_link_manager
                    .named_ranges
                    .first()
                    .cloned()
                    .unwrap_or_default();
            }
            self.data_link_manager.preview = read_tabular_file(
                &path,
                (!self.data_link_manager.sheet.is_empty())
                    .then_some(self.data_link_manager.sheet.as_str()),
                self.data_link_manager.range_kind,
                (!self.data_link_manager.range.is_empty())
                    .then_some(self.data_link_manager.range.as_str()),
            )
            .unwrap_or_default();
        }
        Task::none()
    }

    pub(super) fn on_data_link_save(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let name = self.data_link_manager.name.trim().to_string();
        let input_path = PathBuf::from(self.data_link_manager.path.trim());
        if name.is_empty() {
            self.data_link_manager.status = crate::t!("Enter a data link name.").into_owned();
            return Task::none();
        }
        if self.data_link_manager.links.iter().any(|link| {
            link.name.eq_ignore_ascii_case(&name)
                && Some(link.handle) != self.data_link_manager.editing_handle
        }) {
            self.data_link_manager.status =
                crate::t!("A data link with that name already exists.").into_owned();
            return Task::none();
        }
        let range = (self.data_link_manager.range_kind != LinkRange::EntireSheet)
            .then(|| self.data_link_manager.range.trim().to_string())
            .filter(|value| !value.is_empty());
        let preview = match read_tabular_file(
            &input_path,
            (!self.data_link_manager.sheet.is_empty())
                .then_some(self.data_link_manager.sheet.as_str()),
            self.data_link_manager.range_kind,
            range.as_deref(),
        ) {
            Ok(rows) => rows,
            Err(error) => {
                self.data_link_manager.status = error;
                return Task::none();
            }
        };
        let stored_path = match self.data_link_manager.path_type {
            LinkPathType::Full => std::fs::canonicalize(&input_path).unwrap_or(input_path.clone()),
            LinkPathType::Relative => self.tabs[i]
                .scene
                .document
                .source_path
                .as_deref()
                .map(Path::new)
                .and_then(Path::parent)
                .and_then(|parent| input_path.strip_prefix(parent).ok())
                .map(Path::to_path_buf)
                .unwrap_or(input_path.clone()),
            LinkPathType::FileName => input_path
                .file_name()
                .map(PathBuf::from)
                .unwrap_or(input_path.clone()),
        };
        self.push_undo_snapshot(i, "DATALINK");
        let handle = if let Some(handle) = self.data_link_manager.editing_handle {
            if let Some(link) = data_link_mut(&mut self.tabs[i].scene.document, handle) {
                link.description = name;
                link.tooltip = input_path.to_string_lossy().into_owned();
                link.connection_string = stored_path.to_string_lossy().into_owned();
                link.path_option = match self.data_link_manager.path_type {
                    LinkPathType::Full => 1,
                    LinkPathType::Relative => 2,
                    LinkPathType::FileName => 3,
                };
                link.option = i32::from(self.data_link_manager.allow_write);
                link.flags = i32::from(self.data_link_manager.use_source_formatting)
                    | (i32::from(self.data_link_manager.update_source_formatting) << 1);
                link.status_flags = 1;
                link.update_status = "Linked".into();
                link.custom_data = link_custom_data(
                    &self.data_link_manager.sheet,
                    self.data_link_manager.range_kind,
                    range.as_deref(),
                );
            }
            handle
        } else {
            let handle = self.tabs[i].scene.document.allocate_handle();
            let mut object = ClassObject::new(ClassObjectData::DataLink(DataLink {
                data_adapter: if input_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| {
                        value.eq_ignore_ascii_case("csv") || value.eq_ignore_ascii_case("txt")
                    }) {
                    "CSV".into()
                } else {
                    "Spreadsheet".into()
                },
                description: name,
                tooltip: input_path.to_string_lossy().into_owned(),
                connection_string: stored_path.to_string_lossy().into_owned(),
                option: i32::from(self.data_link_manager.allow_write),
                flags: i32::from(self.data_link_manager.use_source_formatting)
                    | (i32::from(self.data_link_manager.update_source_formatting) << 1),
                path_option: match self.data_link_manager.path_type {
                    LinkPathType::Full => 1,
                    LinkPathType::Relative => 2,
                    LinkPathType::FileName => 3,
                },
                status_flags: 1,
                update_status: "Linked".into(),
                custom_data: link_custom_data(
                    &self.data_link_manager.sheet,
                    self.data_link_manager.range_kind,
                    range.as_deref(),
                ),
                ..DataLink::default()
            }));
            object.handle = handle;
            self.tabs[i]
                .scene
                .document
                .objects
                .insert(handle, ObjectType::ClassObject(object));
            handle
        };
        self.tabs[i].dirty = true;
        self.data_link_manager.editing = false;
        self.data_link_manager.editing_handle = None;
        self.data_link_manager.preview = preview.clone();
        self.refresh_data_link_manager(Some(handle));
        if self.data_link_manager.insert_table {
            let style = table_style_handle(
                &self.tabs[i].scene.document,
                &self.tabs[i].scene.document.header.current_table_style_name,
            );
            let table = build_table(&preview, style, None);
            let command =
                crate::modules::annotate::data_link::DataLinkPlaceCommand::existing(table, handle);
            self.active_modal = None;
            self.reset_modal_geometry();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        }
        Task::none()
    }

    pub(super) fn on_data_link_delete(&mut self) -> Task<Message> {
        let Some(handle) = self
            .data_link_manager
            .selected
            .as_ref()
            .map(|value| value.handle)
        else {
            return Task::none();
        };
        let i = self.active_tab;
        self.push_undo_snapshot(i, "DATALINK");
        for entity in self.tabs[i].scene.document.entities_mut() {
            if let EntityType::Table(table) = entity {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        if cell.data_link_handle == Some(handle) {
                            cell.data_link_handle = None;
                            cell.has_linked_data = false;
                            cell.data_link_rows = 0;
                            cell.data_link_columns = 0;
                            cell.state.remove(
                                codec::entities::table::CellStateFlags::LINKED
                                    | codec::entities::table::CellStateFlags::CONTENT_LOCKED
                                    | codec::entities::table::CellStateFlags::FORMAT_LOCKED,
                            );
                        }
                    }
                }
            }
        }
        self.tabs[i].scene.document.objects.remove(&handle);
        self.tabs[i].dirty = true;
        self.data_link_manager.selected = None;
        self.refresh_data_link_manager(None);
        self.data_link_manager.status =
            crate::t!("Data link deleted; existing table values were retained.").into_owned();
        Task::none()
    }

    pub(super) fn on_data_link_insert(&mut self) -> Task<Message> {
        let Some(choice) = self.data_link_manager.selected.clone() else {
            return Task::none();
        };
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let Some(link) = data_link(doc, choice.handle) else {
            return Task::none();
        };
        let rows = match read_link_rows(doc, link) {
            Ok(rows) => rows,
            Err(error) => {
                self.data_link_manager.status = error;
                return Task::none();
            }
        };
        let style = table_style_handle(doc, &doc.header.current_table_style_name);
        let command = crate::modules::annotate::data_link::DataLinkPlaceCommand::existing(
            build_table(&rows, style, None),
            choice.handle,
        );
        self.active_modal = None;
        self.reset_modal_geometry();
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    pub(super) fn on_data_link_close(&mut self) -> Task<Message> {
        if self.data_link_parent_table {
            self.data_link_parent_table = false;
            let links = data_link_choices(&self.tabs[self.active_tab].scene.document);
            self.table_insert.links = links;
            self.table_insert.link = self.table_insert.links.first().cloned();
            self.reset_annotation_modal(ModalKind::InsertTable);
        } else {
            self.active_modal = None;
            self.reset_modal_geometry();
            self.ribbon.deactivate_tool();
        }
        Task::none()
    }

    pub(super) fn on_data_extraction_field(&mut self, field: DataExtractionField) -> Task<Message> {
        match field {
            DataExtractionField::Begin(value) => self.data_extraction.begin = value,
            DataExtractionField::SettingsPath(value) => self.data_extraction.settings_path = value,
            DataExtractionField::Source(value) => self.data_extraction.source = value,
            DataExtractionField::IncludeCurrent(value) => {
                self.data_extraction.include_current = value
            }
            DataExtractionField::IncludeSubfolders(value) => {
                self.data_extraction.include_subfolders = value
            }
            DataExtractionField::Object(index, value) => {
                if let Some(item) = self.data_extraction.objects.get_mut(index) {
                    item.checked = value;
                }
            }
            DataExtractionField::Property(index, value) => {
                if let Some(item) = self.data_extraction.properties.get_mut(index) {
                    item.checked = value;
                }
            }
            DataExtractionField::Category(category, value) => {
                if value {
                    self.data_extraction.categories.insert(category);
                } else {
                    self.data_extraction.categories.remove(&category);
                }
            }
            DataExtractionField::CombineIdentical(value) => {
                self.data_extraction.combine_identical = value
            }
            DataExtractionField::ShowCount(value) => self.data_extraction.show_count = value,
            DataExtractionField::ShowName(value) => self.data_extraction.show_name = value,
            DataExtractionField::OutputTable(value) => self.data_extraction.output_table = value,
            DataExtractionField::OutputFile(value) => self.data_extraction.output_file = value,
            DataExtractionField::OutputPath(value) => self.data_extraction.output_path = value,
            DataExtractionField::TableStyle(value) => self.data_extraction.table_style = value,
            DataExtractionField::TableTitle(value) => self.data_extraction.table_title = value,
        }
        self.data_extraction.error.clear();
        if self.data_extraction.page == ExtractionPage::Refine {
            self.update_extraction_preview();
        }
        Task::none()
    }

    fn refresh_extraction_objects(&mut self) -> Result<(), String> {
        let state = self.data_extraction.clone();
        let selected = state
            .selection_handles
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut counts = BTreeMap::new();
        let current = &self.tabs[self.active_tab].scene.document;
        match state.source {
            ExtractionSource::CurrentDrawing => collect_type_counts(current, None, &mut counts),
            ExtractionSource::CurrentSelection => {
                collect_type_counts(current, Some(&selected), &mut counts)
            }
            ExtractionSource::DrawingsAndFolders => {
                if state.include_current {
                    collect_type_counts(current, None, &mut counts);
                }
                for_each_extraction_document(&state, |document| {
                    collect_type_counts(document, None, &mut counts);
                })?;
            }
        }
        let previous = self
            .data_extraction
            .objects
            .iter()
            .map(|item| (item.name.to_ascii_lowercase(), item.checked))
            .collect::<BTreeMap<_, _>>();
        self.data_extraction.objects = counts
            .into_iter()
            .map(
                |(name, count)| crate::ui::window::annotation_data::ExtractionObject {
                    checked: previous
                        .get(&name.to_ascii_lowercase())
                        .copied()
                        .unwrap_or(true),
                    name,
                    count,
                },
            )
            .collect();
        Ok(())
    }

    fn update_extraction_preview(&mut self) {
        match extraction_preview(
            &self.data_extraction,
            &self.tabs[self.active_tab].scene.document,
        ) {
            Ok(rows) => {
                self.data_extraction.preview = rows;
                self.data_extraction.error.clear();
            }
            Err(error) => self.data_extraction.error = error,
        }
    }

    pub(super) fn on_data_extraction_back(&mut self) -> Task<Message> {
        self.data_extraction.page = match self.data_extraction.page {
            ExtractionPage::Begin => ExtractionPage::Begin,
            ExtractionPage::Source => ExtractionPage::Begin,
            ExtractionPage::Objects => ExtractionPage::Source,
            ExtractionPage::Properties => ExtractionPage::Objects,
            ExtractionPage::Refine => ExtractionPage::Properties,
            ExtractionPage::Output => ExtractionPage::Refine,
            ExtractionPage::TableStyle => ExtractionPage::Output,
            ExtractionPage::Finish if self.data_extraction.output_table => {
                ExtractionPage::TableStyle
            }
            ExtractionPage::Finish => ExtractionPage::Output,
        };
        self.data_extraction.error.clear();
        Task::none()
    }

    pub(super) fn on_data_extraction_next(&mut self) -> Task<Message> {
        self.data_extraction.error.clear();
        match self.data_extraction.page {
            ExtractionPage::Begin => {
                if self.data_extraction.settings_path.trim().is_empty() {
                    return self.on_data_extraction_browse_settings();
                }
                if self.data_extraction.begin != ExtractionBegin::New {
                    if let Err(error) = load_extraction_settings(&mut self.data_extraction) {
                        self.data_extraction.error = error;
                        return Task::none();
                    }
                }
                self.data_extraction.page = ExtractionPage::Source;
            }
            ExtractionPage::Source => {
                if self.data_extraction.source == ExtractionSource::CurrentSelection
                    && self.data_extraction.selection_handles.is_empty()
                {
                    self.data_extraction.error =
                        crate::t!("No objects were selected when the wizard opened.").into_owned();
                    return Task::none();
                }
                if self.data_extraction.source == ExtractionSource::DrawingsAndFolders
                    && !self.data_extraction.include_current
                    && self.data_extraction.source_files.is_empty()
                {
                    self.data_extraction.error =
                        crate::t!("Add a drawing or include the current drawing.").into_owned();
                    return Task::none();
                }
                if let Err(error) = self.refresh_extraction_objects() {
                    self.data_extraction.error = error;
                    return Task::none();
                }
                if self.data_extraction.objects.is_empty() {
                    self.data_extraction.error =
                        crate::t!("No extractable objects were found in the selected source.")
                            .into_owned();
                    return Task::none();
                }
                self.data_extraction.page = ExtractionPage::Objects;
            }
            ExtractionPage::Objects => {
                if !self
                    .data_extraction
                    .objects
                    .iter()
                    .any(|value| value.checked)
                {
                    self.data_extraction.error =
                        crate::t!("Select at least one object type.").into_owned();
                    return Task::none();
                }
                self.data_extraction.page = ExtractionPage::Properties;
            }
            ExtractionPage::Properties => {
                if !self
                    .data_extraction
                    .properties
                    .iter()
                    .any(|value| value.checked)
                {
                    self.data_extraction.error =
                        crate::t!("Select at least one property.").into_owned();
                    return Task::none();
                }
                self.data_extraction.page = ExtractionPage::Refine;
                self.update_extraction_preview();
            }
            ExtractionPage::Refine => {
                self.update_extraction_preview();
                if self.data_extraction.error.is_empty() {
                    self.data_extraction.page = ExtractionPage::Output;
                }
            }
            ExtractionPage::Output => {
                if !self.data_extraction.output_table && !self.data_extraction.output_file {
                    self.data_extraction.error =
                        crate::t!("Choose at least one output.").into_owned();
                    return Task::none();
                }
                if self.data_extraction.output_file
                    && self.data_extraction.output_path.trim().is_empty()
                {
                    return self.on_data_extraction_browse_output();
                }
                self.data_extraction.page = if self.data_extraction.output_table {
                    ExtractionPage::TableStyle
                } else {
                    ExtractionPage::Finish
                };
            }
            ExtractionPage::TableStyle => self.data_extraction.page = ExtractionPage::Finish,
            ExtractionPage::Finish => return self.on_data_extraction_finish(),
        }
        Task::none()
    }

    pub(super) fn on_data_extraction_browse_settings(&mut self) -> Task<Message> {
        let save = self.data_extraction.begin == ExtractionBegin::New;
        Task::perform(
            async move {
                let dialog = crate::sys::file_dialog()
                    .set_title(crate::t!("Data Extraction Settings").as_ref())
                    .add_filter(crate::t!("Data Extraction Settings").as_ref(), &["dxex"])
                    .add_filter(crate::t!("All Files").as_ref(), &["*"]);
                let handle = if save {
                    dialog.set_file_name("extraction.dxex").save_file().await
                } else {
                    dialog.pick_file().await
                };
                let handle = handle?;
                let path = crate::sys::handle_path(&handle);
                // Loading settings in the browser: the picked .dxex has no
                // path to read back from, so its bytes are kept for the load
                // that follows this message.
                #[cfg(target_arch = "wasm32")]
                if !save {
                    let fresh = register_picked_source(
                        &path.to_string_lossy(),
                        Arc::new(handle.read().await),
                    );
                    if !fresh {
                        return None;
                    }
                }
                Some(path)
            },
            Message::DataExtractionBrowseSettingsResult,
        )
    }

    pub(super) fn on_data_extraction_browse_settings_result(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        if let Some(name) = take_last_collision().take() {
            self.data_extraction.error = crate::tf!(
                "\"{name}\" is already loaded with different contents; the existing file was kept."
            )
            .into_owned();
        }
        if let Some(path) = path {
            self.data_extraction.settings_path = path.to_string_lossy().into_owned();
            if self.data_extraction.begin != ExtractionBegin::New {
                if let Err(error) = load_extraction_settings(&mut self.data_extraction) {
                    self.data_extraction.error = error;
                    return Task::none();
                }
            }
            self.data_extraction.page = ExtractionPage::Source;
        }
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_data_extraction_add_drawings(&mut self) -> Task<Message> {
        Task::perform(
            async {
                Ok(crate::sys::file_dialog()
                    .set_title(crate::t!("Add Drawings").as_ref())
                    .add_filter(
                        crate::t!("CAD Files").as_ref(),
                        &["dwg", "dxf", "DWG", "DXF"],
                    )
                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                    .pick_files()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|handle| crate::sys::handle_path(&handle))
                    .collect())
            },
            Message::DataExtractionAddDrawingsResult,
        )
    }

    /// Web: the picked drawings have no paths to re-open later, so each one is
    /// parsed on the shared worker right here and the document registered
    /// under its name — the pipeline then reads the registry instead of the
    /// filesystem. The first file that fails to parse aborts the add with the
    /// same `name: error` shape the native refresh reports.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_data_extraction_add_drawings(&mut self) -> Task<Message> {
        Task::perform(
            async {
                let handles = crate::sys::file_dialog()
                    .set_title(crate::t!("Add Drawings").as_ref())
                    .add_filter(
                        crate::t!("CAD Files").as_ref(),
                        &["dwg", "dxf", "DWG", "DXF"],
                    )
                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                    .pick_files()
                    .await
                    .unwrap_or_default();
                // Each picked file becomes a full parsed document held in
                // the registry for the session; a bound keeps a giant
                // multi-pick from exhausting the wasm heap (tab death).
                const MAX_EXTRACTION_SOURCES: usize = 16;
                if handles.len() > MAX_EXTRACTION_SOURCES {
                    return Err(crate::tf!(
                        "At most {MAX_EXTRACTION_SOURCES} drawings can be added in the browser."
                    )
                    .into_owned());
                }
                let mut paths = Vec::new();
                for handle in handles {
                    let name = handle.file_name();
                    let bytes = handle.read().await;
                    let document = crate::io::parse_drawing_bytes_web(&name, &bytes)
                        .await
                        .map_err(|error| format!("{name}: {error}"))?;
                    register_extraction_document(&name, document);
                    paths.push(PathBuf::from(name));
                }
                Ok(paths)
            },
            Message::DataExtractionAddDrawingsResult,
        )
    }

    pub(super) fn on_data_extraction_add_drawings_result(
        &mut self,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        for path in paths {
            let text = path.to_string_lossy().into_owned();
            if !self
                .data_extraction
                .source_files
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&text))
            {
                self.data_extraction.source_files.push(text);
            }
        }
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_data_extraction_add_folder(&mut self) -> Task<Message> {
        Task::perform(
            async {
                crate::sys::file_dialog()
                    .set_title(crate::t!("Add Drawing Folder").as_ref())
                    .pick_folder()
                    .await
                    .map(|handle| crate::sys::handle_path(&handle))
            },
            Message::DataExtractionAddFolderResult,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_data_extraction_add_folder(&mut self) -> Task<Message> {
        Task::none()
    }

    pub(super) fn on_data_extraction_add_folder_result(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            let text = path.to_string_lossy().into_owned();
            if !self
                .data_extraction
                .source_files
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&text))
            {
                self.data_extraction.source_files.push(text);
            }
        }
        Task::none()
    }

    pub(super) fn on_data_extraction_browse_output(&mut self) -> Task<Message> {
        Task::perform(
            async {
                crate::sys::file_dialog()
                    .set_title(crate::t!("Save Data Extraction").as_ref())
                    .set_file_name("extraction.csv")
                    .add_filter(crate::t!("CSV").as_ref(), &["csv"])
                    .add_filter(crate::t!("Tab-separated text").as_ref(), &["txt"])
                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                    .save_file()
                    .await
                    .map(|handle| crate::sys::handle_path(&handle))
            },
            Message::DataExtractionBrowseOutputResult,
        )
    }

    pub(super) fn on_data_extraction_browse_output_result(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            self.data_extraction.output_path = path.to_string_lossy().into_owned();
            self.data_extraction.output_file = true;
            if self.data_extraction.page == ExtractionPage::Output {
                self.data_extraction.page = if self.data_extraction.output_table {
                    ExtractionPage::TableStyle
                } else {
                    ExtractionPage::Finish
                };
            }
        }
        Task::none()
    }

    pub(super) fn on_data_extraction_finish(&mut self) -> Task<Message> {
        self.update_extraction_preview();
        if !self.data_extraction.error.is_empty() {
            return Task::none();
        }
        if let Err(error) = save_extraction_settings(&self.data_extraction) {
            self.data_extraction.error = error;
            return Task::none();
        }
        let rows = self.data_extraction.preview.clone();
        if self.data_extraction.output_file {
            let path = PathBuf::from(self.data_extraction.output_path.trim());
            let text = if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
            {
                rows.iter()
                    .map(|row| row.join("\t"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            } else {
                rows_to_csv(&rows)
            };
            // Web output mirrors the native CSV/TXT write (there is no xlsx
            // writer on either target): the browser gets the same bytes as a
            // download under the picked name.
            #[cfg(target_arch = "wasm32")]
            {
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "extraction.csv".into());
                crate::sys::download_bytes(&name, text.as_bytes());
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(error) = std::fs::write(&path, text) {
                self.data_extraction.error = error.to_string();
                return Task::none();
            }
        }
        let i = self.active_tab;
        if self.data_extraction.output_table {
            let style = table_style_handle(
                &self.tabs[i].scene.document,
                &self.data_extraction.table_style,
            );
            let table = build_table(&rows, style, Some(&self.data_extraction.table_title));
            let command =
                crate::modules::annotate::data_link::DataLinkPlaceCommand::unlinked(table);
            self.active_modal = None;
            self.reset_modal_geometry();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        } else {
            self.active_modal = None;
            self.reset_modal_geometry();
            self.ribbon.deactivate_tool();
            self.command_line
                .push_output(crate::t!("DATAEXTRACTION: external file created.").as_ref());
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    /// A refused same-name re-pick must KEEP the old bytes: overwriting
    /// would silently retarget every reference reading that name.
    #[test]
    fn collision_refusal_keeps_the_registered_bytes() {
        register_picked_source("a.csv", Arc::new(b"old".to_vec()));
        let fresh = register_picked_source("a.csv", Arc::new(b"new".to_vec()));
        assert!(!fresh, "differing bytes under a live name are refused");
        assert_eq!(
            picked_source_bytes("a.csv").as_ref().map(|b| b.as_slice()),
            Some(&b"old"[..]),
            "the old registration survives the refusal"
        );
        // Identical bytes are a no-op accept.
        assert!(register_picked_source("a.csv", Arc::new(b"old".to_vec())));
    }

    use super::*;

    #[test]
    fn cell_ranges_are_normalized_and_missing_cells_are_blank() {
        let rows = vec![vec!["a".into(), "b".into(), "c".into()], vec!["d".into()]];
        assert_eq!(
            crop_range(rows, "C2:A1").unwrap(),
            vec![
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
                vec!["d".to_string(), String::new(), String::new()],
            ]
        );
    }

    #[test]
    fn named_range_targets_accept_quoted_sheet_names() {
        assert_eq!(
            named_range_target("='Sheet One'!$A$1:$D$10"),
            Some(("Sheet One".into(), "A1:D10".into()))
        );
    }

    #[test]
    fn csv_links_apply_the_selected_cell_range() {
        let path = std::env::temp_dir().join(format!(
            "ocs-data-link-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "a,b,c\nd,e,f\n").unwrap();
        let rows = read_tabular_file(&path, None, LinkRange::CellRange, Some("B1:C2"));
        let _ = std::fs::remove_file(path);
        assert_eq!(
            rows.unwrap(),
            vec![
                vec!["b".to_string(), "c".to_string()],
                vec!["e".to_string(), "f".to_string()],
            ]
        );
    }

    /// `column_index` only checks `checked_mul(26)`, so a range like
    /// `A1:ZZZZZZ1` parses fine — and `(c0..=c1)` then collects one `String`
    /// per column: ~308 M slots (~7 GB) for 6 Z's, and a `capacity overflow`
    /// panic at 13 Z's. Columns past the widest row are empty in *every*
    /// row, so the window must be clamped to the data width.
    #[test]
    fn crop_range_bounds_oversized_column_spans() {
        let rows = vec![vec!["x".to_string()]];
        // 13 Z's: pre-fix, size_hint × size_of::<String>() exceeds isize::MAX
        // and Vec's capacity check panics before a single cell is read.
        assert_eq!(
            crop_range(rows.clone(), "A1:ZZZZZZZZZZZZZ1").unwrap(),
            vec![vec!["x".to_string()]]
        );
        // 6 Z's — the bug-hunt report's repro — stays bounded too.
        assert_eq!(
            crop_range(rows, "A1:ZZZZZZ1").unwrap(),
            vec![vec!["x".to_string()]]
        );
    }

    #[test]
    fn csv_links_read_from_registered_bytes() {
        let name = format!("ocs-registry-{}-{}.csv", std::process::id(), line!());
        register_picked_source(&name, Arc::new(b"a,b,c\nd,e,f\n".to_vec()));
        let rows = read_tabular_file(Path::new(&name), None, LinkRange::CellRange, Some("B1:C2"));
        assert_eq!(
            rows.unwrap(),
            vec![
                vec!["b".to_string(), "c".to_string()],
                vec!["e".to_string(), "f".to_string()],
            ]
        );
    }

    #[test]
    fn registered_bytes_match_by_file_name() {
        let key = format!("ocs-match-{}-{}.xlsx", std::process::id(), line!());
        register_picked_source(&format!(r"C:\drawings\{key}"), Arc::new(Vec::new()));
        // Exact key…
        assert!(picked_source_bytes(&format!(r"C:\drawings\{key}")).is_some());
        // …same file under a differently-cased bare name (the web stores only
        // the name, links may store the author's path)…
        assert!(picked_source_bytes(&key.to_ascii_uppercase()).is_some());
        // …and not another file.
        assert!(picked_source_bytes("ocs-match-other.xlsx").is_none());
    }

    #[test]
    fn registered_extraction_documents_match_by_file_name() {
        let key = format!("ocs-doc-{}-{}.dwg", std::process::id(), line!());
        register_extraction_document(&key, CadDocument::new());
        assert!(extraction_document(&key.to_ascii_uppercase()).is_some());
        assert!(extraction_document("ocs-doc-other.dwg").is_none());
    }

    #[test]
    fn workbook_metadata_bytes_rejects_non_workbooks() {
        assert!(workbook_metadata_bytes(b"not a workbook").is_err());
    }
}
