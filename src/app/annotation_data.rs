use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use codec::entities::{table::CellRange, TableBuilder};
use codec::objects::{ClassObject, ClassObjectData, DataLink, DataLinkCustomData, ObjectType};
use codec::types::{Handle, Vector3};
use codec::{Document, EntityType};
use iced::Task;

use super::{Message, ModalKind, OpenCADStudio};
use crate::command::CadCommand;
use crate::ui::window::annotation_data::{
    DataExtractionField, DataExtractionState, DataLinkChoice, DataLinkField, ExtractionBegin,
    ExtractionPage, ExtractionSource, LinkPathType, LinkRange, TableInsertField, TableInsertion,
    TableSource,
};

/// One extracted object: the name it is listed under and the value of every
/// property it offers.
#[derive(Clone)]
struct ExtractionRecord {
    name: String,
    values: BTreeMap<&'static str, String>,
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
fn extraction_documents() -> &'static Mutex<HashMap<String, Arc<Document>>> {
    static DOCUMENTS: OnceLock<Mutex<HashMap<String, Arc<Document>>>> = OnceLock::new();
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
pub(crate) fn register_extraction_document(name: &str, document: Document) {
    extraction_documents()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(name.to_string(), Arc::new(document));
}

/// The parsed drawing registered under this source reference, matched by exact
/// key or by file name.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn extraction_document(path: &str) -> Option<Arc<Document>> {
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
        self.values.get(key).cloned().unwrap_or_default()
    }
}

fn table_styles(doc: &Document) -> Vec<String> {
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

/// Named-objects dictionary that holds the drawing's data links, keyed by
/// link name.
pub(crate) const DATA_LINK_DICTIONARY: &str = "ACAD_DATALINK";
/// Data adapter of spreadsheet links.
pub(crate) const EXCEL_LINK_ADAPTER: &str = "AcExcel";
/// Data adapter of the links a data extraction table keeps to its settings.
pub(crate) const EXTRACTION_LINK_ADAPTER: &str = "OpenCADStudio.DataExtraction.DataLinkAdapter";

/// Whether a link adapter is a data extraction one: ours, or the adapter
/// other extraction tools file their links under (`...DataExtraction.DxDataLinkAdapter`).
fn is_extraction_adapter(adapter: &str) -> bool {
    adapter == EXTRACTION_LINK_ADAPTER || adapter.ends_with(".DataExtraction.DxDataLinkAdapter")
}
/// Data-link option: keep a cached copy of the source data.
pub(crate) const LINK_OPTION_PERSIST_CACHE: i32 = 0x1;
/// Update-option bits.
pub(crate) const UPDATE_SKIP_FORMAT: i32 = 0x2_0000;
pub(crate) const UPDATE_ALLOW_SOURCE_UPDATE: i32 = 0x10_0000;
pub(crate) const UPDATE_SKIP_FORMAT_AFTER_FIRST: i32 = 0x100_0000;

/// Every data link of the drawing as `(name, handle)`: the entries of the
/// data-link dictionary first, then links stored outside it (older OpenCAD
/// drawings) under their description or file name.
pub(crate) fn data_link_entries(doc: &Document) -> Vec<(String, Handle)> {
    let mut entries = Vec::new();
    if let Some(dictionary) = data_link_dictionary(doc) {
        if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get(&dictionary) {
            for (name, handle) in &dictionary.entries {
                if data_link(doc, *handle).is_some() {
                    entries.push((name.clone(), *handle));
                }
            }
        }
    }
    for (handle, object) in &doc.objects {
        if let ObjectType::ClassObject(object) = object {
            if let ClassObjectData::DataLink(link) = &object.data {
                if !entries.iter().any(|(_, known)| known == handle) {
                    entries.push((link_fallback_name(link), *handle));
                }
            }
        }
    }
    entries
}

fn data_link_dictionary(doc: &Document) -> Option<Handle> {
    let Some(ObjectType::Dictionary(root)) =
        doc.objects.get(&doc.header.named_objects_dict_handle)
    else {
        return None;
    };
    root.get(DATA_LINK_DICTIONARY)
        .filter(|handle| matches!(doc.objects.get(handle), Some(ObjectType::Dictionary(_))))
}

fn link_fallback_name(link: &DataLink) -> String {
    if !link.description.trim().is_empty() && link.description != link.connection_string {
        return link.description.clone();
    }
    Path::new(&split_connection(&link.connection_string).0)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Data Link".into())
}

/// Stores a data link under `name` in the data-link dictionary (created when
/// the drawing has none) and returns its handle.
pub(crate) fn add_data_link(doc: &mut Document, name: &str, mut link: DataLink) -> Handle {
    stamp_data_link(&mut link);
    let handle = doc.allocate_handle();
    let mut object = ClassObject::new(ClassObjectData::DataLink(link));
    object.handle = handle;
    doc.objects.insert(handle, ObjectType::ClassObject(object));
    put_data_link_entry(doc, handle, name);
    handle
}

/// Files a link under `name` in the data-link dictionary, replacing the
/// entry it had, and makes the dictionary its owner.
fn put_data_link_entry(doc: &mut Document, handle: Handle, name: &str) {
    let dictionary = data_link_dictionary(doc).or_else(|| {
        let root = doc.header.named_objects_dict_handle;
        if !matches!(doc.objects.get(&root), Some(ObjectType::Dictionary(_))) {
            return None;
        }
        let dictionary_handle = doc.allocate_handle();
        let mut dictionary = codec::objects::Dictionary::new();
        dictionary.handle = dictionary_handle;
        dictionary.owner = root;
        dictionary.hard_owner = true;
        dictionary.reactors = vec![root];
        doc.objects
            .insert(dictionary_handle, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root) {
            root.add_entry(DATA_LINK_DICTIONARY, dictionary_handle);
        }
        Some(dictionary_handle)
    });
    let Some(dictionary) = dictionary else {
        return;
    };
    if let Some(ObjectType::Dictionary(entries)) = doc.objects.get_mut(&dictionary) {
        let old: Vec<String> = entries
            .entries
            .iter()
            .filter(|(_, h)| *h == handle)
            .map(|(key, _)| key.clone())
            .collect();
        for key in old {
            entries.set_entry_hard_owner(&key, false);
        }
        entries.entries.retain(|(_, h)| *h != handle);
        entries.add_entry(name, handle);
        entries.set_entry_hard_owner(name, true);
    }
    if let Some(ObjectType::ClassObject(object)) = doc.objects.get_mut(&handle) {
        object.owner = dictionary;
        object.reactors = vec![dictionary];
    }
}

/// The link's last-update time (UTC).
pub(crate) fn stamp_data_link(link: &mut DataLink) {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = elapsed.as_secs() as i64;
    let (year, month, day) = crate::patreon::civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    link.year = year as i16;
    link.month = month as i16;
    link.day = day as i16;
    link.hour = (of_day / 3_600) as i16;
    link.minute = (of_day / 60 % 60) as i16;
    link.second = (of_day % 60) as i16;
    link.millisecond = elapsed.subsec_millis() as i16;
}

/// Stamps the last-update time of the link at `handle`.
pub(crate) fn stamp_data_link_handle(doc: &mut Document, handle: Handle) {
    if let Some(link) = data_link_mut(doc, handle) {
        stamp_data_link(link);
    }
}

/// DXEVAL: the events that compare extraction tables with their data —
/// 1 open, 2 save, 4 plot, 8 publish, 16 eTransmit/archive — and 32 save,
/// 64 plot, 128 publish, 256 eTransmit/archive updating them without
/// asking. Kept in the drawing; 12 (plot and publish) until set.
pub(crate) fn dxeval(doc: &Document) -> i32 {
    crate::io::drawing_variable(doc, "DXEVAL")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(12)
}

/// An extraction table whose data no longer matches the drawing.
#[derive(Debug, Clone)]
pub(crate) struct OutdatedExtraction {
    pub table: Handle,
    pub link: Handle,
    /// The settings file the extraction runs from.
    pub settings: String,
    /// What the table shows once updated: its title over the new data.
    pub rows: Vec<Vec<String>>,
}

/// The rows an extraction table shows from its data now, under its title
/// row; `None` when `link` is not an extraction link.
pub(crate) fn extraction_table_rows(
    doc: &Document,
    doc_path: Option<&Path>,
    table: Handle,
    link: Handle,
) -> Option<Result<Vec<Vec<String>>, String>> {
    let settings = extraction_link_settings(doc, link)?;
    Some(rerun_extraction(doc, doc_path, &settings).map(|rows| {
        let title = match doc.get_entity(table) {
            Some(EntityType::Table(table)) => table.cell_text(0, 0).unwrap_or_default().to_string(),
            _ => String::new(),
        };
        std::iter::once(vec![title]).chain(rows).collect()
    }))
}

/// The extraction tables of the drawing whose cells differ from what their
/// extraction gives now.
pub(crate) fn outdated_extraction_tables(
    doc: &Document,
    doc_path: Option<&Path>,
) -> Vec<OutdatedExtraction> {
    let mut outdated = Vec::new();
    for entity in doc.entities() {
        let EntityType::Table(table) = entity else {
            continue;
        };
        let Some(link) = table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .find_map(|cell| cell.data_link_handle)
        else {
            continue;
        };
        let handle = entity.common().handle;
        let Some(Ok(rows)) = extraction_table_rows(doc, doc_path, handle, link) else {
            continue;
        };
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let current = table.row_count() == rows.len()
            && table.column_count() == columns
            && rows.iter().enumerate().all(|(r, values)| {
                (0..columns).all(|c| {
                    table.cell_text(r, c).unwrap_or_default()
                        == values.get(c).map(String::as_str).unwrap_or("")
                })
            });
        if !current {
            outdated.push(OutdatedExtraction {
                table: handle,
                link,
                settings: extraction_link_settings(doc, link).unwrap_or_default(),
                rows,
            });
        }
    }
    outdated
}

/// Renames a link (filing it in the dictionary when it was outside it).
fn rename_data_link(doc: &mut Document, handle: Handle, name: &str) {
    put_data_link_entry(doc, handle, name);
}

/// Removes a link and its dictionary entry.
fn remove_data_link(doc: &mut Document, handle: Handle) {
    if let Some(dictionary) = data_link_dictionary(doc) {
        if let Some(ObjectType::Dictionary(entries)) = doc.objects.get_mut(&dictionary) {
            entries.entries.retain(|(_, h)| *h != handle);
        }
    }
    doc.objects.remove(&handle);
}

/// Splits a spreadsheet connection string `file!sheet!range` (or
/// `file!named-range`) into the file and the parts after it.
pub(crate) fn split_connection(connection: &str) -> (String, Vec<String>) {
    let name_start = connection.rfind(['/', '\\']).map_or(0, |at| at + 1);
    match connection[name_start..].find('!') {
        Some(at) => {
            let split = name_start + at;
            (
                connection[..split].to_string(),
                connection[split + 1..]
                    .split('!')
                    .map(str::to_string)
                    .collect(),
            )
        }
        None => (connection.to_string(), Vec::new()),
    }
}

/// Builds the connection string of a spreadsheet link.
fn join_connection(path: &str, sheet: &str, kind: LinkRange, range: Option<&str>) -> String {
    let range = range.map(str::trim).filter(|value| !value.is_empty());
    match (kind, range) {
        (LinkRange::NamedRange, Some(name)) => format!("{path}!{name}"),
        (LinkRange::CellRange, Some(cells)) if !sheet.trim().is_empty() => {
            format!("{path}!{}!{cells}", sheet.trim())
        }
        _ if !sheet.trim().is_empty() => format!("{path}!{}", sheet.trim()),
        _ => path.to_string(),
    }
}

fn resolve_link_path(doc: &Document, link: &DataLink) -> PathBuf {
    let stored = PathBuf::from(split_connection(&link.connection_string).0);
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

/// What a link takes from its source, as the manager describes it.
fn link_details(doc: &Document, link: &DataLink) -> String {
    if is_extraction_adapter(&link.data_adapter) {
        return crate::t!("Data extraction settings").into_owned();
    }
    let (sheet, kind, range) = link_selection(doc, link);
    let sheet = sheet.unwrap_or_default();
    match (kind, range) {
        (LinkRange::NamedRange, Some(name)) => crate::tf!("Named range: {name}").into_owned(),
        (LinkRange::CellRange, Some(cells)) => {
            crate::tf!("Range: {sheet}!{cells}").into_owned()
        }
        _ => crate::tf!("Entire sheet: {sheet}").into_owned(),
    }
}

fn data_link_choices(doc: &Document) -> Vec<DataLinkChoice> {
    let mut links = data_link_entries(doc)
        .into_iter()
        .filter_map(|(name, handle)| {
            let link = data_link(doc, handle)?;
            let path = resolve_link_path(doc, link);
            let path_text = path.to_string_lossy().into_owned();
            // The web has no filesystem: a link is valid when its
            // picked bytes are still registered in-session.
            let valid = path.is_file() || picked_source_bytes(&path_text).is_some();
            Some(DataLinkChoice {
                handle,
                name,
                path: path_text,
                details: link_details(doc, link),
                valid,
            })
        })
        .collect::<Vec<_>>();
    links.sort_by_key(|link| link.name.to_ascii_lowercase());
    links
}

fn data_link<'a>(doc: &'a Document, handle: Handle) -> Option<&'a DataLink> {
    match doc.objects.get(&handle) {
        Some(ObjectType::ClassObject(object)) => match &object.data {
            ClassObjectData::DataLink(link) => Some(link),
            _ => None,
        },
        _ => None,
    }
}

fn data_link_mut<'a>(doc: &'a mut Document, handle: Handle) -> Option<&'a mut DataLink> {
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

/// The drawing's table styles, as the table commands pick them.
fn table_style_choices(doc: &Document) -> Vec<crate::modules::annotate::table_cmd::StyleChoice> {
    let mut styles: Vec<_> = doc
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::TableStyle(style) => {
                Some(crate::modules::annotate::table_cmd::StyleChoice {
                    name: style.name.clone(),
                    handle: *handle,
                    style: style.clone(),
                })
            }
            _ => None,
        })
        .collect();
    styles.sort_by_key(|choice| choice.name.to_ascii_lowercase());
    styles
}

fn table_style_handle(doc: &Document, name: &str) -> Option<Handle> {
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

/// The sheet, range kind and range a link reads: from its connection string,
/// or from the custom data older OpenCAD drawings kept them in.
fn link_selection(
    doc: &Document,
    link: &DataLink,
) -> (Option<String>, LinkRange, Option<String>) {
    let mut sheet = None;
    let mut kind = LinkRange::EntireSheet;
    let mut range = None;
    let mut legacy = false;
    for item in &link.custom_data {
        if let Some(value) = item.value.strip_prefix("sheet:") {
            legacy = true;
            sheet = (!value.is_empty()).then(|| value.to_string());
        } else if let Some(value) = item.value.strip_prefix("range-kind:") {
            legacy = true;
            kind = if value.eq_ignore_ascii_case("named") {
                LinkRange::NamedRange
            } else if value.eq_ignore_ascii_case("cell") {
                LinkRange::CellRange
            } else {
                LinkRange::EntireSheet
            };
        } else if let Some(value) = item.value.strip_prefix("range:") {
            legacy = true;
            range = (!value.is_empty()).then(|| value.to_string());
            if kind == LinkRange::EntireSheet {
                kind = LinkRange::CellRange;
            }
        }
    }
    if legacy {
        return (sheet, kind, range);
    }
    let (_, parts) = split_connection(&link.connection_string);
    match parts.as_slice() {
        [] => (None, LinkRange::EntireSheet, None),
        [sheet, cells, ..] => (
            Some(sheet.clone()),
            LinkRange::CellRange,
            Some(cells.clone()),
        ),
        [single] => {
            // One part names a sheet, or a named range when the workbook has
            // no sheet by that name.
            let sheets = workbook_metadata(&resolve_link_path(doc, link))
                .map(|(sheets, _)| sheets)
                .unwrap_or_default();
            if sheets.is_empty() || sheets.iter().any(|name| name.eq_ignore_ascii_case(single)) {
                (Some(single.clone()), LinkRange::EntireSheet, None)
            } else {
                (None, LinkRange::NamedRange, Some(single.clone()))
            }
        }
    }
}

/// Spreadsheet-link settings as the manager edits them.
pub(crate) struct SpreadsheetLinkSettings<'a> {
    pub path: &'a str,
    pub stored_path: &'a str,
    pub sheet: &'a str,
    pub kind: LinkRange,
    pub range: Option<&'a str>,
    pub path_option: i16,
    pub allow_write: bool,
    pub use_source_formatting: bool,
    pub keep_source_formatting: bool,
}

/// Writes spreadsheet-link settings into a link: the file, sheet and range in
/// the connection string, the editing options in the update options.
pub(crate) fn set_spreadsheet_link(link: &mut DataLink, settings: &SpreadsheetLinkSettings) {
    link.data_adapter = EXCEL_LINK_ADAPTER.into();
    link.description.clear();
    link.tooltip = settings.path.to_string();
    link.connection_string = join_connection(
        settings.stored_path,
        settings.sheet,
        settings.kind,
        settings.range,
    );
    link.path_option = settings.path_option;
    link.option = LINK_OPTION_PERSIST_CACHE;
    link.flags = 1;
    let mut update = 0;
    if !settings.use_source_formatting {
        update |= UPDATE_SKIP_FORMAT;
    }
    if settings.allow_write {
        update |= UPDATE_ALLOW_SOURCE_UPDATE;
    }
    if !settings.keep_source_formatting {
        update |= UPDATE_SKIP_FORMAT_AFTER_FIRST;
    }
    link.update_option = update;
    link.status_flags = 0;
    link.update_status.clear();
    link.custom_data.retain(|item| {
        !(item.value.starts_with("sheet:")
            || item.value.starts_with("range-kind:")
            || item.value.starts_with("range:"))
    });
}

/// Lists an object a link feeds among the link's connected objects.
pub(crate) fn connect_data_link(doc: &mut Document, link: Handle, target: Handle) {
    if let Some(link) = data_link_mut(doc, link) {
        if !link.custom_data.iter().any(|item| item.target == target) {
            link.custom_data.push(DataLinkCustomData {
                target,
                value: String::new(),
            });
        }
    }
}

/// The link a data extraction table keeps to its settings file, named
/// `Data Extraction - <file>`.
pub(crate) fn add_extraction_link(doc: &mut Document, settings: &str) -> Handle {
    let file = Path::new(settings)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| settings.to_string());
    let stem = Path::new(&file)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.clone());
    let base = format!("Data Extraction - {file}");
    let names = data_link_entries(doc);
    let mut name = base.clone();
    let mut n = 1;
    while names.iter().any(|(known, _)| known.eq_ignore_ascii_case(&name)) {
        n += 1;
        name = format!("{base} ({n})");
    }
    let link = DataLink {
        data_adapter: EXTRACTION_LINK_ADAPTER.into(),
        tooltip: format!("Created from Data Extraction:\r\n{stem}"),
        connection_string: settings.to_string(),
        option: LINK_OPTION_PERSIST_CACHE,
        update_option: UPDATE_SKIP_FORMAT,
        flags: 1,
        path_option: 5,
        ..DataLink::default()
    };
    add_data_link(doc, &name, link)
}

/// The settings file of a data extraction link.
pub(crate) fn extraction_link_settings(doc: &Document, handle: Handle) -> Option<String> {
    data_link(doc, handle)
        .filter(|link| is_extraction_adapter(&link.data_adapter))
        .map(|link| link.connection_string.clone())
}

/// Whether the link lets tables write their data back to the source.
pub(crate) fn data_link_allows_write(doc: &Document, handle: Handle) -> bool {
    data_link(doc, handle)
        .is_some_and(|link| link.update_option & UPDATE_ALLOW_SOURCE_UPDATE != 0)
}

fn read_link_rows(doc: &Document, link: &DataLink) -> Result<Vec<Vec<String>>, String> {
    let (sheet, kind, range) = link_selection(doc, link);
    read_tabular_file(
        &resolve_link_path(doc, link),
        sheet.as_deref(),
        kind,
        range.as_deref(),
    )
}

pub(crate) fn read_data_link(
    doc: &Document,
    handle: Handle,
) -> Result<Vec<Vec<String>>, String> {
    let link = data_link(doc, handle)
        .ok_or_else(|| crate::t!("The data link no longer exists.").into_owned())?;
    read_link_rows(doc, link)
}

pub(crate) fn data_link_write_path(doc: &Document, handle: Handle) -> Result<PathBuf, String> {
    let link = data_link(doc, handle)
        .ok_or_else(|| crate::t!("The data link no longer exists.").into_owned())?;
    if link.update_option & UPDATE_ALLOW_SOURCE_UPDATE == 0 {
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

/// The objects the extraction reads from a drawing: model space objects
/// other than block definitions' own markers and the drawing of a table,
/// dimension or other object kept in an anonymous block (a table is one
/// object, not its lines and texts).
fn extraction_entities<'a>(
    doc: &'a Document,
    allowed_handles: Option<&'a BTreeSet<Handle>>,
) -> impl Iterator<Item = &'a EntityType> + 'a {
    let anonymous: BTreeSet<Handle> = doc
        .block_records
        .iter()
        .filter(|record| {
            let name = record.name.to_ascii_lowercase();
            name.starts_with('*')
                && !name.starts_with("*model_space")
                && !name.starts_with("*paper_space")
        })
        .map(|record| record.handle)
        .collect();
    doc.entities().filter(move |entity| {
        !matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_))
            && !anonymous.contains(&entity.common().owner_handle)
            && allowed_handles.is_none_or(|handles| handles.contains(&entity.common().handle))
    })
}

fn collect_records(
    doc: &Document,
    path: Option<&Path>,
    allowed_handles: Option<&BTreeSet<Handle>>,
    allowed_names: &BTreeSet<String>,
    out: &mut Vec<ExtractionRecord>,
) {
    use crate::app::extraction_catalog as catalog;
    let facts = catalog::DrawingFacts::of(doc, path);
    for entity in extraction_entities(doc, allowed_handles) {
        let (name, _) = catalog::object_name(entity);
        if !allowed_names.contains(&name) {
            continue;
        }
        let values = catalog::properties(entity)
            .into_iter()
            .map(|property| (property.key, catalog::value(entity, property.key, &facts)))
            .collect();
        out.push(ExtractionRecord { name, values });
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
    mut visit: impl FnMut(&Document, Option<&Path>),
) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    for source in &state.source_files {
        let document = extraction_document(source).ok_or_else(|| {
            crate::tf!("Drawing not loaded in the browser: {name}.", name = source).into_owned()
        })?;
        visit(&document, None);
    }
    #[cfg(not(target_arch = "wasm32"))]
    for source in extraction_drawing_paths(state)? {
        let document = crate::io::load_file(&source)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        visit(&document, Some(&source));
    }
    Ok(())
}

/// Per listed object: whether it is a block, how many there are, and the
/// properties it offers.
type ObjectCatalog = BTreeMap<String, (bool, usize, Vec<crate::app::extraction_catalog::Property>)>;

fn collect_object_catalog(
    doc: &Document,
    allowed_handles: Option<&BTreeSet<Handle>>,
    catalog: &mut ObjectCatalog,
) {
    for entity in extraction_entities(doc, allowed_handles) {
        let (name, block) = crate::app::extraction_catalog::object_name(entity);
        let entry = catalog.entry(name).or_insert((block, 0, Vec::new()));
        entry.1 += 1;
        for property in crate::app::extraction_catalog::properties(entity) {
            if !entry.2.iter().any(|known| known.key == property.key) {
                entry.2.push(property);
            }
        }
    }
}

/// The extraction's rows: a header (Count, Name, then the chosen properties
/// in the order their objects list them), then one row per object, or per
/// group of identical objects when they are combined.
fn extraction_preview(
    state: &DataExtractionState,
    current: &Document,
    current_path: Option<&Path>,
) -> Result<Vec<Vec<String>>, String> {
    let allowed_names = state
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
            collect_records(current, current_path, None, &allowed_names, &mut records)
        }
        ExtractionSource::CurrentSelection => collect_records(
            current,
            current_path,
            Some(&selected),
            &allowed_names,
            &mut records,
        ),
        ExtractionSource::DrawingsAndFolders => {
            if state.include_current {
                collect_records(current, current_path, None, &allowed_names, &mut records);
            }
            for_each_extraction_document(state, |document, path| {
                collect_records(document, path, None, &allowed_names, &mut records);
            })?;
        }
    }
    let props = state
        .properties
        .iter()
        .filter(|value| value.checked && state.categories.contains(&value.category))
        .collect::<Vec<_>>();
    if props.is_empty() {
        return Err(crate::t!("Select at least one property.").into_owned());
    }
    let mut header = Vec::new();
    if state.show_count {
        header.push("Count".to_string());
    }
    if state.show_name {
        header.push("Name".to_string());
    }
    header.extend(props.iter().map(|value| value.name.clone()));
    let values_of = |record: &ExtractionRecord| {
        let mut values = Vec::new();
        if state.show_name {
            values.push(record.name.clone());
        }
        values.extend(props.iter().map(|value| record.value(&value.key)));
        values
    };
    let mut rows: Vec<(Vec<String>, usize)> = Vec::new();
    for record in &records {
        let values = values_of(record);
        if state.combine_identical {
            if let Some(row) = rows.iter_mut().find(|(known, _)| *known == values) {
                row.1 += 1;
                continue;
            }
        }
        rows.push((values, 1));
    }
    let mut output = vec![header];
    output.extend(rows.into_iter().map(|(values, count)| {
        if state.show_count {
            std::iter::once(count.to_string()).chain(values).collect()
        } else {
            values
        }
    }));
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

/// Writes the extraction rows as a workbook (.xlsx), comma-separated values
/// (.csv) or tab-separated text (.txt).
fn write_extraction_file(path: &Path, rows: &[Vec<String>]) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = match ext.as_str() {
        "xlsx" => xlsx_bytes(rows)?,
        "csv" => rows_to_csv(rows).into_bytes(),
        "txt" => (rows
            .iter()
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n")
            .into_bytes(),
        _ => {
            return Err(crate::t!(
                "The external file must be an .xlsx, .csv or .txt file."
            )
            .into_owned())
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("extraction.{ext}"));
        crate::sys::download_bytes(&name, &bytes);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::write(path, bytes).map_err(|error| error.to_string())
    }
}

/// A one-sheet workbook holding the rows as text cells.
fn xlsx_bytes(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    fn column_name(mut index: usize) -> String {
        let mut name = String::new();
        index += 1;
        while index > 0 {
            let rem = (index - 1) % 26;
            name.insert(0, (b'A' + rem as u8) as char);
            index = (index - 1) / 26;
        }
        name
    }
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let mut sheet = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    for (r, row) in rows.iter().enumerate() {
        sheet.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, value) in row.iter().enumerate() {
            if value.is_empty() {
                continue;
            }
            sheet.push_str(&format!(
                "<c r=\"{}{}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                column_name(c),
                r + 1,
                escape(value)
            ));
        }
        sheet.push_str("</row>");
    }
    sheet.push_str("</sheetData></worksheet>");
    let parts = [
        ("[Content_Types].xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>".to_string()),
        ("_rels/.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>".to_string()),
        ("xl/workbook.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".to_string()),
        ("xl/_rels/workbook.xml.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>".to_string()),
        ("xl/worksheets/sheet1.xml", sheet),
    ];
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in parts {
        writer
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        writer
            .write_all(body.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(writer
        .finish()
        .map_err(|error| error.to_string())?
        .into_inner())
}

/// The listed objects with what was checked before (new ones start checked),
/// each object's properties in name order, objects in name order.
fn objects_from_catalog(
    catalog: ObjectCatalog,
    previous: &BTreeMap<String, bool>,
) -> Vec<crate::ui::window::annotation_data::ExtractionObject> {
    let mut objects: Vec<_> = catalog
        .into_iter()
        .map(|(name, (block, count, mut properties))| {
            properties.sort_by_key(|property| property.name.to_ascii_lowercase());
            crate::ui::window::annotation_data::ExtractionObject {
                checked: previous
                    .get(&name.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(true),
                name,
                count,
                block,
                properties: properties
                    .into_iter()
                    .map(|property| {
                        (
                            property.key.to_string(),
                            property.name.to_string(),
                            property.category.to_string(),
                        )
                    })
                    .collect(),
            }
        })
        .collect();
    objects.sort_by_key(|object| object.name.to_ascii_lowercase());
    objects
}

/// The property list for the checked objects (each property once, in the
/// order the objects list them) and the categories to show, keeping what
/// was checked before; new properties start checked.
fn extraction_properties(
    state: &DataExtractionState,
    previous: &BTreeMap<String, bool>,
) -> (
    Vec<crate::ui::window::annotation_data::ExtractionProperty>,
    BTreeSet<String>,
) {
    let mut properties: Vec<crate::ui::window::annotation_data::ExtractionProperty> = Vec::new();
    for object in state
        .objects
        .iter()
        .filter(|object| object.checked && state.shows_object(object))
    {
        for (key, name, category) in &object.properties {
            if properties.iter().any(|known| &known.key == key) {
                continue;
            }
            properties.push(crate::ui::window::annotation_data::ExtractionProperty {
                checked: previous.get(key).copied().unwrap_or(true),
                key: key.clone(),
                name: name.clone(),
                category: category.clone(),
            });
        }
    }
    // Every category is shown unless the user filtered it out.
    let categories = properties
        .iter()
        .map(|value| value.category.clone())
        .filter(|category| !state.hidden_categories.contains(category))
        .collect();
    (properties, categories)
}

/// Runs a saved extraction again on a drawing (DATALINKUPDATE on an
/// extraction table): the settings file's sources, objects, properties and
/// row options.
pub(crate) fn rerun_extraction(
    doc: &Document,
    doc_path: Option<&Path>,
    settings: &str,
) -> Result<Vec<Vec<String>>, String> {
    let mut state = DataExtractionState {
        settings_path: settings.to_string(),
        ..DataExtractionState::default()
    };
    load_extraction_settings(&mut state)?;
    let mut catalog = ObjectCatalog::new();
    match state.source {
        ExtractionSource::CurrentDrawing | ExtractionSource::CurrentSelection => {
            collect_object_catalog(doc, None, &mut catalog)
        }
        ExtractionSource::DrawingsAndFolders => {
            if state.include_current {
                collect_object_catalog(doc, None, &mut catalog);
            }
            for_each_extraction_document(&state, |document, _| {
                collect_object_catalog(document, None, &mut catalog);
            })?;
        }
    }
    let saved_objects = state.saved_objects.take().unwrap_or_default();
    let previous = catalog
        .keys()
        .map(|name| {
            (
                name.to_ascii_lowercase(),
                saved_objects.is_empty() || saved_objects.contains(name),
            )
        })
        .collect();
    state.objects = objects_from_catalog(catalog, &previous);
    let saved_properties = state.saved_properties.take().unwrap_or_default();
    let previous = state
        .objects
        .iter()
        .flat_map(|object| object.properties.iter())
        .map(|(key, _, _)| {
            (
                key.clone(),
                saved_properties.is_empty() || saved_properties.contains(key),
            )
        })
        .collect();
    let (properties, categories) = extraction_properties(&state, &previous);
    state.properties = properties;
    state.categories = categories;
    if state.source == ExtractionSource::CurrentSelection {
        state.source = ExtractionSource::CurrentDrawing;
    }
    extraction_preview(&state, doc, doc_path)
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
        "titleCellStyle": state.title_cell_style,
        "headerCellStyle": state.header_cell_style,
        "dataCellStyle": state.data_cell_style,
        "propertyHeaders": state.property_headers,
        "displayAllTypes": state.display_all_types,
        "blocksOnly": state.blocks_only,
        "attributedBlocksOnly": state.attributed_blocks_only,
        "inUseOnly": state.in_use_only,
        "hiddenCategories": state.hidden_categories,
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
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|error| error.to_string())?;
    if value["drawingDataExtractor"].is_object() {
        load_extractor_settings(state, &value);
        return Ok(());
    }
    state.source = match value["source"].as_str() {
        Some("currentSelection") => ExtractionSource::CurrentSelection,
        Some("currentDrawing") => ExtractionSource::CurrentDrawing,
        _ => ExtractionSource::DrawingsAndFolders,
    };
    let flag = |key: &str, current: bool| value[key].as_bool().unwrap_or(current);
    let text_of = |key: &str, current: &str| value[key].as_str().unwrap_or(current).to_string();
    let set_of = |key: &str| {
        value[key].as_array().map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<BTreeSet<_>>()
        })
    };
    state.include_current = flag("includeCurrent", state.include_current);
    state.include_subfolders = flag("includeSubfolders", state.include_subfolders);
    if let Some(files) = value["sourceFiles"].as_array() {
        state.source_files = files
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
    }
    state.saved_objects = set_of("objectTypes");
    state.saved_properties = set_of("properties");
    for item in &mut state.objects {
        if let Some(saved) = &state.saved_objects {
            item.checked = saved.is_empty() || saved.contains(&item.name);
        }
    }
    for item in &mut state.properties {
        if let Some(saved) = &state.saved_properties {
            item.checked = saved.is_empty() || saved.contains(&item.key);
        }
    }
    if let Some(hidden) = set_of("hiddenCategories") {
        state.categories.retain(|category| !hidden.contains(category));
        state.hidden_categories = hidden;
    }
    state.combine_identical = flag("combineIdentical", state.combine_identical);
    state.show_count = flag("showCount", state.show_count);
    state.show_name = flag("showName", state.show_name);
    state.output_table = flag("outputTable", state.output_table);
    state.output_file = flag("outputFile", state.output_file);
    state.output_path = text_of("outputPath", &state.output_path);
    state.table_style = text_of("tableStyle", &state.table_style);
    state.table_title = text_of("tableTitle", &state.table_title);
    state.title_cell_style = text_of("titleCellStyle", &state.title_cell_style);
    state.header_cell_style = text_of("headerCellStyle", &state.header_cell_style);
    state.data_cell_style = text_of("dataCellStyle", &state.data_cell_style);
    state.property_headers = flag("propertyHeaders", state.property_headers);
    state.display_all_types = flag("displayAllTypes", state.display_all_types);
    state.blocks_only = flag("blocksOnly", state.blocks_only);
    state.attributed_blocks_only = flag("attributedBlocksOnly", state.attributed_blocks_only);
    state.in_use_only = flag("inUseOnly", state.in_use_only);
    Ok(())
}

/// Settings in the layout other extraction tools write: the source files
/// under `drawingDataExtractor.settings.fileList`, the object types and the
/// report's columns (`strGlobalName`, the same keys these settings use;
/// flag 16 marks a hidden column), and the table output.
fn load_extractor_settings(state: &mut DataExtractionState, value: &serde_json::Value) {
    let settings = &value["drawingDataExtractor"]["settings"];
    let files = &settings["fileList"];
    let paths = |key: &str| {
        files[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["absolute"].as_str().or_else(|| entry.as_str()))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    state.source_files = paths("files").into_iter().chain(paths("folders")).collect();
    state.include_current = files["currentFile"].is_object();
    state.source = if state.source_files.is_empty() {
        ExtractionSource::CurrentDrawing
    } else {
        ExtractionSource::DrawingsAndFolders
    };
    state.saved_objects = Some(
        settings["selectedTypesAndProperties"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|kind| kind["displayName"].as_str().map(str::to_string))
            .collect(),
    );
    let report = &value["report"];
    let columns: Vec<&str> = report["ColLists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|column| column["flags"].as_i64().unwrap_or(0) & 16 == 0)
        .filter_map(|column| column["strGlobalName"].as_str())
        .collect();
    state.show_count = columns.contains(&"AcDxCount");
    state.show_name = columns.contains(&"AcDxObjectTypeName");
    state.saved_properties = Some(
        columns
            .into_iter()
            .filter(|key| !matches!(*key, "AcDxCount" | "AcDxObjectTypeName"))
            .map(str::to_string)
            .collect(),
    );
    if let Some(combine) = report["bInAggregateMode"].as_bool() {
        state.combine_identical = combine;
    }
    let output = &value["outputSettings"];
    let text_of = |key: &str, current: &str| output[key].as_str().unwrap_or(current).to_string();
    state.output_path = text_of("fileName", &state.output_path);
    state.output_file = !state.output_path.trim().is_empty();
    state.output_table = output["ouputFlags"].as_i64().map_or(state.output_table, |f| f & 2 != 0);
    state.table_style = text_of("tableStyleName", &state.table_style);
    state.table_title = text_of("tableTitle", &state.table_title);
    state.title_cell_style = text_of("titleCellStyle", &state.title_cell_style);
    state.header_cell_style = text_of("headerCellStyle", &state.header_cell_style);
    state.data_cell_style = text_of("dataCellStyle", &state.data_cell_style);
    if let Some(headers) = output["usePropertyNameAsColumnHeader"].as_bool() {
        state.property_headers = headers;
    }
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
        let saved = *crate::modules::annotate::table_cmd::session();
        let state = &mut self.table_insert;
        state.columns = saved.columns.to_string();
        state.data_rows = saved.data_rows.to_string();
        state.column_width = crate::entities::common::format_length(
            saved.cell_width(self.tabs[i].scene.document.header.measurement == 1),
        );
        state.row_height = saved.row_lines.to_string();
        state.insertion = if saved.window {
            TableInsertion::Window
        } else {
            TableInsertion::Point
        };
        state.columns_fixed = saved.columns_fixed;
        state.rows_fixed = saved.rows_fixed;
        self.table_insert.error.clear();
        self.reset_annotation_modal(ModalKind::InsertTable);
    }

    /// -TABLE: the table settings on the command line.
    pub(super) fn start_command_line_table(&mut self) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let styles = table_style_choices(doc);
        let current = styles
            .iter()
            .find(|choice| choice.name.eq_ignore_ascii_case(&doc.header.current_table_style_name))
            .or(styles.first())
            .cloned();
        let links = data_link_choices(doc)
            .into_iter()
            .map(|choice| (choice.name, choice.handle))
            .collect();
        let command = crate::modules::annotate::table_cmd::TableCommand::command_line(
            current,
            styles,
            links,
            doc.header.measurement == 1,
        );
        self.command_line.push_output(&command.status_line());
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// -TABLE's data Link option: `<link handle> <table style>`.
    pub(super) fn place_command_line_link_table(&mut self, args: &str) {
        let i = self.active_tab;
        let (handle, style) = args.split_once(' ').unwrap_or((args, ""));
        let doc = &self.tabs[i].scene.document;
        let Some((handle, link)) = u64::from_str_radix(handle.trim(), 16)
            .ok()
            .map(Handle::new)
            .and_then(|handle| data_link(doc, handle).map(|link| (handle, link)))
        else {
            self.command_line.push_error(crate::t!("Data link not found.").as_ref());
            return;
        };
        let rows = match read_link_rows(doc, link) {
            Ok(rows) => rows,
            Err(error) => {
                self.command_line.push_error(&error);
                return;
            }
        };
        let table = build_table(&rows, table_style_handle(doc, style.trim()), None);
        let command =
            crate::modules::annotate::data_link::DataLinkPlaceCommand::existing_from_table(
                table, handle,
            );
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
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
        // The extraction keeps settings next to the drawing and lists the
        // drawing among its sources, so it needs a saved drawing.
        if self.tabs[i].current_path.is_none() {
            self.reset_annotation_modal(ModalKind::DataExtractionUnsaved);
            return;
        }
        let doc = &self.tabs[i].scene.document;
        let selection_handles = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle)
            .collect::<Vec<_>>();
        let mut state = DataExtractionState::default();
        state.selection_handles = selection_handles;
        state.table_styles = table_styles(doc);
        state.table_style = doc.header.current_table_style_name.clone();
        self.data_extraction = state;
        self.reset_annotation_modal(ModalKind::DataExtraction);
    }

    /// Save in the unsaved-drawing notice: pick where the drawing goes.
    pub(super) fn on_data_extraction_save_drawing(&mut self) -> Task<Message> {
        let name = format!("{}.dwg", self.tabs[self.active_tab].tab_display_name());
        Task::perform(
            async move {
                let handle = crate::sys::file_dialog()
                    .set_title(crate::t!("Save Drawing As").as_ref())
                    .add_filter(crate::t!("Drawing (*.dwg)").as_ref(), &["dwg"])
                    .set_file_name(&name)
                    .save_file()
                    .await?;
                Some(crate::sys::handle_path(&handle))
            },
            Message::DataExtractionSaveDrawingResult,
        )
    }

    /// The drawing's new file: save it there, then open the wizard.
    pub(super) fn on_data_extraction_save_drawing_result(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(path) = path else {
            return Task::none();
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let i = self.active_tab;
            if let Err(error) = self.save_tab_synchronously_protected(i, path, true) {
                self.command_line.push_error(&error.to_string());
                return Task::none();
            }
            self.open_data_extraction();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = path;
        Task::none()
    }

    /// -DATAEXTRACTION with a settings file: run that extraction on the
    /// drawing — its external file, then its table at a picked point.
    pub(in crate::app) fn run_extraction_template(&mut self, path: &str) {
        let i = self.active_tab;
        let mut state = DataExtractionState {
            settings_path: path.to_string(),
            ..DataExtractionState::default()
        };
        if let Err(error) = load_extraction_settings(&mut state) {
            self.command_line.push_error(&error);
            return;
        }
        let rows = match rerun_extraction(
            &self.tabs[i].scene.document,
            self.tabs[i].current_path.as_deref(),
            path,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                self.command_line.push_error(&error);
                return;
            }
        };
        if state.output_file && !state.output_path.trim().is_empty() {
            // A relative output file goes next to the drawing.
            let mut file = PathBuf::from(state.output_path.trim());
            if let Some(dir) = self.tabs[i]
                .current_path
                .as_deref()
                .and_then(std::path::Path::parent)
                .filter(|_| file.is_relative())
            {
                file = dir.join(file);
            }
            match write_extraction_file(&file, &rows) {
                Ok(()) => self.command_line.push_output(
                    crate::tf!(
                        "External file \"{}\" was successfully created.",
                        file.display()
                    )
                    .as_ref(),
                ),
                Err(error) => self.command_line.push_error(&error),
            }
        }
        if state.output_table {
            let saved = std::mem::replace(&mut self.data_extraction, state);
            let table = self.extraction_table(&rows);
            self.data_extraction = saved;
            let command = crate::modules::annotate::data_link::DataLinkPlaceCommand::extraction(
                table,
                path.to_string(),
            );
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        }
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
            TableInsertField::ColumnsFixed(value) => self.table_insert.columns_fixed = value,
            TableInsertField::RowsFixed(value) => self.table_insert.rows_fixed = value,
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
        let state = &self.table_insert;
        let window = state.insertion == TableInsertion::Window;
        let (Some(columns), Some(data_rows), Some(column_width), Some(row_lines)) = (
            parse_usize(&state.columns),
            parse_usize(&state.data_rows),
            parse_number(&state.column_width),
            parse_usize(&state.row_height),
        ) else {
            self.table_insert.error =
                crate::t!("Columns, rows, width, and height must be positive values.").into_owned();
            return Task::none();
        };
        {
            let mut saved = crate::modules::annotate::table_cmd::session();
            saved.columns = columns;
            saved.data_rows = data_rows;
            saved.column_width = column_width;
            saved.row_lines = row_lines;
            saved.window = window;
            saved.columns_fixed = state.columns_fixed;
            saved.rows_fixed = state.rows_fixed;
        }
        let doc = &self.tabs[i].scene.document;
        let style = table_style_choices(doc)
            .into_iter()
            .find(|choice| choice.name.eq_ignore_ascii_case(&state.style));
        let command = crate::modules::annotate::table_cmd::TableCommand::configured(
            style,
            (!window || state.columns_fixed).then_some(columns),
            (!window || state.rows_fixed).then_some(data_rows),
            column_width,
            row_lines,
            window,
            true,
            [
                &state.first_row_style,
                &state.second_row_style,
                &state.other_row_style,
            ],
        );
        self.active_modal = None;
        self.reset_modal_geometry();
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    pub(super) fn on_data_link_new(&mut self) -> Task<Message> {
        self.data_link_manager.name.clear();
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
        self.data_link_manager.name = choice.name.clone();
        let path = resolve_link_path(&self.tabs[self.active_tab].scene.document, link);
        self.data_link_manager.path = path.to_string_lossy().into_owned();
        self.data_link_manager.path_type = match link.path_option {
            2 => LinkPathType::Relative,
            3 => LinkPathType::FileName,
            _ => LinkPathType::Full,
        };
        let (sheet, range_kind, range) =
            link_selection(&self.tabs[self.active_tab].scene.document, link);
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
        self.data_link_manager.allow_write = link.update_option & UPDATE_ALLOW_SOURCE_UPDATE != 0;
        self.data_link_manager.use_source_formatting = link.update_option & UPDATE_SKIP_FORMAT == 0;
        self.data_link_manager.update_source_formatting =
            link.update_option & UPDATE_SKIP_FORMAT_AFTER_FIRST == 0;
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
            // A full path as the user knows it, without the verbatim prefix
            // Windows canonical paths carry.
            LinkPathType::Full => std::fs::canonicalize(&input_path)
                .map(|path| {
                    let text = path.to_string_lossy().into_owned();
                    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_string())
                })
                .unwrap_or(input_path.clone()),
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
        let path_option = match self.data_link_manager.path_type {
            LinkPathType::Full => 1,
            LinkPathType::Relative => 2,
            LinkPathType::FileName => 3,
        };
        let input_text = input_path.to_string_lossy().into_owned();
        let stored_text = stored_path.to_string_lossy().into_owned();
        let sheet = self.data_link_manager.sheet.clone();
        let settings = SpreadsheetLinkSettings {
            path: &input_text,
            stored_path: &stored_text,
            sheet: &sheet,
            kind: self.data_link_manager.range_kind,
            range: range.as_deref(),
            path_option,
            allow_write: self.data_link_manager.allow_write,
            use_source_formatting: self.data_link_manager.use_source_formatting,
            keep_source_formatting: self.data_link_manager.update_source_formatting,
        };
        self.push_undo_snapshot(i, "DATALINK");
        let document = &mut self.tabs[i].scene.document;
        let handle = if let Some(handle) = self.data_link_manager.editing_handle {
            if let Some(link) = data_link_mut(document, handle) {
                set_spreadsheet_link(link, &settings);
                stamp_data_link(link);
            }
            rename_data_link(document, handle, &name);
            handle
        } else {
            let mut link = DataLink::default();
            set_spreadsheet_link(&mut link, &settings);
            add_data_link(document, &name, link)
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
        remove_data_link(&mut self.tabs[i].scene.document, handle);
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

    /// Back from the Table Style dialog: the style lists and the current
    /// style as that dialog left them.
    pub(in crate::app) fn return_to_table_insert(&mut self) {
        let doc = &self.tabs[self.active_tab].scene.document;
        self.table_insert.styles = table_styles(doc);
        self.table_insert.style = doc.header.current_table_style_name.clone();
        self.reset_annotation_modal(ModalKind::InsertTable);
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
                    self.data_extraction.hidden_categories.remove(&category);
                    self.data_extraction.categories.insert(category);
                } else {
                    self.data_extraction.hidden_categories.insert(category.clone());
                    self.data_extraction.categories.remove(&category);
                }
            }
            DataExtractionField::CombineIdentical(value) => {
                self.data_extraction.combine_identical = value
            }
            DataExtractionField::ShowCount(value) => self.data_extraction.show_count = value,
            DataExtractionField::ShowName(value) => self.data_extraction.show_name = value,
            DataExtractionField::OutputTable(value) => self.data_extraction.output_table = value,
            DataExtractionField::OutputFile(value) => {
                self.data_extraction.output_file = value;
                if value && self.data_extraction.output_path.trim().is_empty() {
                    self.data_extraction.output_path = self.default_extraction_output();
                }
            }
            DataExtractionField::OutputPath(value) => self.data_extraction.output_path = value,
            DataExtractionField::TableStyle(value) => self.data_extraction.table_style = value,
            DataExtractionField::TableTitle(value) => self.data_extraction.table_title = value,
            DataExtractionField::DisplayAllTypes(value) => {
                self.data_extraction.display_all_types = value
            }
            DataExtractionField::BlocksOnly(value) => self.data_extraction.blocks_only = value,
            DataExtractionField::AttributedBlocksOnly(value) => {
                self.data_extraction.attributed_blocks_only = value
            }
            DataExtractionField::InUseOnly(value) => self.data_extraction.in_use_only = value,
            DataExtractionField::TitleCellStyle(value) => {
                self.data_extraction.title_cell_style = value
            }
            DataExtractionField::HeaderCellStyle(value) => {
                self.data_extraction.header_cell_style = value
            }
            DataExtractionField::DataCellStyle(value) => {
                self.data_extraction.data_cell_style = value
            }
            DataExtractionField::PropertyHeaders(value) => {
                self.data_extraction.property_headers = value
            }
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
        let mut catalog = ObjectCatalog::new();
        let current = &self.tabs[self.active_tab].scene.document;
        match state.source {
            ExtractionSource::CurrentDrawing => collect_object_catalog(current, None, &mut catalog),
            ExtractionSource::CurrentSelection => {
                collect_object_catalog(current, Some(&selected), &mut catalog)
            }
            ExtractionSource::DrawingsAndFolders => {
                if state.include_current {
                    collect_object_catalog(current, None, &mut catalog);
                }
                for_each_extraction_document(&state, |document, _| {
                    collect_object_catalog(document, None, &mut catalog);
                })?;
            }
        }
        let mut previous = self
            .data_extraction
            .objects
            .iter()
            .map(|item| (item.name.to_ascii_lowercase(), item.checked))
            .collect::<BTreeMap<_, _>>();
        if let Some(saved) = self.data_extraction.saved_objects.take() {
            for name in catalog.keys() {
                previous.insert(
                    name.to_ascii_lowercase(),
                    saved.is_empty() || saved.contains(name),
                );
            }
        }
        let objects = objects_from_catalog(catalog, &previous);
        self.data_extraction.objects = objects;
        Ok(())
    }

    fn refresh_extraction_properties(&mut self) {
        let mut previous = self
            .data_extraction
            .properties
            .iter()
            .map(|item| (item.key.clone(), item.checked))
            .collect::<BTreeMap<_, _>>();
        if let Some(saved) = self.data_extraction.saved_properties.take() {
            for object in &self.data_extraction.objects {
                for (key, _, _) in &object.properties {
                    previous.insert(key.clone(), saved.is_empty() || saved.contains(key));
                }
            }
        }
        let (properties, categories) =
            extraction_properties(&self.data_extraction, &previous);
        self.data_extraction.categories = categories;
        self.data_extraction.properties = properties;
    }

    fn update_extraction_preview(&mut self) {
        match extraction_preview(
            &self.data_extraction,
            &self.tabs[self.active_tab].scene.document,
            self.tabs[self.active_tab].current_path.as_deref(),
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
                    .any(|value| value.checked && self.data_extraction.shows_object(value))
                {
                    self.data_extraction.error =
                        crate::t!("Select at least one object type.").into_owned();
                    return Task::none();
                }
                self.refresh_extraction_properties();
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
                    .set_title(
                        if save {
                            crate::t!("Save Data Extraction As")
                        } else {
                            crate::t!("Data Extraction Settings")
                        }
                        .as_ref(),
                    )
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
            if let Err(error) = write_extraction_file(&path, &rows) {
                self.data_extraction.error = error;
                return Task::none();
            }
            self.command_line.push_output(
                crate::tf!(
                    "External file \"{}\" was successfully created.",
                    path.display()
                )
                .as_ref(),
            );
        }
        let i = self.active_tab;
        self.active_modal = None;
        self.reset_modal_geometry();
        if self.data_extraction.output_table {
            let table = self.extraction_table(&rows);
            let settings = self.data_extraction.settings_path.trim().to_string();
            let command = crate::modules::annotate::data_link::DataLinkPlaceCommand::extraction(
                table, settings,
            );
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        } else {
            self.ribbon.deactivate_tool();
        }
        Task::none()
    }

    /// The extraction table: a title row (empty when no title was given),
    /// the column headers, then the data, in the chosen table and cell
    /// styles, each column as wide as its longest entry.
    fn extraction_table(&self, rows: &[Vec<String>]) -> codec::entities::Table {
        let state = &self.data_extraction;
        let doc = &self.tabs[self.active_tab].scene.document;
        let style = doc.objects.iter().find_map(|(handle, object)| match object {
            ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(&state.table_style) => {
                Some((*handle, style))
            }
            _ => None,
        });
        let body: Vec<Vec<String>> = if state.property_headers {
            rows.to_vec()
        } else {
            rows.iter().skip(1).cloned().collect()
        };
        let columns = body.iter().map(Vec::len).max().unwrap_or(1).max(1);
        // Title, header and data rows each use their own text style and height.
        let (text_heights, fonts, margins) = match style {
            Some((_, table_style)) => {
                let rows = [
                    &table_style.title_row_style,
                    &table_style.header_row_style,
                    &table_style.data_row_style,
                ];
                (
                    rows.map(|row| row.text_height.max(1.0e-6)),
                    rows.map(|row| {
                        crate::entities::text_support::resolve_text_style(
                            &row.text_style_name,
                            doc,
                        )
                        .font_name
                    }),
                    (table_style.horizontal_margin, table_style.vertical_margin),
                )
            }
            None => {
                let font =
                    crate::entities::text_support::resolve_text_style("Standard", doc).font_name;
                ([0.25, 0.25, 0.18], [font.clone(), font.clone(), font], (0.06, 0.06))
            }
        };
        // A text's extent: from the left of its ink or pen origin to the right
        // of its ink or pen advance, and the height of its ink.
        let extent = |text: &str, slot: usize| -> Option<(f64, f64)> {
            let bounds = crate::entities::text_support::text_local_bounds(
                &fonts[slot],
                text,
                text_heights[slot] as f32,
                1.0,
                0.0,
            )?;
            let width = bounds.advance.max(bounds.ink_max[0]) - bounds.ink_min[0].min(0.0);
            let height = (bounds.ink_max[1] - bounds.ink_min[1]).max(0.0);
            Some((f64::from(width), f64::from(height)))
        };
        let header = usize::from(state.property_headers);
        let slot_of = |row_index: usize| if row_index < header { 1 } else { 2 };
        // A row is as high as its tallest text ink, but at least a line and a
        // third of its text height, plus both vertical margins (measured:
        // Arial 4 -> 6.333 with 0.5 margins; Times 3 -> 5.034 / 5.032 by the
        // row's own letters; empty title row -> a line and a third).
        let row_height = |slot: usize, texts: &[String]| {
            let ink = texts
                .iter()
                .filter_map(|text| extent(text, slot))
                .map(|(_, height)| height)
                .fold(0.0_f64, f64::max);
            ink.max(text_heights[slot] * 4.0 / 3.0) + 2.0 * margins.1
        };
        let mut table = TableBuilder::new(body.len() + 1, columns)
            .at(Vector3::new(0.0, 0.0, 0.0))
            .row_height(row_height(2, &[]))
            .column_width(2.0)
            .build();
        table.table_style_handle = style.map(|(handle, _)| handle);
        let title = state.table_title.trim().to_string();
        table.set_cell_text(0, 0, &title);
        if columns > 1 {
            table.merge_cells(CellRange::new(0, 0, 0, columns - 1));
        }
        for (row_index, row) in body.iter().enumerate() {
            for (column_index, value) in row.iter().enumerate() {
                table.set_cell_text(row_index + 1, column_index, value);
            }
        }
        // A column is as wide as its widest text (header included) and five
        // horizontal cell margins (measured with Arial, Times, txt and simplex
        // at text heights 2.5 to 5 and margins 0.5 to 2.5: exact to 1e-4).
        for (column_index, column) in table.columns.iter_mut().enumerate() {
            let widest = body
                .iter()
                .enumerate()
                .filter_map(|(row_index, row)| {
                    row.get(column_index)
                        .and_then(|value| extent(value, slot_of(row_index)))
                })
                .map(|(width, _)| width)
                .fold(0.0_f64, f64::max);
            column.width = widest + 5.0 * margins.0;
        }
        let title_texts: &[String] = if title.is_empty() {
            &[]
        } else {
            std::slice::from_ref(&title)
        };
        let heights: Vec<f64> = std::iter::once(row_height(0, title_texts))
            .chain(
                body.iter()
                    .enumerate()
                    .map(|(row_index, row)| row_height(slot_of(row_index), row)),
            )
            .collect();
        for (row, height) in table.rows.iter_mut().zip(&heights) {
            row.height = *height;
        }
        if let Some((_, table_style)) = style {
            let names = [
                state.title_cell_style.as_str(),
                if state.property_headers {
                    state.header_cell_style.as_str()
                } else {
                    state.data_cell_style.as_str()
                },
                state.data_cell_style.as_str(),
            ];
            for (row_index, row) in table.rows.iter_mut().enumerate() {
                let slot = row_index.min(2);
                row.style = Some(crate::modules::annotate::table_cmd::selected_row_style(
                    table_style,
                    names[slot],
                ));
            }
        }
        table
    }

    /// The external file the extraction suggests: the drawing's name as a
    /// workbook beside the drawing.
    fn default_extraction_output(&self) -> String {
        let tab = &self.tabs[self.active_tab];
        let stem = tab
            .current_path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| tab.tab_display_name());
        let folder = tab
            .current_path
            .as_deref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        folder
            .join(format!("{stem}.xlsx"))
            .to_string_lossy()
            .into_owned()
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
        register_extraction_document(&key, Document::new());
        assert!(extraction_document(&key.to_ascii_uppercase()).is_some());
        assert!(extraction_document("ocs-doc-other.dwg").is_none());
    }

    #[test]
    fn workbook_metadata_bytes_rejects_non_workbooks() {
        assert!(workbook_metadata_bytes(b"not a workbook").is_err());
    }
}
