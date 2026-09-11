//! Top-level DWG file writer
//!
//! Orchestrates all section writers to produce a complete DWG binary file.
//! Supports three file formats:
//!
//! - **AC15** (R13/R14/R2000): Linear format with sequential sections
//! - **AC18** (R2004/R2010+): Page-based format with LZ77 compression
//! - **AC21** (R2007): RS-encoded pages with LZ77 AC21 compression and CRC-64
//!
//! ## Usage
//!
//! ```no_run
//! use acadrust::document::CadDocument;
//! use acadrust::io::dwg::DwgWriter;
//!
//! let doc = CadDocument::new();
//! DwgWriter::write_to_file("output.dwg", &doc).unwrap();
//! ```
//!
//! Based on the reference `DwgWriter` class.

use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::Path;

use crate::document::{CadDocument, HeaderVariables};
use crate::error::{DxfError, Result};
use crate::types::{DxfVersion, Handle};

use super::dwg_stream_writers::{
    app_info_writer, aux_header_writer, classes_writer, handle_writer, header_writer,
    DwgObjectWriter,
};
use super::file_headers::{
    section_names, DwgFileHeaderWriterAC15, DwgFileHeaderWriterAC18, DwgFileHeaderWriterAC21,
};

// ════════════════════════════════════════════════════════════════════════════
//  Public API
// ════════════════════════════════════════════════════════════════════════════

/// DWG binary file writer.
///
/// Produces a complete DWG file from a [`CadDocument`].
/// The output version is determined by [`CadDocument::version`].
pub struct DwgWriter;

impl DwgWriter {
    /// Write a DWG file to the given path.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The version is `Unknown`
    /// - An I/O error occurs
    /// - The document contains invalid data
    pub fn write_to_file<P: AsRef<Path>>(path: P, document: &CadDocument) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        Self::write_to_writer(writer, document)
    }

    /// Write a DWG file to any `Write + Seek` output.
    pub fn write_to_writer<W: Write + Seek>(mut output: W, document: &CadDocument) -> Result<()> {
        let mut prepared = crate::io::loft_parameters::prepared(document);
        prepare_surface_classes(&mut prepared);
        prepare_database_references(&mut prepared);
        let document = prepared.as_ref();
        let perf = std::env::var_os("PERF").is_some();
        let started = web_time::Instant::now();
        validate_version(document.version)?;
        let version = document.version;

        // Programmatically added table entries may carry NULL handles
        // (Layer::new + layers.add never assigns one). DWG table records
        // must each carry a real handle - a record written under handle 0
        // is dropped by the handle map and disappears from the re-opened
        // drawing. Assign fresh handles on a cloned document up front so
        // the controls, entries and handle map all agree (issue #51/#64
        // class of bug).
        let mut owned;
        let document: &CadDocument =
            if document.has_null_table_entries() || document.version < DxfVersion::AC1027 {
                owned = document.clone();
                if owned.has_null_table_entries() {
                    owned.assign_table_entry_handles();
                }
                if owned.version < DxfVersion::AC1027 {
                    owned.classes.retain_legacy_dwg_classes();
                    prepare_legacy_document(&mut owned);
                }
                &owned
            } else {
                document
            };

        let result = if uses_ac21_format(version) {
            write_ac21(&mut output, document, version)
        } else if uses_paged_format(version) {
            write_ac18(&mut output, document, version)
        } else {
            write_ac15(&mut output, document, version)
        };
        if perf {
            eprintln!(
                "[perf] dwg-write total={:.1}ms version={:?} entities={} objects={}",
                started.elapsed().as_secs_f64() * 1000.0,
                version,
                document.entities().count(),
                document.objects.len(),
            );
        }
        result
    }

    /// Write an AC21 DWG file **without LZ77 compression** (diagnostic).
    ///
    /// Pages are still RS-encoded but LZ77 is bypassed, storing raw data.
    /// Useful for isolating whether a read error is caused by compression
    /// or by the object data itself.
    pub fn write_to_file_no_lz77<P: AsRef<Path>>(path: P, document: &CadDocument) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        Self::write_to_writer_no_lz77(writer, document)
    }

    /// Write an AC21 DWG without LZ77 to any `Write + Seek` output.
    pub fn write_to_writer_no_lz77<W: Write + Seek>(
        mut output: W,
        document: &CadDocument,
    ) -> Result<()> {
        validate_version(document.version)?;
        let mut prepared = crate::io::loft_parameters::prepared(document);
        prepare_surface_classes(&mut prepared);
        prepare_database_references(&mut prepared);
        write_ac21_impl(&mut output, prepared.as_ref(), document.version, true)
    }

    /// Write a DWG file to a byte vector (useful for testing).
    pub fn write_to_vec(document: &CadDocument) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());
        Self::write_to_writer(&mut buffer, document)?;
        Ok(buffer.into_inner())
    }
}

/// Repair relationships that are stored in both directions in a DWG database.
/// The caller's document stays unchanged; corrections exist only in the output
/// copy.
fn prepare_database_references(document: &mut std::borrow::Cow<'_, CadDocument>) {
    use crate::entities::EntityType;
    use crate::objects::ObjectType;
    use std::collections::{HashMap, HashSet};

    let mut missing_hatch_reactors = Vec::new();
    let mut invalid_hatches = Vec::new();
    for entity in document.entities() {
        let EntityType::Hatch(hatch) = entity else {
            continue;
        };
        if !hatch.is_associative {
            continue;
        }
        let boundaries: Vec<Handle> = hatch
            .paths
            .iter()
            .flat_map(|path| path.boundary_handles.iter().copied())
            .collect();
        if boundaries.is_empty()
            || boundaries
                .iter()
                .any(|handle| document.get_entity(*handle).is_none())
        {
            invalid_hatches.push(hatch.common.handle);
            continue;
        }
        for boundary_handle in boundaries {
            if document
                .get_entity(boundary_handle)
                .is_some_and(|boundary| !boundary.common().reactors.contains(&hatch.common.handle))
            {
                missing_hatch_reactors.push((boundary_handle, hatch.common.handle));
            }
        }
    }

    let root_handle = document.header.named_objects_dict_handle;
    let layout_dictionary = document
        .objects
        .get(&root_handle)
        .and_then(|object| match object {
            ObjectType::Dictionary(dictionary) => dictionary.get("ACAD_LAYOUT"),
            _ => None,
        })
        .unwrap_or(document.header.acad_layout_dict_handle);
    let layout_names: HashMap<Handle, String> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::Layout(layout) => Some((*handle, layout.name.clone())),
            _ => None,
        })
        .collect();
    let layout_dictionary_needs_repair = document
        .objects
        .get(&layout_dictionary)
        .and_then(|object| match object {
            ObjectType::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        })
        .is_some_and(|dictionary| {
            let targets: HashSet<Handle> = dictionary
                .entries
                .iter()
                .filter_map(|(key, handle)| {
                    layout_names
                        .get(handle)
                        .filter(|name| *name == key)
                        .map(|_| *handle)
                })
                .collect();
            dictionary.entries.len() != targets.len() || targets.len() != layout_names.len()
        });

    if missing_hatch_reactors.is_empty()
        && invalid_hatches.is_empty()
        && !layout_dictionary_needs_repair
    {
        return;
    }

    let output = document.to_mut();
    for (boundary_handle, hatch_handle) in missing_hatch_reactors {
        if let Some(boundary) = output.get_entity_mut(boundary_handle) {
            if !boundary.common().reactors.contains(&hatch_handle) {
                boundary.common_mut().reactors.push(hatch_handle);
            }
        }
    }
    for hatch_handle in invalid_hatches {
        if let Some(EntityType::Hatch(hatch)) = output.get_entity_mut(hatch_handle) {
            hatch.is_associative = false;
            for path in &mut hatch.paths {
                path.boundary_handles.clear();
            }
        }
    }

    if layout_dictionary_needs_repair {
        if let Some(ObjectType::Dictionary(dictionary)) = output.objects.get_mut(&layout_dictionary)
        {
            dictionary.entries.clear();
            let mut layouts: Vec<_> = layout_names.into_iter().collect();
            layouts.sort_by_key(|(handle, _)| handle.value());
            let mut names = HashSet::new();
            for (handle, name) in layouts {
                if names.insert(name.clone()) {
                    dictionary.entries.push((name, handle));
                }
            }
        }
        for object in output.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                layout.owner = layout_dictionary;
            }
        }
    }
}

/// Surface records use class numbers, not fixed object codes. Documents created
/// from scratch or opened before a surface subtype was added may lack its class.
/// Append only missing classes on the output copy, retaining existing class
/// order, numbers, and metadata for every other object in the drawing.
fn prepare_surface_classes(document: &mut std::borrow::Cow<'_, CadDocument>) {
    use crate::entities::{EntityType, SurfaceKind};

    let mut missing = Vec::new();
    for entity in document.entities() {
        let EntityType::Surface(surface) = entity else {
            continue;
        };
        let name = surface.kind.dxf_name();
        if document.classes.contains(name) || missing.iter().any(|(dxf, _)| *dxf == name) {
            continue;
        }
        let cpp = match surface.kind {
            SurfaceKind::Generic => "AcDbSurface",
            SurfaceKind::Plane => "AcDbPlaneSurface",
            SurfaceKind::Extruded => "AcDbExtrudedSurface",
            SurfaceKind::Lofted => "AcDbLoftedSurface",
            SurfaceKind::Revolved => "AcDbRevolvedSurface",
            SurfaceKind::Swept => "AcDbSweptSurface",
            SurfaceKind::Nurb => "AcDbNurbSurface",
        };
        missing.push((name, cpp));
    }
    if missing.is_empty() {
        return;
    }
    let output = document.to_mut();
    for (name, cpp) in missing {
        output
            .classes
            .add_or_update(crate::classes::DxfClass::new_entity(name, cpp));
    }
}

/// Remove default objects that use post-R2000 binary layouts.
///
/// The modern document initializer adds `ACAD_MLEADERSTYLE` and
/// `ACAD_TABLESTYLE`. Their object encodings are not valid in AC15 streams,
/// so their root-dictionary entries and owned objects must be removed together.
fn prepare_legacy_document(document: &mut CadDocument) {
    use crate::objects::ObjectType;

    let root_handle = document.header.named_objects_dict_handle;
    let mut obsolete = Vec::new();
    if let Some(ObjectType::Dictionary(root)) = document.objects.get_mut(&root_handle) {
        root.entries.retain(|(name, handle)| {
            let remove = matches!(name.as_str(), "ACAD_MLEADERSTYLE" | "ACAD_TABLESTYLE");
            if remove {
                obsolete.push(*handle);
            }
            !remove
        });
    }

    for handle in obsolete {
        if let Some(ObjectType::Dictionary(dictionary)) = document.objects.remove(&handle) {
            for (_, child) in dictionary.entries {
                document.objects.remove(&child);
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Validation
// ════════════════════════════════════════════════════════════════════════════

/// Validate that the document version is supported for DWG writing.
fn validate_version(version: DxfVersion) -> Result<()> {
    match version {
        DxfVersion::Unknown => Err(DxfError::UnsupportedVersion("Unknown version".to_string())),
        _ => Ok(()),
    }
}

/// Build class records from the objects that were actually encoded.
///
/// A zero-version class with no emitted instances is an unloaded declaration,
/// not a live runtime class. Describing it as live leaves class dictionary
/// slots that a database audit has to replace with dummy entries. Fixed object
/// classes remain registered even when their records use fixed type codes.
fn reconciled_classes(
    document: &CadDocument,
    instance_counts: &std::collections::HashMap<i16, i32>,
    counts_complete: bool,
) -> Vec<crate::classes::DxfClass> {
    const FIXED_CLASSES: &[&str] = &[
        "ACDBDICTIONARYWDFLT",
        "DICTIONARYVAR",
        "LAYOUT",
        "ACDBPLACEHOLDER",
        "PLOTSETTINGS",
        "SCALE",
    ];

    document
        .classes
        .iter()
        .cloned()
        .map(|mut class| {
            if let Some(&count) = instance_counts.get(&class.class_number) {
                class.instance_count = count;
                class.was_zombie = false;
            } else if counts_complete {
                class.instance_count = 0;
                if class.dwg_version == 0
                    && class.maintenance_version == 0
                    && !FIXED_CLASSES
                        .iter()
                        .any(|name| class.dxf_name.eq_ignore_ascii_case(name))
                {
                    class.was_zombie = true;
                }
            }
            class
        })
        .collect()
}

fn acds_data<'a>(
    document: &'a CadDocument,
    version: DxfVersion,
    sab_entries: &[(Handle, Vec<u8>)],
) -> std::borrow::Cow<'a, [u8]> {
    let fingerprint = super::sab_fingerprint(
        sab_entries
            .iter()
            .map(|(handle, bytes)| (*handle, bytes.as_slice())),
    );
    if document.dwg_source_version == Some(version) && fingerprint == document.raw_acds_fingerprint
    {
        if let Some(raw) = document.raw_acds_data.as_deref() {
            return std::borrow::Cow::Borrowed(raw.as_slice());
        }
    }
    std::borrow::Cow::Owned(build_acds_prototype(sab_entries))
}

/// Whether the version uses the AC21 (R2007) file format.
///
/// AC1021 uses RS-encoded pages, LZ77 AC21 compression, and CRC-64
/// checksums — distinct from both the AC18 and AC15 formats.
fn uses_ac21_format(version: DxfVersion) -> bool {
    version == DxfVersion::AC1021
}

/// Whether the version uses the AC18 page-based file format.
fn uses_paged_format(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

/// Prepare the header for writing by synchronizing all handle references
/// from the actual document objects and correcting the handle seed.
///
/// This is critical because after a DWG read-roundtrip, header handle
/// references may be NULL (the header reader for R2007+ doesn't correctly
/// read handles from the three-stream merged format). Without syncing,
/// the header would write NULL handles for table controls and the root
/// dictionary, causing IntelliCAD (and other CAD apps) to report
/// "null object id" for every object.
///
/// Also updates EXTMIN/EXTMAX from the computed model-space extents so that
/// "Zoom Extents" works correctly when the file is first opened.
fn prepare_header(
    document: &CadDocument,
    handle_map: &[(u64, u32)],
    extents: &Option<crate::types::BoundingBox3D>,
) -> HeaderVariables {
    let mut h = document.header.clone();
    let emitted = |handle: Handle| {
        !handle.is_null() && handle_map.iter().any(|(value, _)| *value == handle.value())
    };

    // ── Sync table control handles from actual table objects ──
    // The tables always have valid handles from initialize_defaults(),
    // but the header might have NULL handles after a DWG read.
    h.block_control_handle = document.block_records.handle();
    h.layer_control_handle = document.layers.handle();
    h.style_control_handle = document.text_styles.handle();
    h.linetype_control_handle = document.line_types.handle();
    h.view_control_handle = document.views.handle();
    h.ucs_control_handle = document.ucss.handle();
    h.vport_control_handle = document.vports.handle();
    h.appid_control_handle = document.app_ids.handle();
    h.dimstyle_control_handle = document.dim_styles.handle();

    // ── Sync root dictionary handle ──
    // Find the root dictionary by scanning document.objects for a
    // Dictionary with owner == NULL. Prefer non-0x0C handles (file's
    // root dict) over the initialize_defaults() one.
    if h.named_objects_dict_handle.is_null() {
        h.named_objects_dict_handle = find_root_dict_handle(&document.objects);
    }
    // Verify the root dict handle actually exists in objects
    if !h.named_objects_dict_handle.is_null()
        && !document.objects.contains_key(&h.named_objects_dict_handle)
    {
        // Handle points to nonexistent object — try to find the real root dict
        h.named_objects_dict_handle = find_root_dict_handle(&document.objects);
    }

    // ── Sync child dictionary handles from root dict entries ──
    // Always overwrite — reader may produce garbage handles.
    // If root dict doesn't have an entry, set handle to NULL.
    let root_dict_entries = match document.objects.get(&h.named_objects_dict_handle) {
        Some(crate::objects::ObjectType::Dictionary(root_dict)) => Some(root_dict),
        _ => None,
    };
    h.acad_group_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_GROUP"))
        .unwrap_or(Handle::NULL);
    h.acad_mlinestyle_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_MLINESTYLE"))
        .unwrap_or(Handle::NULL);
    h.acad_layout_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_LAYOUT"))
        .unwrap_or(Handle::NULL);
    h.acad_plotsettings_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_PLOTSETTINGS"))
        .unwrap_or(Handle::NULL);
    h.acad_plotstylename_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_PLOTSTYLENAME"))
        .unwrap_or(Handle::NULL);
    h.acad_material_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_MATERIAL"))
        .unwrap_or(Handle::NULL);
    h.acad_color_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_COLOR"))
        .unwrap_or(Handle::NULL);
    h.acad_visualstyle_dict_handle = root_dict_entries
        .and_then(|d| d.get("ACAD_VISUALSTYLE"))
        .unwrap_or(Handle::NULL);

    // ── Sync linetype handles by name ──
    if let Some(lt) = document.line_types.get("ByLayer") {
        h.bylayer_linetype_handle = lt.handle;
    }
    if let Some(lt) = document.line_types.get("ByBlock") {
        h.byblock_linetype_handle = lt.handle;
    }
    if let Some(lt) = document.line_types.get("Continuous") {
        h.continuous_linetype_handle = lt.handle;
    }

    // ── Sync model/paper space block handles ──
    if let Some(br) = document.block_records.get("*Model_Space") {
        h.model_space_block_handle = br.handle;
    }
    if let Some(br) = document.block_records.get("*Paper_Space") {
        h.paper_space_block_handle = br.handle;
    }

    // ── Sync current style handles (validate against actual objects) ──
    // CLAYER: must point to an actual layer; fall back to "0" if invalid
    {
        let clayer_valid = !h.current_layer_handle.is_null()
            && document
                .layers
                .iter()
                .any(|l| l.handle == h.current_layer_handle);
        if !clayer_valid {
            if let Some(layer) = document.layers.get("0") {
                h.current_layer_handle = layer.handle;
            }
        }
    }

    // The header reader can produce garbage (multi-byte) handle values
    // when the bit stream is misaligned.  Unconditionally sync every
    // "current style" handle so that garbage values are overwritten
    // with valid handles from the document model.
    {
        let text_valid = !h.current_text_style_handle.is_null()
            && document
                .text_styles
                .iter()
                .any(|s| s.handle == h.current_text_style_handle);
        if !text_valid {
            h.current_text_style_handle = document
                .text_styles
                .get("Standard")
                .map(|s| s.handle)
                .unwrap_or(Handle::NULL);
        }
    }
    {
        let ds_valid = !h.current_dimstyle_handle.is_null()
            && document
                .dim_styles
                .iter()
                .any(|ds| ds.handle == h.current_dimstyle_handle);
        if !ds_valid {
            h.current_dimstyle_handle = document
                .dim_styles
                .get("Standard")
                .map(|ds| ds.handle)
                .unwrap_or(Handle::NULL);
        }
    }
    {
        let lt_valid = !h.current_linetype_handle.is_null()
            && document
                .line_types
                .iter()
                .any(|lt| lt.handle == h.current_linetype_handle);
        if !lt_valid {
            h.current_linetype_handle = h.bylayer_linetype_handle;
        }
    }
    {
        let mls_valid = !h.current_multiline_style_handle.is_null()
            && document.objects.iter().any(|(_, obj)| {
                if let crate::objects::ObjectType::MLineStyle(mls) = obj {
                    mls.handle == h.current_multiline_style_handle
                } else {
                    false
                }
            });
        if !mls_valid {
            let dictionary_standard = document
                .objects
                .get(&h.acad_mlinestyle_dict_handle)
                .and_then(|object| match object {
                    crate::objects::ObjectType::Dictionary(dictionary) => {
                        dictionary.get("Standard")
                    }
                    _ => None,
                })
                .filter(|handle| {
                    matches!(
                        document.objects.get(handle),
                        Some(crate::objects::ObjectType::MLineStyle(style))
                            if style.handle == *handle
                    )
                });

            h.current_multiline_style_handle = dictionary_standard
                .or_else(|| {
                    document
                        .objects
                        .values()
                        .filter_map(|object| match object {
                            crate::objects::ObjectType::MLineStyle(style)
                                if style.name.eq_ignore_ascii_case("Standard")
                                    && !style.handle.is_null() =>
                            {
                                Some(style.handle)
                            }
                            _ => None,
                        })
                        .min_by_key(|handle| handle.value())
                })
                .unwrap_or(Handle::NULL);
        }
    }

    // R2007+: current_material_handle — validate against emitted objects.
    // Unsupported raw materials may be dropped during a version conversion.
    {
        if !emitted(h.current_material_handle) {
            h.current_material_handle = Handle::NULL;
        }
    }

    // dim_text_style_handle — validate against text styles
    {
        let dts_valid = !h.dim_text_style_handle.is_null()
            && document
                .text_styles
                .iter()
                .any(|s| s.handle == h.dim_text_style_handle);
        if !dts_valid {
            h.dim_text_style_handle = document
                .text_styles
                .get("Standard")
                .map(|s| s.handle)
                .unwrap_or(Handle::NULL);
        }
    }

    // UCS ortho ref handles — validate against UCS table
    {
        let ucs_valid = |handle: Handle| -> bool {
            handle.is_null() || document.ucss.iter().any(|u| u.handle == handle)
        };
        if !ucs_valid(h.paper_ucs_ortho_ref) {
            h.paper_ucs_ortho_ref = Handle::NULL;
        }
        if !ucs_valid(h.ucs_ortho_ref) {
            h.ucs_ortho_ref = Handle::NULL;
        }
    }

    // ── Validate dim linetype handles against actual linetypes ──
    // These can become corrupt during header read/write due to stream alignment.
    {
        let valid_lt = |h: Handle| -> bool {
            h.is_null() || document.line_types.iter().any(|lt| lt.handle == h)
        };
        if !valid_lt(h.dim_linetype_handle) {
            h.dim_linetype_handle = Handle::NULL;
        }
        if !valid_lt(h.dim_linetype1_handle) {
            h.dim_linetype1_handle = Handle::NULL;
        }
        if !valid_lt(h.dim_linetype2_handle) {
            h.dim_linetype2_handle = Handle::NULL;
        }
    }

    // ── Correct HANDSEED ──
    let max_handle = handle_map.iter().map(|&(ha, _)| ha).max().unwrap_or(0);
    if h.handle_seed <= max_handle {
        h.handle_seed = max_handle + 1;
    }

    // ── Update model-space extents ──
    if let Some(ref ext) = extents {
        h.model_space_extents_min = ext.min;
        h.model_space_extents_max = ext.max;
    }

    h
}

/// Find the root dictionary handle by scanning the objects map.
///
/// The root dictionary is a Dictionary with `owner == Handle::NULL`.
/// If multiple candidates exist (e.g., from `initialize_defaults` and
/// from file data), prefer the one with more entries (the file's root dict).
fn find_root_dict_handle(
    objects: &std::collections::HashMap<crate::types::Handle, crate::objects::ObjectType>,
) -> crate::types::Handle {
    use crate::objects::ObjectType;
    use crate::types::Handle;

    let mut best_handle = Handle::NULL;
    let mut best_entry_count = 0usize;

    for (handle, obj) in objects {
        if let ObjectType::Dictionary(dict) = obj {
            if dict.owner.is_null() {
                // Prefer the dictionary with more entries (richer = file's root dict);
                // on tie, prefer higher handle (likely from file, not initialize_defaults)
                if dict.entries.len() > best_entry_count
                    || (dict.entries.len() == best_entry_count
                        && handle.value() > best_handle.value())
                {
                    best_handle = *handle;
                    best_entry_count = dict.entries.len();
                }
            }
        }
    }

    best_handle
}

// ════════════════════════════════════════════════════════════════════════════
//  AC15 format (R13/R14/R2000) — linear file layout
// ════════════════════════════════════════════════════════════════════════════

fn write_ac15<W: Write + Seek>(
    output: &mut W,
    document: &CadDocument,
    version: DxfVersion,
) -> Result<()> {
    let mut fhw = DwgFileHeaderWriterAC15::new(version);
    // New documents do not carry source maintenance metadata. AC15 files use
    // the established R2000-era default rather than zero.
    fhw.set_maintenance_version(if document.maintenance_version == 0 {
        15
    } else {
        document.maintenance_version
    });
    fhw.set_code_page(crate::io::dxf::code_page::dwg_code_page_index(
        &document.header.code_page,
    ));

    // ── Phase 1: Compute objects FIRST to get handle map ──
    let objects_started = web_time::Instant::now();
    let obj_writer = DwgObjectWriter::new(document)?;
    let (
        obj_data,
        handle_map_u32,
        extents,
        _sab_entries,
        class_instance_counts,
        class_counts_complete,
    ) = obj_writer.write_with_class_metadata();
    if std::env::var_os("PERF").is_some() {
        eprintln!(
            "[perf] dwg-write objects={:.1}ms bytes={} handles={} format=ac15",
            objects_started.elapsed().as_secs_f64() * 1000.0,
            obj_data.len(),
            handle_map_u32.len(),
        );
    }

    // ── Phase 2: Prepare header (sync handles + correct HANDSEED) ──
    let corrected_header = prepare_header(document, &handle_map_u32, &extents);

    // ── Section: Header (uses synced + corrected header) ──
    let maint = document.maintenance_version;
    let header_encoding =
        crate::io::dxf::code_page::encoding_from_code_page(&document.header.code_page)
            .unwrap_or(encoding_rs::WINDOWS_1252);
    let header_data = header_writer::write_header_with_encoding(
        version,
        &corrected_header,
        maint,
        header_encoding,
    );
    fhw.add_section(section_names::HEADER, header_data);

    // ── Section: Classes ──
    let classes = reconciled_classes(document, &class_instance_counts, class_counts_complete);
    let classes_data =
        classes_writer::write_classes_with_encoding(version, &classes, maint, header_encoding);
    fhw.add_section(section_names::CLASSES, classes_data);

    // ── Section: AcDbObjects (pre-computed) ──
    fhw.add_section(section_names::ACDB_OBJECTS, obj_data);

    // ── Section: ObjFreeSpace ──
    let obj_free_space = build_obj_free_space(version, document, handle_map_u32.len());
    fhw.add_section(section_names::OBJ_FREE_SPACE, obj_free_space);

    // ── Section: Template ──
    let template = build_template();
    fhw.add_section(section_names::TEMPLATE, template);

    // ── Section: AuxHeader (uses corrected HANDSEED) ──
    let aux_data = aux_header_writer::write_aux_header(version, &corrected_header);
    fhw.add_section(section_names::AUX_HEADER, aux_data);

    // ── Section: Handles (must be last — needs objects offset) ──
    let section_offset = fhw.handle_section_offset() as i32;
    let handle_map_i64: Vec<(u64, i64)> =
        handle_map_u32.iter().map(|&(h, o)| (h, o as i64)).collect();
    let handles_data = handle_writer::write_handles(&handle_map_i64, section_offset);
    fhw.add_section(section_names::HANDLES, handles_data);

    // ── Section: Preview ──
    // Preview is the last section, so its file offset is known now; the
    // container's image `start` fields are absolute file offsets relative to it.
    let preview_base = fhw.pending_section_offset() as u64;
    let preview_data =
        crate::io::dwg::preview::build_preview(document.preview.as_ref(), preview_base);
    fhw.add_section(section_names::PREVIEW, preview_data);

    // ── Write final file ──
    fhw.write_file(output)?;

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
//  AC18 format (R2004/R2010/R2013/R2018) — page-based with LZ77
// ════════════════════════════════════════════════════════════════════════════

fn write_ac18<W: Write + Seek>(
    output: &mut W,
    document: &CadDocument,
    version: DxfVersion,
) -> Result<()> {
    // The maintenance-release version must match the value the AuxHeader
    // writes AND the file-header metadata byte, because readers gate the
    // R2010+ per-section "extra RL" (which locates the header/classes string
    // stream) on `maintenance_version > 3`. A fresh document defaults to 0,
    // which for R2013 (AC1027) omits that RL and makes the header string
    // stream unreadable in AutoCAD/TrueView. Use the canonical per-version
    // value so R2013 files are always well-formed.
    let maint = aux_header_writer::dwg_maintenance_version(version) as u8;

    // AC18 writer reserves 0x100 bytes at file start for metadata
    let mut fhw = DwgFileHeaderWriterAC18::new(version, maint, output)?;
    fhw.set_code_page(crate::io::dxf::code_page::dwg_code_page_index(
        &document.header.code_page,
    ));

    // R2004+ default page size for most sections
    const PAGE_SIZE: usize = 0x7400;
    // Smaller page for metadata-style sections
    const SMALL_PAGE: usize = 0x80;

    // ── Phase 1: Compute objects FIRST to get handle map ──
    let objects_started = web_time::Instant::now();
    let obj_writer = DwgObjectWriter::new(document)?;
    let (
        obj_data,
        handle_map_u32,
        extents,
        sab_entries,
        class_instance_counts,
        class_counts_complete,
    ) = obj_writer.write_with_class_metadata();
    if std::env::var_os("PERF").is_some() {
        eprintln!(
            "[perf] dwg-write objects={:.1}ms bytes={} handles={} format=ac18",
            objects_started.elapsed().as_secs_f64() * 1000.0,
            obj_data.len(),
            handle_map_u32.len(),
        );
    }

    // ── Phase 2: Prepare header (sync handles + correct HANDSEED) ──
    let corrected_header = prepare_header(document, &handle_map_u32, &extents);

    // ── Section: Header (uses synced + corrected header) ──
    let header_encoding =
        crate::io::dxf::code_page::encoding_from_code_page(&document.header.code_page)
            .unwrap_or(encoding_rs::WINDOWS_1252);
    let header_data = header_writer::write_header_with_encoding(
        version,
        &corrected_header,
        maint,
        header_encoding,
    );
    fhw.add_section(output, section_names::HEADER, &header_data, true, PAGE_SIZE)?;

    // ── Section: Classes ──
    let classes = reconciled_classes(document, &class_instance_counts, class_counts_complete);
    let classes_data =
        classes_writer::write_classes_with_encoding(version, &classes, maint, header_encoding);
    fhw.add_section(
        output,
        section_names::CLASSES,
        &classes_data,
        true,
        PAGE_SIZE,
    )?;

    // ── Section: SummaryInfo ──
    let summary_data = build_summary_info(version);
    fhw.add_section(
        output,
        section_names::SUMMARY_INFO,
        &summary_data,
        false,
        0x100,
    )?;

    // ── Section: Preview ──
    // The image `start` fields are absolute file offsets, so the preview page's
    // data position must be known BEFORE building the container (the ODA page
    // checksum covers the final bytes — patching offsets afterward would break
    // it). `add_section` aligns via `write_magic_number` (pads by `pos % 0x20`)
    // then writes a 0x20 page header; the header's preview seeker points at
    // `page + 0x20`, which is where the container lands.
    let cur = output.seek(std::io::SeekFrom::Current(0))? as u64;
    let preview_base = (cur + cur % 0x20) + 0x20;
    let preview_data =
        crate::io::dwg::preview::build_preview(document.preview.as_ref(), preview_base);
    // Keep the whole preview in one contiguous page (a split would scatter the
    // container across page headers): a decompressed size ≥ its length, rounded
    // up to a 0x20 multiple so the uncompressed page needs no compression pad.
    let preview_page = ((preview_data.len() + 0x1F) & !0x1F).max(0x20);
    fhw.add_section(
        output,
        section_names::PREVIEW,
        &preview_data,
        false,
        preview_page,
    )?;

    // ── Section: AppInfo ──
    let app_info_data = app_info_writer::write_app_info(version);
    fhw.add_section(
        output,
        section_names::APP_INFO,
        &app_info_data,
        false,
        SMALL_PAGE,
    )?;

    // ── Section: FileDepList ──
    let file_dep_data = build_file_dep_list();
    fhw.add_section(
        output,
        section_names::FILE_DEP_LIST,
        &file_dep_data,
        false,
        SMALL_PAGE,
    )?;

    // ── Section: RevHistory ──
    let rev_history_data = build_rev_history();
    fhw.add_section(
        output,
        section_names::REV_HISTORY,
        &rev_history_data,
        true,
        PAGE_SIZE,
    )?;

    // ── Section: AuxHeader (uses corrected HANDSEED) ──
    let aux_data = aux_header_writer::write_aux_header(version, &corrected_header);
    fhw.add_section(
        output,
        section_names::AUX_HEADER,
        &aux_data,
        true,
        PAGE_SIZE,
    )?;

    // ── Section: AcDbObjects (pre-computed) ──
    fhw.add_section(
        output,
        section_names::ACDB_OBJECTS,
        &obj_data,
        true,
        PAGE_SIZE,
    )?;

    // ── Section: AcDsPrototype_1b (AC1027+ ACIS SAB storage) ──
    if !sab_entries.is_empty()
        || (document.dwg_source_version == Some(version) && document.raw_acds_data.is_some())
    {
        let acds_data = acds_data(document, version, &sab_entries);
        fhw.add_section(
            output,
            section_names::ACDS_PROTOTYPE,
            &acds_data,
            true,
            PAGE_SIZE,
        )?;
    }

    // ── Section: ObjFreeSpace ──
    let obj_free_space = build_obj_free_space(version, document, handle_map_u32.len());
    fhw.add_section(
        output,
        section_names::OBJ_FREE_SPACE,
        &obj_free_space,
        true,
        PAGE_SIZE,
    )?;

    // ── Section: Template ──
    let template = build_template();
    fhw.add_section(output, section_names::TEMPLATE, &template, true, PAGE_SIZE)?;

    // ── Section: Handles (last — needs objects data) ──
    let section_offset = fhw.handle_section_offset() as i32;
    let handle_map_i64: Vec<(u64, i64)> =
        handle_map_u32.iter().map(|&(h, o)| (h, o as i64)).collect();
    let handles_data = handle_writer::write_handles(&handle_map_i64, section_offset);
    fhw.add_section(
        output,
        section_names::HANDLES,
        &handles_data,
        true,
        PAGE_SIZE,
    )?;

    // ── Write file header, section map, and page map ──
    fhw.write_file(output)?;

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
//  AC21 format (R2007) — RS-encoded pages with LZ77 AC21 compression
// ════════════════════════════════════════════════════════════════════════════

fn write_ac21<W: Write + Seek>(
    output: &mut W,
    document: &CadDocument,
    version: DxfVersion,
) -> Result<()> {
    write_ac21_impl(output, document, version, false)
}

fn write_ac21_impl<W: Write + Seek>(
    output: &mut W,
    document: &CadDocument,
    version: DxfVersion,
    skip_lz77: bool,
) -> Result<()> {
    // AC21 writer reserves 0x480 bytes at file start (0x80 metadata + 0x400 file header)
    let mut fhw = DwgFileHeaderWriterAC21::new(version, output)?;
    fhw.skip_lz77 = skip_lz77;

    // ── Phase 1: Compute objects FIRST to get handle map ──
    let objects_started = web_time::Instant::now();
    let obj_writer = DwgObjectWriter::new(document)?;
    let (
        obj_data,
        handle_map_u32,
        extents,
        sab_entries,
        class_instance_counts,
        class_counts_complete,
    ) = obj_writer.write_with_class_metadata();
    if std::env::var_os("PERF").is_some() {
        eprintln!(
            "[perf] dwg-write objects={:.1}ms bytes={} handles={} format=ac21",
            objects_started.elapsed().as_secs_f64() * 1000.0,
            obj_data.len(),
            handle_map_u32.len(),
        );
    }

    // ── Phase 2: Prepare header (sync handles + correct HANDSEED) ──
    let corrected_header = prepare_header(document, &handle_map_u32, &extents);

    // ── Sections in spec §5.1 stream order ──
    // AC21 add_section looks up encoding/encryption/page_size automatically
    // from ac21_section_info, so no page_size or compressed flag needed.

    // SummaryInfo
    let summary_data = build_summary_info(version);
    fhw.add_section(output, section_names::SUMMARY_INFO, &summary_data)?;

    // Preview
    // AC21 `encoding=1` sections store raw, un-RS-encoded, 32-byte-aligned data,
    // so the preview container lands contiguously at the current file position
    // (full 1024-byte pages leave no gaps) and its seeker in the header points
    // straight at it. That position is the `base` the container's absolute image
    // `start` offsets are relative to; compute it before building (the per-page
    // checksum covers the final bytes).
    let preview_base = output.seek(std::io::SeekFrom::Current(0))? as u64;
    let preview_data =
        crate::io::dwg::preview::build_preview(document.preview.as_ref(), preview_base);
    fhw.add_section(output, section_names::PREVIEW, &preview_data)?;

    // AppInfo
    let app_info_data = app_info_writer::write_app_info(version);
    fhw.add_section(output, section_names::APP_INFO, &app_info_data)?;

    // FileDepList
    let file_dep_data = build_file_dep_list();
    fhw.add_section(output, section_names::FILE_DEP_LIST, &file_dep_data)?;

    // RevHistory
    let rev_history_data = build_rev_history();
    fhw.add_section(output, section_names::REV_HISTORY, &rev_history_data)?;

    // AcDbObjects (pre-computed)
    fhw.add_section(output, section_names::ACDB_OBJECTS, &obj_data)?;

    // AcDsPrototype_1b (AC1027+ ACIS SAB storage)
    if !sab_entries.is_empty()
        || (document.dwg_source_version == Some(version) && document.raw_acds_data.is_some())
    {
        let acds_data = acds_data(document, version, &sab_entries);
        fhw.add_section(output, section_names::ACDS_PROTOTYPE, &acds_data)?;
    }

    // ObjFreeSpace
    let obj_free_space = build_obj_free_space(version, document, handle_map_u32.len());
    fhw.add_section(output, section_names::OBJ_FREE_SPACE, &obj_free_space)?;

    // Template
    let template = build_template();
    fhw.add_section(output, section_names::TEMPLATE, &template)?;

    // Handles (needs objects data for offsets)
    let section_offset = fhw.handle_section_offset() as i32;
    let handle_map_i64: Vec<(u64, i64)> =
        handle_map_u32.iter().map(|&(h, o)| (h, o as i64)).collect();
    let handles_data = handle_writer::write_handles(&handle_map_i64, section_offset);
    fhw.add_section(output, section_names::HANDLES, &handles_data)?;

    // Classes
    let classes = reconciled_classes(document, &class_instance_counts, class_counts_complete);
    let maint = document.maintenance_version;
    let header_encoding =
        crate::io::dxf::code_page::encoding_from_code_page(&document.header.code_page)
            .unwrap_or(encoding_rs::WINDOWS_1252);
    let classes_data =
        classes_writer::write_classes_with_encoding(version, &classes, maint, header_encoding);
    fhw.add_section(output, section_names::CLASSES, &classes_data)?;

    // AuxHeader (uses corrected HANDSEED)
    let aux_data = aux_header_writer::write_aux_header(version, &corrected_header);
    fhw.add_section(output, section_names::AUX_HEADER, &aux_data)?;

    // Header (uses corrected HANDSEED)
    let header_data = header_writer::write_header_with_encoding(
        version,
        &corrected_header,
        maint,
        header_encoding,
    );
    fhw.add_section(output, section_names::HEADER, &header_data)?;

    // ── Finalize: section map, page map, file header, metadata ──
    fhw.write_file(output)?;

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
//  Section data builders for simple/metadata sections
// ════════════════════════════════════════════════════════════════════════════

/// Build ObjFreeSpace section data.
///
/// Contains approximate object count and a fixed data template.
/// Matches the reference `writeObjFreeSpace`.
fn build_obj_free_space(
    version: DxfVersion,
    document: &CadDocument,
    handle_count: usize,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(64);

    // Int32: 0
    data.extend_from_slice(&0i32.to_le_bytes());
    // UInt32: approximate number of objects (handles)
    data.extend_from_slice(&(handle_count as u32).to_le_bytes());

    // Julian datetime (8 bytes)
    // For simplicity, write zeros (ODA-compatible)
    if version >= DxfVersion::AC1015 {
        let _ = &document.header; // future: use TDUPDATE
    }
    data.extend_from_slice(&0i32.to_le_bytes()); // jdate
    data.extend_from_slice(&0i32.to_le_bytes()); // milli

    // UInt32: offset of objects section (0 for paged format)
    data.extend_from_slice(&0u32.to_le_bytes());

    // UInt8: number of 64-bit values that follow (ODA writes 4)
    data.push(4);
    // 4 × (u32 + u32) = 4 × 8 bytes of fixed ODA values
    data.extend_from_slice(&0x00000032u32.to_le_bytes());
    data.extend_from_slice(&0x00000000u32.to_le_bytes());
    data.extend_from_slice(&0x00000064u32.to_le_bytes());
    data.extend_from_slice(&0x00000000u32.to_le_bytes());
    data.extend_from_slice(&0x00000200u32.to_le_bytes());
    data.extend_from_slice(&0x00000000u32.to_le_bytes());
    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    data.extend_from_slice(&0x00000000u32.to_le_bytes());

    data
}

/// Build Template section data.
///
/// Contains template description length (0 = no template).
/// AutoCAD reference files only write the 4-byte RL length with no
/// MEASUREMENT field when the template description is empty.
fn build_template() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    // RL (raw long = 4 bytes): template description length (0)
    data.extend_from_slice(&0i32.to_le_bytes());
    data
}

/// Build SummaryInfo section data (AC18+ only).
///
/// Writes empty summary info fields (all empty strings).
///
/// **AC1018 (R2004)**: Windows-1252 (ANSI) strings.
///   Format: UInt16(byte_count_incl_null) + bytes + null.
///   Empty → UInt16(1) + 0x00 = 3 bytes.
///
/// **AC1021 (R2007)**: UTF-16LE strings.
///   Format: UInt16(char_count_incl_null) + UTF-16LE chars.
///   Empty → UInt16(1) + 0x00 0x00 = 4 bytes.
fn build_summary_info(version: DxfVersion) -> Vec<u8> {
    let mut data = Vec::with_capacity(128);
    let is_utf16 = version >= DxfVersion::AC1021;

    // 8 × empty strings
    // Title, Subject, Author, Keywords, Comments, LastSavedBy, RevisionNumber, HyperlinkBase
    for _ in 0..8 {
        data.extend_from_slice(&1u16.to_le_bytes()); // char/byte count including null
        if is_utf16 {
            // UTF-16LE null terminator: 2 bytes
            data.push(0);
            data.push(0);
        } else {
            // ANSI null terminator: 1 byte
            data.push(0);
        }
    }

    // Total editing time: 2 × Int32 (zeros)
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());

    // Created date: 8 bytes (zeros)
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());

    // Modified date: 8 bytes (zeros)
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());

    // Property count: Int16 (0)
    data.extend_from_slice(&0u16.to_le_bytes());

    // 2 × Int32 (trailing zeros)
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());

    data
}

/// Build FileDepList section data (AC18+ only).
///
/// Empty dependency list (0 features, 0 files).
fn build_file_dep_list() -> Vec<u8> {
    let mut data = Vec::with_capacity(8);
    // Int32: feature count (0)
    data.extend_from_slice(&0u32.to_le_bytes());
    // Int32: file count (0)
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

/// Build RevHistory section data (AC18+ only).
///
/// Empty revision history (3 × Int32 zeros).
fn build_rev_history() -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes()); // revision counter
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

/// Build AcDsPrototype_1b section data (AC1027+ only).
///
/// This section stores SAB binary ACIS data for 3DSOLID, REGION, and BODY
/// entities in R2013+ DWG files.  The entity stream writes `acis_empty=true`
/// and the actual SAB data lives here, linked by entity handle.
///
/// ## Binary format (reverse-engineered from IntelliCAD-saved reference files)
///
/// The section consists of a 128-byte "jard" header followed by 7 segments:
///
/// | Segment   | ID | Description                                   |
/// |-----------|----|-----------------------------------------------|
/// | `_data_`  | 2  | SAB binary data records (one per entity)      |
/// | `_data_`  | 3  | Thumbnail data table (empty, boilerplate)     |
/// | `datidx`  | 4  | Data index                                    |
/// | `schdat`  | 5  | Schema column definitions                     |
/// | `schidx`  | 6  | Schema index + schema names                   |
/// | `search`  | 7  | Handle-based search/lookup index              |
/// | `segidx`  | 1  | Segment index (offsets of all other segments)  |
///
/// Each segment has a 48-byte header:
///   `marker[8] + id[4] + pad[4] + size[8] + records[8] + meta[8] + fill[8]`
///
/// Segments are padded with 0x70 bytes to 16-byte alignment.
fn build_acds_prototype(sab_entries: &[(Handle, Vec<u8>)]) -> Vec<u8> {
    if sab_entries.is_empty() {
        return Vec::new();
    }

    // ── Segment 1: _data_ id=2 (SAB data records, one per ACIS entity) ──
    let data2 = build_acds_data2_segment(sab_entries);

    // ── Segment 2: _data_ id=3 (thumbnail, empty boilerplate) ─────
    #[rustfmt::skip]
    let data3: &[u8] = &[
        0xAC, 0xD5, 0x5F, 0x64, 0x61, 0x74, 0x61, 0x5F, // marker "_data_"
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // id=3
        0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // size=64
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // records=1
        0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, // meta: 0, cols=4
        0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, // fill "UUUUUUUU"
        0x62, 0x62, 0x62, 0x62, 0x62, 0x62, 0x62, 0x62, // content "bbbb..."
        0x62, 0x62, 0x62, 0x62, 0x62, 0x62, 0x62, 0x62,
    ];

    // ── Segment 3: datidx id=4 ────────────────────────────────────
    let datidx = build_acds_datidx(sab_entries.len());

    // ── Segment 4: schdat id=5 (schema definitions, fixed) ────────
    let schdat = ACDS_SCHDAT_TEMPLATE;

    // ── Segment 5: schidx id=6 (schema index, fixed) ─────────────
    let schidx = ACDS_SCHIDX_TEMPLATE;

    // ── Segment 6: search id=7 ───────────────────────────────────
    let handles: Vec<u32> = sab_entries.iter().map(|(h, _)| h.value() as u32).collect();
    let search = build_acds_search_segment(&handles);

    // ── Segment 7: segidx id=1 ───────────────────────────────────
    // Compute offsets (all relative to section start = after jard header)
    let off_data2 = 0x80u32;
    let off_data3 = off_data2 + data2.len() as u32;
    let off_datidx = off_data3 + data3.len() as u32;
    let off_schdat = off_datidx + datidx.len() as u32;
    let off_schidx = off_schdat + schdat.len() as u32;
    let off_search = off_schidx + schidx.len() as u32;
    let off_segidx = off_search + search.len() as u32;
    let segidx_size = 192u32;

    let segidx = build_acds_segidx(
        off_segidx,
        segidx_size,
        off_data2,
        data2.len() as u32,
        off_data3,
        data3.len() as u32,
        off_datidx,
        datidx.len() as u32,
        off_schdat,
        schdat.len() as u32,
        off_schidx,
        schidx.len() as u32,
        off_search,
        search.len() as u32,
    );

    // ── Jard header ──────────────────────────────────────────────
    let total_size = off_segidx + segidx_size;
    let segidx_offset = off_segidx;
    let header = build_acds_jard_header(segidx_offset, total_size);

    // ── Assemble ─────────────────────────────────────────────────
    let mut result = Vec::with_capacity(total_size as usize);
    result.extend_from_slice(&header);
    result.extend_from_slice(&data2);
    result.extend_from_slice(data3);
    result.extend_from_slice(&datidx);
    result.extend_from_slice(schdat);
    result.extend_from_slice(schidx);
    result.extend_from_slice(&search);
    result.extend_from_slice(&segidx);

    debug_assert_eq!(result.len(), total_size as usize);
    result
}

/// Build the AcDsPrototype_1b file header ("jard", 128 bytes).
///
/// Fourteen little-endian `RL` fields per the ODA datastore layout (field names
/// follow libredwg's `acds.spec`), then zero padding to 128 bytes. A strict
/// reader validates these; a wrong `ds_version` in particular
/// made the whole section read as "invalid data". The segment ordering emitted
/// by [`build_acds_prototype`] is fixed (segidx=1, _data_=2, thumbnail=3,
/// datidx=4, schdat=5, schidx=6, search=7), so the `*_segidx` pointers are
/// constant.
fn build_acds_jard_header(segidx_offset: u32, file_size: u32) -> Vec<u8> {
    let mut h = vec![0u8; 128];
    h[0..4].copy_from_slice(b"jard"); // file_signature
    h[4..8].copy_from_slice(&128u32.to_le_bytes()); // file_header_size
    h[8..12].copy_from_slice(&2u32.to_le_bytes()); // unknown_1 (always 2)
    h[12..16].copy_from_slice(&2u32.to_le_bytes()); // version (always 2)
    h[16..20].copy_from_slice(&0u32.to_le_bytes()); // unknown_2 (always 0)
    h[20..24].copy_from_slice(&1u32.to_le_bytes()); // ds_version (datastore revision)
    h[24..28].copy_from_slice(&segidx_offset.to_le_bytes()); // segidx_offset
    h[28..32].copy_from_slice(&0u32.to_le_bytes()); // segidx_unknown
    h[32..36].copy_from_slice(&8u32.to_le_bytes()); // num_segidx (null + 7 segments)
    h[36..40].copy_from_slice(&6u32.to_le_bytes()); // schidx_segidx
    h[40..44].copy_from_slice(&4u32.to_le_bytes()); // datidx_segidx
    h[44..48].copy_from_slice(&7u32.to_le_bytes()); // search_segidx
    h[48..52].copy_from_slice(&0u32.to_le_bytes()); // prvsav_segidx
    h[52..56].copy_from_slice(&file_size.to_le_bytes()); // file_size
                                                         // Remaining bytes are zero (padding to file_header_size).
    h
}

/// Build `_data_` segment id=2 containing one SAB record per ACIS entity.
///
/// Each record is a 36-byte metadata block (record index, entity handle, blob
/// size) followed by the raw SAB blob; records are concatenated in entity
/// order and the whole segment is padded to a 16-byte boundary.
fn build_acds_data2_segment(entries: &[(Handle, Vec<u8>)]) -> Vec<u8> {
    // Raw size = 48 (segment header) + Σ (36 metadata + blob) per record.
    let records_size: usize = entries.iter().map(|(_, sab)| 36 + sab.len()).sum();
    let raw_size = 48 + records_size;
    let seg_size = align16(raw_size);
    let padding = seg_size - raw_size;

    let mut seg = Vec::with_capacity(seg_size);

    // Segment header (48 bytes)
    seg.extend_from_slice(&[0xAC, 0xD5, 0x5F, 0x64, 0x61, 0x74, 0x61, 0x5F]); // "_data_"
    seg.extend_from_slice(&2u32.to_le_bytes()); // segment_idx=2
    seg.extend_from_slice(&0u32.to_le_bytes()); // is_blob01
    seg.extend_from_slice(&(seg_size as u64).to_le_bytes()); // segment size
    seg.extend_from_slice(&1u32.to_le_bytes()); // ds_version (always 1, NOT the record count)
    seg.extend_from_slice(&0u32.to_le_bytes()); // unknown_3
    seg.extend_from_slice(&0u32.to_le_bytes()); // meta field1 = 0
    seg.extend_from_slice(&5u32.to_le_bytes()); // meta field2 = 5 (num columns)
    seg.extend_from_slice(&[0x55; 8]); // fill "UUUUUUUU"

    for (i, (handle, sab_data)) in entries.iter().enumerate() {
        let handle_val = handle.value() as u32;
        // Record metadata (36 bytes)
        seg.extend_from_slice(&0x14u32.to_le_bytes()); // col0 = 20
        seg.extend_from_slice(&((i + 1) as u32).to_le_bytes()); // col1 = record index (1-based)
        seg.extend_from_slice(&(handle_val as u64).to_le_bytes()); // col2 = entity handle
        seg.extend_from_slice(&0u32.to_le_bytes()); // col3 = 0
        seg.extend_from_slice(&[0x62; 12]); // col4 fill "bbbbbbbbbbbb"
        seg.extend_from_slice(&(sab_data.len() as u32).to_le_bytes()); // SAB blob size
        seg.extend_from_slice(sab_data); // SAB binary data
    }

    // Padding with 0x70 to 16-byte alignment
    seg.extend(std::iter::repeat(0x70u8).take(padding));

    debug_assert_eq!(seg.len(), seg_size);
    seg
}

/// Write the 48-byte AcDs segment header (signature + 6-char name + the fixed
/// field block: segment_idx, is_blob01=0, segsize, unknown_2=0, ds_version=1,
/// unknown_3=0, align offsets=0, 8× 0x55 fill).
fn acds_segment_header(seg: &mut [u8], name: &[u8; 6], segment_idx: u32) {
    let seg_len = seg.len() as u64;
    seg[0..2].copy_from_slice(&[0xAC, 0xD5]);
    seg[2..8].copy_from_slice(name);
    seg[8..12].copy_from_slice(&segment_idx.to_le_bytes());
    seg[12..16].copy_from_slice(&0u32.to_le_bytes()); // is_blob01
    seg[16..24].copy_from_slice(&seg_len.to_le_bytes()); // segsize + unknown_2
    seg[24..28].copy_from_slice(&1u32.to_le_bytes()); // ds_version (always 1)
    seg[28..32].copy_from_slice(&0u32.to_le_bytes()); // unknown_3
    seg[32..40].copy_from_slice(&0u64.to_le_bytes()); // data/objdata align offsets
    seg[40..48].copy_from_slice(&[0x55; 8]); // fill
}

/// Build `datidx` segment id=4 — one index entry per ACIS record.
///
/// Layout (matches the documented datastore reference): the 48-byte header,
/// then `num_entries` (RL), `di_unknown` (RL = 0), then one 12-byte index entry
/// per record — `(segidx = 2 [the _data_ segment], offset = i*20, schidx = 1)`.
fn build_acds_datidx(num_records: usize) -> Vec<u8> {
    let num = num_records.max(1);
    let raw = 48 + 8 + num * 12;
    let seg_size = align16(raw).max(128);
    let mut seg = vec![0x70u8; seg_size];
    acds_segment_header(&mut seg, b"datidx", 4);

    seg[48..52].copy_from_slice(&(num as u32).to_le_bytes()); // num_entries
    seg[52..56].copy_from_slice(&0u32.to_le_bytes()); // di_unknown
    let mut pos = 56;
    for i in 0..num {
        seg[pos..pos + 4].copy_from_slice(&2u32.to_le_bytes()); // segidx (→ _data_ id=2)
        seg[pos + 4..pos + 8].copy_from_slice(&((i as u32) * 20).to_le_bytes()); // offset
        seg[pos + 8..pos + 12].copy_from_slice(&1u32.to_le_bytes()); // schidx
        pos += 12;
    }
    seg
}

/// Build `search` segment id=7 — the datastore's two per-schema sorted lookup
/// indexes (matching the documented DWG datastore layout).
///
/// Both schemas start with `namidx (RL)` + `count (RL)`. Schema 0 (the record-ID
/// schema, namidx=1) then holds `count` 8-byte keys `i << 32` (sorted by record
/// index) followed by `num_ididxs = 0` and `unknown = 1`. Schema 1 (the data
/// schema, namidx=0) holds `count` 24-byte entries `(handle, 1, record_index)`
/// **sorted ascending by handle** — the handle→record lookup — followed by a
/// fixed 24-byte tail. `handles[i]` is the handle of the i-th SAB record, so the
/// record index stored is the pre-sort position.
fn build_acds_search_segment(handles: &[u32]) -> Vec<u8> {
    let n = handles.len();
    let mut content: Vec<u8> = Vec::new();
    content.extend_from_slice(&2u32.to_le_bytes()); // num_search = 2 schemas

    // Schema 0: keyed by record index.
    content.extend_from_slice(&1u32.to_le_bytes()); // schema_namidx
    content.extend_from_slice(&(n as u32).to_le_bytes()); // count
    for i in 0..n {
        content.extend_from_slice(&((i as u64) << 32).to_le_bytes());
    }
    content.extend_from_slice(&0u32.to_le_bytes()); // num_ididxs
    content.extend_from_slice(&1u32.to_le_bytes()); // unknown

    // Schema 1: keyed by entity handle → (handle, 1, record_index), sorted by
    // handle so a reader can binary-search a handle to its SAB record.
    content.extend_from_slice(&0u32.to_le_bytes()); // schema_namidx
    content.extend_from_slice(&(n as u32).to_le_bytes()); // count
    let mut by_handle: Vec<(u32, usize)> = handles
        .iter()
        .copied()
        .enumerate()
        .map(|(i, h)| (h, i))
        .collect();
    by_handle.sort_by_key(|&(h, _)| h);
    for &(handle, record_index) in &by_handle {
        content.extend_from_slice(&(handle as u64).to_le_bytes());
        content.extend_from_slice(&1u64.to_le_bytes());
        content.extend_from_slice(&(record_index as u64).to_le_bytes());
    }
    // Fixed 24-byte schema-1 tail from the reference layout.
    for v in [0u32, 0, 0, 1, 0, 0] {
        content.extend_from_slice(&v.to_le_bytes());
    }

    let raw = 48 + content.len();
    let seg_size = align16(raw).max(192);
    let mut seg = vec![0x70u8; seg_size];
    acds_segment_header(&mut seg, b"search", 7);
    seg[48..48 + content.len()].copy_from_slice(&content);
    seg
}

/// Build `segidx` segment id=1 with offsets for all other segments.
#[allow(clippy::too_many_arguments)]
fn build_acds_segidx(
    off_segidx: u32,
    sz_segidx: u32,
    off_data2: u32,
    sz_data2: u32,
    off_data3: u32,
    sz_data3: u32,
    off_datidx: u32,
    sz_datidx: u32,
    off_schdat: u32,
    sz_schdat: u32,
    off_schidx: u32,
    sz_schidx: u32,
    off_search: u32,
    sz_search: u32,
) -> Vec<u8> {
    let mut seg = vec![0x70u8; sz_segidx as usize];

    // Segment header
    seg[0..8].copy_from_slice(&[0xAC, 0xD5, 0x73, 0x65, 0x67, 0x69, 0x64, 0x78]); // "segidx"
    seg[8..12].copy_from_slice(&1u32.to_le_bytes()); // id=1
    seg[12..16].copy_from_slice(&0u32.to_le_bytes()); // pad
    seg[16..24].copy_from_slice(&(sz_segidx as u64).to_le_bytes()); // segment size
    seg[24..32].copy_from_slice(&1u64.to_le_bytes()); // record count
    seg[32..40].copy_from_slice(&0u64.to_le_bytes()); // meta
    seg[40..48].copy_from_slice(&[0x55; 8]); // fill

    // Content: 8 entries × 12 bytes = 96 bytes
    // Entry format: (u32 offset, u32 pad=0, u32 size)
    let mut pos = 48;

    // Entry 0: null
    write_segidx_entry(&mut seg, pos, 0, 0);
    pos += 12;
    // Entry 1: segidx
    write_segidx_entry(&mut seg, pos, off_segidx, sz_segidx);
    pos += 12;
    // Entry 2: _data_ id=2
    write_segidx_entry(&mut seg, pos, off_data2, sz_data2);
    pos += 12;
    // Entry 3: _data_ id=3
    write_segidx_entry(&mut seg, pos, off_data3, sz_data3);
    pos += 12;
    // Entry 4: datidx
    write_segidx_entry(&mut seg, pos, off_datidx, sz_datidx);
    pos += 12;
    // Entry 5: schdat
    write_segidx_entry(&mut seg, pos, off_schdat, sz_schdat);
    pos += 12;
    // Entry 6: schidx
    write_segidx_entry(&mut seg, pos, off_schidx, sz_schidx);
    pos += 12;
    // Entry 7: search
    write_segidx_entry(&mut seg, pos, off_search, sz_search);
    // Rest is padding (already 0x70)

    seg
}

/// Write one segidx entry: (offset u32, pad u32, size u32).
fn write_segidx_entry(buf: &mut [u8], pos: usize, offset: u32, size: u32) {
    buf[pos..pos + 4].copy_from_slice(&offset.to_le_bytes());
    buf[pos + 4..pos + 8].copy_from_slice(&0u32.to_le_bytes());
    buf[pos + 8..pos + 12].copy_from_slice(&size.to_le_bytes());
}

/// Round up to next 16-byte boundary.
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Schema data template (448 bytes) — fixed content defining column
/// types and names for AcDb_Thumbnail_Schema and AcDb3DSolid_ASM_Data.
#[rustfmt::skip]
const ACDS_SCHDAT_TEMPLATE: &[u8] = &[
    // Segment header
    0xAC, 0xD5, 0x73, 0x63, 0x68, 0x64, 0x61, 0x74, // "schdat"
    0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // id=5
    0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // size=448
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // records=1
    0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // meta: field_count=20
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, // fill
    // Column type definitions (8 bytes each: type u32, flags u32)
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Schema field descriptors
    0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Schema records (sub-structures)
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x08, 0x00, 0x00, 0x00, 0x06, 0x00,
    0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00,
    // NUL-terminated schema field name strings
    0x73, 0x73, 0x73, // "sss" (separator/padding?)
    0x07, 0x00, 0x00, 0x00,
    // "AcDbDs::ID"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x49, 0x44, 0x00,
    // "Thumbnail_Data"
    0x54, 0x68, 0x75, 0x6D, 0x62, 0x6E, 0x61, 0x69, 0x6C, 0x5F, 0x44, 0x61, 0x74, 0x61, 0x00,
    // "ASM_Data"
    0x41, 0x53, 0x4D, 0x5F, 0x44, 0x61, 0x74, 0x61, 0x00,
    // "AcDbDs::TreatedAsObjectData"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A,
    0x54, 0x72, 0x65, 0x61, 0x74, 0x65, 0x64, 0x41, 0x73, 0x4F, 0x62, 0x6A, 0x65, 0x63, 0x74, 0x44,
    0x61, 0x74, 0x61, 0x00,
    // "AcDbDs::Legacy"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x4C, 0x65, 0x67, 0x61, 0x63, 0x79, 0x00,
    // "AcDs:Indexable"
    0x41, 0x63, 0x44, 0x73, 0x3A, 0x49, 0x6E, 0x64, 0x65, 0x78, 0x61, 0x62, 0x6C, 0x65, 0x00,
    // "AcDbDs::HandleAttribute"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x48, 0x61, 0x6E, 0x64, 0x6C, 0x65, 0x41,
    0x74, 0x74, 0x72, 0x69, 0x62, 0x75, 0x74, 0x65, 0x00,
    // Padding to 448 bytes
    0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70,
];

/// Schema index template (448 bytes) — fixed content listing schema
/// names and column offset tables.
#[rustfmt::skip]
const ACDS_SCHIDX_TEMPLATE: &[u8] = &[
    // Segment header
    0xAC, 0xD5, 0x73, 0x63, 0x68, 0x69, 0x64, 0x78, // "schidx"
    0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // id=6
    0xC0, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // size=448
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // records=1
    0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // meta: 15
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, // fill
    // Schema index content
    0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x40, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0xD2, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0xE4, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0xF6, 0x00, 0x00, 0x00,
    0x0C, 0xF1, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x18, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x28, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x38, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    // Schema names (NUL-terminated strings)
    0x06, 0x00, 0x00, 0x00,
    // "AcDb_Thumbnail_Schema"
    0x41, 0x63, 0x44, 0x62, 0x5F, 0x54, 0x68, 0x75, 0x6D, 0x62, 0x6E, 0x61,
    0x69, 0x6C, 0x5F, 0x53, 0x63, 0x68, 0x65, 0x6D, 0x61, 0x00,
    // "AcDb3DSolid_ASM_Data"
    0x41, 0x63, 0x44, 0x62, 0x33, 0x44,
    0x53, 0x6F, 0x6C, 0x69, 0x64, 0x5F, 0x41, 0x53, 0x4D, 0x5F, 0x44, 0x61, 0x74, 0x61, 0x00,
    // "AcDbDs::TreatedAsObjectDataSchema"
    0x41,
    0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x54, 0x72, 0x65, 0x61, 0x74, 0x65, 0x64, 0x41, 0x73,
    0x4F, 0x62, 0x6A, 0x65, 0x63, 0x74, 0x44, 0x61, 0x74, 0x61, 0x53, 0x63, 0x68, 0x65, 0x6D, 0x61,
    0x00,
    // "AcDbDs::LegacySchema"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x4C, 0x65, 0x67, 0x61, 0x63, 0x79,
    0x53, 0x63, 0x68, 0x65, 0x6D, 0x61, 0x00,
    // "AcDbDs::IndexedPropertySchema"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x49, 0x6E,
    0x64, 0x65, 0x78, 0x65, 0x64, 0x50, 0x72, 0x6F, 0x70, 0x65, 0x72, 0x74, 0x79, 0x53, 0x63, 0x68,
    0x65, 0x6D, 0x61, 0x00,
    // "AcDbDs::HandleAttributeSchema"
    0x41, 0x63, 0x44, 0x62, 0x44, 0x73, 0x3A, 0x3A, 0x48, 0x61, 0x6E, 0x64,
    0x6C, 0x65, 0x41, 0x74, 0x74, 0x72, 0x69, 0x62, 0x75, 0x74, 0x65, 0x53, 0x63, 0x68, 0x65, 0x6D,
    0x61, 0x00,
    // Padding
    0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70,
    0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70,
    0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70, 0x70,
];

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadDocument;
    use crate::types::{DxfVersion, Handle};

    #[test]
    fn test_validate_version_r2007_ok() {
        assert!(validate_version(DxfVersion::AC1021).is_ok());
    }

    #[test]
    fn test_validate_version_unknown_rejected() {
        let result = validate_version(DxfVersion::Unknown);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_version_r2000_ok() {
        assert!(validate_version(DxfVersion::AC1015).is_ok());
    }

    #[test]
    fn test_validate_version_r2004_ok() {
        assert!(validate_version(DxfVersion::AC1018).is_ok());
    }

    #[test]
    fn test_validate_version_r2010_ok() {
        assert!(validate_version(DxfVersion::AC1024).is_ok());
    }

    #[test]
    fn class_metadata_matches_emitted_records() {
        let document = CadDocument::with_version(DxfVersion::AC1032);
        let action_param = document
            .classes
            .get_by_name("ACDBASSOCACTIONPARAM")
            .expect("action parameter class");
        let point_ref = document
            .classes
            .get_by_name("ACDBASSOCPOINTREFACTIONPARAM")
            .expect("point reference class");
        assert!(!action_param.was_zombie);
        assert!(!point_ref.was_zombie);

        let (_, _, _, _, counts, complete) = DwgObjectWriter::new(&document)
            .expect("object writer")
            .write_with_class_metadata();
        let classes = reconciled_classes(&document, &counts, complete);
        let by_name = |name: &str| {
            classes
                .iter()
                .find(|class| class.dxf_name.eq_ignore_ascii_case(name))
                .expect("class entry")
        };

        assert!(by_name("ACDBASSOCACTIONPARAM").was_zombie);
        assert!(by_name("ACDBASSOCPOINTREFACTIONPARAM").was_zombie);
        assert!(by_name("ACDBDICTIONARYWDFLT").instance_count > 0);
    }

    #[test]
    fn output_copy_repairs_associative_hatch_reactors() {
        use crate::entities::{BoundaryPath, Circle, EntityType, Hatch};

        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let boundary = document
            .add_entity(EntityType::Circle(Circle::new()))
            .expect("boundary");
        let mut hatch = Hatch::solid();
        hatch.is_associative = true;
        let mut path = BoundaryPath::new();
        path.boundary_handles.push(boundary);
        hatch.paths.push(path);
        let hatch_handle = document
            .add_entity(EntityType::Hatch(hatch))
            .expect("hatch");

        let mut prepared = std::borrow::Cow::Borrowed(&document);
        prepare_database_references(&mut prepared);

        assert!(document
            .get_entity(boundary)
            .unwrap()
            .common()
            .reactors
            .is_empty());
        assert!(prepared
            .get_entity(boundary)
            .unwrap()
            .common()
            .reactors
            .contains(&hatch_handle));
    }

    #[test]
    fn output_copy_synchronizes_layout_dictionary_names() {
        use crate::objects::ObjectType;

        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let dictionary_handle = document.header.acad_layout_dict_handle;
        let layout_handle = match document.objects.get(&dictionary_handle) {
            Some(ObjectType::Dictionary(dictionary)) => dictionary.entries[0].1,
            _ => panic!("layout dictionary"),
        };
        if let Some(ObjectType::Layout(layout)) = document.objects.get_mut(&layout_handle) {
            layout.name = "Renamed".to_string();
        }

        let mut prepared = std::borrow::Cow::Borrowed(&document);
        prepare_database_references(&mut prepared);

        let dictionary = match prepared.objects.get(&dictionary_handle) {
            Some(ObjectType::Dictionary(dictionary)) => dictionary,
            _ => panic!("layout dictionary"),
        };
        assert_eq!(dictionary.get("Renamed"), Some(layout_handle));
    }

    #[test]
    fn same_version_roundtrip_preserves_non_entity_data_store_section() {
        use crate::io::dwg::DwgReader;
        use crate::objects::ObjectType;
        use std::sync::Arc;

        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let layout_handle = document
            .objects
            .iter()
            .find_map(|(handle, object)| matches!(object, ObjectType::Layout(_)).then_some(*handle))
            .expect("default layout");
        document.dwg_source_version = Some(DxfVersion::AC1032);
        document.dwg_data_store_handles.insert(layout_handle);
        document.raw_acds_data = Some(Arc::new(build_acds_prototype(&[])));

        let bytes = DwgWriter::write_to_vec(&document).expect("write drawing");
        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let roundtripped = reader.read().expect("read drawing");

        assert!(roundtripped.raw_acds_data.is_some());
        assert!(roundtripped.dwg_data_store_handles.contains(&layout_handle));
    }

    #[test]
    fn test_build_template() {
        let t = build_template();
        assert_eq!(t.len(), 4);
        assert_eq!(i32::from_le_bytes([t[0], t[1], t[2], t[3]]), 0); // desc length (RL = 4 bytes)
    }

    #[test]
    fn test_build_obj_free_space() {
        let doc = CadDocument::new();
        let data = build_obj_free_space(DxfVersion::AC1015, &doc, 42);
        // Int32(0) + UInt32(42) + 8 date + UInt32(0) + 1 + 32
        assert_eq!(data.len(), 4 + 4 + 8 + 4 + 1 + 32);
        // Check handle count at offset 4
        let count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        assert_eq!(count, 42);
    }

    #[test]
    fn test_build_file_dep_list() {
        let d = build_file_dep_list();
        assert_eq!(d.len(), 8);
    }

    #[test]
    fn test_build_rev_history() {
        let d = build_rev_history();
        assert_eq!(d.len(), 16);
    }

    #[test]
    fn test_build_summary_info_ac18() {
        let d = build_summary_info(DxfVersion::AC1018);
        // 8 × 3 bytes (u16(1) + ANSI null) + 8 + 16 + 2 + 8 = 58
        assert_eq!(d.len(), 58);
        assert_eq!(u16::from_le_bytes([d[0], d[1]]), 1);
        assert_eq!(d[2], 0);
        // Next string starts at offset 3
        assert_eq!(u16::from_le_bytes([d[3], d[4]]), 1);
    }

    #[test]
    fn test_build_summary_info_ac21() {
        let d = build_summary_info(DxfVersion::AC1021);
        // 8 × 4 bytes (u16(1) + UTF-16LE null) + 8 + 16 + 2 + 8 = 66
        assert_eq!(d.len(), 66);
        // First string: u16(1) + 00 00
        assert_eq!(u16::from_le_bytes([d[0], d[1]]), 1);
        assert_eq!(d[2], 0);
        assert_eq!(d[3], 0);
        // Next string starts at offset 4
        assert_eq!(u16::from_le_bytes([d[4], d[5]]), 1);
    }

    #[test]
    fn test_write_to_vec_r2000() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        // AC15 file header magic: "AC1015"
        assert!(bytes.len() > 100);
        let magic = std::str::from_utf8(&bytes[0..6]).unwrap_or("");
        assert_eq!(magic, "AC1015");
    }

    #[test]
    fn test_write_to_vec_r2004() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1018;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(bytes.len() > 0x100);
        // AC18 magic at offset 0
        let magic = std::str::from_utf8(&bytes[0..6]).unwrap_or("");
        assert_eq!(magic, "AC1018");
    }

    #[test]
    fn test_write_to_vec_r2010() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1024;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(bytes.len() > 0x100);
    }

    #[test]
    fn test_write_to_vec_r2013() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1027;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_to_vec_r2018() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1032;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_to_vec_r2007() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1021;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok(), "AC1021 writing should succeed");
        let bytes = result.unwrap();
        assert!(
            bytes.len() > 0x480,
            "AC1021 file should be larger than header"
        );
        let magic = std::str::from_utf8(&bytes[0..6]).unwrap_or("");
        assert_eq!(magic, "AC1021");
    }

    #[test]
    fn test_uses_ac21_format() {
        assert!(uses_ac21_format(DxfVersion::AC1021));
        assert!(!uses_ac21_format(DxfVersion::AC1018));
        assert!(!uses_ac21_format(DxfVersion::AC1024));
        assert!(!uses_ac21_format(DxfVersion::AC1015));
    }

    #[test]
    fn test_write_to_vec_r14() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1014;
        let result = DwgWriter::write_to_vec(&doc);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        let magic = std::str::from_utf8(&bytes[0..6]).unwrap_or("");
        assert_eq!(magic, "AC1014");
    }

    #[test]
    fn test_roundtrip_file_write() {
        let doc = CadDocument::new();
        let mut doc2 = doc.clone();
        doc2.version = DxfVersion::AC1015;

        let bytes = DwgWriter::write_to_vec(&doc2).unwrap();
        // Verify non-trivial output
        assert!(bytes.len() > 200, "DWG file should be non-trivial");
    }

    #[test]
    fn test_pre_r2013_write_uses_legacy_document_profile() {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;

        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let mut reader = crate::io::dwg::DwgReader::from_stream(std::io::Cursor::new(bytes));
        let decoded = reader.read().unwrap();

        assert_eq!(decoded.classes.len(), 38);
        assert_eq!(decoded.objects.len(), 13);
    }

    #[test]
    fn test_prepare_header_syncs_null_handles() {
        // Simulate the bug: create a document and zero out all header handles
        // (as would happen after reading a DWG with a broken header reader).
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;

        // Save correct handles for later verification
        let correct_block_control = doc.block_records.handle();
        let correct_layer_control = doc.layers.handle();
        let correct_style_control = doc.text_styles.handle();
        let correct_ltype_control = doc.line_types.handle();
        let correct_view_control = doc.views.handle();
        let correct_ucs_control = doc.ucss.handle();
        let correct_vport_control = doc.vports.handle();
        let correct_appid_control = doc.app_ids.handle();
        let correct_dimstyle_control = doc.dim_styles.handle();
        let correct_root_dict = doc.header.named_objects_dict_handle;

        // Zero out all header handles (simulate the reader bug)
        doc.header.block_control_handle = Handle::NULL;
        doc.header.layer_control_handle = Handle::NULL;
        doc.header.style_control_handle = Handle::NULL;
        doc.header.linetype_control_handle = Handle::NULL;
        doc.header.view_control_handle = Handle::NULL;
        doc.header.ucs_control_handle = Handle::NULL;
        doc.header.vport_control_handle = Handle::NULL;
        doc.header.appid_control_handle = Handle::NULL;
        doc.header.dimstyle_control_handle = Handle::NULL;
        doc.header.named_objects_dict_handle = Handle::NULL;
        doc.header.acad_group_dict_handle = Handle::NULL;
        doc.header.acad_mlinestyle_dict_handle = Handle::NULL;
        doc.header.acad_layout_dict_handle = Handle::NULL;
        doc.header.bylayer_linetype_handle = Handle::NULL;
        doc.header.byblock_linetype_handle = Handle::NULL;
        doc.header.continuous_linetype_handle = Handle::NULL;
        doc.header.current_layer_handle = Handle::NULL;
        doc.header.current_text_style_handle = Handle::NULL;
        doc.header.current_dimstyle_handle = Handle::NULL;
        doc.header.current_linetype_handle = Handle::NULL;

        // prepare_header should sync all handles from document objects
        let handle_map = vec![(1u64, 0u32), (2, 100), (3, 200)]; // dummy
        let prepared = prepare_header(&doc, &handle_map, &None);

        // Table control handles must be synced from the actual table objects
        assert_eq!(
            prepared.block_control_handle, correct_block_control,
            "block_control_handle should be synced from block_records.handle()"
        );
        assert_eq!(
            prepared.layer_control_handle, correct_layer_control,
            "layer_control_handle should be synced from layers.handle()"
        );
        assert_eq!(
            prepared.style_control_handle, correct_style_control,
            "style_control_handle should be synced from text_styles.handle()"
        );
        assert_eq!(
            prepared.linetype_control_handle, correct_ltype_control,
            "linetype_control_handle should be synced from line_types.handle()"
        );
        assert_eq!(
            prepared.view_control_handle, correct_view_control,
            "view_control_handle should be synced from views.handle()"
        );
        assert_eq!(
            prepared.ucs_control_handle, correct_ucs_control,
            "ucs_control_handle should be synced from ucss.handle()"
        );
        assert_eq!(
            prepared.vport_control_handle, correct_vport_control,
            "vport_control_handle should be synced from vports.handle()"
        );
        assert_eq!(
            prepared.appid_control_handle, correct_appid_control,
            "appid_control_handle should be synced from app_ids.handle()"
        );
        assert_eq!(
            prepared.dimstyle_control_handle, correct_dimstyle_control,
            "dimstyle_control_handle should be synced from dim_styles.handle()"
        );

        // Root dictionary must be found
        assert_eq!(
            prepared.named_objects_dict_handle, correct_root_dict,
            "named_objects_dict_handle should be found by scanning objects"
        );
        assert!(
            !prepared.named_objects_dict_handle.is_null(),
            "named_objects_dict_handle must not be NULL"
        );

        // Dict handles from root dict entries must be resolved
        assert!(
            !prepared.acad_group_dict_handle.is_null(),
            "acad_group_dict_handle must be resolved from root dict"
        );
        assert!(
            !prepared.acad_mlinestyle_dict_handle.is_null(),
            "acad_mlinestyle_dict_handle must be resolved from root dict"
        );
        assert!(
            !prepared.acad_layout_dict_handle.is_null(),
            "acad_layout_dict_handle must be resolved from root dict"
        );

        // Linetype handles must be resolved
        assert!(
            !prepared.bylayer_linetype_handle.is_null(),
            "bylayer_linetype_handle must be resolved"
        );
        assert!(
            !prepared.byblock_linetype_handle.is_null(),
            "byblock_linetype_handle must be resolved"
        );
        assert!(
            !prepared.continuous_linetype_handle.is_null(),
            "continuous_linetype_handle must be resolved"
        );

        // Current style handles must be resolved
        assert!(
            !prepared.current_layer_handle.is_null(),
            "current_layer_handle must be resolved"
        );
        assert!(
            !prepared.current_text_style_handle.is_null(),
            "current_text_style_handle must be resolved"
        );
        assert!(
            !prepared.current_dimstyle_handle.is_null(),
            "current_dimstyle_handle must be resolved"
        );
        assert!(
            !prepared.current_linetype_handle.is_null(),
            "current_linetype_handle must be resolved (default to ByLayer)"
        );
    }

    #[test]
    fn test_prepare_header_prefers_dictionary_standard_mlinestyle() {
        let mut doc = CadDocument::new();
        let dictionary_standard = doc.header.current_multiline_style_handle;
        let orphan_handle = (1..dictionary_standard.value())
            .map(Handle::new)
            .find(|handle| !doc.objects.contains_key(handle))
            .expect("an unused lower handle");
        let mut orphan = crate::objects::MLineStyle::standard();
        orphan.handle = orphan_handle;
        doc.objects.insert(
            orphan_handle,
            crate::objects::ObjectType::MLineStyle(orphan),
        );
        doc.header.current_multiline_style_handle = Handle::NULL;

        let prepared = prepare_header(&doc, &[], &None);

        assert_eq!(prepared.current_multiline_style_handle, dictionary_standard);
    }

    #[test]
    fn test_prepare_header_mlinestyle_recovery_is_deterministic() {
        let mut doc = CadDocument::new();
        let dictionary_handle = doc.header.acad_mlinestyle_dict_handle;
        let original_standard = doc.header.current_multiline_style_handle;
        let recovery_handle = (1..original_standard.value())
            .map(Handle::new)
            .find(|handle| !doc.objects.contains_key(handle))
            .expect("an unused lower handle");
        let mut recovery = crate::objects::MLineStyle::standard();
        recovery.handle = recovery_handle;
        doc.objects.insert(
            recovery_handle,
            crate::objects::ObjectType::MLineStyle(recovery),
        );
        let crate::objects::ObjectType::Dictionary(dictionary) = doc
            .objects
            .get_mut(&dictionary_handle)
            .expect("ACAD_MLINESTYLE dictionary")
        else {
            panic!("ACAD_MLINESTYLE handle should reference a dictionary");
        };
        dictionary
            .entries
            .retain(|(name, _)| !name.eq_ignore_ascii_case("Standard"));
        doc.header.current_multiline_style_handle = Handle::NULL;

        let prepared = prepare_header(&doc, &[], &None);

        assert_eq!(prepared.current_multiline_style_handle, recovery_handle);
    }

    #[test]
    fn test_prepare_header_null_handles_write_produces_valid_dwg() {
        // Simulate bug and verify the written DWG file is still valid
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;

        // Zero out all header handles
        doc.header.block_control_handle = Handle::NULL;
        doc.header.layer_control_handle = Handle::NULL;
        doc.header.named_objects_dict_handle = Handle::NULL;

        // Writing should succeed (prepare_header syncs handles)
        let result = DwgWriter::write_to_vec(&doc);
        assert!(
            result.is_ok(),
            "Writing with NULL headers should succeed after sync"
        );
        let bytes = result.unwrap();
        assert!(bytes.len() > 200, "Output should be non-trivial");
    }

    // ── File-level DWG roundtrip tests for 3DSOLID / REGION / BODY ──

    fn make_sat_sample() -> &'static str {
        "700 0 1 0\n\
         @7 unknown 12 ACIS 7.0 NT 24 Wed Jan 01 00:00:00 2025 1.0 9.9999999999999995e-007 1e-010\n\
         body $-1 $1 $-1 $-1 #\n\
         lump $-1 $-1 $2 $0 #\n\
         shell $-1 $-1 $-1 $3 $-1 $1 #\n\
         face $-1 $-1 $-1 $4 $2 $5 forward single #\n\
         loop $-1 $-1 $6 $3 #\n\
         plane-surface $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
         coedge $-1 $6 $6 $-1 $7 forward $4 $-1 #\n\
         edge $-1 $8 0 $8 1 $6 $9 forward #\n\
         vertex $-1 $7 $10 #\n\
         straight-curve $-1 -5 -5 5 1 0 0 I I #\n\
         point $-1 -5 -5 5 #\n\
         End-of-ACIS-data\n"
    }

    /// Write a Solid3D with SAT data to DWG R2000, read back, verify SAT preserved.
    #[test]
    fn test_roundtrip_solid3d_r2000() {
        use crate::entities::solid3d::Solid3D;
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;
        let solid = Solid3D::from_sat(make_sat_sample());
        let _ = doc.add_entity(EntityType::Solid3D(solid));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2000 should succeed");
        assert!(bytes.len() > 200);

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2000 should succeed");

        let solids: Vec<&Solid3D> = doc2
            .entities()
            .filter_map(|e| {
                if let EntityType::Solid3D(s) = e {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(solids.len(), 1, "should have exactly one Solid3D");
        assert!(!solids[0].acis_data.is_binary, "R2000 should use SAT text");
        assert!(
            solids[0].acis_data.sat_data.contains("body"),
            "SAT data must contain 'body'"
        );
        assert!(
            solids[0].acis_data.sat_data.contains("plane-surface"),
            "SAT data must contain 'plane-surface'"
        );
    }

    /// Write a Solid3D with SAT data to DWG R2004, read back, verify SAT preserved.
    #[test]
    fn test_roundtrip_solid3d_r2004() {
        use crate::entities::solid3d::Solid3D;
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1018;
        let solid = Solid3D::from_sat(make_sat_sample());
        let _ = doc.add_entity(EntityType::Solid3D(solid));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2004 should succeed");
        assert!(bytes.len() > 200);

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2004 should succeed");

        let solids: Vec<&Solid3D> = doc2
            .entities()
            .filter_map(|e| {
                if let EntityType::Solid3D(s) = e {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(solids.len(), 1, "should have exactly one Solid3D");
        assert!(!solids[0].acis_data.is_binary, "R2004 should use SAT text");
        assert!(solids[0].acis_data.sat_data.contains("body"));
    }

    /// Write a Solid3D with SAT data to DWG R2007 (SAB binary format), read back.
    #[test]
    fn test_roundtrip_solid3d_r2007() {
        use crate::entities::solid3d::Solid3D;
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1021;
        let solid = Solid3D::from_sat(make_sat_sample());
        let _ = doc.add_entity(EntityType::Solid3D(solid));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2007 should succeed");
        assert!(bytes.len() > 200);

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2007 should succeed");

        let solids: Vec<&Solid3D> = doc2
            .entities()
            .filter_map(|e| {
                if let EntityType::Solid3D(s) = e {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(solids.len(), 1, "should have exactly one Solid3D");
        // R2007 should use SAT text format since we provided SAT text —
        // the version in acis_data controls what's written, not the DWG version alone.
        assert!(solids[0].acis_data.has_data(), "should have ACIS data");
    }

    /// Write a Region with SAT data to DWG R2000, read back.
    #[test]
    fn test_roundtrip_region_r2000() {
        use crate::entities::solid3d::Region;
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1015;
        let region = Region::from_sat(make_sat_sample());
        let _ = doc.add_entity(EntityType::Region(region));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2000 should succeed");

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2000 should succeed");

        let regions: Vec<&Region> = doc2
            .entities()
            .filter_map(|e| {
                if let EntityType::Region(r) = e {
                    Some(r)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(regions.len(), 1, "should have exactly one Region");
        assert!(regions[0].acis_data.sat_data.contains("body"));
    }

    /// Write a Body with SAT data to DWG R2004, read back.
    #[test]
    fn test_roundtrip_body_r2004() {
        use crate::entities::solid3d::Body;
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1018;
        let body = Body::from_sat(make_sat_sample());
        let _ = doc.add_entity(EntityType::Body(body));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2004 should succeed");

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2004 should succeed");

        let bodies: Vec<&Body> = doc2
            .entities()
            .filter_map(|e| {
                if let EntityType::Body(b) = e {
                    Some(b)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(bodies.len(), 1, "should have exactly one Body");
        assert!(bodies[0].acis_data.sat_data.contains("body"));
    }

    /// Write multiple ACIS entities to a single DWG at R2010, read back all three.
    #[test]
    fn test_roundtrip_mixed_acis_r2010() {
        use crate::entities::solid3d::{Body, Region, Solid3D};
        use crate::entities::EntityType;
        use crate::io::dwg::DwgReader;

        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1024;

        let _ = doc.add_entity(EntityType::Solid3D(Solid3D::from_sat(make_sat_sample())));
        let _ = doc.add_entity(EntityType::Region(Region::from_sat(make_sat_sample())));
        let _ = doc.add_entity(EntityType::Body(Body::from_sat(make_sat_sample())));

        let bytes = DwgWriter::write_to_vec(&doc).expect("write R2010 should succeed");

        let mut reader = DwgReader::from_stream(std::io::Cursor::new(bytes));
        let doc2 = reader.read().expect("read R2010 should succeed");

        let n_solid = doc2
            .entities()
            .filter(|e| matches!(e, EntityType::Solid3D(_)))
            .count();
        let n_region = doc2
            .entities()
            .filter(|e| matches!(e, EntityType::Region(_)))
            .count();
        let n_body = doc2
            .entities()
            .filter(|e| matches!(e, EntityType::Body(_)))
            .count();
        assert_eq!(n_solid, 1, "should have 1 Solid3D");
        assert_eq!(n_region, 1, "should have 1 Region");
        assert_eq!(n_body, 1, "should have 1 Body");

        // Verify data integrity on each
        for e in doc2.entities() {
            match e {
                EntityType::Solid3D(s) => assert!(s.acis_data.sat_data.contains("body")),
                EntityType::Region(r) => assert!(r.acis_data.sat_data.contains("body")),
                EntityType::Body(b) => assert!(b.acis_data.sat_data.contains("body")),
                _ => {}
            }
        }
    }
}
