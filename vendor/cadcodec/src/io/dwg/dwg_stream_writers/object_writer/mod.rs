//! DWG Object / Entity Writer (Sprint 4)
//!
//! Writes all DWG object records — table controls, table entries,
//! block headers, entities in each block, and non-graphical objects
//! (dictionaries, layouts, etc.).
//!
//! Ported from the reference `DwgObjectWriter` (partial class across
//! `DwgObjectWriter.cs`, `…Common.cs`, `…Entities.cs`, `…Objects.cs`).
//!
//! ## Record format
//!
//! Each object record in the output stream is:
//! ```text
//! [ModularShort(len)] [merged-stream bytes] [CRC16]
//! ```
//! The merged-stream bytes contain main + text + handle sub-streams
//! interleaved per the DWG spec.

pub mod associative;
pub mod class_object;
pub mod common;
pub mod data_objects;
pub mod dynamic_block;
pub mod entities;
pub mod field;
pub mod objects;

use std::collections::{HashMap, HashSet, VecDeque};

use crate::document::CadDocument;
use crate::entities::{EntityCommon, EntityType};
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::io::dwg::dwg_stream_writers::DwgMergedWriter;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::tables::{BlockRecord, TableEntry};
use crate::types::{BoundingBox3D, DxfVersion, Handle};

// ── Helpers ─────────────────────────────────────────────────────────

/// Convert a deduplicated block name back to the DWG binary name.
///
/// In DWG format, all paper-space blocks are stored as `*Paper_Space`
/// and anonymous blocks share names like `*U`, `*D`, etc. (no numeric
/// suffixes).  Our reader adds suffixes (`*Paper_Space0`, `*U1`, …)
/// for deduplication.  This function strips them back for writing.
fn dwg_block_name(name: &str) -> &str {
    // Known multi-word prefixes first
    for prefix in &["*Model_Space", "*Paper_Space"] {
        if name
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let rest = &name[prefix.len()..];
            if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
                return &name[..prefix.len()];
            }
        }
    }
    // Generic anonymous: *<alpha><digits> → *<alpha>
    if name.starts_with('*') && name.len() >= 2 {
        let alpha_end = name[1..]
            .find(|c: char| !c.is_ascii_alphabetic())
            .map(|p| 1 + p)
            .unwrap_or(name.len());
        let rest = &name[alpha_end..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return &name[..alpha_end];
        }
    }
    name
}

// ── Public struct ───────────────────────────────────────────────────

/// Writes all DWG object records (entities + table entries + objects)
/// into a contiguous byte stream, tracking handle→offset pairs for
/// the handle section.
pub struct DwgObjectWriter<'a> {
    /// Target DWG version (controls which fields are emitted)
    pub(super) version: DwgVersion,
    /// DXF version (for R2013/R2018 flag checks)
    pub(super) dxf_version: DxfVersion,
    /// Reference to the CAD document being written
    pub(super) document: &'a CadDocument,
    /// Per-object scratch writer (main + text + handle streams)
    pub(super) writer: DwgMergedWriter,
    /// Accumulated output bytes (all object records)
    pub(super) output: Vec<u8>,
    /// Handle → byte-offset map (for handle section)
    pub(super) handle_map: Vec<(u64, u32)>,
    /// Queue of non-graphical objects still to be written
    pub(super) object_queue: VecDeque<Handle>,
    /// Previous entity handle for pre-R2004 entity chain
    pub(super) prev_handle: Option<Handle>,
    /// Next entity handle for pre-R2004 entity chain
    pub(super) next_handle: Option<Handle>,
    /// Next handle value for allocating sub-entity handles (vertices, seqend)
    pub(super) next_alloc_handle: u64,
    /// Computed model space extents for VPort view adjustment and header EXTMIN/EXTMAX
    pub(crate) model_space_extents: Option<BoundingBox3D>,
    /// SAB data entries collected during entity writing (AC1027+).
    /// Each entry is (entity_handle, sab_binary_data).
    pub(super) sab_entries: Vec<(Handle, Vec<u8>)>,
    /// Set by a modeler-entity writer immediately before it emits the common
    /// entity data, so the R2013+ `has_ds_data` bit is written `true` for a
    /// 3DSOLID/REGION/BODY/SURFACE that contributes a SAB blob to the AcDs
    /// section. `write_common_entity_data` consumes and clears it, so every
    /// other entity writes `false`.
    pub(super) pending_has_ds_data: bool,
    /// Tracks which object handles have already been written to prevent duplicates.
    pub(super) visited_objects: HashSet<Handle>,
    /// Number of emitted records for every class-based type code.
    pub(super) class_instance_counts: HashMap<i16, i32>,
    /// Type code belonging to the record currently being assembled.
    pub(super) pending_type_code: Option<i16>,
    /// False when an opaque record prevents an exact class census.
    pub(super) class_counts_complete: bool,
    /// Every handle actually emitted to the object map. Central guard against
    /// writing the same handle twice (e.g. an xdictionary XRECORD reachable
    /// from more than one path): a duplicate handle is a hard DWG integrity
    /// error that AutoCAD's audit rejects, so register_* skips repeats.
    pub(super) registered_handles: HashSet<u64>,
    /// Owner handle overrides for extension dictionaries whose parent entity
    /// was re-allocated (e.g. ATTRIB children of INSERT).
    pub(super) owner_overrides: std::collections::HashMap<Handle, Handle>,
    /// Pre-allocated handles for table entries that have Handle::NULL in the
    /// document (e.g. user-created linetypes). Keyed by linetype name (uppercase).
    /// Populated before writing any table controls so controls and records agree.
    #[allow(dead_code)]
    pub(super) linetype_handles: std::collections::HashMap<String, Handle>,
}

struct ParallelEntityBatch {
    output: Vec<u8>,
    handle_map: Vec<(u64, u32)>,
    object_queue: VecDeque<Handle>,
    registered_handles: HashSet<u64>,
    class_instance_counts: HashMap<i16, i32>,
    class_counts_complete: bool,
}

impl<'a> DwgObjectWriter<'a> {
    // ── Constructor ─────────────────────────────────────────────────

    /// Create a new object writer for the given document and version.
    pub fn new(document: &'a CadDocument) -> crate::error::Result<Self> {
        let version = DwgVersion::from_dxf_version(document.version)?;
        let dxf_version = document.version;
        let encoding =
            crate::io::dxf::code_page::encoding_from_code_page(&document.header.code_page)
                .unwrap_or(encoding_rs::WINDOWS_1252);
        let writer = DwgMergedWriter::with_encoding(version, dxf_version, encoding);

        // Compute safe starting handle for allocation.
        // document.header.handle_seed may be stale (e.g. DWG roundtrip
        // without resolve_references), so scan all handles and use whichever
        // value is higher.
        let mut max_h = document.header.handle_seed;
        for entity in document.entities() {
            let h = entity.common().handle.value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for (handle, _) in &document.objects {
            let h = handle.value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for br in document.block_records.iter() {
            let h = br.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
            // Also scan block entity and endblk entity handles:
            // these are written verbatim by write_block_begin/write_block_end
            // but are NOT part of document.entities(), so they would otherwise
            // be missed and cause alloc_handle() to re-issue their handle values.
            let h2 = br.block_entity_handle.value() + 1;
            if h2 > max_h {
                max_h = h2;
            }
            let h3 = br.block_end_handle.value() + 1;
            if h3 > max_h {
                max_h = h3;
            }
        }
        for ly in document.layers.iter() {
            let h = ly.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for lt in document.line_types.iter() {
            let h = lt.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for ts in document.text_styles.iter() {
            let h = ts.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        // Also scan the remaining table entries that were previously missed:
        // app_ids, dim_styles, views, vports, ucss
        for a in document.app_ids.iter() {
            let h = a.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for ds in document.dim_styles.iter() {
            let h = ds.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for v in document.views.iter() {
            let h = v.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for vp in document.vports.iter() {
            let h = vp.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }
        for u in document.ucss.iter() {
            let h = u.handle().value() + 1;
            if h > max_h {
                max_h = h;
            }
        }

        Ok(Self {
            version,
            dxf_version,
            document,
            writer,
            output: Vec::with_capacity(64 * 1024),
            handle_map: Vec::with_capacity(1024),
            object_queue: VecDeque::new(),
            registered_handles: HashSet::new(),
            prev_handle: None,
            next_handle: None,
            next_alloc_handle: max_h,
            model_space_extents: None,
            sab_entries: Vec::new(),
            pending_has_ds_data: false,
            visited_objects: HashSet::new(),
            class_instance_counts: HashMap::new(),
            pending_type_code: None,
            class_counts_complete: true,
            owner_overrides: std::collections::HashMap::new(),
            linetype_handles: std::collections::HashMap::new(),
        })
    }

    // ── Main entry point ────────────────────────────────────────────

    pub fn write(
        self,
    ) -> (
        Vec<u8>,
        Vec<(u64, u32)>,
        Option<BoundingBox3D>,
        Vec<(Handle, Vec<u8>)>,
    ) {
        let (output, handles, extents, sab_entries, _, _) = self.write_with_class_metadata();
        (output, handles, extents, sab_entries)
    }

    /// Write all objects and return the encoded records and their derived metadata.
    ///
    /// For AC1027+, ACIS entities (3DSOLID, REGION, BODY) are written with
    /// `acis_empty=true` in the entity stream; their SAB binary data is
    /// collected into `sab_entries` for writing into the `AcDb:AcDsPrototype_1b`
    /// section.
    pub(crate) fn write_with_class_metadata(
        mut self,
    ) -> (
        Vec<u8>,
        Vec<(u64, u32)>,
        Option<BoundingBox3D>,
        Vec<(Handle, Vec<u8>)>,
        HashMap<i16, i32>,
        bool,
    ) {
        // Compute model space extents for VPort view adjustment
        self.model_space_extents = self.compute_model_space_extents();

        // R2004+: 0x0DCA marker at the start
        if self.version.r2004_plus() {
            self.output.extend_from_slice(&0x0DCAi32.to_le_bytes());
        }

        // Enqueue root dictionary for later.
        // If the header handle is NULL (e.g., after a DWG read where the
        // header reader failed to parse handles), scan document.objects to
        // find the root dictionary (a Dictionary with owner == NULL).
        let mut root_dict_handle = self.document.header.named_objects_dict_handle;
        if root_dict_handle.is_null() {
            root_dict_handle = self.find_root_dict_handle();
        }
        if !root_dict_handle.is_null() {
            self.object_queue.push_back(root_dict_handle);
        }

        // ── Table controls ──────────────────────────────────────
        self.write_block_control();
        self.write_table_control(
            self.document.layers.handle(),
            common::OBJ_LAYER_CONTROL,
            &self
                .document
                .layers
                .iter()
                .map(|l| l.handle)
                .collect::<Vec<_>>(),
        );
        self.write_text_style_control();
        self.write_ltype_control();
        self.write_table_control(
            self.document.views.handle(),
            common::OBJ_VIEW_CONTROL,
            &self
                .document
                .views
                .iter()
                .map(|v| v.handle)
                .collect::<Vec<_>>(),
        );
        self.write_table_control(
            self.document.ucss.handle(),
            common::OBJ_UCS_CONTROL,
            &self
                .document
                .ucss
                .iter()
                .map(|u| u.handle)
                .collect::<Vec<_>>(),
        );
        self.write_table_control(
            self.document.vports.handle(),
            common::OBJ_VPORT_CONTROL,
            &self
                .document
                .vports
                .iter()
                .map(|v| v.handle)
                .collect::<Vec<_>>(),
        );
        let mut appid_handles: Vec<_> = self
            .document
            .app_ids
            .iter()
            .map(|a| (a.name.eq_ignore_ascii_case("ACAD"), a.handle))
            .collect();
        appid_handles.sort_by_key(|(is_acad, _)| !*is_acad);
        self.write_table_control(
            self.document.app_ids.handle(),
            common::OBJ_APPID_CONTROL,
            &appid_handles
                .into_iter()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>(),
        );
        self.write_dimstyle_control();

        // R13-R2000 only: VPEntHdr control (viewport entity header table)
        if self.version.r13_15_only() {
            self.write_vpent_hdr_control();
        }

        // ── Table entries ───────────────────────────────────────
        self.write_layer_entries();
        self.write_text_style_entries();
        self.write_ltype_entries();
        self.write_view_entries();
        self.write_ucs_entries();
        self.write_vport_entries();
        self.write_appid_entries();
        self.write_dimstyle_entries();
        if self.version.r13_15_only() {
            self.write_vx_entries();
        }

        // ── Block entities ──────────────────────────────────────
        self.write_block_entities();

        // ── Drain object queue ──────────────────────────────────
        self.write_objects();

        (
            self.output,
            self.handle_map,
            self.model_space_extents,
            self.sab_entries,
            self.class_instance_counts,
            self.class_counts_complete,
        )
    }

    /// Whether this version stores ACIS data externally (AcDsPrototype_1b section)
    /// rather than inline in the entity stream.
    /// Currently disabled: always write inline because the DWG reader doesn't yet
    /// parse the AcDsPrototype_1b section, which causes ACIS data loss on read-back.
    fn needs_acds_section(&self) -> bool {
        // R2013+ stores ACIS (3DSOLID/REGION/BODY/SURFACE) geometry in the
        // AcDsPrototype_1b section rather than inline in the entity stream.
        self.version.r2013_plus(self.dxf_version)
    }

    /// Find the root dictionary handle by scanning document.objects.
    ///
    /// The root dictionary is a Dictionary with `owner == Handle::NULL`.
    /// If multiple candidates exist, prefer the one with the most entries.
    fn find_root_dict_handle(&self) -> Handle {
        let mut best_handle = Handle::NULL;
        let mut best_entry_count = 0usize;

        for (handle, obj) in &self.document.objects {
            if let crate::objects::ObjectType::Dictionary(dict) = obj {
                if dict.owner.is_null() {
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

    /// Compute the bounding box of all entities in the *Model_Space block.
    fn compute_model_space_extents(&self) -> Option<BoundingBox3D> {
        let ms_block = self.document.block_records.get("*Model_Space")?;
        let mut extents: Option<BoundingBox3D> = None;
        for eh in &ms_block.entity_handles {
            if let Some(&idx) = self.document.entity_index.get(eh) {
                let bbox = self.document.entities[idx].as_entity().bounding_box();
                extents = Some(match extents {
                    Some(existing) => existing.merge(&bbox),
                    None => bbox,
                });
            }
        }
        extents
    }

    // ── Table control writers ───────────────────────────────────────

    /// Generic table control object: type code, count, soft-owner handles.
    fn write_table_control(
        &mut self,
        table_handle: Handle,
        type_code: i16,
        entry_handles: &[Handle],
    ) {
        // Owner is always 0 for table controls (owned by header)
        self.write_common_non_entity_data(type_code, table_handle, Handle::NULL, &[], &None);

        // Entry count
        self.writer.write_bit_long(entry_handles.len() as i32);

        // Entry handles (soft ownership)
        for h in entry_handles {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, h.value());
        }

        self.register_object(table_handle);
    }

    /// BLOCK_CONTROL — special: excludes *Model_Space and *Paper_Space
    /// from the count, writes them as hard-owner references at the end.
    fn write_block_control(&mut self) {
        let table_handle = self.document.block_records.handle();

        self.write_common_non_entity_data(
            common::OBJ_BLOCK_CONTROL,
            table_handle,
            Handle::NULL,
            &[],
            &None,
        );

        // Gather handles
        let mut regular: Vec<Handle> = Vec::new();
        let mut ms_handle = Handle::NULL;
        let mut ps_handle = Handle::NULL;

        for br in self.document.block_records.iter() {
            if br.name.eq_ignore_ascii_case("*Model_Space") {
                ms_handle = br.handle;
            } else if br.name.eq_ignore_ascii_case("*Paper_Space") {
                ps_handle = br.handle;
            } else {
                regular.push(br.handle);
            }
        }

        // Count excludes model/paper space
        self.writer.write_bit_long(regular.len() as i32);

        for h in &regular {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, h.value());
        }

        // *Model_Space, *Paper_Space (hard owner)
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, ms_handle.value());
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, ps_handle.value());

        self.register_object(table_handle);
    }

    /// STYLE_CONTROL
    fn write_text_style_control(&mut self) {
        let handles: Vec<Handle> = self.document.text_styles.iter().map(|s| s.handle).collect();
        self.write_table_control(
            self.document.text_styles.handle(),
            common::OBJ_STYLE_CONTROL,
            &handles,
        );
    }

    /// LTYPE_CONTROL — special: excludes ByLayer/ByBlock from count.
    fn write_ltype_control(&mut self) {
        let table_handle = self.document.line_types.handle();
        self.write_common_non_entity_data(
            common::OBJ_LTYPE_CONTROL,
            table_handle,
            Handle::NULL,
            &[],
            &None,
        );

        let mut regular = Vec::new();
        let mut byblock_handle = Handle::NULL;
        let mut bylayer_handle = Handle::NULL;

        for lt in self.document.line_types.iter() {
            if lt.name.eq_ignore_ascii_case("ByBlock") {
                byblock_handle = lt.handle;
            } else if lt.name.eq_ignore_ascii_case("ByLayer") {
                bylayer_handle = lt.handle;
            } else {
                regular.push(lt.handle);
            }
        }

        self.writer.write_bit_long(regular.len() as i32);
        for h in &regular {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, h.value());
        }
        // ByBlock, ByLayer (hard owner)
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, byblock_handle.value());
        self.writer
            .write_handle(DwgReferenceType::HardOwnership, bylayer_handle.value());

        self.register_object(table_handle);
    }

    /// DIMSTYLE_CONTROL — special: has an extra undocumented byte in R2000+.
    fn write_dimstyle_control(&mut self) {
        let table_handle = self.document.dim_styles.handle();
        let handles: Vec<Handle> = self.document.dim_styles.iter().map(|d| d.handle).collect();

        self.write_common_non_entity_data(
            common::OBJ_DIMSTYLE_CONTROL,
            table_handle,
            Handle::NULL,
            &[],
            &None,
        );

        self.writer.write_bit_long(handles.len() as i32);

        // Undocumented byte in R2000+
        if self.version.r2000_plus() {
            self.writer.write_byte(0);
        }

        for h in &handles {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, h.value());
        }

        self.register_object(table_handle);
    }

    /// VPENT_HDR_CONTROL — R13-R2000 only.
    /// Viewport entity header table control.
    /// The header section references this via hard-ownership handle.
    fn write_vpent_hdr_control(&mut self) {
        let table_handle = self.document.vx_table.handle();
        if table_handle.is_null() {
            return;
        }

        let mut handles = self.document.vx_control_entries.clone();
        for record in self.document.vx_table.iter() {
            if !handles.contains(&record.handle) {
                handles.push(record.handle);
            }
        }

        self.write_common_non_entity_data(
            common::OBJ_VPENT_HDR_CONTROL,
            table_handle,
            Handle::NULL,
            &[],
            &None,
        );
        self.writer.write_bit_short(handles.len() as i16);
        for handle in &handles {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, handle.value());
        }
        self.register_object(table_handle);
    }

    // ── Table entry writers ─────────────────────────────────────────

    fn write_vx_entries(&mut self) {
        let entries: Vec<_> = self.document.vx_table.iter().cloned().collect();
        for entry in &entries {
            self.write_vx_entry(entry);
        }
    }

    fn write_vx_entry(&mut self, entry: &crate::tables::VxTableRecord) {
        self.write_common_non_entity_data(
            common::OBJ_VPENT_HDR,
            entry.handle,
            self.document.vx_table.handle(),
            &[],
            &None,
        );

        self.writer.write_variable_text(&entry.name);
        self.writer.write_bit(entry.is_xref_reference);
        self.writer
            .write_bit_short(if entry.is_xref_resolved { 256 } else { 0 });
        self.writer.write_bit(entry.is_xref_dependent);
        self.writer.write_bit(entry.is_on);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, entry.xref_handle.value());
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, entry.viewport.value());
        self.writer
            .write_handle(DwgReferenceType::HardPointer, entry.previous_entry.value());
        self.register_object(entry.handle);
    }

    fn write_layer_entries(&mut self) {
        let entries: Vec<_> = self.document.layers.iter().map(|l| l.clone()).collect();
        for layer in &entries {
            self.write_layer(layer);
        }
    }

    fn write_layer(&mut self, layer: &crate::tables::Layer) {
        let transparency_app = self.document.app_ids.get("AcCmTransparency");
        let had_transparency_eed = transparency_app.is_some_and(|app| {
            self.document
                .eed_by_handle
                .get(&layer.handle)
                .is_some_and(|blocks| {
                    blocks
                        .iter()
                        .any(|(app_handle, _)| *app_handle == app.handle.value())
                })
        });
        let transparency_eed = transparency_app.and_then(|app| {
            (!layer.transparency.is_opaque() || had_transparency_eed).then(|| {
                let mut bytes = vec![71];
                bytes.extend_from_slice(&layer.transparency.to_dxf_value().to_le_bytes());
                (app.handle.value(), bytes)
            })
        });
        self.write_common_non_entity_data_eed(
            common::OBJ_LAYER,
            layer.handle,
            self.document.layers.handle(),
            &[],
            &None,
            transparency_eed,
        );

        // Entry name
        self.writer.write_variable_text(&layer.name);

        // Xref-dependant bit
        self.write_xref_dependant_bit_value(layer.flags.xref_dependent);

        if self.version.r2000_plus() {
            let lw_index = layer.line_weight.to_dwg_index() as i16;
            let mut values: i16 = (lw_index & 0x1F) << 5; // lineweight in bits 5..9

            if layer.flags.frozen {
                values |= 0b0001;
            }
            // "off" flag goes in bit 1 (inverted: on → bit clear)
            if layer.flags.off {
                values |= 0b0010;
            }
            if layer.flags.frozen_in_new_viewport {
                values |= 0b0100;
            }
            if layer.flags.locked {
                values |= 0b1000;
            }
            if layer.is_plottable {
                values |= 0b10000;
            }
            self.writer.write_bit_short(values);
        } else {
            self.writer.write_bit(layer.flags.frozen);
            self.writer.write_bit(layer.flags.off); // off flag (0=on, 1=off, same as R2000+)
            self.writer.write_bit(layer.flags.frozen_in_new_viewport);
            self.writer.write_bit(layer.flags.locked);
        }

        // Color (CMC)
        self.writer.write_cm_color_with_names(
            &layer.color,
            layer.color_name.as_deref(),
            layer.book_name.as_deref(),
        );

        // External reference block handle
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            layer.xref_block_record_handle.value(),
        );

        if self.version.r2000_plus() {
            // Plotstyle handle
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                layer.plotstyle_handle.value(),
            );
        }

        if self.version.r2007_plus() {
            // Material handle
            let material = if self.is_writable_object(&layer.material) {
                layer.material
            } else {
                Handle::NULL
            };
            self.writer
                .write_handle(DwgReferenceType::HardPointer, material.value());
        }

        // Linetype handle — look up by name
        let lt_handle = self
            .document
            .line_types
            .get(&layer.line_type)
            .map(|lt| lt.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, lt_handle.value());

        if self.version.r2013_plus(self.dxf_version) {
            self.writer.write_handle(DwgReferenceType::HardPointer, 0);
        }

        self.register_object(layer.handle);
    }

    fn write_text_style_entries(&mut self) {
        let entries: Vec<_> = self
            .document
            .text_styles
            .iter()
            .map(|s| s.clone())
            .collect();
        for style in &entries {
            self.write_text_style(style);
        }
    }

    fn write_text_style(&mut self, style: &crate::tables::TextStyle) {
        let anno = self.annotative_eed_block(style.annotative);
        self.write_common_non_entity_data_eed(
            common::OBJ_STYLE,
            style.handle,
            self.document.text_styles.handle(),
            &[],
            &None,
            anno,
        );

        // Entry name
        self.writer.write_variable_text(&style.name);

        // Xref-dependant
        self.write_xref_dependant_bit();

        // Shape file flag
        self.writer.write_bit(style.is_shape_file);
        // Vertical flag
        self.writer.write_bit(style.is_vertical);

        // Fixed height
        self.writer.write_bit_double(style.height);
        // Width factor
        self.writer.write_bit_double(style.width_factor);
        // Oblique angle
        self.writer.write_bit_double(style.oblique_angle);
        // Generation (mirror flags)
        self.writer.write_byte(0);
        // Last height (must be > 0; use effective_last_height)
        self.writer.write_bit_double(style.effective_last_height());
        // Font name
        self.writer.write_variable_text(&style.font_file);
        // Big font name
        self.writer.write_variable_text(&style.big_font_file);

        // External reference block handle (hard pointer)
        // Null for non-xref-dependent styles
        self.writer.write_handle(DwgReferenceType::HardPointer, 0);

        self.register_object(style.handle);
    }

    fn write_ltype_entries(&mut self) {
        let entries: Vec<_> = self
            .document
            .line_types
            .iter()
            .map(|lt| lt.clone())
            .collect();
        for lt in &entries {
            self.write_line_type(lt);
        }
    }

    fn write_line_type(&mut self, ltype: &crate::tables::LineType) {
        self.write_common_non_entity_data(
            common::OBJ_LTYPE,
            ltype.handle,
            self.document.line_types.handle(),
            &[],
            &None,
        );

        // Entry name
        self.writer.write_variable_text(&ltype.name);
        // Xref
        self.write_xref_dependant_bit();
        // Description
        self.writer.write_variable_text(&ltype.description);
        // Pattern length
        self.writer.write_bit_double(ltype.pattern_length);
        // Alignment
        self.writer.write_byte(b'A');
        // Num dashes
        self.writer.write_byte(ltype.elements.len() as u8);

        let unicode_text = self.version.r2007_plus();
        let mut text_area = if unicode_text {
            ltype
                .elements
                .iter()
                .any(|seg| seg.complex.as_ref().is_some_and(|cx| cx.is_text()))
                .then(|| vec![0u8; 512])
        } else {
            Some(vec![0u8; 256])
        };
        let mut text_cursor = if self.dxf_version <= DxfVersion::AC1014 {
            1usize
        } else {
            0usize
        };
        let mut shape_numbers = Vec::with_capacity(ltype.elements.len());
        let mut shape_flags = Vec::with_capacity(ltype.elements.len());

        for seg in &ltype.elements {
            let c = seg.complex.as_ref();
            let flags = if let Some(ref cx) = c {
                let mut f: i16 = 0;
                if cx.is_absolute_rotation() {
                    f |= 0x01;
                }
                if cx.is_text() {
                    f |= 0x02;
                }
                if cx.is_shape() {
                    f |= 0x04;
                }
                f
            } else {
                0
            };
            shape_flags.push(flags);

            let mut shape_number = c.and_then(|cx| cx.shape_number()).unwrap_or(0);
            if let (Some(cx), Some(area)) = (c, text_area.as_mut()) {
                if let Some(text) = cx.text().filter(|text| !text.is_empty()) {
                    let bytes = if unicode_text {
                        text.encode_utf16()
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>()
                    } else {
                        self.writer.encode_legacy_text(text)
                    };
                    let terminator_len = if unicode_text { 2 } else { 1 };
                    if text_cursor + bytes.len() + terminator_len <= area.len() {
                        shape_number = text_cursor as i16;
                        area[text_cursor..text_cursor + bytes.len()].copy_from_slice(&bytes);
                        text_cursor += bytes.len() + terminator_len;
                    } else {
                        shape_number = 0;
                    }
                }
            }
            shape_numbers.push(shape_number);
        }

        for ((seg, shape_number), flags) in
            ltype.elements.iter().zip(shape_numbers).zip(shape_flags)
        {
            let c = seg.complex.as_ref();
            self.writer.write_bit_double(seg.length);
            self.writer.write_bit_short(shape_number);
            self.writer
                .write_raw_double(c.map_or(0.0, |cx| cx.offset[0]));
            self.writer
                .write_raw_double(c.map_or(0.0, |cx| cx.offset[1]));
            self.writer.write_bit_double(c.map_or(1.0, |cx| cx.scale));
            self.writer
                .write_bit_double(c.map_or(0.0, |cx| cx.rotation));
            self.writer.write_bit_short(flags);
        }

        // R2004- always carries 256 encoded bytes. R2007+ carries 512
        // UTF-16LE bytes only when at least one text segment exists.
        if let Some(area) = text_area {
            for byte in area {
                self.writer.write_byte(byte);
            }
        }

        // External reference block handle
        self.writer.write_handle(DwgReferenceType::HardPointer, 0);

        // Shape file handles for each segment
        for seg in &ltype.elements {
            let sh = seg.complex.as_ref().map_or(0, |cx| cx.style_handle.value());
            self.writer.write_handle(DwgReferenceType::HardPointer, sh);
        }

        self.register_object(ltype.handle);
    }

    fn write_view_entries(&mut self) {
        let entries: Vec<_> = self.document.views.iter().map(|v| v.clone()).collect();
        for view in &entries {
            self.write_view(view);
        }
    }

    fn write_view(&mut self, view: &crate::tables::View) {
        self.write_common_non_entity_data(
            common::OBJ_VIEW,
            view.handle,
            self.document.views.handle(),
            &[],
            &None,
        );

        self.writer.write_variable_text(&view.name);
        self.write_xref_table_flags(view.xref_reference, view.xref_resolved, view.xref_dependent);

        self.writer.write_bit_double(view.height);
        self.writer.write_bit_double(view.width);
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: view.center.x,
            y: view.center.y,
        });
        self.writer.write_3bit_double(view.target);
        self.writer.write_3bit_double(view.direction);
        self.writer.write_bit_double(view.twist_angle);
        self.writer.write_bit_double(view.lens_length);
        self.writer.write_bit_double(view.front_clip);
        self.writer.write_bit_double(view.back_clip);

        // View mode (4 bits)
        self.writer.write_bit(view.perspective);
        self.writer.write_bit(view.front_clipping);
        self.writer.write_bit(view.back_clipping);
        self.writer.write_bit(view.front_clip_at_eye);

        if self.version.r2000_plus() {
            self.writer.write_byte(view.render_mode.to_value() as u8);
        }

        if self.version.r2007_plus() {
            self.writer.write_bit(view.use_default_lights);
            self.writer.write_byte(view.default_lighting_type as u8);
            self.writer.write_bit_double(view.brightness);
            self.writer.write_bit_double(view.contrast);
            self.writer.write_cm_color(&view.ambient_color);
        }

        self.writer.write_bit(view.paper_space);

        if self.version.r2000_plus() {
            self.writer.write_bit(view.ucs_associated);
            if view.ucs_associated {
                self.writer.write_3bit_double(view.ucs_origin);
                self.writer.write_3bit_double(view.ucs_x_axis);
                self.writer.write_3bit_double(view.ucs_y_axis);
                self.writer.write_bit_double(view.ucs_elevation);
                self.writer.write_bit_short(view.ucs_ortho_type);
            }
        }

        if self.version.r2007_plus() {
            self.writer.write_bit(view.camera_plottable);
        }

        self.writer
            .write_handle(DwgReferenceType::HardPointer, view.xref_handle.value());

        if self.version.r2007_plus() {
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                view.background_handle.value(),
            );
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                view.visual_style_handle.value(),
            );
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, view.sun_handle.value());
        }

        if self.version.r2000_plus() && view.ucs_associated {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, view.base_ucs_handle.value());
            self.writer
                .write_handle(DwgReferenceType::HardPointer, view.named_ucs_handle.value());
        }

        if self.version.r2007_plus() {
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                view.live_section_handle.value(),
            );
        }

        self.register_object(view.handle);
    }

    fn write_ucs_entries(&mut self) {
        let entries: Vec<_> = self.document.ucss.iter().map(|u| u.clone()).collect();
        for ucs in &entries {
            self.write_ucs(ucs);
        }
    }

    fn write_ucs(&mut self, ucs: &crate::tables::Ucs) {
        self.write_common_non_entity_data(
            common::OBJ_UCS,
            ucs.handle,
            self.document.ucss.handle(),
            &[],
            &None,
        );

        self.writer.write_variable_text(&ucs.name);
        self.write_xref_table_flags(ucs.xref_reference, ucs.xref_resolved, ucs.xref_dependent);

        self.writer.write_3bit_double(ucs.origin);
        self.writer.write_3bit_double(ucs.x_axis);
        self.writer.write_3bit_double(ucs.y_axis);

        if self.version.r2000_plus() {
            self.writer.write_bit_double(ucs.elevation);
            self.writer.write_bit_short(ucs.ortho_view_type);
            self.writer.write_bit_short(ucs.ortho_type);
        }

        self.writer
            .write_handle(DwgReferenceType::HardPointer, ucs.xref_handle.value());

        if self.version.r2000_plus() {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ucs.base_ucs_handle.value());
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ucs.named_ucs_handle.value());
        }

        self.register_object(ucs.handle);
    }

    fn write_vport_entries(&mut self) {
        let entries: Vec<_> = self.document.vports.iter().cloned().collect();
        for vp in &entries {
            self.write_vport(vp);
        }
    }

    fn write_vport(&mut self, vport: &crate::tables::VPort) {
        self.write_common_non_entity_data(
            common::OBJ_VPORT,
            vport.handle,
            self.document.vports.handle(),
            &[],
            &None,
        );

        // Common: Entry name TV 2
        self.writer.write_variable_text(&vport.name);
        self.write_xref_table_flags(
            vport.xref_reference,
            vport.xref_resolved,
            vport.xref_dependent,
        );

        // View height BD 40
        self.writer.write_bit_double(vport.view_height);
        // Aspect ratio BD 41 — DWG stores aspect_ratio * view_height
        // (R13 quirk; reader divides by view_height to get actual ratio)
        self.writer
            .write_bit_double(vport.aspect_ratio * vport.view_height);
        // View Center 2RD 12
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.view_center.x,
            y: vport.view_center.y,
        });
        // View target 3BD 17
        self.writer.write_3bit_double(vport.view_target);
        // View dir 3BD 16
        self.writer.write_3bit_double(vport.view_direction);
        // View twist BD 51
        self.writer.write_bit_double(vport.view_twist);
        // Lens length BD 42
        self.writer.write_bit_double(vport.lens_length);
        // Front clip BD 43
        self.writer.write_bit_double(vport.front_clip);
        // Back clip BD 44
        self.writer.write_bit_double(vport.back_clip);

        // View mode X 71 — 4 bits: 0123
        self.writer.write_bit(vport.perspective);
        self.writer.write_bit(vport.front_clipping);
        self.writer.write_bit(vport.back_clipping);
        self.writer.write_bit(vport.front_clip_at_eye);

        // R2000+: Render Mode RC 281
        if self.version.r2000_plus() {
            self.writer.write_byte(vport.render_mode.to_value() as u8);
        }

        // R2007+: lighting
        if self.version.r2007_plus() {
            // Use default lights B 292
            self.writer.write_bit(vport.use_default_lights);
            // Default lighting type RC 282
            self.writer.write_byte(vport.default_lighting_type as u8);
            // Brightness BD 141
            self.writer.write_bit_double(vport.brightness);
            // Contrast BD 142
            self.writer.write_bit_double(vport.contrast);
            // Ambient Color CMC 63
            self.writer.write_cm_color(&vport.ambient_color);
        }

        // Common: Lower left 2RD 10
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.lower_left.x,
            y: vport.lower_left.y,
        });
        // Common: Upper right 2RD 11
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.upper_right.x,
            y: vport.upper_right.y,
        });

        // UCSFOLLOW B 71
        self.writer.write_bit(vport.ucsfollow);
        // Circle zoom BS 72
        self.writer.write_bit_short(vport.circle_zoom);
        // Fast zoom B 73
        self.writer.write_bit(vport.fast_zoom);
        // UCSICON X 74 — 2 individual bits
        self.writer.write_bit(vport.ucsicon_lower);
        self.writer.write_bit(vport.ucsicon_origin);
        // Grid on/off B 76
        self.writer.write_bit(vport.grid_on);
        // Grid spacing 2RD 15
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.grid_spacing.x,
            y: vport.grid_spacing.y,
        });
        // Snap on/off B 75
        self.writer.write_bit(vport.snap_on);
        // Snap style B 77
        self.writer.write_bit(vport.snap_style);
        // Snap isopair BS 78
        self.writer.write_bit_short(vport.snap_isopair);
        // Snap rot BD 50
        self.writer.write_bit_double(vport.snap_rotation);
        // Snap base 2RD 13
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.snap_base.x,
            y: vport.snap_base.y,
        });
        // Snap spacing 2RD 14
        self.writer.write_2raw_double(crate::types::Vector2 {
            x: vport.snap_spacing.x,
            y: vport.snap_spacing.y,
        });

        // R2000+
        if self.version.r2000_plus() {
            self.writer.write_bit(vport.ucs_at_origin);
            self.writer.write_bit(vport.ucs_per_viewport);
            self.writer.write_3bit_double(vport.ucs_origin);
            self.writer.write_3bit_double(vport.ucs_x_axis);
            self.writer.write_3bit_double(vport.ucs_y_axis);
            self.writer.write_bit_double(vport.ucs_elevation);
            self.writer.write_bit_short(vport.ucs_ortho_type);
        }

        // R2007+
        if self.version.r2007_plus() {
            // Grid flags BS 60 — adaptive grid enabled
            self.writer.write_bit_short(vport.grid_flags.to_bits());
            // Grid major BS 61
            self.writer.write_bit_short(vport.grid_major);
        }

        // Common: External reference block handle (hard pointer)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, vport.xref_handle.value());

        // R2007+
        if self.version.r2007_plus() {
            // Background handle H 332 soft pointer (code 4)
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                vport.background_handle.value(),
            );
            // Visual Style handle H 348 hard pointer (code 5)
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                vport.visual_style_handle.value(),
            );
            // Sun handle H 361 hard owner (code 3)
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, vport.sun_handle.value());
        }

        // R2000+
        if self.version.r2000_plus() {
            // Named UCS Handle H 345 hard pointer
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                vport.named_ucs_handle.value(),
            );
            // Base UCS Handle H 346 hard pointer
            self.writer
                .write_handle(DwgReferenceType::HardPointer, vport.base_ucs_handle.value());
        }

        self.register_object(vport.handle);
    }

    fn write_appid_entries(&mut self) {
        let mut entries: Vec<_> = self.document.app_ids.iter().map(|a| a.clone()).collect();
        entries.sort_by_key(|app| !app.name.eq_ignore_ascii_case("ACAD"));
        for app in &entries {
            self.write_appid(app);
        }
    }

    fn write_appid(&mut self, app: &crate::tables::AppId) {
        self.write_common_non_entity_data(
            common::OBJ_APPID,
            app.handle,
            self.document.app_ids.handle(),
            &[],
            &None,
        );

        // Sanitize name: strip control chars and forbidden symbol table characters
        let name: String = app
            .name
            .chars()
            .filter(|c| {
                !c.is_control()
                    && !matches!(
                        c,
                        '<' | '>'
                            | '/'
                            | '\\'
                            | '"'
                            | ':'
                            | ';'
                            | '?'
                            | '*'
                            | '|'
                            | ','
                            | '='
                            | '`'
                    )
            })
            .collect();
        self.writer.write_variable_text(&name);
        self.write_xref_dependant_bit();

        // Unknown byte (group 71)
        self.writer.write_byte(0);

        // External reference block handle
        self.writer.write_handle(DwgReferenceType::HardPointer, 0);

        self.register_object(app.handle);
    }

    fn write_dimstyle_entries(&mut self) {
        let entries: Vec<_> = self.document.dim_styles.iter().map(|d| d.clone()).collect();
        for ds in &entries {
            self.write_dimstyle(ds);
        }
    }

    fn write_dimstyle(&mut self, ds: &crate::tables::DimStyle) {
        let dimclrd_color = ds
            .dimclrd_true_color
            .unwrap_or_else(|| crate::types::Color::from_index(ds.dimclrd));
        let dimclre_color = ds
            .dimclre_true_color
            .unwrap_or_else(|| crate::types::Color::from_index(ds.dimclre));
        let dimclrt_color = ds
            .dimclrt_true_color
            .unwrap_or_else(|| crate::types::Color::from_index(ds.dimclrt));
        let dimtfillclr_color = ds
            .dimtfillclr_true_color
            .unwrap_or_else(|| crate::types::Color::from_index(ds.dimtfillclr));
        let anno = self.annotative_eed_block(ds.annotative);
        self.write_common_non_entity_data_eed(
            common::OBJ_DIMSTYLE,
            ds.handle,
            self.document.dim_styles.handle(),
            &[],
            &None,
            anno,
        );

        // Common: Entry name TV 2
        self.writer.write_variable_text(&ds.name);
        self.write_xref_table_flags(ds.xref_reference, ds.xref_resolved, ds.xref_dependent);

        // ── R13/R14 Only: DimStyle fields ───────────────────────────
        // These fields are ONLY written for R13/R14 (not R2000+).
        // Field order matches the reference writeDimensionStyle() R13_14Only block.
        if self.version.r13_14_only() {
            // DIMTOL B 71
            self.writer.write_bit(ds.dimtol);
            // DIMLIM B 72
            self.writer.write_bit(ds.dimlim);
            // DIMTIH B 73
            self.writer.write_bit(ds.dimtih);
            // DIMTOH B 74
            self.writer.write_bit(ds.dimtoh);
            // DIMSE1 B 75
            self.writer.write_bit(ds.dimse1);
            // DIMSE2 B 76
            self.writer.write_bit(ds.dimse2);
            // DIMALT B 170
            self.writer.write_bit(ds.dimalt);
            // DIMTOFL B 172
            self.writer.write_bit(ds.dimtofl);
            // DIMSAH B 173
            self.writer.write_bit(ds.dimsah);
            // DIMTIX B 174
            self.writer.write_bit(ds.dimtix);
            // DIMSOXD B 175
            self.writer.write_bit(ds.dimsoxd);
            // DIMALTD RC 171
            self.writer.write_byte(ds.dimaltd as u8);
            // DIMZIN RC 78
            self.writer.write_byte(ds.dimzin as u8);
            // DIMSD1 B 281
            self.writer.write_bit(ds.dimsd1);
            // DIMSD2 B 282
            self.writer.write_bit(ds.dimsd2);
            // DIMTOLJ RC 283
            self.writer.write_byte(ds.dimtolj as u8);
            // DIMJUST RC 280
            self.writer.write_byte(ds.dimjust as u8);
            // DIMFIT RC 287
            self.writer.write_byte(ds.dimfit as u8);
            // DIMUPT B 288
            self.writer.write_bit(ds.dimupt);
            // DIMTZIN RC 284
            self.writer.write_byte(ds.dimtzin as u8);
            // DIMALTZ RC 285
            self.writer.write_byte(ds.dimaltz as u8);
            // DIMALTTZ RC 286
            self.writer.write_byte(ds.dimalttz as u8);
            // DIMTAD RC 77
            self.writer.write_byte(ds.dimtad as u8);
            // DIMUNIT BS 270
            self.writer.write_bit_short(ds.dimunit);
            // DIMAUNIT BS 275
            self.writer.write_bit_short(ds.dimaunit);
            // DIMDEC BS 271
            self.writer.write_bit_short(ds.dimdec);
            // DIMTDEC BS 272
            self.writer.write_bit_short(ds.dimtdec);
            // DIMALTU BS 273
            self.writer.write_bit_short(ds.dimaltu);
            // DIMALTTD BS 274
            self.writer.write_bit_short(ds.dimalttd);
            // DIMSCALE BD 40
            self.writer.write_bit_double(ds.dimscale);
            // DIMASZ BD 41
            self.writer.write_bit_double(ds.dimasz);
            // DIMEXO BD 42
            self.writer.write_bit_double(ds.dimexo);
            // DIMDLI BD 43
            self.writer.write_bit_double(ds.dimdli);
            // DIMEXE BD 44
            self.writer.write_bit_double(ds.dimexe);
            // DIMRND BD 45
            self.writer.write_bit_double(ds.dimrnd);
            // DIMDLE BD 46
            self.writer.write_bit_double(ds.dimdle);
            // DIMTP BD 47
            self.writer.write_bit_double(ds.dimtp);
            // DIMTM BD 48
            self.writer.write_bit_double(ds.dimtm);
            // DIMTXT BD 140
            self.writer.write_bit_double(ds.dimtxt);
            // DIMCEN BD 141
            self.writer.write_bit_double(ds.dimcen);
            // DIMTSZ BD 142
            self.writer.write_bit_double(ds.dimtsz);
            // DIMALTF BD 143
            self.writer.write_bit_double(ds.dimaltf);
            // DIMLFAC BD 144
            self.writer.write_bit_double(ds.dimlfac);
            // DIMTVP BD 145
            self.writer.write_bit_double(ds.dimtvp);
            // DIMTFAC BD 146
            self.writer.write_bit_double(ds.dimtfac);
            // DIMGAP BD 147
            self.writer.write_bit_double(ds.dimgap);
            // DIMPOST T 3
            self.writer.write_variable_text(&ds.dimpost);
            // DIMAPOST T 4
            self.writer.write_variable_text(&ds.dimapost);
            // DIMBLK T 5
            self.writer.write_variable_text(&ds.dimblk_name);
            // DIMBLK1 T 6
            self.writer.write_variable_text(&ds.dimblk1_name);
            // DIMBLK2 T 7
            self.writer.write_variable_text(&ds.dimblk2_name);
            // DIMCLRD BS 176
            self.writer.write_cm_color(&dimclrd_color);
            // DIMCLRE BS 177
            self.writer.write_cm_color(&dimclre_color);
            // DIMCLRT BS 178
            self.writer.write_cm_color(&dimclrt_color);
        }

        // ── R2000+ DimStyle fields ──────────────────────────────────
        // Field order, data types, and version guards match the reference implementation
        // DwgObjectWriter.writeDimensionStyle() exactly.
        if self.version.r2000_plus() {
            // DIMPOST TV 3
            self.writer.write_variable_text(&ds.dimpost);
            // DIMAPOST TV 4
            self.writer.write_variable_text(&ds.dimapost);
            // DIMSCALE BD 40
            self.writer.write_bit_double(ds.dimscale);
            // DIMASZ BD 41
            self.writer.write_bit_double(ds.dimasz);
            // DIMEXO BD 42
            self.writer.write_bit_double(ds.dimexo);
            // DIMDLI BD 43
            self.writer.write_bit_double(ds.dimdli);
            // DIMEXE BD 44
            self.writer.write_bit_double(ds.dimexe);
            // DIMRND BD 45
            self.writer.write_bit_double(ds.dimrnd);
            // DIMDLE BD 46
            self.writer.write_bit_double(ds.dimdle);
            // DIMTP BD 47
            self.writer.write_bit_double(ds.dimtp);
            // DIMTM BD 48
            self.writer.write_bit_double(ds.dimtm);
        }

        // R2007+
        if self.version.r2007_plus() {
            // DIMFXL BD 49
            self.writer.write_bit_double(ds.dimfxl);
            // DIMJOGANG BD 50 — clamp to valid range [5°..90°]
            self.writer
                .write_bit_double(ds.dimjogang.clamp(0.0872665, 1.5708));
            // DIMTFILL BS 69
            self.writer.write_bit_short(ds.dimtfill);
            // DIMTFILLCLR CMC 70
            self.writer.write_cm_color(&dimtfillclr_color);
        }

        // R2000+
        if self.version.r2000_plus() {
            // DIMTOL B 71
            self.writer.write_bit(ds.dimtol);
            // DIMLIM B 72
            self.writer.write_bit(ds.dimlim);
            // DIMTIH B 73
            self.writer.write_bit(ds.dimtih);
            // DIMTOH B 74
            self.writer.write_bit(ds.dimtoh);
            // DIMSE1 B 75
            self.writer.write_bit(ds.dimse1);
            // DIMSE2 B 76
            self.writer.write_bit(ds.dimse2);
            // DIMTAD BS 77
            self.writer.write_bit_short(ds.dimtad);
            // DIMZIN BS 78
            self.writer.write_bit_short(ds.dimzin);
            // DIMAZIN BS 79
            self.writer.write_bit_short(ds.dimazin);
        }

        // R2007+
        if self.version.r2007_plus() {
            // DIMARCSYM BS 90
            self.writer.write_bit_short(ds.dimarcsym);
        }

        // R2000+
        if self.version.r2000_plus() {
            // DIMTXT BD 140
            self.writer.write_bit_double(ds.dimtxt);
            // DIMCEN BD 141
            self.writer.write_bit_double(ds.dimcen);
            // DIMTSZ BD 142
            self.writer.write_bit_double(ds.dimtsz);
            // DIMALTF BD 143
            self.writer.write_bit_double(ds.dimaltf);
            // DIMLFAC BD 144
            self.writer.write_bit_double(ds.dimlfac);
            // DIMTVP BD 145
            self.writer.write_bit_double(ds.dimtvp);
            // DIMTFAC BD 146
            self.writer.write_bit_double(ds.dimtfac);
            // DIMGAP BD 147
            self.writer.write_bit_double(ds.dimgap);
            // DIMALTRND BD 148
            self.writer.write_bit_double(ds.dimaltrnd);
            // DIMALT B 170
            self.writer.write_bit(ds.dimalt);
            // DIMALTD BS 171
            self.writer.write_bit_short(ds.dimaltd);
            // DIMTOFL B 172
            self.writer.write_bit(ds.dimtofl);
            // DIMSAH B 173
            self.writer.write_bit(ds.dimsah);
            // DIMTIX B 174
            self.writer.write_bit(ds.dimtix);
            // DIMSOXD B 175
            self.writer.write_bit(ds.dimsoxd);
            // DIMCLRD BS 176
            self.writer.write_cm_color(&dimclrd_color);
            // DIMCLRE BS 177
            self.writer.write_cm_color(&dimclre_color);
            // DIMCLRT BS 178
            self.writer.write_cm_color(&dimclrt_color);
            // DIMADEC BS 179
            self.writer.write_bit_short(ds.dimadec);
            // DIMDEC BS 271
            self.writer.write_bit_short(ds.dimdec);
            // DIMTDEC BS 272
            self.writer.write_bit_short(ds.dimtdec);
            // DIMALTU BS 273
            self.writer.write_bit_short(ds.dimaltu);
            // DIMALTTD BS 274
            self.writer.write_bit_short(ds.dimalttd);
            // DIMAUNIT BS 275
            self.writer.write_bit_short(ds.dimaunit);
            // DIMFRAC BS 276
            self.writer.write_bit_short(ds.dimfrac);
            // DIMLUNIT BS 277
            self.writer.write_bit_short(ds.dimlunit);
            // DIMDSEP BS 278
            self.writer.write_bit_short(ds.dimdsep);
            // DIMTMOVE BS 279
            self.writer.write_bit_short(ds.dimtmove);
            // DIMJUST BS 280
            self.writer.write_bit_short(ds.dimjust);
            // DIMSD1 B 281
            self.writer.write_bit(ds.dimsd1);
            // DIMSD2 B 282
            self.writer.write_bit(ds.dimsd2);
            // DIMTOLJ BS 283
            self.writer.write_bit_short(ds.dimtolj);
            // DIMTZIN BS 284
            self.writer.write_bit_short(ds.dimtzin);
            // DIMALTZ BS 285
            self.writer.write_bit_short(ds.dimaltz);
            // DIMALTTZ BS 286
            self.writer.write_bit_short(ds.dimalttz);
            // DIMUPT B 288
            self.writer.write_bit(ds.dimupt);
            // DIMATFIT BS 289
            self.writer.write_bit_short(ds.dimatfit);
        }

        // R2007+
        if self.version.r2007_plus() {
            // DIMFXLON B 290
            self.writer.write_bit(ds.dimfxlon);
        }

        // R2010+
        if self.version.r2010_plus() {
            // DIMTXTDIRECTION B 295
            self.writer.write_bit(ds.dimtxtdirection);
            // DIMALTMZF BD
            self.writer.write_bit_double(ds.dimaltmzf);
            // DIMALTMZS T
            self.writer.write_variable_text(&ds.dimaltmzs);
            // DIMMZF BD
            self.writer.write_bit_double(ds.dimmzf);
            // DIMMZS T
            self.writer.write_variable_text(&ds.dimmzs);
        }

        // R2000+
        if self.version.r2000_plus() {
            // DIMLWD BS 371
            self.writer.write_bit_short(ds.dimlwd);
            // DIMLWE BS 372
            self.writer.write_bit_short(ds.dimlwe);
        }

        // Common: Unknown B 70
        self.writer.write_bit(false);

        // ── Handle references ───────────────────────────────────────

        // External reference block handle (hard pointer)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ds.xref_handle.value());

        // 340 DIMTXSTY (hard pointer)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ds.dimtxsty_handle.value());

        // R2000+
        if self.version.r2000_plus() {
            // 341 DIMLDRBLK (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimldrblk.value());
            // 342 DIMBLK (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimblk.value());
            // 343 DIMBLK1 (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimblk1.value());
            // 344 DIMBLK2 (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimblk2.value());
        }

        // R2007+
        if self.version.r2007_plus() {
            // 345 dimltype (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimltex_handle.value());
            // 346 dimltex1 (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimltex1_handle.value());
            // 347 dimltex2 (hard pointer)
            self.writer
                .write_handle(DwgReferenceType::HardPointer, ds.dimltex2_handle.value());
        }

        self.register_object(ds.handle);
    }

    // ── Block entity writing ────────────────────────────────────────

    /// Write block begin/entities/end for every block record.
    fn write_block_entities(&mut self) {
        use crate::io::dwg::parallel::{map_chunks, worker_count};

        let block_records: Vec<BlockRecord> = self
            .document
            .block_records
            .iter()
            .map(|br| br.clone())
            .collect();

        for br in &block_records {
            // An xref block record references an external drawing: its contents
            // live in that file, not here. Some hosts merge the resolved xref
            // geometry into the block record for display; serializing those as
            // owned entities would bind/explode the xref into the host file on
            // the next open. Write the header with an empty owned list and skip
            // the entity loop entirely, leaving only the BLOCK/ENDBLK markers.
            // This matches the reader, which writes/reads no owned-object count
            // for an xref block (see the `is_xref` guards in the header).
            let is_xref = br.flags.is_xref || br.flags.is_xref_overlay;

            // Keep only live entities directly owned by the block header.
            let live_handles: Vec<Handle> = br
                .entity_handles
                .iter()
                .copied()
                .filter(|h| self.document.entity_index.contains_key(h))
                .collect();
            let entity_handles_for_header = if is_xref {
                Vec::new()
            } else {
                // Preserve the original order when it still matches the live set.
                match self.document.block_entity_handles.get(&br.handle) {
                    Some(orig) => {
                        use std::collections::HashSet;
                        let valid: HashSet<u64> = live_handles.iter().map(|h| h.value()).collect();
                        let filtered: Vec<Handle> = orig
                            .iter()
                            .copied()
                            .filter(|h| valid.contains(&h.value()))
                            .collect();
                        if filtered.len() == live_handles.len() {
                            filtered
                        } else {
                            live_handles.clone()
                        }
                    }
                    None => live_handles.clone(),
                }
            };
            self.write_block_header_with_handles(br, &entity_handles_for_header);
            self.write_block_begin(br);

            if !is_xref {
                // Look up entities by handle from the document. Iterate the
                // dangling-filtered list so the pre-R2004 prev/next links
                // never point at a handle that is not in the stream.
                let handles = &live_handles;
                let len = handles.len();
                let parallel = self.version.r2004_plus() && handles.len() >= 1_024;
                let mut i = 0usize;
                while i < len {
                    if parallel
                        && self
                            .document
                            .entity_index
                            .get(&handles[i])
                            .is_some_and(|idx| {
                                Self::parallel_entity_safe(self.document.entities[*idx].as_ref())
                            })
                    {
                        let start = i;
                        i += 1;
                        while i < len
                            && self
                                .document
                                .entity_index
                                .get(&handles[i])
                                .is_some_and(|idx| {
                                    Self::parallel_entity_safe(
                                        self.document.entities[*idx].as_ref(),
                                    )
                                })
                        {
                            i += 1;
                        }
                        let run = &handles[start..i];
                        let mut unique = ahash::AHashSet::new();
                        let safe_to_batch = run.len() >= 1_024
                            && run.iter().all(|handle| {
                                !self.registered_handles.contains(&handle.value())
                                    && unique.insert(handle.value())
                            });
                        if safe_to_batch {
                            let worker_count = worker_count();
                            let chunk_size =
                                ((run.len() + worker_count * 4 - 1) / (worker_count * 4)).max(512);
                            let batches: Vec<ParallelEntityBatch> =
                                map_chunks(run, chunk_size, |chunk| {
                                    self.serialize_parallel_entity_batch(chunk)
                                });
                            for batch in batches {
                                self.append_parallel_entity_batch(batch);
                            }
                            continue;
                        }
                        for (offset, eh) in run.iter().enumerate() {
                            if let Some(&idx) = self.document.entity_index.get(eh) {
                                self.prev_handle =
                                    (start + offset > 0).then(|| handles[start + offset - 1]);
                                self.next_handle =
                                    (start + offset + 1 < len).then(|| handles[start + offset + 1]);
                                self.write_entity(&self.document.entities[idx]);
                            }
                        }
                        continue;
                    }

                    let eh = &handles[i];
                    if let Some(&idx) = self.document.entity_index.get(eh) {
                        let entity = &self.document.entities[idx];
                        // Set prev/next for entity linking (pre-R2004)
                        self.prev_handle = if i > 0 { Some(handles[i - 1]) } else { None };
                        self.next_handle = if i + 1 < len {
                            Some(handles[i + 1])
                        } else {
                            None
                        };

                        self.write_entity(entity);
                    }
                    i += 1;
                }
            }

            self.prev_handle = None;
            self.next_handle = None;

            self.write_block_end(br);
        }
    }

    fn parallel_entity_safe(entity: &EntityType) -> bool {
        !matches!(
            entity,
            EntityType::Insert(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline3D(_)
                | EntityType::PolyfaceMesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::Polyline(_)
                | EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Block(_)
                | EntityType::BlockEnd(_)
        )
    }

    fn serialize_parallel_entity_batch(&self, handles: &[Handle]) -> ParallelEntityBatch {
        let encoding =
            crate::io::dxf::code_page::encoding_from_code_page(&self.document.header.code_page)
                .unwrap_or(encoding_rs::WINDOWS_1252);
        let mut worker = Self {
            version: self.version,
            dxf_version: self.dxf_version,
            document: self.document,
            writer: DwgMergedWriter::with_encoding(self.version, self.dxf_version, encoding),
            output: Vec::with_capacity(handles.len().saturating_mul(64)),
            handle_map: Vec::with_capacity(handles.len()),
            object_queue: VecDeque::new(),
            prev_handle: None,
            next_handle: None,
            next_alloc_handle: self.next_alloc_handle,
            model_space_extents: None,
            sab_entries: Vec::new(),
            pending_has_ds_data: false,
            visited_objects: HashSet::new(),
            class_instance_counts: HashMap::new(),
            pending_type_code: None,
            class_counts_complete: true,
            registered_handles: HashSet::with_capacity(handles.len()),
            owner_overrides: std::collections::HashMap::new(),
            linetype_handles: std::collections::HashMap::new(),
        };
        for handle in handles {
            if let Some(&idx) = worker.document.entity_index.get(handle) {
                worker.write_entity(&worker.document.entities[idx]);
            }
        }
        ParallelEntityBatch {
            output: worker.output,
            handle_map: worker.handle_map,
            object_queue: worker.object_queue,
            registered_handles: worker.registered_handles,
            class_instance_counts: worker.class_instance_counts,
            class_counts_complete: worker.class_counts_complete,
        }
    }

    fn append_parallel_entity_batch(&mut self, batch: ParallelEntityBatch) {
        debug_assert!(
            batch
                .registered_handles
                .iter()
                .all(|handle| !self.registered_handles.contains(handle)),
            "parallel entity batch emitted a duplicate handle"
        );
        let base = self.output.len() as u32;
        self.output.extend_from_slice(&batch.output);
        self.handle_map.extend(
            batch
                .handle_map
                .into_iter()
                .map(|(handle, offset)| (handle, base + offset)),
        );
        self.object_queue.extend(batch.object_queue);
        self.registered_handles.extend(batch.registered_handles);
        for (class_number, count) in batch.class_instance_counts {
            *self.class_instance_counts.entry(class_number).or_default() += count;
        }
        self.class_counts_complete &= batch.class_counts_complete;
    }

    /// Write a BLOCK_HEADER (block record) object with explicit entity handles.
    fn write_block_header_with_handles(&mut self, record: &BlockRecord, entity_handles: &[Handle]) {
        self.write_common_non_entity_data(
            common::OBJ_BLOCK_HEADER,
            record.handle,
            self.document.block_records.handle(),
            &[],
            &None,
        );

        // Entry name (DWG uses bare names without numeric suffixes)
        let dwg_name = dwg_block_name(&record.name);
        self.writer.write_variable_text(dwg_name);
        // Xref dependant
        self.write_xref_dependant_bit();

        // Anonymous flag
        self.writer.write_bit(record.flags.anonymous);
        // Has attributes
        self.writer.write_bit(record.flags.has_attributes);
        // Is xref
        self.writer.write_bit(record.flags.is_xref);
        // Is xref overlay
        self.writer.write_bit(record.flags.is_xref_overlay);

        // R2000+: loaded bit
        if self.version.r2000_plus() {
            self.writer.write_bit(false); // is loaded
        }

        // R2004+: owned object count (non-xref)
        if self.version.r2004_plus() && !record.flags.is_xref && !record.flags.is_xref_overlay {
            self.writer.write_bit_long(entity_handles.len() as i32);
        }

        // Base point (from Block entity if found)
        let base_pt = record
            .entity_handles
            .iter()
            .find_map(|eh| {
                if let Some(EntityType::Block(b)) = self
                    .document
                    .entity_index
                    .get(eh)
                    .map(|&idx| self.document.entities[idx].as_ref())
                {
                    Some(b.base_point)
                } else {
                    None
                }
            })
            .unwrap_or(record.base_point);
        self.writer.write_3bit_double(base_pt);

        // Xref path
        self.writer.write_variable_text(&record.xref_path);

        // R2000+: insert count bytes + block description + preview data
        if self.version.r2000_plus() {
            // Insert count bytes (non-zero bytes followed by zero terminator)
            for &b in &record.insert_count_bytes {
                self.writer.write_byte(b);
            }
            self.writer.write_byte(0);

            // Block description
            self.writer.write_variable_text(&record.description);

            // Preview data
            self.writer.write_bit_long(record.preview_data.len() as i32);
            for &b in &record.preview_data {
                self.writer.write_byte(b);
            }
        }

        // R2007+: units, explodable, scaling
        if self.version.r2007_plus() {
            self.writer.write_bit_short(record.units);
            self.writer.write_bit(record.explodable);
            self.writer
                .write_byte(if record.scale_uniformly { 1 } else { 0 });
        }

        // NULL handle (hard pointer)
        self.writer.write_handle(DwgReferenceType::HardPointer, 0);

        // BLOCK entity handle (hard owner)
        self.writer.write_handle(
            DwgReferenceType::HardOwnership,
            record.block_entity_handle.value(),
        );

        // R13-R2000: first/last entity handles
        if self.version.r13_15_only() && !record.flags.is_xref && !record.flags.is_xref_overlay {
            let first = entity_handles.first().copied().unwrap_or(Handle::NULL);
            let last = entity_handles.last().copied().unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, first.value());
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, last.value());
        }

        // R2004+: entity handles (hard owner). Xref blocks own no entities in
        // the format — the owned-object count is skipped above, so the reader
        // reads zero handles here. Writing any would desync the handle stream
        // and corrupt the block record on read, matching the R13-R2000 guard.
        if self.version.r2004_plus() && !record.flags.is_xref && !record.flags.is_xref_overlay {
            for h in entity_handles {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, h.value());
            }
        }

        // ENDBLK handle (hard owner)
        self.writer.write_handle(
            DwgReferenceType::HardOwnership,
            record.block_end_handle.value(),
        );

        // R2000+: insert handles come BEFORE the layout handle (ODA spec
        // order: endblk, inserts[num_inserts], layout). Writing layout first
        // desyncs the handle stream for any block that is referenced by an
        // insert, making AutoCAD discard the block record (eWrongObjectType).
        if self.version.r2000_plus() {
            for &ih in &record.insert_handles {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, ih.value());
            }
        }

        // R2000+: layout handle
        if self.version.r2000_plus() {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, record.layout.value());
        }

        self.register_object(record.handle);
    }

    /// Write BLOCK entity (block begin).
    fn write_block_begin(&mut self, record: &BlockRecord) {
        // New documents reserve marker handles in BlockRecord without storing
        // BLOCK entities. Synthesize those markers; imported ones retain metadata.
        let block = if !record.block_entity_handle.is_null() {
            self.document
                .entity_index
                .get(&record.block_entity_handle)
                .and_then(|&idx| {
                    if let EntityType::Block(b) = self.document.entities[idx].as_ref() {
                        Some(b.clone())
                    } else {
                        None
                    }
                })
        } else {
            None
        };

        let (handle, name, use_raw_name) = if let Some(ref b) = block {
            (b.common.handle, b.name.as_str(), true)
        } else {
            (record.block_entity_handle, record.name.as_str(), false)
        };

        let mut common = block
            .as_ref()
            .map(|b| &b.common)
            .cloned()
            .unwrap_or_else(|| EntityCommon {
                handle,
                owner_handle: record.handle,
                ..Default::default()
            });
        // The block record is the structural owner even when a damaged or
        // loosely decoded BLOCK marker carried a different common owner.
        common.owner_handle = record.handle;

        self.write_common_entity_data(
            common::OBJ_BLOCK,
            common.handle,
            common.owner_handle,
            &common.layer,
            &common.color,
            &common.line_weight,
            &common.transparency,
            common.invisible,
            common.linetype_scale,
            &common.linetype,
            &common.linetype_handle,
            &common.extended_data,
            &common.reactors,
            &common.xdictionary_handle,
            common.graphic_data.as_deref(),
            common.entity_mode,
            common.material_flags,
            &common.material_handle,
            common.shadow_flags,
            common.plotstyle_flags,
            &common.plotstyle_handle,
            &common.color_book_handle,
            &common.full_visual_style_handle,
            &common.face_visual_style_handle,
            &common.edge_visual_style_handle,
        );

        // Use the original name as-is when we have the Block entity from binary;
        // only apply dwg_block_name() for programmatically-created blocks.
        if use_raw_name {
            self.writer.write_variable_text(name);
        } else {
            self.writer.write_variable_text(dwg_block_name(name));
        }

        self.register_object(common.handle);
    }

    /// Write ENDBLK entity (block end).
    fn write_block_end(&mut self, record: &BlockRecord) {
        let block_end = if !record.block_end_handle.is_null() {
            self.document
                .entity_index
                .get(&record.block_end_handle)
                .and_then(|&idx| {
                    if let EntityType::BlockEnd(be) = self.document.entities[idx].as_ref() {
                        Some(be.clone())
                    } else {
                        None
                    }
                })
        } else {
            None
        };

        let mut common = block_end
            .map(|be| be.common)
            .unwrap_or_else(|| EntityCommon {
                handle: record.block_end_handle,
                owner_handle: record.handle,
                ..Default::default()
            });
        common.owner_handle = record.handle;

        self.write_common_entity_data(
            common::OBJ_ENDBLK,
            common.handle,
            common.owner_handle,
            &common.layer,
            &common.color,
            &common.line_weight,
            &common.transparency,
            common.invisible,
            common.linetype_scale,
            &common.linetype,
            &common.linetype_handle,
            &common.extended_data,
            &common.reactors,
            &common.xdictionary_handle,
            common.graphic_data.as_deref(),
            common.entity_mode,
            common.material_flags,
            &common.material_handle,
            common.shadow_flags,
            common.plotstyle_flags,
            &common.plotstyle_handle,
            &common.color_book_handle,
            &common.full_visual_style_handle,
            &common.face_visual_style_handle,
            &common.edge_visual_style_handle,
        );

        self.register_object(common.handle);
    }

    // ── Object queue draining ───────────────────────────────────────

    /// Drain the object queue, writing each non-graphical object.
    fn write_objects(&mut self) {
        // Phase 1: drain the queue (root dict entries + xdict handles)
        while let Some(handle) = self.object_queue.pop_front() {
            if self.visited_objects.contains(&handle) {
                continue;
            }
            if let Some(obj) = self.document.objects.get(&handle) {
                self.visited_objects.insert(handle);
                let obj = obj.clone();
                self.write_object(&obj);
            }
        }

        // Phase 2: write any remaining objects not yet visited.
        // Extension dictionaries on table entries (layers, block records,
        // etc.) may not be reachable from the root dictionary chain because
        // the table entry structs don't store xdictionary handles.  This
        // loop catches all orphaned dictionaries, XRecords, etc.
        //
        // First, seed visited_objects with ALL handles already written
        // (table controls, table entries, block entities, Phase 1 objects)
        // to prevent Phase 2 from creating duplicate handle→offset entries
        // that would corrupt the Object Map.
        for &(handle_val, _) in &self.handle_map {
            self.visited_objects.insert(Handle::from(handle_val));
        }

        let mut remaining: Vec<(Handle, crate::objects::ObjectType)> = self
            .document
            .objects
            .iter()
            .filter(|(h, _)| !self.visited_objects.contains(h))
            .map(|(h, o)| (*h, o.clone()))
            .collect();
        // `objects` is a HashMap: iterate in handle order so the same
        // document writes the same bytes on every call.
        remaining.sort_by_key(|(h, _)| h.value());

        for (handle, obj) in remaining {
            self.visited_objects.insert(handle);
            self.write_object(&obj);
            // Drain any newly enqueued objects (children of orphan dicts)
            while let Some(child) = self.object_queue.pop_front() {
                if self.visited_objects.contains(&child) {
                    continue;
                }
                if let Some(child_obj) = self.document.objects.get(&child) {
                    self.visited_objects.insert(child);
                    let child_obj = child_obj.clone();
                    self.write_object(&child_obj);
                }
            }
        }
    }

    // ── Access helpers ──────────────────────────────────────────────

    /// Get the output bytes.
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Get the handle map.
    pub fn handle_map(&self) -> &[(u64, u32)] {
        &self.handle_map
    }
}

// ── Tests ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_writer_creates_for_default_document() {
        let doc = CadDocument::new();
        let writer = DwgObjectWriter::new(&doc);
        assert!(writer.is_ok());
    }

    #[test]
    fn object_writer_writes_basic_document() {
        let doc = CadDocument::new();
        let writer = DwgObjectWriter::new(&doc).unwrap();
        let (output, handle_map, _, _) = writer.write();
        // Should have produced some output (at least the 0x0DCA marker)
        assert!(!output.is_empty());
        // Should have recorded some handles (table controls + entries)
        assert!(!handle_map.is_empty());
    }

    #[test]
    fn object_writer_encodes_dca_marker() {
        let doc = CadDocument::new();
        let writer = DwgObjectWriter::new(&doc).unwrap();
        let (output, _, _, _) = writer.write();
        // First 4 bytes should be 0x0DCA as little-endian i32
        if output.len() >= 4 {
            let marker = i32::from_le_bytes([output[0], output[1], output[2], output[3]]);
            assert_eq!(marker, 0x0DCA);
        }
    }
}
