//! DWG file reader
//!
//! Reads DWG binary files and extracts structural information including
//! file headers, section metadata, and integrity checksums (CRC values).
//!
//! ## AC1021 (R2007) CRC-64 Extraction
//!
//! The AC1021 format stores a 64-bit CRC in the compressed metadata header.
//! This reader extracts and reports all CRC values, including:
//! - **Header CRC-64**: The master integrity checksum at offset 0x108
//! - **Pages Map CRC**: Checksums for the page directory
//! - **Sections Map CRC**: Checksums for the section directory
//! - **Per-page CRC**: Individual page checksums (in section map entries)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use acadrust::io::dwg::dwg_reader::DwgReader;
//!
//! let reader = DwgReader::from_file("drawing.dwg")?;
//! let info = reader.read_file_header()?;
//!
//! // Access CRC-64 from AC1021 files
//! if let Some(metadata) = &info.ac21_metadata {
//!     println!("Header CRC-64: {:#018X}", metadata.header_crc64);
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::DxfError;
use crate::io::dwg::checksum::{apply_magic_sequence, apply_mask};
use crate::io::dwg::decompressor_ac18::decompress_ac18;
use crate::io::dwg::decompressor_ac21::decompress_ac21;
use crate::io::dwg::dwg21_metadata::Dwg21CompressedMetadata;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::io::dwg::parallel::{map_ordered, worker_count};
use crate::io::dwg::reed_solomon::reed_solomon_decode;
use crate::io::read::{push_read_diagnostic, ReadDiagnostic, ReadStage, SourceFormat};
use crate::notification::{NotificationCollection, NotificationType};

fn report_read_error(
    notifications: &mut NotificationCollection,
    diagnostics: &mut Vec<ReadDiagnostic>,
    code: &str,
    stage: ReadStage,
    section: Option<&str>,
    message: String,
) {
    notifications.notify(NotificationType::Error, message.clone());
    let mut diagnostic = ReadDiagnostic::new(code, stage, message);
    diagnostic.section = section.map(str::to_owned);
    push_read_diagnostic(diagnostics, diagnostic);
}

/// AC1021 file header offset (data pages start after this)
const AC21_FILE_HEADER_SIZE: u64 = 0x480;

fn ac21_page_layout(
    compressed_size: u64,
    correction_factor: u64,
    block_size: usize,
) -> Result<(usize, usize, usize), DxfError> {
    let aligned = compressed_size.wrapping_add(7) & 0xFFFF_FFF8;
    let total_size = aligned.wrapping_mul(correction_factor) as usize;
    if total_size == 0 || total_size > 100_000_000 {
        return Err(DxfError::InvalidFormat(format!(
            "Invalid page buffer size: {} (compressed={}, factor={}, aligned={})",
            total_size, compressed_size, correction_factor, aligned
        )));
    }
    let factor = (total_size + block_size - 1) / block_size;
    Ok((total_size, factor, factor * 255))
}

fn decode_ac21_page(
    encoded: &[u8],
    total_size: usize,
    factor: usize,
    block_size: usize,
    compressed_size: u64,
    uncompressed_size: u64,
) -> Vec<u8> {
    let mut compressed_data = vec![0u8; total_size];
    reed_solomon_decode(encoded, &mut compressed_data, factor, block_size);
    if compressed_size == uncompressed_size {
        compressed_data.truncate(uncompressed_size as usize);
        return compressed_data;
    }

    let mut padded_source = vec![0u8; compressed_data.len() + 64];
    padded_source[..compressed_data.len()].copy_from_slice(&compressed_data);
    let mut decompressed_data = vec![0u8; uncompressed_size as usize + 64];
    decompress_ac21(
        &padded_source,
        0,
        compressed_size as u32,
        &mut decompressed_data,
    );
    decompressed_data.truncate(uncompressed_size as usize);
    decompressed_data
}

/// Results from reading a DWG file header.
///
/// Contains version info, section layout, and all extracted CRC values.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgFileHeaderInfo {
    /// DWG version string (e.g., "AC1021")
    pub version_string: String,
    /// Parsed DWG version enum
    pub version: DwgVersion,
    /// AutoCAD maintenance version
    pub acad_maintenance_version: u8,
    /// Preview image address
    pub preview_address: i32,
    /// DWG internal version byte
    pub dwg_version: u8,
    /// Application release version
    pub app_release_version: u8,
    /// Drawing code page
    pub code_page: u16,
    /// Security type
    pub security_type: i32,
    /// Summary info address
    pub summary_info_addr: i32,
    /// VBA project address
    pub vba_project_addr: i32,

    // ── AC1021-specific data ──
    /// AC1021 compressed metadata (contains CRC-64 and section layout)
    pub ac21_metadata: Option<Dwg21CompressedMetadata>,
    /// Raw Reed-Solomon decoded values from file header
    pub ac21_header_crc: Option<i64>,
    /// Unknown key from AC1021 header
    pub ac21_unknown_key: Option<i64>,
    /// CRC of compressed data in AC1021 header
    pub ac21_compressed_data_crc: Option<i64>,
    /// Page records: page_id → (offset, size)
    pub page_records: HashMap<i32, (i64, i64)>,
    /// Section descriptors from the section map
    pub section_descriptors: Vec<DwgSectionInfo>,

    // ── AC15-specific data ──
    /// Section locator records for AC15 format: name → (file_offset, size)
    pub section_locators: HashMap<String, (i64, i64)>,
    /// Base file offset of AcDb:AcDbObjects section (AC15 only).
    /// Handle offsets in AC15 are absolute; subtract this to get buffer-relative.
    pub objects_base_offset: i64,
    /// Whether this file uses AC18 format (R2004/R2010/R2013/R2018).
    /// Determines which decompression path to use in `get_section_buffer`.
    pub is_ac18_format: bool,
}

/// Information about a DWG section (from the section map).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgSectionInfo {
    /// Section name (e.g., "AcDb:Header")
    pub name: String,
    /// Compressed size
    pub compressed_size: u64,
    /// Decompressed size
    pub decompressed_size: u64,
    /// Encryption flag
    pub encrypted: u64,
    /// Hash code
    pub hash_code: u64,
    /// Encoding type (4 = Reed-Solomon + LZ77)
    pub encoding: u64,
    /// Number of pages
    pub page_count: u64,
    /// Per-page CRC values and metadata
    pub pages: Vec<DwgPageCrcInfo>,
}

/// CRC information for a single page within a section.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgPageCrcInfo {
    /// Page number
    pub page_number: i64,
    /// Page offset within section
    pub offset: u64,
    /// Page size
    pub size: i64,
    /// Decompressed size
    pub decompressed_size: u64,
    /// Compressed size
    pub compressed_size: u64,
    /// Checksum value
    pub checksum: u64,
    /// **CRC value for this page**
    pub crc: u64,
}

/// Options controlling how DWG files are read.
///
/// When `failsafe` is enabled the reader will attempt to recover as
/// much data as possible from damaged or partially-corrupt files
/// instead of returning an error.  Specific behaviours:
///
/// * File-header parsing errors are caught and an empty/partial
///   result is returned rather than propagating the error.
/// * Missing pages in a section are skipped instead of aborting.
/// * Skipped records and sections are reported through the
///   [`CadDocument::notifications`](crate::document::CadDocument::notifications) collection.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgReadOptions {
    /// When `true`, recover as much data as possible from corrupt files.
    pub failsafe: bool,
}

impl Default for DwgReadOptions {
    fn default() -> Self {
        Self { failsafe: false }
    }
}

impl DwgReadOptions {
    /// Create options with failsafe mode enabled.
    pub fn failsafe() -> Self {
        Self { failsafe: true }
    }
}

/// Find the first occurrence of `needle` in `haystack` at or after `from`.
fn find_subsequence(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

/// First index `>= from` where a SAB blob header magic begins — either
/// `"ACIS BinaryFile"` or `"ASM BinaryFile"`, whichever comes first. A single
/// forward scan over `buf` (both magics start with `b'A'`, used as a cheap
/// first-byte gate), so callers that advance `from` past each match walk the
/// buffer once in total rather than re-scanning for each magic separately.
fn find_acds_magic(buf: &[u8], from: usize) -> Option<usize> {
    const ACIS_MAGIC: &[u8] = b"ACIS BinaryFile";
    const ASM_MAGIC: &[u8] = b"ASM BinaryFile";
    let mut i = from;
    while i < buf.len() {
        if buf[i] == b'A' {
            let rest = &buf[i..];
            if rest.starts_with(ACIS_MAGIC) || rest.starts_with(ASM_MAGIC) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// End-of-body terminator for an Autodesk ShapeManager (ASM) SAB blob, written
/// as separate tagged tokens: `0E 03 "End" 0E 02 "of" 0E 03 "ASM" 0D 04 "data"`.
/// This is what AutoCAD 2013+ / BricsCAD emit.
const ASM_END_MARKER: &[u8] = b"\x0E\x03End\x0E\x02of\x0E\x03ASM\x0D\x04data";
/// End-of-body terminator for a classic ACIS SAB blob, written as one tagged
/// identifier string. This is what acadrust's own `SabWriter` emits, so the
/// reader must recognise it to round-trip natively-built solids (primitives and
/// the exact planar/NURBS export), not just ASM bodies read from other apps.
const ACIS_END_MARKER: &[u8] = b"End-of-ACIS-data";

/// First end-marker (ASM or classic ACIS) in `buf[from..to]`, with its length so
/// the caller can advance past the whole terminator.
///
/// `to` bounds the search: a SAB body carries only ONE of the two end-marker
/// families, so the search for the absent one would otherwise run to the end of
/// the whole (often hundreds-of-MB) AcDs buffer on every call. With thousands of
/// records that is O(records × buf) — the dominant cost of opening a 3D-heavy
/// DWG. Callers pass the tightest known upper bound (segment end, next blob, or
/// the pre-table region) so a missing marker costs one bounded scan, not a full
/// one. (#203)
fn find_acds_end(buf: &[u8], from: usize, to: usize) -> Option<(usize, usize)> {
    let hay = &buf[..to.min(buf.len())];
    let asm = find_subsequence(hay, ASM_END_MARKER, from).map(|e| (e, ASM_END_MARKER.len()));
    let acis = find_subsequence(hay, ACIS_END_MARKER, from).map(|e| (e, ACIS_END_MARKER.len()));
    match (asm, acis) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Each AcDs SAB blob paired with its owning entity handle, read from the
/// `_data_` record table(s) — authoritative regardless of blob / record / handle
/// ordering (that ordering diverges in some BIM exports, so guessing it gave one
/// solid another solid's geometry and body transform).
///
/// A record-table `_data_` segment's content (after the 48-byte header) opens
/// with a `col0 = 0x14` entry; its id is 2 in small files but a high index in
/// large multi-segment datastores, so match on that content marker, not the id,
/// and process every such segment. Each 20-byte entry is
/// `col0(0x14) idx handle data_offset`, where `data_offset` is the cumulative
/// start of the record's blob in the blob data that follows the table. Record r's
/// blob therefore lives in `[base + off[r] .. base + off[r+1]]`; locate the SAB
/// header there and read through its end marker (which can run a few bytes past
/// `off[r+1]`). Records with an empty range carry no geometry and are skipped.
/// Empty when there is no such table (the caller then falls back to order-based
/// attachment).
fn extract_acds_record_blobs(buf: &[u8], modeler_handles: &HashSet<u64>) -> Vec<(u64, Vec<u8>)> {
    let rd = |p: usize| -> Option<u32> {
        buf.get(p..p + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let marker = [0xACu8, 0xD5, 0x5F, 0x64, 0x61, 0x74, 0x61, 0x5F]; // "\xAC\xD5_data_"
    let mut out: Vec<(u64, Vec<u8>)> = Vec::new();
    // Records whose blob is not in the table's own data — handled below from the
    // pre-table blob pool. Kept in record order.
    let mut orphan_handles: Vec<u64> = Vec::new();
    let mut first_table = usize::MAX;
    // A large datastore splits its records across several `_data_` segments, so
    // process every one whose content is a record table (opens with `col0=0x14`).
    let mut scan = 0;
    while let Some(i) = buf[scan..].windows(8).position(|w| w == marker) {
        let seg = scan + i;
        scan = seg + 8;
        if rd(seg + 48) != Some(0x14) {
            continue; // e.g. the empty thumbnail `_data_` segment
        }
        first_table = first_table.min(seg);
        // Segment size (RQ at header offset 16) bounds this segment's blob data,
        // so the last record does not run into the following segment.
        let seg_size = buf
            .get(seg + 16..seg + 24)
            .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()) as usize);
        let seg_end = seg.saturating_add(seg_size).min(buf.len());
        // Record table: (handle, cumulative data offset) per record.
        let mut recs: Vec<(u64, usize)> = Vec::new();
        let mut p = seg + 48;
        while rd(p) == Some(0x14) {
            let (Some(handle), Some(off)) = (rd(p + 8), rd(p + 16)) else {
                break;
            };
            recs.push((handle as u64, off as usize));
            p += 20;
        }
        // A genuine record table is a run of consecutive 20-byte entries. A
        // single entry is a different layout — e.g. `SabWriter`'s own segment
        // interleaves a 36-byte record header before each blob, so the walk
        // stops after one; skip it (the caller then falls back to order-based
        // attachment, which round-trips those files).
        if recs.len() < 2 {
            continue;
        }
        let base = seg + 48 + recs.len() * 20; // blob data follows the table
        for k in 0..recs.len() {
            let (handle, off) = recs[k];
            let region_start = base + off;
            let region_end = recs.get(k + 1).map_or(seg_end, |&(_, o)| base + o);
            if region_start >= seg_end || region_end <= region_start {
                if modeler_handles.contains(&handle) {
                    orphan_handles.push(handle); // blob lives in the pre-table pool
                }
                continue;
            }
            let region = &buf[region_start..region_end.min(seg_end)];
            let Some(mp) = region
                .windows(14)
                .position(|w| w == b"ASM BinaryFile")
                .or_else(|| region.windows(15).position(|w| w == b"ACIS BinaryFile"))
            else {
                if modeler_handles.contains(&handle) {
                    orphan_handles.push(handle); // blob lives in the pre-table pool
                }
                continue;
            };
            let start = region_start + mp;
            // This record's blob ends at its region boundary; the end marker can
            // trail a few bytes past `off[r+1]`, so bound to `region_end` plus a
            // small slack (clamped to the segment). Bounding to `region_end`, not
            // `seg_end`, is what makes this linear: a big datastore is often ONE
            // segment (seg_end ≈ buf.len()), so a per-record scan to seg_end for
            // the absent marker family is still O(records × buf). (#203)
            let end_bound = (region_end + ASM_END_MARKER.len()).min(seg_end);
            if let Some((end, marker_len)) = find_acds_end(buf, start, end_bound) {
                out.push((handle, buf[start..end + marker_len].to_vec()));
            }
        }
    }
    // Records whose table entry carries no inline blob take theirs from a blob
    // pool that sits before the first record table. Those blobs appear there in
    // record order, so pair them one-for-one with the orphan records — but only
    // when the counts match exactly, so a stray magic can't shift the pairing.
    if !orphan_handles.is_empty() && first_table != usize::MAX {
        let mut pool: Vec<Vec<u8>> = Vec::new();
        let mut pos = 0;
        while let Some(start) = find_acds_magic(buf, pos) {
            if start >= first_table {
                break;
            }
            // Pool blobs sit before the first record table, so bound the end
            // search there rather than scanning into (or past) the tables.
            match find_acds_end(buf, start, first_table) {
                Some((end, marker_len)) => {
                    pool.push(buf[start..end + marker_len].to_vec());
                    pos = end + marker_len;
                }
                None => break,
            }
        }
        if pool.len() == orphan_handles.len() {
            out.extend(orphan_handles.into_iter().zip(pool));
        }
    }
    out
}

/// Extract every SAB (ACIS/ASM binary) blob from a decompressed AcDs section, in
/// the order they appear — the `_data_` record order, matching the handle list
/// order they appear.
///
/// Each blob runs from its header magic — `"ACIS BinaryFile"` (classic ACIS) or
/// `"ASM BinaryFile"` (Autodesk ShapeManager, AutoCAD 2013+) — through its
/// end-of-body terminator (`End-of-ASM-data` or `End-of-ACIS-data`).
fn extract_acds_sab_blobs(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut blobs = Vec::new();
    let mut pos = 0usize;
    // A single forward walk: each `find_acds_magic` resumes from `pos`, and the
    // end-marker search and `pos = stop` advance past the blob just taken, so
    // the buffer is scanned once overall — not once per blob per magic, which
    // was quadratic on 3D-heavy files with many SAB bodies (issue #203).
    while let Some(start) = find_acds_magic(buf, pos) {
        // A body's end marker precedes the next blob's header magic, so bound
        // the (possibly-absent) marker search there instead of running to the
        // end of the whole buffer on every blob — the same O(n²) trap as the
        // record-table path. (#203)
        let bound = find_acds_magic(buf, start + 1).unwrap_or(buf.len());
        match find_acds_end(buf, start, bound) {
            Some((end, marker_len)) => {
                let stop = end + marker_len;
                blobs.push(buf[start..stop].to_vec());
                pos = stop;
            }
            None => break,
        }
    }
    blobs
}

/// Remove AcDs `blob01` continuation frames embedded at 1 MiB boundaries.
///
/// Large ASM bodies are split across storage segments. The decompressed AcDs
/// byte stream places an 80-byte segment header/continuation descriptor between
/// consecutive SAB chunks:
/// `AC D5 "blob01" ... 55×8 ...`. Those bytes belong to the datastore, not the
/// SAB token stream; leaving them in makes the ACIS decoder fail at 0x0f_ffb0.
fn strip_acds_blob01_frames(blob: Vec<u8>) -> Vec<u8> {
    const PREFIX: &[u8] = b"\xAC\xD5blob01";
    const FRAME_LEN: usize = 80;
    const PADDING: [u8; 8] = [0x55; 8];
    let mut clean = Vec::with_capacity(blob.len());
    let mut cursor = 0usize;
    let mut scan = 0usize;
    while let Some(relative) = blob[scan..].windows(PREFIX.len()).position(|w| w == PREFIX) {
        let start = scan + relative;
        let valid = start + FRAME_LEN <= blob.len()
            && blob.get(start + 12..start + 16) == Some(&1u32.to_le_bytes())
            && blob.get(start + 16..start + 24) == Some(&48u64.to_le_bytes())
            && blob.get(start + 40..start + 48) == Some(&PADDING);
        if valid {
            clean.extend_from_slice(&blob[cursor..start]);
            cursor = start + FRAME_LEN;
            scan = cursor;
        } else {
            scan = start + 1;
        }
    }
    if cursor == 0 {
        return blob;
    }
    clean.extend_from_slice(&blob[cursor..]);
    clean
}

/// Fill an `AcisData` from a SAB blob and mark it binary v2.
fn acds_fill(acis: &mut crate::entities::solid3d::AcisData, blob: Vec<u8>) {
    acis.sab_data = strip_acds_blob01_frames(blob);
    acis.sat_data = String::new();
    acis.is_binary = true;
    acis.version = crate::entities::solid3d::AcisVersion::Version2;
}

/// Apply a SAB blob to a modeler entity, deriving its placement reference point
/// now that the geometry is available. Returns false for non-modeler entities.
fn acds_apply(entity: &mut crate::entities::EntityType, blob: Vec<u8>) -> bool {
    use crate::entities::EntityType;
    match entity {
        EntityType::Solid3D(s) => {
            acds_fill(&mut s.acis_data, blob);
            // The inline wireframe anchor (bbox centre) is the preferred
            // reference point; fall back to the SAB placement translation
            // only when the file carried none.
            if s.point_of_reference == crate::types::Vector3::ZERO {
                if let Some(p) = s
                    .acis_data
                    .geometry_centre()
                    .or_else(|| s.acis_data.placement_origin())
                {
                    s.point_of_reference = p;
                }
            }
            true
        }
        EntityType::Region(r) => {
            acds_fill(&mut r.acis_data, blob);
            // The inline wireframe anchor (bbox centre) is the preferred
            // reference point; fall back to the SAB placement translation
            // only when the file carried none.
            if r.point_of_reference == crate::types::Vector3::ZERO {
                if let Some(p) = r
                    .acis_data
                    .geometry_centre()
                    .or_else(|| r.acis_data.placement_origin())
                {
                    r.point_of_reference = p;
                }
            }
            true
        }
        EntityType::Body(b) => {
            acds_fill(&mut b.acis_data, blob);
            // The inline wireframe anchor (bbox centre) is the preferred
            // reference point; fall back to the SAB placement translation
            // only when the file carried none.
            if b.point_of_reference == crate::types::Vector3::ZERO {
                if let Some(p) = b
                    .acis_data
                    .geometry_centre()
                    .or_else(|| b.acis_data.placement_origin())
                {
                    b.point_of_reference = p;
                }
            }
            true
        }
        EntityType::Surface(s) => {
            acds_fill(&mut s.acis_data, blob);
            true
        }
        _ => false,
    }
}

/// Attach handle-paired AcDs SAB blobs (from [`extract_acds_record_blobs`]) to
/// their owning modeler entities — authoritative, each blob to the entity the
/// record table named. Returns the number attached.
fn attach_acds_record_blobs(
    document: &mut crate::document::CadDocument,
    record_blobs: Vec<(u64, Vec<u8>)>,
) -> usize {
    document.acis_sab_handles.clear();
    let mut attached = 0usize;
    for (h, blob) in record_blobs {
        if let Some(entity) = document.get_entity_mut(crate::Handle::new(h)) {
            if acds_apply(entity, blob) {
                attached += 1;
            }
        }
    }
    attached
}

/// Attach order-extracted AcDs SAB blobs when no record table is parseable: pair
/// by the object-stream-ordered handle list (`document.acis_sab_handles`), else
/// positionally in document order. Returns the number attached.
fn attach_acds_sab_blobs(
    document: &mut crate::document::CadDocument,
    blobs: Vec<Vec<u8>>,
) -> usize {
    use crate::entities::EntityType;

    // Fallback A: attach by the object-stream-ordered handle list.
    let ordered = std::mem::take(&mut document.acis_sab_handles);
    if !ordered.is_empty() {
        let mut attached = 0usize;
        for (handle, blob) in ordered.into_iter().zip(blobs.into_iter()) {
            if let Some(entity) = document.get_entity_mut(handle) {
                if acds_apply(entity, blob) {
                    attached += 1;
                }
            }
        }
        return attached;
    }

    // Fallback B: positional attach in document order.
    let mut it = blobs.into_iter();
    let mut attached = 0usize;
    for entity in document.entities_mut() {
        // Only consume a blob for ACIS-backed entities, in document order.
        if matches!(
            entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
        ) {
            let Some(blob) = it.next() else { break };
            if acds_apply(entity, blob) {
                attached += 1;
            }
            continue;
        }
    }
    attached
}

/// Decode a section name from a fixed 64-byte, null-terminated field.
///
/// The name ends at the first null byte. Some writers leave non-zero garbage
/// in the bytes *after* the terminator instead of zero-padding the field, so
/// trimming trailing nulls is not enough — it would keep the embedded null and
/// the junk that follows (e.g. `"AcDb:Handles\0t…"`), and the name would then
/// fail to match when looking the section up.
fn section_name_from_field(name_buf: &[u8; 64]) -> String {
    let end = name_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(name_buf.len());
    String::from_utf8_lossy(&name_buf[..end]).into_owned()
}

/// DWG file reader with CRC-64 extraction support.
///
/// Reads DWG binary files and provides access to all internal
/// integrity checksums including the AC1021 Header CRC-64.
/// Read one `T16` string (2-byte character-count prefix). R2007+ stores the
/// characters as UTF-16LE; earlier versions as one byte per character.
fn read_t16(cur: &mut &[u8], utf16: bool) -> String {
    if cur.len() < 2 {
        *cur = &[];
        return String::new();
    }
    let count = u16::from_le_bytes([cur[0], cur[1]]) as usize;
    *cur = &cur[2..];
    if utf16 {
        let bytes = count.saturating_mul(2).min(cur.len());
        let units: Vec<u16> = cur[..bytes]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        *cur = &cur[bytes..];
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        let bytes = count.min(cur.len());
        let s = String::from_utf8_lossy(&cur[..bytes]).into_owned();
        *cur = &cur[bytes..];
        s.trim_end_matches('\0').to_string()
    }
}

/// Parse the decompressed `AcDb:SummaryInfo` section (R2004+): eight fixed
/// strings, three 8-byte timers, then the custom-property pairs.
fn parse_summary_info(buf: &[u8], utf16: bool) -> crate::document::SummaryInfo {
    let mut cur: &[u8] = buf;
    let mut si = crate::document::SummaryInfo {
        title: read_t16(&mut cur, utf16),
        subject: read_t16(&mut cur, utf16),
        author: read_t16(&mut cur, utf16),
        keywords: read_t16(&mut cur, utf16),
        comments: read_t16(&mut cur, utf16),
        last_saved_by: read_t16(&mut cur, utf16),
        revision_number: read_t16(&mut cur, utf16),
        hyperlink_base: read_t16(&mut cur, utf16),
        custom_properties: Vec::new(),
    };
    // TDINDWG, TDCREATE, TDUPDATE — each a TIMERLL (2×u32 = 8 bytes).
    if cur.len() >= 24 {
        cur = &cur[24..];
    }
    if cur.len() >= 2 {
        let n = u16::from_le_bytes([cur[0], cur[1]]) as usize;
        cur = &cur[2..];
        for _ in 0..n.min(256) {
            let tag = read_t16(&mut cur, utf16);
            let val = read_t16(&mut cur, utf16);
            if tag.is_empty() && val.is_empty() {
                break;
            }
            si.custom_properties.push((tag, val));
        }
    }
    si
}

pub struct DwgReader<R: Read + Seek> {
    stream: R,
    /// Options controlling read behaviour.
    pub options: DwgReadOptions,
    /// Notifications collected during reading
    pub notifications: NotificationCollection,
    /// Source path, when opened from a file — copied onto the document so the
    /// `Filename` / `FilePath` fields can resolve.
    source_path: Option<String>,
    /// Optional monotonic read progress callback. The value is in 0..=1000 so
    /// callers can map DWG parsing into a larger file-open pipeline without
    /// coupling this crate to a UI or an async runtime.
    progress: Option<std::sync::Arc<dyn Fn(u16) + Send + Sync>>,
}

impl DwgReader<File> {
    /// Open a DWG file from a filesystem path.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, DxfError> {
        Self::from_file_with_options(path, DwgReadOptions::default())
    }

    /// Open a DWG file from a filesystem path with custom options.
    pub fn from_file_with_options<P: AsRef<Path>>(
        path: P,
        options: DwgReadOptions,
    ) -> Result<Self, DxfError> {
        let path = path.as_ref();
        let file = File::open(path)?;
        Ok(Self {
            stream: file,
            options,
            notifications: NotificationCollection::new(),
            source_path: Some(path.to_string_lossy().into_owned()),
            progress: None,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DwgReader<Cursor<memmap2::Mmap>> {
    /// Open through a read-only memory map. Section seeks then become pointer
    /// moves and the OS can page/readahead the file without repeated syscalls.
    pub fn from_mmap<P: AsRef<Path>>(path: P) -> Result<Self, DxfError> {
        let path = path.as_ref();
        let file = File::open(path)?;
        // SAFETY: read-only map owns its file-backed virtual-memory mapping;
        // this reader never mutates the file while the mapping is alive.
        let map = unsafe { memmap2::MmapOptions::new().map(&file)? };
        Ok(Self {
            stream: Cursor::new(map),
            options: DwgReadOptions::default(),
            notifications: NotificationCollection::new(),
            source_path: Some(path.to_string_lossy().into_owned()),
            progress: None,
        })
    }
}

impl<R: Read + Seek> DwgReader<R> {
    /// Create a reader from any seekable stream.
    pub fn from_stream(stream: R) -> Self {
        Self {
            stream,
            options: DwgReadOptions::default(),
            notifications: NotificationCollection::new(),
            source_path: None,
            progress: None,
        }
    }

    /// Create a reader from any seekable stream with custom options.
    pub fn from_stream_with_options(stream: R, options: DwgReadOptions) -> Self {
        Self {
            stream,
            options,
            notifications: NotificationCollection::new(),
            source_path: None,
            progress: None,
        }
    }

    /// Receive monotonic parsing progress in the inclusive range 0..=1000.
    ///
    /// The callback may run from Rayon worker threads while object records are
    /// decoded, so it must be thread-safe and should only perform cheap atomic
    /// updates.
    pub fn set_progress_callback(&mut self, progress: std::sync::Arc<dyn Fn(u16) + Send + Sync>) {
        self.progress = Some(progress);
    }

    #[inline]
    fn report_progress(&self, value: u16) {
        if let Some(progress) = &self.progress {
            progress(value.min(1000));
        }
    }

    /// Read the DWG file and reconstruct a `CadDocument`.
    ///
    /// This is the main entry point for reading a DWG file into a usable
    /// document. It orchestrates:
    /// 1. File header parsing
    /// 2. Section buffer extraction (Classes, Header, Handles, Objects)
    /// 3. Classes, header variables, and handle map parsing
    /// 4. Object dispatch and entity/object mapping via `DwgDocumentBuilder`
    ///
    /// In failsafe mode, file-header errors are caught and a partial
    /// document is returned instead of propagating the error.
    pub fn read(&mut self) -> std::result::Result<crate::document::CadDocument, DxfError> {
        self.read_with_stats().map(|outcome| outcome.document)
    }

    /// Read the file and return the document with source/decode statistics.
    pub fn read_with_stats(
        &mut self,
    ) -> std::result::Result<crate::io::read::ReadOutcome, DxfError> {
        let failsafe = self.options.failsafe;
        let perf = std::env::var_os("PERF").is_some();
        let total_started = web_time::Instant::now();
        let mut diagnostics = Vec::new();
        self.report_progress(0);

        // 1. Read the DWG file header and section map
        let stage_started = web_time::Instant::now();
        let info = match self.read_file_header() {
            Ok(info) => info,
            Err(e) if failsafe => {
                report_read_error(
                    &mut self.notifications,
                    &mut diagnostics,
                    "file-header-read-failed",
                    ReadStage::FileHeader,
                    None,
                    format!(
                        "Failsafe: file header read failed, returning partial document: {}",
                        e
                    ),
                );
                let mut doc = crate::document::CadDocument::default();
                doc.notifications
                    .extend(std::mem::take(&mut self.notifications));
                let stats = crate::io::read::ReadStats::from_document(
                    &doc,
                    SourceFormat::Dwg,
                    0,
                    0,
                    0,
                    0,
                    false,
                    true,
                    false,
                    diagnostics,
                );
                return Ok(crate::io::read::ReadOutcome::new(doc, stats));
            }
            Err(e) => return Err(e),
        };
        if perf {
            eprintln!(
                "[perf] dwg-read header={:.1}ms sections={} pages={}",
                stage_started.elapsed().as_secs_f64() * 1000.0,
                info.section_descriptors.len(),
                info.page_records.len(),
            );
        }
        let dxf_version = crate::types::DxfVersion::parse(&info.version_string)
            .unwrap_or(crate::types::DxfVersion::Unknown);
        self.report_progress(20);
        let mut document = crate::document::CadDocument::with_version(dxf_version);
        document.maintenance_version = info.acad_maintenance_version;
        document.dwg_source_version = Some(dxf_version);

        // 2. Read Classes (AcDb:Classes)
        match self.get_section_buffer("AcDb:Classes", &info) {
            Ok(classes_buf) => {
                match crate::io::dwg::dwg_stream_readers::classes_reader::read_classes_with_encoding(
                    &classes_buf,
                    dxf_version,
                    info.acad_maintenance_version,
                    crate::io::dxf::code_page::encoding_from_dwg_code_page(info.code_page),
                ) {
                    Ok(classes) => document.classes = classes,
                    Err(e) => report_read_error(
                        &mut self.notifications,
                        &mut diagnostics,
                        "classes-decode-failed",
                        ReadStage::Classes,
                        Some("AcDb:Classes"),
                        format!("Failed to read classes: {}", e),
                    ),
                }
            }
            Err(e) => report_read_error(
                &mut self.notifications,
                &mut diagnostics,
                "classes-extract-failed",
                ReadStage::Classes,
                Some("AcDb:Classes"),
                format!("Failed to extract classes section: {}", e),
            ),
        }
        self.report_progress(30);

        // 3. Read Header Variables (AcDb:Header)
        match self.get_section_buffer("AcDb:Header", &info) {
            Ok(header_buf) => {
                match crate::io::dwg::dwg_stream_readers::header_reader::read_header_with_encoding(
                    &header_buf,
                    dxf_version,
                    info.acad_maintenance_version,
                    crate::io::dxf::code_page::encoding_from_dwg_code_page(info.code_page),
                ) {
                    Ok(header_vars) => document.header = header_vars,
                    Err(e) => report_read_error(
                        &mut self.notifications,
                        &mut diagnostics,
                        "header-decode-failed",
                        ReadStage::Header,
                        Some("AcDb:Header"),
                        format!("Failed to read header: {}", e),
                    ),
                }
            }
            Err(e) => report_read_error(
                &mut self.notifications,
                &mut diagnostics,
                "header-extract-failed",
                ReadStage::Header,
                Some("AcDb:Header"),
                format!("Failed to extract header section: {}", e),
            ),
        }
        document.header.code_page =
            crate::io::dxf::code_page::dwg_code_page_name(info.code_page).to_string();
        self.report_progress(40);

        // 4. Read Handle Map (AcDb:Handles)
        let mut handles_section_read = false;
        let handle_map = match self.get_section_buffer("AcDb:Handles", &info) {
            Ok(handle_buf) => {
                handles_section_read = true;
                match crate::io::dwg::dwg_stream_readers::handle_reader::read_handles(&handle_buf) {
                    Ok(mut hm) => {
                        // AC15: Handle offsets are absolute file positions.
                        // Convert to buffer-relative by subtracting the objects
                        // section base offset.
                        if info.objects_base_offset != 0 {
                            let base = info.objects_base_offset;
                            for offset in hm.values_mut() {
                                *offset -= base;
                            }
                        }
                        hm
                    }
                    Err(e) => {
                        report_read_error(
                            &mut self.notifications,
                            &mut diagnostics,
                            "handles-decode-failed",
                            ReadStage::Handles,
                            Some("AcDb:Handles"),
                            format!("Failed to read handles: {}", e),
                        );
                        std::collections::HashMap::new()
                    }
                }
            }
            Err(e) => {
                report_read_error(
                    &mut self.notifications,
                    &mut diagnostics,
                    "handles-extract-failed",
                    ReadStage::Handles,
                    Some("AcDb:Handles"),
                    format!("Failed to extract handles section: {}", e),
                );
                std::collections::HashMap::new()
            }
        };
        self.report_progress(50);

        // 5. Read Objects (AcDb:AcDbObjects) and build document
        let handle_count = handle_map.len();
        let mut objects_section_read = false;
        let mut record_stream_read = false;
        let mut decoded_source_records = 0usize;
        let mut skipped_source_records = 0usize;
        let objects_started = web_time::Instant::now();
        if !handle_map.is_empty() {
            match self.get_section_buffer("AcDb:AcDbObjects", &info) {
                Ok(objects_buf) => {
                    objects_section_read = true;
                    match crate::io::dwg::dwg_stream_readers::object_reader::DwgObjectReader::with_encoding(
                    objects_buf,
                    dxf_version,
                    handle_map,
                    crate::io::dxf::code_page::encoding_from_dwg_code_page(info.code_page),
                ) {
                    Ok(obj_reader) => {
                        record_stream_read = true;
                        let mut builder = crate::io::dwg::dwg_document_builder::DwgDocumentBuilder::new(obj_reader);
                        builder.set_failsafe(failsafe);
                        if let Some(progress) = &self.progress {
                            builder.set_progress_callback(progress.clone());
                        }
                        let build_outcome = builder.build_with_stats(&mut document);
                        decoded_source_records = build_outcome.decoded_records;
                        skipped_source_records = build_outcome.skipped_records;
                        diagnostics.extend(build_outcome.diagnostics);
                        self.notifications.extend(build_outcome.notifications);
                    },
                    Err(e) => report_read_error(
                        &mut self.notifications,
                        &mut diagnostics,
                        "objects-init-failed",
                        ReadStage::Objects,
                        Some("AcDb:AcDbObjects"),
                        format!("Failed to init object reader: {}", e),
                    ),
                    }
                }
                Err(e) => report_read_error(
                    &mut self.notifications,
                    &mut diagnostics,
                    "objects-extract-failed",
                    ReadStage::Objects,
                    Some("AcDb:AcDbObjects"),
                    format!("Failed to extract objects section: {}", e),
                ),
            }
        }
        self.report_progress(930);
        if perf {
            eprintln!(
                "[perf] dwg-read objects={:.1}ms handles={} entities={} objects={}",
                objects_started.elapsed().as_secs_f64() * 1000.0,
                handle_count,
                document.entities().count(),
                document.objects.len(),
            );
        }

        // 6. R2013+ (AC1027+): 3DSOLID / REGION / BODY ACIS geometry is not
        //    stored inline — it lives as SAB blobs in the AcDs (Autodesk Data
        //    Store) section. Extract those blobs and attach them, in document
        //    order, to the modeler-geometry entities that arrived with only a
        //    stub. Files without an AcDs section keep their inline data.
        if let Ok(acds_buf) = self.get_section_buffer("AcDb:AcDsPrototype_1b", &info) {
            // Authoritative: the `_data_` record table(s) bind each blob to its
            // owning handle. When present, attach by handle — the only mapping
            // that survives BIM exports whose blob/record/handle orders diverge.
            let modeler_handles: HashSet<u64> = document
                .entities()
                .filter(|entity| {
                    matches!(
                        entity,
                        crate::entities::EntityType::Solid3D(_)
                            | crate::entities::EntityType::Region(_)
                            | crate::entities::EntityType::Body(_)
                            | crate::entities::EntityType::Surface(_)
                    ) && entity.common().has_ds_data
                })
                .map(|entity| entity.common().handle.value())
                .collect();
            let record_blobs = extract_acds_record_blobs(&acds_buf, &modeler_handles);
            let attached = if !record_blobs.is_empty() {
                attach_acds_record_blobs(&mut document, record_blobs)
            } else {
                // No parseable record table: magic-scan the blobs and pair by
                // the object-stream handle list, else positionally.
                let blobs = extract_acds_sab_blobs(&acds_buf);
                if blobs.is_empty() {
                    0
                } else {
                    attach_acds_sab_blobs(&mut document, blobs)
                }
            };
            let fingerprint = super::sab_fingerprint(document.entities().filter_map(|entity| {
                let acis = match entity {
                    crate::entities::EntityType::Solid3D(entity) => &entity.acis_data,
                    crate::entities::EntityType::Region(entity) => &entity.acis_data,
                    crate::entities::EntityType::Body(entity) => &entity.acis_data,
                    crate::entities::EntityType::Surface(entity) => &entity.acis_data,
                    _ => return None,
                };
                (!acis.sab_data.is_empty())
                    .then_some((entity.common().handle, acis.sab_data.as_slice()))
            }));
            document.raw_acds_data = Some(std::sync::Arc::new(acds_buf));
            document.raw_acds_fingerprint = fingerprint;
            if attached > 0 {
                self.notifications.notify(
                    NotificationType::Warning,
                    format!(
                        "AcDs: attached {} SAB blob(s) to modeler entities",
                        attached
                    ),
                );
            }
        }
        self.report_progress(970);

        // 7. Preview / thumbnail image. Stored uncompressed at the raw file
        //    offset in the header's preview seeker (all versions), wrapped in a
        //    sentinel-bracketed container. Best-effort: a malformed or absent
        //    preview simply leaves `document.preview` as `None`.
        if info.preview_address > 0 {
            let base = info.preview_address as u64;
            if self.stream.seek(SeekFrom::Start(base)).is_ok() {
                let mut head = [0u8; 20];
                if self.stream.read_exact(&mut head).is_ok() {
                    if let Some(overall) = crate::io::dwg::preview::overall_size(&head) {
                        // Guard against a garbage size before allocating.
                        if overall > 0 && overall < 64 * 1024 * 1024 {
                            let total = crate::io::dwg::preview::container_len(overall);
                            let mut buf = vec![0u8; total];
                            if self.stream.seek(SeekFrom::Start(base)).is_ok()
                                && self.stream.read_exact(&mut buf).is_ok()
                            {
                                document.preview =
                                    crate::io::dwg::preview::parse_preview(&buf, base);
                            }
                        }
                    }
                }
            }
        }

        // Record the source path (Filename / FilePath fields) when opened from a
        // file rather than a bare stream.
        document.source_path = self.source_path.clone();

        // Document summary information (Author/Title/Subject/… → the
        // Document-category dynamic-text fields).
        if let Ok(buf) = self.get_section_buffer("AcDb:SummaryInfo", &info) {
            let utf16 = crate::io::dwg::dwg_version::DwgVersion::from_dxf_version(dxf_version)
                .map(|v| v.r2007_plus())
                .unwrap_or(true);
            document.summary_info = parse_summary_info(&buf, utf16);
        }

        // R2000/R14 down-saved gradient hatches store their gradient in the
        // ACAD round-trip mechanism, not the object stream (the DWG gradient
        // block is R2004+): the two colours in EED (GradientColor1ACI /
        // GradientColor2ACI) and the gradient type in an ACAD_XREC_ROUNDTRIP
        // XRecord under the hatch's extension dictionary. Reconstruct the
        // gradient so it renders as AutoCAD/ODA show it instead of a flat
        // solid fill.
        //
        // Gate strictly to pre-R2004: a native R2004+ file carries the real
        // gradient in its hatch object (read_hatch reads `is_gradient` and the
        // colours directly), so that flag is authoritative. Such hatches often
        // ALSO keep stale GradientColor1/2ACI round-trip EED from an earlier
        // edit; recovering from it would resurrect a gradient the drawing has
        // since turned off (native `is_gradient` = 0), painting a solid-fill
        // hatch as a spurious gradient. Only R2000/R14, whose object stream has
        // no gradient block, need the round-trip fallback.
        let pre_r2004 = crate::io::dwg::dwg_version::DwgVersion::from_dxf_version(dxf_version)
            .map(|v| !v.r2004_plus())
            .unwrap_or(false);
        if pre_r2004 {
            recover_roundtrip_gradients(&mut document);
            // Pre-R2004 stores an MTEXT background fill as round-trip EED
            // instead of the native codes (dimension text fills, etc.).
            recover_mtext_bg_roundtrip(&mut document);
        }

        // 7. Preview / thumbnail image. Stored uncompressed at the raw file
        //    offset in the header's preview seeker (all versions), wrapped in a
        //    sentinel-bracketed container. Best-effort: a malformed or absent
        //    preview simply leaves `document.preview` as `None`.
        if info.preview_address > 0 {
            let base = info.preview_address as u64;
            if self.stream.seek(SeekFrom::Start(base)).is_ok() {
                let mut head = [0u8; 20];
                if self.stream.read_exact(&mut head).is_ok() {
                    if let Some(overall) = crate::io::dwg::preview::overall_size(&head) {
                        // Guard against a garbage size before allocating.
                        if overall > 0 && overall < 64 * 1024 * 1024 {
                            let total = crate::io::dwg::preview::container_len(overall);
                            let mut buf = vec![0u8; total];
                            if self.stream.seek(SeekFrom::Start(base)).is_ok()
                                && self.stream.read_exact(&mut buf).is_ok()
                            {
                                document.preview =
                                    crate::io::dwg::preview::parse_preview(&buf, base);
                            }
                        }
                    }
                }
            }
        }

        // Record the source path (Filename / FilePath fields) when opened from a
        // file rather than a bare stream.
        document.source_path = self.source_path.clone();

        // Document summary information (Author/Title/Subject/… → the
        // Document-category dynamic-text fields).
        if let Ok(buf) = self.get_section_buffer("AcDb:SummaryInfo", &info) {
            let utf16 = crate::io::dwg::dwg_version::DwgVersion::from_dxf_version(dxf_version)
                .map(|v| v.r2007_plus())
                .unwrap_or(true);
            document.summary_info = parse_summary_info(&buf, utf16);
        }

        // Transfer reader notifications to the document so callers can
        // inspect them via `document.notifications`.
        document
            .notifications
            .extend(std::mem::take(&mut self.notifications));

        if perf {
            eprintln!(
                "[perf] dwg-read total={:.1}ms entities={} objects={} classes={}",
                total_started.elapsed().as_secs_f64() * 1000.0,
                document.entities().count(),
                document.objects.len(),
                document.classes.iter().count(),
            );
        }
        self.report_progress(1000);
        let source_sections =
            usize::from(handles_section_read).saturating_add(usize::from(objects_section_read));
        let stats = crate::io::read::ReadStats::from_document(
            &document,
            SourceFormat::Dwg,
            source_sections,
            handle_count,
            decoded_source_records,
            skipped_source_records,
            record_stream_read,
            failsafe,
            true,
            diagnostics,
        );
        Ok(crate::io::read::ReadOutcome::new(document, stats))
    }

    /// Read the file header and extract all CRC values.
    ///
    /// For AC1021 files, this extracts:
    /// - The Header CRC-64 from the compressed metadata
    /// - Page map CRC values
    /// - Section map CRC values
    /// - Per-page CRC values
    ///
    /// # Returns
    /// A `DwgFileHeaderInfo` containing all extracted data.
    pub fn read_file_header(&mut self) -> Result<DwgFileHeaderInfo, DxfError> {
        self.stream.seek(SeekFrom::Start(0))?;

        // Read version string (6 bytes)
        let mut version_buf = [0u8; 6];
        self.stream.read_exact(&mut version_buf)?;
        let version_string = String::from_utf8_lossy(&version_buf).to_string();

        let version = DwgVersion::from_version_string(&version_string)
            .ok_or_else(|| DxfError::UnsupportedVersion(version_string.clone()))?;

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "Reading DWG file version: {} ({:?})",
                version_string, version
            ),
        );

        let mut info = DwgFileHeaderInfo {
            version_string,
            version,
            acad_maintenance_version: 0,
            preview_address: 0,
            dwg_version: 0,
            app_release_version: 0,
            code_page: 0,
            security_type: 0,
            summary_info_addr: 0,
            vba_project_addr: 0,
            ac21_metadata: None,
            ac21_header_crc: None,
            ac21_unknown_key: None,
            ac21_compressed_data_crc: None,
            page_records: HashMap::new(),
            section_descriptors: Vec::new(),
            section_locators: HashMap::new(),
            objects_base_offset: 0,
            is_ac18_format: false,
        };

        match version {
            DwgVersion::AC21 => {
                self.read_file_metadata(&mut info)?;
                self.read_file_header_ac21(&mut info)?;
            }
            DwgVersion::AC18 | DwgVersion::AC24 => {
                self.read_file_metadata(&mut info)?;
                self.read_file_header_ac18(&mut info)?;
            }
            _ => {
                // AC15 format (R13/R14/R2000) — linear file with section locator records
                self.read_file_header_ac15(&mut info)?;
            }
        }

        Ok(info)
    }

    /// Read common file metadata shared between AC18 and AC21 formats.
    ///
    /// This reads bytes 6–0xFF of the file (after the version string).
    fn read_file_metadata(&mut self, info: &mut DwgFileHeaderInfo) -> Result<(), DxfError> {
        // Skip 5 bytes after version string
        let mut skip = [0u8; 5];
        self.stream.read_exact(&mut skip)?;

        // Maintenance version (1 byte)
        info.acad_maintenance_version = self.stream.read_u8()?;

        // Skip 1 byte
        self.stream.read_exact(&mut [0u8; 1])?;

        // Preview address (4 bytes)
        info.preview_address = self.stream.read_i32::<LittleEndian>()?;

        // DWG version (1 byte)
        info.dwg_version = self.stream.read_u8()?;

        // App release version (1 byte)
        info.app_release_version = self.stream.read_u8()?;

        // Drawing code page (2 bytes)
        info.code_page = self.stream.read_u16::<LittleEndian>()?;

        // Skip 3 bytes
        self.stream.read_exact(&mut [0u8; 3])?;

        // Security type (4 bytes)
        info.security_type = self.stream.read_i32::<LittleEndian>()?;

        // Skip unknown (4 bytes)
        self.stream.read_i32::<LittleEndian>()?;

        // Summary info address (4 bytes)
        info.summary_info_addr = self.stream.read_i32::<LittleEndian>()?;

        // VBA project address (4 bytes)
        info.vba_project_addr = self.stream.read_i32::<LittleEndian>()?;

        // Skip 2 unknown ints (8 bytes)
        self.stream.read_i32::<LittleEndian>()?;
        self.stream.read_i32::<LittleEndian>()?;

        // Skip 80 bytes of padding/unknown data
        let mut pad = [0u8; 80];
        self.stream.read_exact(&mut pad)?;

        Ok(())
    }

    /// Read AC15 (R13/R14/R2000) file header with section locator records.
    ///
    /// The AC15 file header is 0x61 (97) bytes:
    /// ```text
    /// [0x00] Version string (6 bytes)
    /// [0x06] Padding + maintenance version (7 bytes)
    /// [0x0D] Preview seeker (4 bytes)
    /// [0x11] Magic bytes (2 bytes: 0x1B, 0x19)
    /// [0x13] Code page (2 bytes LE)
    /// [0x15] Record count (4 bytes LE) — always 6
    /// [0x19] 6 × Section locator records (9 bytes each)
    /// [0x4F] CRC-16 (2 bytes)
    /// [0x51] End sentinel (16 bytes)
    /// [0x61] End of header → section data starts
    /// ```
    ///
    /// Section numbers: 0=Header, 1=Classes, 2=Handles,
    /// 3=ObjFreeSpace, 4=Template, 5=AuxHeader.
    /// AcDbObjects is not in the locator table — its position is
    /// inferred from the gap between the Classes section end and the
    /// Handles section start (see the calculation below for why the
    /// AuxHeader position is only used as a legacy fallback).
    fn read_file_header_ac15(&mut self, info: &mut DwgFileHeaderInfo) -> Result<(), DxfError> {
        use crate::io::dwg::file_headers::section_definition::names;

        // Seek past the version string (6 bytes already read)
        self.stream.seek(SeekFrom::Start(6))?;

        // 0x06: 5 zero bytes + maintenance version + 1 unknown byte (7 bytes total)
        let mut pad = [0u8; 5];
        self.stream.read_exact(&mut pad)?;
        info.acad_maintenance_version = self.stream.read_u8()?;
        let _unknown = self.stream.read_u8()?;

        // 0x0D: Preview seeker (4 bytes LE)
        info.preview_address = self.stream.read_i32::<LittleEndian>()?;

        // 0x11: Magic bytes (2 bytes)
        let _magic1 = self.stream.read_u8()?;
        let _magic2 = self.stream.read_u8()?;

        // 0x13: Code page (2 bytes LE)
        info.code_page = self.stream.read_u16::<LittleEndian>()?;

        // 0x15: Number of locator records (4 bytes LE) — should be 6
        let record_count = self.stream.read_i32::<LittleEndian>()?;

        // 0x19: Read locator records
        // Each record: number(1) + seeker(4) + size(4) = 9 bytes
        let section_name_for = |n: u8| -> &str {
            match n {
                0 => names::HEADER,
                1 => names::CLASSES,
                2 => names::HANDLES,
                3 => names::OBJ_FREE_SPACE,
                4 => names::TEMPLATE,
                5 => names::AUX_HEADER,
                _ => "Unknown",
            }
        };

        let mut handles_seeker: i64 = 0;
        let mut aux_header_end: i64 = 0;
        let mut classes_end: i64 = 0;

        for _ in 0..record_count.min(6) {
            let number = self.stream.read_u8()?;
            let seeker = self.stream.read_i32::<LittleEndian>()? as i64;
            let size = self.stream.read_i32::<LittleEndian>()? as i64;

            let name = section_name_for(number);
            info.section_locators
                .insert(name.to_string(), (seeker, size));

            // Track offsets for AcDbObjects position calculation
            if number == 1 {
                // Classes section
                classes_end = seeker + size;
            }
            if number == 2 {
                // Handles section
                handles_seeker = seeker;
            }
            if number == 5 {
                // AuxHeader — objects start right after this
                aux_header_end = seeker + size;
            }
        }

        // Calculate AcDbObjects position.
        //
        // AcDbObjects is not in the locator table. It occupies the gap between
        // the end of the Classes section and the start of the Handles section,
        // regardless of where Template/AuxHeader are physically placed: many
        // real-world R2000 files store Template/AuxHeader *after* Handles (and
        // acadrust's own R13/R14 writer places ObjFreeSpace/Template after
        // Handles too), so inferring the region from the AuxHeader end yields
        // a negative size and an empty document (issue #55).
        //
        // Handle-map offsets are absolute file positions, so starting the
        // region at classes_end is always safe: the buffer may then include a
        // few leading non-object bytes (e.g. ObjFreeSpace/Template), but those
        // are never referenced by any handle offset.
        let objects_start: i64;
        let objects_size: i64;
        if classes_end > 0 && handles_seeker > classes_end {
            objects_start = classes_end;
            objects_size = handles_seeker - classes_end;
        } else {
            // Malformed Classes locator — fall back to the legacy inference:
            // objects start right after the AuxHeader, or, when AuxHeader is
            // missing (R13/R14 layout), at 0x61 + the sum of all known
            // sections that precede it.
            if aux_header_end == 0 {
                let file_header_size: i64 = 0x61;
                let mut offset = file_header_size;
                for &sect in &[
                    names::HEADER,
                    names::CLASSES,
                    names::OBJ_FREE_SPACE,
                    names::TEMPLATE,
                    names::AUX_HEADER,
                ] {
                    if let Some(&(_, size)) = info.section_locators.get(sect) {
                        offset += size;
                    }
                }
                aux_header_end = offset;
            }
            objects_start = aux_header_end;
            objects_size = handles_seeker - aux_header_end;
        }

        if objects_size > 0 {
            info.section_locators.insert(
                names::ACDB_OBJECTS.to_string(),
                (objects_start, objects_size),
            );
            info.objects_base_offset = objects_start;
        }

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC15 file header: {} locator records, objects at offset {}, size {}",
                record_count, objects_start, objects_size
            ),
        );

        Ok(())
    }

    /// Read AC18 (R2004/R2010/R2013/R2018) inner file header, page map, and section map.
    ///
    /// The AC18 format stores a 0x6C-byte inner file header at file offset 0x80,
    /// XOR'd with a magic sequence. This header contains pointers to the page map
    /// and section map, which together describe the layout of all section pages.
    fn read_file_header_ac18(&mut self, info: &mut DwgFileHeaderInfo) -> Result<(), DxfError> {
        // Read the 0x6C-byte inner file header at offset 0x80
        self.stream.seek(SeekFrom::Start(0x80))?;
        let mut inner = [0u8; 0x6C];
        self.stream.read_exact(&mut inner)?;

        // XOR unmask with magic sequence
        apply_magic_sequence(&mut inner);

        // Verify identifier "AcFssFcAJMB\0"
        if &inner[..12] != b"AcFssFcAJMB\0" {
            return Err(DxfError::InvalidFormat(
                "Invalid AC18 inner file header identifier".into(),
            ));
        }

        // Parse inner file header fields
        let mut cursor = Cursor::new(&inner[..]);
        cursor.set_position(0x28);
        let _last_page_id = cursor.read_i32::<LittleEndian>()?;
        let _last_section_addr = cursor.read_u64::<LittleEndian>()?;
        let _second_header_addr = cursor.read_u64::<LittleEndian>()?;
        let _gap_amount = cursor.read_u32::<LittleEndian>()?;
        let _section_amount = cursor.read_u32::<LittleEndian>()?;

        cursor.set_position(0x50);
        let _section_page_map_id = cursor.read_u32::<LittleEndian>()?;
        let page_map_address_stored = cursor.read_u64::<LittleEndian>()?;
        let section_map_id = cursor.read_u32::<LittleEndian>()?;

        // The stored address is (actual - 0x100)
        let page_map_address = page_map_address_stored + 0x100;

        info.is_ac18_format = true;

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC18 inner header: page_map_address={:#X}, section_map_id={}",
                page_map_address, section_map_id
            ),
        );

        // Read page map (page_number → file_offset mapping)
        self.read_page_map_ac18(info, page_map_address)?;

        // Read section map (section descriptors with per-page info)
        self.read_section_map_ac18(info, section_map_id as i32)?;

        Ok(())
    }

    /// Read the AC18 page map from a known file offset.
    ///
    /// The page map is a system page (20-byte unmasked header + LZ77 data)
    /// containing (page_number, page_size) pairs. File offsets are computed
    /// by accumulating page sizes from offset 0x100.
    fn read_page_map_ac18(
        &mut self,
        info: &mut DwgFileHeaderInfo,
        page_map_address: u64,
    ) -> Result<(), DxfError> {
        self.stream.seek(SeekFrom::Start(page_map_address))?;

        // Read 20-byte system page header (NOT XOR-masked)
        let _section_type = self.stream.read_i32::<LittleEndian>()?;
        let decomp_size = self.stream.read_i32::<LittleEndian>()?;
        let comp_size = self.stream.read_i32::<LittleEndian>()?;
        let compression = self.stream.read_i32::<LittleEndian>()?;
        let _checksum = self.stream.read_u32::<LittleEndian>()?;

        // Read compressed data
        if comp_size <= 0 || comp_size > 10_000_000 {
            return Err(DxfError::InvalidFormat(format!(
                "Invalid AC18 page map compressed size: {}",
                comp_size
            )));
        }
        let mut compressed = vec![0u8; comp_size as usize];
        self.stream.read_exact(&mut compressed)?;

        // Decompress
        let decompressed = if compression == 2 {
            decompress_ac18(&compressed, decomp_size as usize)
        } else {
            compressed
        };

        // Parse (page_number, page_size) pairs.
        // Pages are written sequentially starting at file offset 0x100.
        let mut cursor = Cursor::new(&decompressed);
        let mut file_offset: i64 = 0x100;

        while (cursor.position() as usize) + 8 <= decompressed.len() {
            let page_number = cursor.read_i32::<LittleEndian>()?;
            let page_size = cursor.read_i32::<LittleEndian>()?;

            // Entries with page_size <= 0 are alignment/padding markers
            // emitted by AutoCAD; skip them so they don't corrupt the
            // running file offset.
            if page_size <= 0 {
                continue;
            }

            if page_number > 0 {
                info.page_records
                    .insert(page_number, (file_offset, page_size as i64));
            }
            // Only advance for positive sizes; negative/zero sizes in gap entries are
            // invalid and must not corrupt subsequent page offsets.
            if page_size > 0 {
                file_offset += page_size as i64;
            }
        }

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC18: Read {} page records from page map",
                info.page_records.len()
            ),
        );

        Ok(())
    }

    /// Read the AC18 section map from a system page.
    ///
    /// The section map page describes all logical sections (Header, Classes,
    /// Handles, Objects, etc.) with their compression settings and per-page info.
    fn read_section_map_ac18(
        &mut self,
        info: &mut DwgFileHeaderInfo,
        section_map_id: i32,
    ) -> Result<(), DxfError> {
        let &(page_offset, _) = info.page_records.get(&section_map_id).ok_or_else(|| {
            DxfError::InvalidFormat(format!(
                "AC18 section map page {} not found in page records",
                section_map_id
            ))
        })?;

        self.stream.seek(SeekFrom::Start(page_offset as u64))?;

        // Read 20-byte system page header (NOT XOR-masked)
        let _section_type = self.stream.read_i32::<LittleEndian>()?;
        let decomp_size = self.stream.read_i32::<LittleEndian>()?;
        let comp_size = self.stream.read_i32::<LittleEndian>()?;
        let compression = self.stream.read_i32::<LittleEndian>()?;
        let _checksum = self.stream.read_u32::<LittleEndian>()?;

        // Read compressed data
        if comp_size <= 0 || comp_size > 10_000_000 {
            return Err(DxfError::InvalidFormat(format!(
                "Invalid AC18 section map compressed size: {}",
                comp_size
            )));
        }
        let mut compressed = vec![0u8; comp_size as usize];
        self.stream.read_exact(&mut compressed)?;

        // Decompress
        let decompressed = if compression == 2 {
            decompress_ac18(&compressed, decomp_size as usize)
        } else {
            compressed
        };

        // Parse section descriptors
        let mut cursor = Cursor::new(&decompressed);

        // Header: numDescriptions(4), 0x02(4), 0x7400(4), 0x00(4), numDescriptions(4)
        let num_descriptions = cursor.read_i32::<LittleEndian>()?;
        let _marker = cursor.read_i32::<LittleEndian>()?; // 0x02
        let _max_decomp = cursor.read_i32::<LittleEndian>()?; // 0x7400
        let _unknown = cursor.read_i32::<LittleEndian>()?; // 0x00
        let _num_desc2 = cursor.read_i32::<LittleEndian>()?; // repeat

        for _ in 0..num_descriptions {
            // Per-descriptor: size(8), pageCount(4), maxDecompSize(4),
            //   unknown(4), compressedCode(4), sectionId(4), encrypted(4), name(64)
            let data_size = cursor.read_u64::<LittleEndian>()?;
            let page_count = cursor.read_i32::<LittleEndian>()?;
            let max_decomp_page_size = cursor.read_i32::<LittleEndian>()?;
            let _unknown = cursor.read_i32::<LittleEndian>()?;
            let compressed_code = cursor.read_i32::<LittleEndian>()?;
            let _section_id = cursor.read_i32::<LittleEndian>()?;
            let encrypted = cursor.read_i32::<LittleEndian>()?;

            // Section name (64-byte field, null-terminated). Some writers leave
            // non-zero garbage in the bytes *after* the terminator instead of
            // zero-padding, so cut at the first null rather than trimming
            // trailing nulls — otherwise the embedded null plus trailing junk
            // survives and the name fails to match (e.g. "AcDb:Handles\0t…").
            let mut name_buf = [0u8; 64];
            cursor.read_exact(&mut name_buf)?;
            let name = section_name_from_field(&name_buf);

            // Per-page entries: pageNumber(4), compressedSize(4), offset(8)
            let mut pages = Vec::new();
            for _ in 0..page_count {
                let page_number = cursor.read_i32::<LittleEndian>()?;
                let page_compressed_size = cursor.read_i32::<LittleEndian>()?;
                let page_offset_in_section = cursor.read_u64::<LittleEndian>()?;

                pages.push(DwgPageCrcInfo {
                    page_number: page_number as i64,
                    offset: page_offset_in_section,
                    size: 0,
                    decompressed_size: max_decomp_page_size as u64,
                    compressed_size: page_compressed_size as u64,
                    checksum: 0,
                    crc: 0,
                });
            }

            if !name.is_empty() {
                info.section_descriptors.push(DwgSectionInfo {
                    name: name.clone(),
                    compressed_size: data_size,
                    decompressed_size: max_decomp_page_size as u64,
                    encrypted: encrypted as u64,
                    hash_code: 0,
                    encoding: compressed_code as u64,
                    page_count: page_count as u64,
                    pages,
                });
            }
        }

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC18: Read {} section descriptors from section map",
                info.section_descriptors.len()
            ),
        );

        Ok(())
    }

    /// Read AC1021 (R2007) file header with CRC-64 extraction.
    ///
    /// This performs the full AC1021 header decoding pipeline:
    /// 1. Reed-Solomon decode the 0x400-byte encoded header
    /// 2. Extract CRC, key, and compression parameters
    /// 3. LZ77 AC21 decompress into 0x110-byte metadata buffer
    /// 4. Parse the `Dwg21CompressedMetadata` (including Header CRC-64)
    /// 5. Decode the page map and section map
    fn read_file_header_ac21(&mut self, info: &mut DwgFileHeaderInfo) -> Result<(), DxfError> {
        // After read_file_metadata, stream is at position 0x80 (128).
        // The Reed-Solomon encoded data follows immediately.
        // Do NOT seek — continue reading from current position.

        // Step 1: Read 0x400 bytes of Reed-Solomon encoded data
        let mut compressed_data = [0u8; 0x400];
        self.stream.read_exact(&mut compressed_data)?;

        // Step 2: Reed-Solomon decode (factor=3, block_size=239)
        let mut decoded_data = vec![0u8; 3 * 239]; // 717 bytes
        reed_solomon_decode(&compressed_data, &mut decoded_data, 3, 239);

        // Step 3: Extract header values from decoded data
        let mut cursor = Cursor::new(&decoded_data);

        let crc = cursor.read_i64::<LittleEndian>()?;
        let unknown_key = cursor.read_i64::<LittleEndian>()?;
        let compressed_data_crc = cursor.read_i64::<LittleEndian>()?;
        let compr_len = cursor.read_i32::<LittleEndian>()?;
        let _length2 = cursor.read_i32::<LittleEndian>()?;

        info.ac21_header_crc = Some(crc);
        info.ac21_unknown_key = Some(unknown_key);
        info.ac21_compressed_data_crc = Some(compressed_data_crc);

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021 header: CRC={:#018X}, UnknownKey={:#018X}, CompressedDataCRC={:#018X}, ComprLen={}",
                crc as u64, unknown_key as u64, compressed_data_crc as u64, compr_len
            ),
        );

        // Step 4: Extract 0x110-byte metadata
        let mut metadata_buffer = vec![0u8; 0x110];

        if compr_len < 0 {
            // Negative ComprLen means data is stored uncompressed (raw).
            // |ComprLen| = raw data length. Copy directly from offset 0x20.
            let raw_len = (-compr_len) as usize;
            let src_start = 32; // offset 0x20 in decoded data
            let copy_len = raw_len
                .min(0x110)
                .min(decoded_data.len().saturating_sub(src_start));
            metadata_buffer[..copy_len]
                .copy_from_slice(&decoded_data[src_start..src_start + copy_len]);
        } else {
            // Positive ComprLen means data is LZ77 compressed.
            // Decompress from byte offset 32 in decoded_data.
            decompress_ac21(&decoded_data, 32, compr_len as u32, &mut metadata_buffer);
        }

        // Step 5: Parse compressed metadata (extracts CRC-64)
        let metadata = Dwg21CompressedMetadata::from_bytes(&metadata_buffer)?;

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021 Header CRC-64 extracted: {:#018X}",
                metadata.header_crc64
            ),
        );

        // Note: The exact CRC-64 algorithm used by Autodesk for this field is
        // undocumented. No known open reference validates
        // this value. It is stored for informational/round-trip purposes.

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021 CRC Seeds: CrcSeed={:#018X}, CrcSeedEncoded={:#018X}, RandomSeed={:#018X}",
                metadata.crc_seed, metadata.crc_seed_encoded, metadata.random_seed
            ),
        );

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021 Pages Map CRC: compressed={:#018X}, uncompressed={:#018X}, seed={:#018X}",
                metadata.pages_map_crc_compressed,
                metadata.pages_map_crc_uncompressed,
                metadata.pages_map_crc_seed
            ),
        );

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021 Sections Map CRC: compressed={:#018X}, uncompressed={:#018X}, seed={:#018X}",
                metadata.sections_map_crc_compressed,
                metadata.sections_map_crc_uncompressed,
                metadata.sections_map_crc_seed
            ),
        );

        // Step 6: Read page map
        self.read_page_map_ac21(info, &metadata)?;

        // Step 7: Read section map
        self.read_section_map_ac21(info, &metadata)?;

        // Store metadata even if reads above fail (for diagnostics)
        info.ac21_metadata = Some(metadata.clone());

        Ok(())
    }

    /// Read the page map from an AC1021 file.
    ///
    /// The page map lists all data pages and their sizes, allowing
    /// the reader to build a page ID → file offset lookup table.
    fn read_page_map_ac21(
        &mut self,
        info: &mut DwgFileHeaderInfo,
        metadata: &Dwg21CompressedMetadata,
    ) -> Result<(), DxfError> {
        let page_buffer = self.get_page_buffer(
            metadata.pages_map_offset,
            metadata.pages_map_size_compressed,
            metadata.pages_map_size_uncompressed,
            metadata.pages_map_correction_factor,
            0xEF,
        )?;

        let mut cursor = Cursor::new(&page_buffer);
        let mut offset: i64 = 0;

        while (cursor.position() as usize) < page_buffer.len() {
            let size = cursor.read_i64::<LittleEndian>()?;
            let id = cursor.read_i64::<LittleEndian>()?;

            if size == 0 && id == 0 {
                // Terminator — all remaining bytes are padding
                break;
            }

            let ind = id.unsigned_abs();

            info.page_records.insert(ind as i32, (offset, size));
            offset += size;
        }

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021: Read {} page records from page map",
                info.page_records.len()
            ),
        );

        Ok(())
    }

    /// Read the section map from an AC1021 file.
    ///
    /// The section map describes all logical sections (Header, Classes,
    /// Handles, Objects, etc.) and their per-page CRC values.
    fn read_section_map_ac21(
        &mut self,
        info: &mut DwgFileHeaderInfo,
        metadata: &Dwg21CompressedMetadata,
    ) -> Result<(), DxfError> {
        // Look up the section map page
        let sections_map_id = metadata.sections_map_id as i32;
        let seeker = info
            .page_records
            .get(&sections_map_id)
            .map(|&(offset, _)| offset)
            .ok_or_else(|| {
                DxfError::InvalidFormat(format!(
                    "Section map page ID {} not found in page records",
                    sections_map_id
                ))
            })?;

        let section_buffer = self.get_page_buffer_at(
            seeker as u64,
            metadata.sections_map_size_compressed,
            metadata.sections_map_size_uncompressed,
            metadata.sections_map_correction_factor,
            239,
        )?;

        let mut cursor = Cursor::new(&section_buffer);

        while (cursor.position() as usize) < section_buffer.len() {
            // Check if there's enough data for at least the fixed header fields
            if section_buffer.len() - (cursor.position() as usize) < 64 {
                break;
            }

            let compressed_size = cursor.read_u64::<LittleEndian>()?;
            let decompressed_size = cursor.read_u64::<LittleEndian>()?;
            let encrypted = cursor.read_u64::<LittleEndian>()?;
            let hash_code = cursor.read_u64::<LittleEndian>()?;
            let section_name_length = cursor.read_i64::<LittleEndian>()?;
            let _unknown = cursor.read_u64::<LittleEndian>()?;
            let encoding = cursor.read_u64::<LittleEndian>()?;
            let page_count = cursor.read_u64::<LittleEndian>()?;

            // Read section name (UTF-16LE)
            let name = if section_name_length > 0 {
                let byte_len = section_name_length as usize;
                if cursor.position() as usize + byte_len > section_buffer.len() {
                    break;
                }
                let mut name_bytes = vec![0u8; byte_len];
                cursor.read_exact(&mut name_bytes)?;
                // Decode UTF-16LE
                let words: Vec<u16> = name_bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&words)
                    .trim_end_matches('\0')
                    .to_string()
            } else {
                String::new()
            };

            // Read per-page CRC information
            let mut pages = Vec::new();
            for _ in 0..page_count {
                if section_buffer.len() - (cursor.position() as usize) < 56 {
                    break;
                }
                let page_offset = cursor.read_u64::<LittleEndian>()?;
                let page_size = cursor.read_i64::<LittleEndian>()?;
                let page_number = cursor.read_i64::<LittleEndian>()?;
                let page_decompressed_size = cursor.read_u64::<LittleEndian>()?;
                let page_compressed_size = cursor.read_u64::<LittleEndian>()?;
                let page_checksum = cursor.read_u64::<LittleEndian>()?;
                let page_crc = cursor.read_u64::<LittleEndian>()?;

                self.notifications.notify(
                    NotificationType::Warning,
                    format!(
                        "  Section '{}' page {}: CRC={:#018X}, Checksum={:#018X}, CompSize={}, DecompSize={}",
                        name, page_number, page_crc, page_checksum,
                        page_compressed_size, page_decompressed_size
                    ),
                );

                pages.push(DwgPageCrcInfo {
                    page_number,
                    offset: page_offset,
                    size: page_size,
                    decompressed_size: page_decompressed_size,
                    compressed_size: page_compressed_size,
                    checksum: page_checksum,
                    crc: page_crc,
                });
            }

            if section_name_length > 0 {
                info.section_descriptors.push(DwgSectionInfo {
                    name: name.clone(),
                    compressed_size,
                    decompressed_size,
                    encrypted,
                    hash_code,
                    encoding,
                    page_count,
                    pages,
                });
            }
        }

        self.notifications.notify(
            NotificationType::Warning,
            format!(
                "AC1021: Read {} section descriptors from section map",
                info.section_descriptors.len()
            ),
        );

        Ok(())
    }

    /// Get a decompressed page buffer from the file.
    ///
    /// Handles the Reed-Solomon + LZ77 AC21 decompression pipeline.
    fn get_page_buffer(
        &mut self,
        page_offset: u64,
        compressed_size: u64,
        uncompressed_size: u64,
        correction_factor: u64,
        block_size: usize,
    ) -> Result<Vec<u8>, DxfError> {
        self.get_page_buffer_at(
            page_offset,
            compressed_size,
            uncompressed_size,
            correction_factor,
            block_size,
        )
    }

    /// Get a decompressed page buffer from a specific file offset.
    fn get_page_buffer_at(
        &mut self,
        page_offset: u64,
        compressed_size: u64,
        uncompressed_size: u64,
        correction_factor: u64,
        block_size: usize,
    ) -> Result<Vec<u8>, DxfError> {
        let (total_size, factor, read_length) =
            ac21_page_layout(compressed_size, correction_factor, block_size)?;

        // Read encoded data from file
        self.stream
            .seek(SeekFrom::Start(AC21_FILE_HEADER_SIZE + page_offset))?;
        let mut encoded_buffer = vec![0u8; read_length];
        let bytes_read = self.stream.read(&mut encoded_buffer)?;
        if bytes_read < read_length {
            // Pad remaining with zeros
            encoded_buffer[bytes_read..].fill(0);
        }

        Ok(decode_ac21_page(
            &encoded_buffer,
            total_size,
            factor,
            block_size,
            compressed_size,
            uncompressed_size,
        ))
    }

    /// Get the merged decompressed buffer for a named section (AC21).
    ///
    /// Reads and concatenates all pages belonging to the given section,
    /// producing the complete section data ready for parsing.
    ///
    /// In failsafe mode, missing or unreadable pages are skipped and a
    /// warning notification is emitted rather than aborting the entire
    /// section read.
    ///
    /// # Arguments
    /// * `section_name` - Section name (e.g., "AcDb:Header", "AcDb:Classes")
    /// * `info` - Previously read file header info containing section descriptors
    ///
    /// # Returns
    /// The complete decompressed section buffer, or an error if the section
    /// is not found or a page cannot be read (unless in failsafe mode).
    pub fn get_section_buffer(
        &mut self,
        section_name: &str,
        info: &DwgFileHeaderInfo,
    ) -> Result<Vec<u8>, DxfError> {
        let failsafe = self.options.failsafe;
        let perf = std::env::var_os("PERF").is_some();
        let started = web_time::Instant::now();

        // ── AC15 path: direct read from section locators ──
        // If we have section_locators (AC15 format), read raw bytes
        // directly from the file at the recorded offset.
        if !info.section_locators.is_empty() {
            if let Some(&(offset, size)) = info.section_locators.get(section_name) {
                if size <= 0 {
                    return Err(DxfError::Parse(format!(
                        "Section '{}' has zero size",
                        section_name
                    )));
                }
                self.stream.seek(SeekFrom::Start(offset as u64))?;
                let mut buf = vec![0u8; size as usize];
                self.stream.read_exact(&mut buf)?;
                if perf {
                    eprintln!(
                        "[perf] dwg-section name={} format=ac15 time={:.1}ms bytes={}",
                        section_name,
                        started.elapsed().as_secs_f64() * 1000.0,
                        buf.len(),
                    );
                }
                return Ok(buf);
            } else {
                return Err(DxfError::Parse(format!(
                    "Section '{}' not found in AC15 locator records",
                    section_name
                )));
            }
        }

        // ── AC18 path: page-based with LZ77 AC18 compression ──
        if info.is_ac18_format {
            let result = self.get_section_buffer_ac18(section_name, info);
            if perf {
                eprintln!(
                    "[perf] dwg-section name={} format=ac18 time={:.1}ms bytes={}",
                    section_name,
                    started.elapsed().as_secs_f64() * 1000.0,
                    result.as_ref().map_or(0, Vec::len),
                );
            }
            return result;
        }

        // ── AC21 path: page-based section descriptors ──
        // Find the section descriptor
        let section = info
            .section_descriptors
            .iter()
            .find(|s| s.name == section_name)
            .ok_or_else(|| {
                DxfError::Parse(format!("Section '{}' not found in file", section_name))
            })?;

        // Field 0x00 ("Data size") holds the total section data size.
        // Field 0x08 ("Max size") is the page partition size per spec §5.4.
        // We truncate to the total data size, not the page size.
        let total_size = section.compressed_size as usize;
        let mut result = Vec::with_capacity(total_size);

        // encoding=1 (stored): data is stored raw — no RS encoding, no LZ77.
        // encoding=4 (compressed): data is LZ77-compressed then RS-encoded with RS(255,251).
        // System pages (page map, section map) use RS(255,239) per §5.3,
        // but those are decoded separately in read_page_map_ac21 / read_section_map_ac21.
        let encoding = section.encoding;
        let block_size: usize = 251;

        enum PagePayload {
            Ready(Vec<u8>),
            Encoded {
                bytes: Vec<u8>,
                total_size: usize,
                factor: usize,
                compressed_size: u64,
                uncompressed_size: u64,
            },
            Failed(String),
        }
        struct PreparedPage {
            number: i64,
            fill_size: usize,
            payload: PagePayload,
        }

        // File seeks stay ordered; CPU-heavy Reed-Solomon and LZ77 work runs
        // concurrently in bounded batches. This keeps peak memory near
        // `result + 2 * worker pages` instead of retaining encoded and decoded
        // copies of the whole section at once.
        let batch_pages = worker_count().saturating_mul(2);
        let mut skipped_pages = 0u32;
        for page_batch in section.pages.chunks(batch_pages) {
            let mut prepared = Vec::with_capacity(page_batch.len());
            for page in page_batch {
                let fill_size = page.decompressed_size as usize;
                let payload = match info.page_records.get(&(page.page_number as i32)) {
                    Some(&(page_offset, _)) if encoding == 1 => {
                        let read = (|| -> Result<Vec<u8>, DxfError> {
                            self.stream.seek(SeekFrom::Start(
                                AC21_FILE_HEADER_SIZE + page_offset as u64,
                            ))?;
                            let mut bytes = vec![0u8; fill_size];
                            self.stream.read_exact(&mut bytes)?;
                            Ok(bytes)
                        })();
                        match read {
                            Ok(bytes) => PagePayload::Ready(bytes),
                            Err(error) if failsafe => PagePayload::Failed(error.to_string()),
                            Err(error) => return Err(error),
                        }
                    }
                    Some(&(page_offset, _)) => {
                        match ac21_page_layout(page.compressed_size, 1, block_size) {
                            Ok((total_size, factor, read_length)) => {
                                let read = (|| -> Result<Vec<u8>, DxfError> {
                                    self.stream.seek(SeekFrom::Start(
                                        AC21_FILE_HEADER_SIZE + page_offset as u64,
                                    ))?;
                                    let mut bytes = vec![0u8; read_length];
                                    let read = self.stream.read(&mut bytes)?;
                                    bytes[read..].fill(0);
                                    Ok(bytes)
                                })();
                                match read {
                                    Ok(bytes) => PagePayload::Encoded {
                                        bytes,
                                        total_size,
                                        factor,
                                        compressed_size: page.compressed_size,
                                        uncompressed_size: page.decompressed_size,
                                    },
                                    Err(error) if failsafe => {
                                        PagePayload::Failed(error.to_string())
                                    }
                                    Err(error) => return Err(error),
                                }
                            }
                            Err(error) if failsafe => PagePayload::Failed(error.to_string()),
                            Err(error) => return Err(error),
                        }
                    }
                    None if failsafe => PagePayload::Failed(format!(
                        "page {} not found in page map",
                        page.page_number
                    )),
                    None => {
                        return Err(DxfError::Parse(format!(
                            "Page {} not found in page map",
                            page.page_number
                        )))
                    }
                };
                prepared.push(PreparedPage {
                    number: page.page_number,
                    fill_size,
                    payload,
                });
            }

            let decoded = map_ordered(prepared, |page| {
                let data = match page.payload {
                    PagePayload::Ready(bytes) => Ok(bytes),
                    PagePayload::Encoded {
                        bytes,
                        total_size,
                        factor,
                        compressed_size,
                        uncompressed_size,
                    } => Ok(decode_ac21_page(
                        &bytes,
                        total_size,
                        factor,
                        block_size,
                        compressed_size,
                        uncompressed_size,
                    )),
                    PagePayload::Failed(error) => Err(error),
                };
                (page.number, page.fill_size, data)
            });

            for (page_number, fill_size, page_result) in decoded {
                match page_result {
                    Ok(page_data) => result.extend_from_slice(&page_data),
                    Err(error) => {
                        skipped_pages += 1;
                        self.notifications.notify(
                            NotificationType::Error,
                            format!(
                                "Failsafe: skipped corrupt page {} in section '{}': {}",
                                page_number, section_name, error
                            ),
                        );
                        result.resize(result.len() + fill_size, 0);
                    }
                }
            }
        }

        if skipped_pages > 0 {
            self.notifications.notify(
                NotificationType::Warning,
                format!(
                    "Failsafe: {} of {} pages skipped in section '{}'",
                    skipped_pages,
                    section.pages.len(),
                    section_name
                ),
            );
        }

        // Truncate to the declared section size (last page may be padded)
        result.truncate(total_size);

        if perf {
            eprintln!(
                "[perf] dwg-section name={} format=ac21 time={:.1}ms bytes={} pages={}",
                section_name,
                started.elapsed().as_secs_f64() * 1000.0,
                result.len(),
                section.pages.len(),
            );
        }
        Ok(result)
    }

    /// Get the merged decompressed buffer for a named section (AC18).
    ///
    /// Reads data pages for the given section, XOR-unmasks their 32-byte
    /// headers, and decompresses the LZ77 AC18 data.
    fn get_section_buffer_ac18(
        &mut self,
        section_name: &str,
        info: &DwgFileHeaderInfo,
    ) -> Result<Vec<u8>, DxfError> {
        let section = info
            .section_descriptors
            .iter()
            .find(|s| s.name == section_name)
            .ok_or_else(|| {
                DxfError::Parse(format!("Section '{}' not found in AC18 file", section_name))
            })?;

        // compressed_size field actually holds the total uncompressed section data size
        let total_size = section.compressed_size as usize;
        let is_compressed = section.encoding == 2;
        let max_page_size = section.decompressed_size as usize;

        let mut result = vec![0u8; total_size];
        let batch_pages = worker_count().saturating_mul(2);

        for page_batch in section.pages.chunks(batch_pages) {
            let mut prepared = Vec::with_capacity(page_batch.len());
            for page in page_batch {
                let page_number = page.page_number as i32;

                let &(page_file_offset, _page_total_size) =
                    info.page_records.get(&page_number).ok_or_else(|| {
                        DxfError::Parse(format!(
                            "AC18 page {} not found in page records for section '{}'",
                            page_number, section_name
                        ))
                    })?;

                self.stream.seek(SeekFrom::Start(page_file_offset as u64))?;

                // Read 32-byte data section header (XOR-masked)
                let mut header = [0u8; 32];
                self.stream.read_exact(&mut header)?;

                // XOR unmask using the page's file position
                apply_mask(&mut header, page_file_offset as u64);

                // Parse header fields
                let mut hcursor = Cursor::new(&header[..]);
                let _section_type = hcursor.read_i32::<LittleEndian>()?;
                let _section_id = hcursor.read_i32::<LittleEndian>()?;
                let data_compressed_size = hcursor.read_i32::<LittleEndian>()?;
                let _page_size = hcursor.read_i32::<LittleEndian>()?;
                let data_offset = hcursor.read_i64::<LittleEndian>()?;

                // Read compressed data
                if data_compressed_size <= 0 || data_compressed_size > 10_000_000 {
                    self.notifications.notify(
                        NotificationType::Warning,
                        format!(
                            "AC18: Invalid compressed size {} for page {} in section '{}'",
                            data_compressed_size, page_number, section_name
                        ),
                    );
                    continue;
                }
                let mut compressed = vec![0u8; data_compressed_size as usize];
                self.stream.read_exact(&mut compressed)?;

                prepared.push((data_offset as usize, compressed));
            }

            let decoded = map_ordered(prepared, |(dst_start, compressed)| {
                let decompressed = if is_compressed {
                    decompress_ac18(&compressed, max_page_size)
                } else {
                    compressed
                };
                (dst_start, decompressed)
            });

            for (dst_start, decompressed) in decoded {
                if dst_start < total_size {
                    let copy_len = decompressed.len().min(total_size - dst_start);
                    result[dst_start..dst_start + copy_len]
                        .copy_from_slice(&decompressed[..copy_len]);
                }
            }
        }

        Ok(result)
    }

    /// Find a section descriptor by name.
    pub fn find_section<'a>(info: &'a DwgFileHeaderInfo, name: &str) -> Option<&'a DwgSectionInfo> {
        info.section_descriptors.iter().find(|s| s.name == name)
    }

    /// Extract all CRC values from the file and return them as a summary.
    ///
    /// This is a convenience method that reads the file header and
    /// formats all CRC values for display.
    pub fn extract_all_crcs(&mut self) -> Result<CrcExtractionReport, DxfError> {
        let info = self.read_file_header()?;

        let mut report = CrcExtractionReport {
            version: info.version_string.clone(),
            header_crc64: None,
            header_crc: info.ac21_header_crc,
            compressed_data_crc: info.ac21_compressed_data_crc,
            pages_map_crc_compressed: None,
            pages_map_crc_uncompressed: None,
            pages_map_crc_seed: None,
            sections_map_crc_compressed: None,
            sections_map_crc_uncompressed: None,
            sections_map_crc_seed: None,
            crc_seed: None,
            crc_seed_encoded: None,
            random_seed: None,
            page_crcs: Vec::new(),
            notifications: std::mem::take(&mut self.notifications),
        };

        if let Some(ref metadata) = info.ac21_metadata {
            report.header_crc64 = Some(metadata.header_crc64);
            report.pages_map_crc_compressed = Some(metadata.pages_map_crc_compressed);
            report.pages_map_crc_uncompressed = Some(metadata.pages_map_crc_uncompressed);
            report.pages_map_crc_seed = Some(metadata.pages_map_crc_seed);
            report.sections_map_crc_compressed = Some(metadata.sections_map_crc_compressed);
            report.sections_map_crc_uncompressed = Some(metadata.sections_map_crc_uncompressed);
            report.sections_map_crc_seed = Some(metadata.sections_map_crc_seed);
            report.crc_seed = Some(metadata.crc_seed);
            report.crc_seed_encoded = Some(metadata.crc_seed_encoded);
            report.random_seed = Some(metadata.random_seed);
        }

        for section in &info.section_descriptors {
            for page in &section.pages {
                report.page_crcs.push(PageCrcEntry {
                    section_name: section.name.clone(),
                    page_number: page.page_number,
                    crc: page.crc,
                    checksum: page.checksum,
                    compressed_size: page.compressed_size,
                    decompressed_size: page.decompressed_size,
                });
            }
        }

        Ok(report)
    }
}

/// Complete CRC extraction report from a DWG file.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CrcExtractionReport {
    /// DWG version string
    pub version: String,
    /// Header CRC-64 (AC1021 only)
    pub header_crc64: Option<u64>,
    /// Header CRC from Reed-Solomon decoded data
    pub header_crc: Option<i64>,
    /// Compressed data CRC
    pub compressed_data_crc: Option<i64>,
    /// Pages map CRC (compressed)
    pub pages_map_crc_compressed: Option<u64>,
    /// Pages map CRC (uncompressed)
    pub pages_map_crc_uncompressed: Option<u64>,
    /// Pages map CRC seed
    pub pages_map_crc_seed: Option<u64>,
    /// Sections map CRC (compressed)
    pub sections_map_crc_compressed: Option<u64>,
    /// Sections map CRC (uncompressed)
    pub sections_map_crc_uncompressed: Option<u64>,
    /// Sections map CRC seed
    pub sections_map_crc_seed: Option<u64>,
    /// Global CRC seed
    pub crc_seed: Option<u64>,
    /// Encoded CRC seed
    pub crc_seed_encoded: Option<u64>,
    /// Random seed
    pub random_seed: Option<u64>,
    /// Per-page CRC values
    pub page_crcs: Vec<PageCrcEntry>,
    /// Notifications collected during reading
    pub notifications: NotificationCollection,
}

/// CRC entry for a single page.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PageCrcEntry {
    /// Section name
    pub section_name: String,
    /// Page number
    pub page_number: i64,
    /// CRC value
    pub crc: u64,
    /// Checksum value
    pub checksum: u64,
    /// Compressed size
    pub compressed_size: u64,
    /// Decompressed size
    pub decompressed_size: u64,
}

impl std::fmt::Display for CrcExtractionReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== DWG CRC Extraction Report ===")?;
        writeln!(f, "Version: {}", self.version)?;
        writeln!(f)?;

        if let Some(crc64) = self.header_crc64 {
            writeln!(f, "--- Header CRC-64 ---")?;
            writeln!(f, "  Header CRC-64:              {:#018X}", crc64)?;
        }

        if let Some(crc) = self.header_crc {
            writeln!(f, "  Header CRC (RS decoded):    {:#018X}", crc as u64)?;
        }
        if let Some(crc) = self.compressed_data_crc {
            writeln!(f, "  Compressed Data CRC:        {:#018X}", crc as u64)?;
        }

        if self.crc_seed.is_some() {
            writeln!(f)?;
            writeln!(f, "--- CRC Seeds ---")?;
            if let Some(v) = self.crc_seed {
                writeln!(f, "  CRC Seed:                   {:#018X}", v)?;
            }
            if let Some(v) = self.crc_seed_encoded {
                writeln!(f, "  CRC Seed Encoded:           {:#018X}", v)?;
            }
            if let Some(v) = self.random_seed {
                writeln!(f, "  Random Seed:                {:#018X}", v)?;
            }
        }

        if self.pages_map_crc_compressed.is_some() {
            writeln!(f)?;
            writeln!(f, "--- Pages Map CRC ---")?;
            if let Some(v) = self.pages_map_crc_compressed {
                writeln!(f, "  Compressed:                 {:#018X}", v)?;
            }
            if let Some(v) = self.pages_map_crc_uncompressed {
                writeln!(f, "  Uncompressed:               {:#018X}", v)?;
            }
            if let Some(v) = self.pages_map_crc_seed {
                writeln!(f, "  Seed:                       {:#018X}", v)?;
            }
        }

        if self.sections_map_crc_compressed.is_some() {
            writeln!(f)?;
            writeln!(f, "--- Sections Map CRC ---")?;
            if let Some(v) = self.sections_map_crc_compressed {
                writeln!(f, "  Compressed:                 {:#018X}", v)?;
            }
            if let Some(v) = self.sections_map_crc_uncompressed {
                writeln!(f, "  Uncompressed:               {:#018X}", v)?;
            }
            if let Some(v) = self.sections_map_crc_seed {
                writeln!(f, "  Seed:                       {:#018X}", v)?;
            }
        }

        if !self.page_crcs.is_empty() {
            writeln!(f)?;
            writeln!(f, "--- Per-Page CRC Values ---")?;
            for entry in &self.page_crcs {
                writeln!(
                    f,
                    "  {} [page {}]: CRC={:#018X}, Checksum={:#018X} (comp={}, decomp={})",
                    entry.section_name,
                    entry.page_number,
                    entry.crc,
                    entry.checksum,
                    entry.compressed_size,
                    entry.decompressed_size
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod section_name_tests {
    use super::section_name_from_field;

    fn field(prefix: &[u8]) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[..prefix.len()].copy_from_slice(prefix);
        b
    }

    #[test]
    fn clean_zero_padded_name() {
        assert_eq!(
            section_name_from_field(&field(b"AcDb:Handles")),
            "AcDb:Handles"
        );
    }

    #[test]
    fn stops_at_first_null_ignoring_trailing_garbage() {
        // Terminator at index 12, then non-zero junk — the real-world case that
        // broke `trim_end_matches('\0')`.
        let mut b = field(b"AcDb:Handles");
        b[13] = b't';
        b[14] = 0x01;
        b[20] = b'X';
        assert_eq!(section_name_from_field(&b), "AcDb:Handles");
    }

    #[test]
    fn empty_when_first_byte_null() {
        assert_eq!(section_name_from_field(&[0u8; 64]), "");
    }
}

/// Reconstruct a gradient hatch that was down-saved (R2000/R2004) as a solid
/// fill plus round-trip metadata: the two colours live in EED
/// (`GradientColor1ACI` / `GradientColor2ACI`) and the gradient type name in an
/// `ACAD_XREC_ROUNDTRIP` XRecord under the hatch's extension dictionary. When a
/// solid hatch carries this metadata but no live gradient block, rebuild
/// `gradient_color` so it renders as the intended gradient.
pub(crate) fn recover_roundtrip_gradients(document: &mut crate::document::CadDocument) {
    use crate::entities::EntityType;
    use crate::objects::ObjectType;
    use crate::types::{Color, Handle};

    const GRADIENT_NAMES: &[&str] = &[
        "SPHERICAL",
        "HEMISPHERICAL",
        "CURVED",
        "CYLINDER",
        "INVSPHERICAL",
        "INVHEMISPHERICAL",
        "INVCURVED",
        "INVCYLINDER",
        "LINEAR",
    ];

    // Phase 1 — collect (immutable): which hatches need a gradient, and what.
    let mut recovered: Vec<(Handle, u8, u8, String)> = Vec::new();
    for e in document.entities() {
        let EntityType::Hatch(h) = e else { continue };
        if h.gradient_color.enabled || !h.is_solid {
            continue;
        }
        let eed = &h.common.extended_data;
        // EED app names vary in case across versions (R14 upper-cases them:
        // GRADIENTCOLOR1ACI vs R2000's GradientColor1ACI), so match loosely.
        let aci = |app: &str| -> Option<u8> {
            eed.records()
                .iter()
                .find(|r| r.application_name.eq_ignore_ascii_case(app))
                .and_then(|r| {
                    r.values.iter().find_map(|v| match v {
                        crate::xdata::XDataValue::Integer16(n) => Some(*n as u8),
                        _ => None,
                    })
                })
        };
        let (Some(c1), Some(c2)) = (aci("GradientColor1ACI"), aci("GradientColor2ACI")) else {
            continue;
        };

        // A live down-saved gradient always carries an ACAD_XREC_ROUNDTRIP
        // XRecord (under the hatch's extension dictionary) alongside the
        // GradientColor*ACI EED. When only the EED survives — no extension
        // dictionary, or one without that XRecord — the colours are stale
        // metadata left behind by an edit that turned a gradient into a plain
        // solid fill (or recoloured it). Resurrecting a gradient then paints a
        // genuine single-colour solid hatch as a two-colour gradient, so treat
        // the missing round-trip XRecord as proof the hatch is really solid.
        //
        // Gradient type: the trailing name string in that same XRecord
        // (hatch xdict → "ACAD_XREC_ROUNDTRIP" → XRecord raw_data).
        let mut name = String::new();
        let mut has_roundtrip = false;
        if let Some(xd) = h.common.xdictionary_handle {
            if let Some(ObjectType::Dictionary(d)) = document.objects.get(&xd) {
                // R14 mis-sizes dictionary key strings, leaving trailing
                // garbage bytes ("ACAD_XREC_ROUNDTRIP\x03q0…"), so match by
                // prefix rather than equality.
                if let Some((_, xrec_h)) = d.entries.iter().find(|(k, _)| {
                    k.len() >= 19 && k[..19].eq_ignore_ascii_case("ACAD_XREC_ROUNDTRIP")
                }) {
                    has_roundtrip = true;
                    if let Some(ObjectType::XRecord(xr)) = document.objects.get(xrec_h) {
                        let ascii: String = xr
                            .raw_data
                            .iter()
                            .map(|&b| {
                                if (32..127).contains(&b) {
                                    b as char
                                } else {
                                    ' '
                                }
                            })
                            .collect();
                        let up = ascii.to_ascii_uppercase();
                        for cand in GRADIENT_NAMES {
                            if up.contains(cand) {
                                name = (*cand).to_string();
                                break;
                            }
                        }
                        // A DXF-sourced round-trip XRecord encodes the gradient
                        // type as a number in the 1004 binary (first LE int32 =
                        // 123 + gradient index in AutoCAD's dropdown order),
                        // not as an ASCII name; decode it when no name matched.
                        if name.is_empty() && xr.raw_data.len() >= 4 {
                            const BY_INDEX: &[&str] = &[
                                "LINEAR",
                                "CYLINDER",
                                "INVCYLINDER",
                                "SPHERICAL",
                                "HEMISPHERICAL",
                                "CURVED",
                                "INVSPHERICAL",
                                "INVHEMISPHERICAL",
                                "INVCURVED",
                            ];
                            let v = u32::from_le_bytes([
                                xr.raw_data[0],
                                xr.raw_data[1],
                                xr.raw_data[2],
                                xr.raw_data[3],
                            ]);
                            if (123..123 + BY_INDEX.len() as u32).contains(&v) {
                                name = BY_INDEX[(v - 123) as usize].to_string();
                            }
                        }
                    }
                }
            }
        }
        // No round-trip XRecord ⇒ the gradient EED is stale; keep it solid.
        if !has_roundtrip {
            continue;
        }
        if name.is_empty() {
            name = "LINEAR".to_string();
        }
        recovered.push((e.common().handle, c1, c2, name));
    }

    // Phase 2 — apply (mutable).
    for (handle, c1, c2, name) in recovered {
        if let Some(EntityType::Hatch(h)) = document.get_entity_mut(handle) {
            h.gradient_color.enabled = true;
            h.gradient_color.is_single_color = c1 == c2;
            h.gradient_color.colors = vec![
                crate::entities::hatch::GradientColorEntry {
                    value: 0.0,
                    color: Color::from_index(c1 as i16),
                },
                crate::entities::hatch::GradientColorEntry {
                    value: 1.0,
                    color: Color::from_index(c2 as i16),
                },
            ];
            h.gradient_color.name = name;
        }
    }
}

/// Recover an MTEXT background fill that a pre-R2004 (R2000/R14) save stored as
/// round-trip EED (`ACAD_MTEXT_BBRT` … `ACAD_MTEXT_BERT`) instead of the native
/// codes (90 flags / 63|421 colour / 45 scale / 441 transparency), which those
/// versions predate. AutoCAD/ODA render the fill from this metadata; without it
/// a down-saved dimension text (or any MTEXT) shows no background. Applied only
/// when the entity has no native fill flag. The colour is stored with the
/// AutoCAD "method byte" (0xC2 = RGB, 0xC3 = ACI, 0xC0/0xC1 = ByLayer/ByBlock).
pub(crate) fn recover_mtext_bg_roundtrip(document: &mut crate::document::CadDocument) {
    use crate::entities::EntityType;
    use crate::types::{Color, Handle};
    use crate::xdata::XDataValue;

    fn color_from_method(v: i32) -> Color {
        let u = v as u32;
        match (u >> 24) & 0xFF {
            0xC0 => Color::ByLayer,
            0xC1 => Color::ByBlock,
            0xC3 => Color::from_index((u & 0xFF) as i16),
            _ => Color::Rgb {
                r: ((u >> 16) & 0xFF) as u8,
                g: ((u >> 8) & 0xFF) as u8,
                b: (u & 0xFF) as u8,
            },
        }
    }

    let mut recovered: Vec<(Handle, i32, f64, Color, i32)> = Vec::new();
    for e in document.entities() {
        let EntityType::MText(m) = e else { continue };
        if m.background_fill_flags != 0 {
            continue; // native fill present — nothing to recover
        }
        let Some(rec) = m
            .common
            .extended_data
            .records()
            .iter()
            .find(|r| r.application_name.eq_ignore_ascii_case("ACAD"))
        else {
            continue;
        };
        let vals = &rec.values;
        let Some(begin) = vals
            .iter()
            .position(|v| matches!(v, XDataValue::String(s) if s == "ACAD_MTEXT_BBRT"))
        else {
            continue;
        };
        // Walk the BBRT…BERT block as (Integer16 code, value) pairs.
        let (mut flags, mut scale, mut color, mut transp) = (0i32, 1.0f64, Color::ByLayer, 0i32);
        let mut i = begin + 1;
        while i + 1 < vals.len() {
            if matches!(&vals[i], XDataValue::String(s) if s == "ACAD_MTEXT_BERT") {
                break;
            }
            if let XDataValue::Integer16(code) = vals[i] {
                match (code, &vals[i + 1]) {
                    (91, XDataValue::Integer32(v)) => flags = *v,
                    (46, XDataValue::Real(v)) => scale = *v,
                    (64, XDataValue::Integer32(v)) => color = color_from_method(*v),
                    (442, XDataValue::Integer32(v)) => transp = *v,
                    _ => {}
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        if flags != 0 {
            recovered.push((m.common.handle, flags, scale, color, transp));
        }
    }
    for (h, flags, scale, color, transp) in recovered {
        if let Some(EntityType::MText(m)) = document.get_entity_mut(h) {
            m.background_fill_flags = flags;
            m.background_scale = scale;
            m.background_color = color;
            m.background_transparency = transp;
        }
    }
}
