//! Common object-writing helpers
//!
//! These methods form the shared infrastructure for writing every DWG
//! object record: CRC wrapping, size encoding, entity/non-entity
//! preambles, extended-data emission, and handle-map registration.

use crate::io::dwg::crc;
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::types::{Color, Handle, Transparency};

use super::DwgObjectWriter;

// ── DWG fixed object-type codes ─────────────────────────────────────
/// Standard DWG object type codes for entities and table entries.
pub const OBJ_TEXT: i16 = 1;
pub const OBJ_ATTRIB: i16 = 2;
pub const OBJ_ATTDEF: i16 = 3;
pub const OBJ_BLOCK: i16 = 4;
pub const OBJ_ENDBLK: i16 = 5;
pub const OBJ_SEQEND: i16 = 6;
pub const OBJ_INSERT: i16 = 7;
pub const OBJ_MINSERT: i16 = 8;
pub const OBJ_VERTEX_2D: i16 = 10;
pub const OBJ_VERTEX_3D: i16 = 11;
pub const OBJ_VERTEX_MESH: i16 = 12;
pub const OBJ_VERTEX_PFACE: i16 = 13;
pub const OBJ_VERTEX_PFACE_FACE: i16 = 14;
pub const OBJ_POLYLINE_2D: i16 = 15;
pub const OBJ_POLYLINE_3D: i16 = 16;
pub const OBJ_ARC: i16 = 17;
pub const OBJ_CIRCLE: i16 = 18;
pub const OBJ_LINE: i16 = 19;
pub const OBJ_DIMENSION_ORDINATE: i16 = 20;
pub const OBJ_DIMENSION_LINEAR: i16 = 21;
pub const OBJ_DIMENSION_ALIGNED: i16 = 22;
pub const OBJ_DIMENSION_ANG_3PT: i16 = 23;
pub const OBJ_DIMENSION_ANG_2LN: i16 = 24;
pub const OBJ_DIMENSION_RADIUS: i16 = 25;
pub const OBJ_DIMENSION_DIAMETER: i16 = 26;
pub const OBJ_POINT: i16 = 27;
pub const OBJ_3DFACE: i16 = 28;
pub const OBJ_POLYLINE_PFACE: i16 = 29;
pub const OBJ_POLYLINE_MESH: i16 = 30;
pub const OBJ_SOLID: i16 = 31;
pub const OBJ_TRACE: i16 = 32;
pub const OBJ_TABLECONTENT: i16 = 105;
pub const OBJ_SHAPE: i16 = 33;
pub const OBJ_VIEWPORT: i16 = 34;
pub const OBJ_ELLIPSE: i16 = 35;
pub const OBJ_SPLINE: i16 = 36;
pub const OBJ_REGION: i16 = 37;
pub const OBJ_3DSOLID: i16 = 38;
pub const OBJ_BODY: i16 = 39;
pub const OBJ_RAY: i16 = 40;
pub const OBJ_XLINE: i16 = 41;
pub const OBJ_DICTIONARY: i16 = 42;
pub const OBJ_OLEFRAME: i16 = 43;
pub const OBJ_MTEXT: i16 = 44;
pub const OBJ_LEADER: i16 = 45;
pub const OBJ_TOLERANCE: i16 = 46;
pub const OBJ_MLINE: i16 = 47;
pub const OBJ_BLOCK_CONTROL: i16 = 48;
pub const OBJ_BLOCK_HEADER: i16 = 49;
pub const OBJ_LAYER_CONTROL: i16 = 50;
pub const OBJ_LAYER: i16 = 51;
pub const OBJ_STYLE_CONTROL: i16 = 52;
pub const OBJ_STYLE: i16 = 53;
pub const OBJ_LTYPE_CONTROL: i16 = 56;
pub const OBJ_LTYPE: i16 = 57;
pub const OBJ_VIEW_CONTROL: i16 = 60;
pub const OBJ_VIEW: i16 = 61;
pub const OBJ_UCS_CONTROL: i16 = 62;
pub const OBJ_UCS: i16 = 63;
pub const OBJ_VPORT_CONTROL: i16 = 64;
pub const OBJ_VPORT: i16 = 65;
pub const OBJ_APPID_CONTROL: i16 = 66;
pub const OBJ_APPID: i16 = 67;
pub const OBJ_DIMSTYLE_CONTROL: i16 = 68;
pub const OBJ_DIMSTYLE: i16 = 69;
pub const OBJ_VPENT_HDR_CONTROL: i16 = 70;
pub const OBJ_VPENT_HDR: i16 = 71;
pub const OBJ_GROUP: i16 = 72;
pub const OBJ_MLINESTYLE: i16 = 73;
pub const OBJ_OLE2FRAME: i16 = 74;
pub const OBJ_DUMMY: i16 = 75;
pub const OBJ_LONG_TRANSACTION: i16 = 76;

// Standard table-type fixed codes (77+)
pub const OBJ_LWPOLYLINE: i16 = 77; // standard fixed type in R14+
pub const OBJ_HATCH: i16 = 78; // standard fixed type

// Class-based (UNLISTED) entity types.
// These must ALWAYS be resolved via class_type_code(), never used directly.
// The values below are just fallbacks — never valid as fixed type codes.
pub const OBJ_IMAGE: i16 = -1; // UNLISTED: always use class number
pub const OBJ_MESH: i16 = -2; // UNLISTED: always use class number
pub const OBJ_MULTILEADER: i16 = -3; // UNLISTED: always use class number
pub const OBJ_SURFACE: i16 = -5; // UNLISTED: ACAD_SURFACE family
pub const OBJ_PDFUNDERLAY: i16 = -12; // UNLISTED: always use class number
pub const OBJ_DWFUNDERLAY: i16 = -13; // UNLISTED: always use class number
pub const OBJ_DGNUNDERLAY: i16 = -14; // UNLISTED: always use class number
pub const OBJ_HELIX: i16 = -15; // UNLISTED: always use class number
pub const OBJ_TABLE: i16 = -16; // UNLISTED: always use class number
pub const OBJ_LIGHT: i16 = -17; // UNLISTED: always use class number
pub const OBJ_ARC_DIMENSION: i16 = -18;
pub const OBJ_LARGE_RADIAL_DIMENSION: i16 = -19;
pub const OBJ_CAMERA: i16 = -20;
pub const OBJ_SECTIONOBJECT: i16 = -21;
pub const OBJ_ARCALIGNEDTEXT: i16 = -22;
pub const OBJ_RTEXT: i16 = -23;
pub const OBJ_GEOPOSITIONMARKER: i16 = -24;
pub const OBJ_NAVISWORKSMODEL: i16 = -25;
pub const OBJ_POINTCLOUD: i16 = -26;
pub const OBJ_POINTCLOUDEX: i16 = -27;
pub const OBJ_MPOLYGON: i16 = -28;
pub const OBJ_PROXY_ENTITY: i16 = 498;
pub const OBJ_PROXY_OBJECT: i16 = 499;
pub const OBJ_BLOCKVISIBILITYPARAMETER: i16 = 0x84;

// Fixed-type non-graphical objects (standard type codes from ODA spec)
pub const OBJ_XRECORD: i16 = 79; // 0x4F
pub const OBJ_PLACEHOLDER: i16 = 80; // 0x50
pub const OBJ_LAYOUT: i16 = 82; // 0x52 (R2004+; for R2004Pre use class number)

// Class-based (variable) non-graphical objects:
// These are UNLISTED in C# — for R2004+ the standard type code works,
// but for pre-R2004 they should use the DXF class number.
// We use the ODA-documented type codes that work for R2004+.
pub const OBJ_DICTIONARYWDFLT: i16 = 0x78; // 120 (class-based)
pub const OBJ_DICTIONARYVAR: i16 = 0x79; // 121
pub const OBJ_PLOTSETTINGS: i16 = 0x7A; // 122
pub const OBJ_MLEADERSTYLE: i16 = 0x7B; // 123
pub const OBJ_IMAGEDEF: i16 = 0x7C; // 124
pub const OBJ_IMAGEDEFREACTOR: i16 = 0x7D; // 125
pub const OBJ_SCALE: i16 = 0x7E; // 126
pub const OBJ_SORTENTSTABLE: i16 = 0x7F; // 127
pub const OBJ_RASTERVARIABLES: i16 = 0x80; // 128
pub const OBJ_DBCOLOR: i16 = 0x81; // 129
pub const OBJ_WIPEOUTVARIABLES: i16 = 0x82; // 130
pub const OBJ_GEODATA: i16 = 0x83; // 131 (class-based; sentinel fallback)
pub const OBJ_SPATIALFILTER: i16 = 0x86; // 134 (class-based; sentinel fallback)
pub const OBJ_PDFDEFINITION: i16 = 0x87; // 135 (class-based; sentinel fallback)
pub const OBJ_DWFDEFINITION: i16 = 0x88; // 136 (class-based; sentinel fallback)
pub const OBJ_DGNDEFINITION: i16 = 0x89; // 137 (class-based; sentinel fallback)

// ── Methods on DwgObjectWriter ──────────────────────────────────────
impl<'a> DwgObjectWriter<'a> {
    // ── register_object ─────────────────────────────────────────────
    /// Finalise the current object record in `self.writer` and append it
    /// to the output stream, recording the handle→offset mapping.
    ///
    /// Record structure (per ODA spec §20.2):
    /// ```text
    /// [ModularShort(size)][R2010+: ModularChar(handle_bits)][merged_data][CRC16]
    /// ```
    ///
    /// **CRC-16** (seed `0xC0C1`) covers everything from the ModularShort
    /// through the end of `merged_data`.  AutoCAD validates this checksum
    /// on every object load ("Level 3" check).
    ///
    /// **Byte alignment**: The merged data from `merge()` is guaranteed
    /// byte-aligned (all three sub-streams are spear-shifted and the
    /// final output is flushed).  This is critical because the CRC-16
    /// operates on complete bytes.
    pub fn register_object(&mut self, handle: Handle) {
        // Duplicate-handle guard: a handle must appear in the object map exactly
        // once. If this one was already emitted (e.g. an xdictionary XRECORD
        // reached from two paths), drop this repeat — duplicate handles are a
        // hard integrity error AutoCAD's audit rejects.
        if !handle.is_null() && !self.registered_handles.insert(handle.value()) {
            self.writer.reset();
            self.pending_type_code = None;
            return;
        }

        // 1. Merge all sub-streams → single byte buffer
        //    (handle_start_bits is recorded during merge by the merged writer)
        let data = self.writer.merge();

        // Verify the merged data is byte-aligned.  If this fires, the
        // merge function has a bug — partial bytes would corrupt the
        // CRC-16 and cause "Invalid Input" in AutoCAD.
        debug_assert!(
            !data.is_empty() || handle.is_null(),
            "register_object: merged data is empty for handle {:#X}",
            handle.value()
        );

        // 2. Compute handle-stream bit count for R2010+ MC header field.
        // Must be computed AFTER merge: handle_bits = total_bits - handle_start.
        // This matches C#: sizeb = (msmain.Length << 3) - SavedPositionInBits
        let handle_bits = if self.version.r2010_plus() {
            let total_bits = (data.len() as i64) * 8;
            let hstart = self.writer.handle_start_bits();
            debug_assert!(
                hstart >= 0 && hstart <= total_bits,
                "handle_start_bits ({}) out of range [0, {}]",
                hstart,
                total_bits
            );
            total_bits - hstart
        } else {
            0
        };

        // 3. Build output record
        let pos = self.output.len() as u32;

        // 3a. Size (modular short) — the merged data byte count.
        //     Per the ODA spec, this is the byte count of the data area
        //     (includes object data AND handle reference data).
        write_modular_short_bytes(&mut self.output, data.len());

        // 3b. R2010+: handle stream size in bits (modular char)
        if self.version.r2010_plus() {
            write_modular_char_bytes(&mut self.output, handle_bits as usize);
        }

        // 3c. Merged data (byte-aligned, complete object record)
        self.output.extend_from_slice(&data);

        // 4. CRC-16 over everything from `pos`:
        //    [MS(size)] [MC(handle_bits)] [merged_data]
        //    Seed 0xC0C1, no final XOR.  This is the per-object checksum
        //    that AutoCAD validates ("Level 3" object integrity check).
        let crc_val = crc::crc16(crc::CRC16_SEED, &self.output[pos as usize..]);
        self.output.extend_from_slice(&crc_val.to_le_bytes());

        // 5. Record handle → byte-offset mapping.
        //    These offsets are relative to the start of the AcDbObjects
        //    section data (which includes the 0x0DCA marker for R2004+).
        //    The Handles section (Object Map / Address Table) uses these
        //    offsets for handle → object lookup.
        if !handle.is_null() {
            self.handle_map.push((handle.value(), pos));
        }

        if let Some(type_code) = self.pending_type_code.take().filter(|code| *code >= 500) {
            *self.class_instance_counts.entry(type_code).or_default() += 1;
        }

        // 6. Reset per-object writer for the next object
        self.writer.reset();
    }

    /// Write a raw (pre-encoded) object record verbatim.
    ///
    /// Used for unknown entities that carry the original merged-stream
    /// bytes from the source file.  The raw data is the exact payload
    /// that was between `[ModularShort(size)]` and `[CRC16]` in the
    /// original file.  We re-frame it with a fresh MS prefix and CRC.
    pub fn register_raw_object(&mut self, handle: Handle, raw_data: &[u8], handle_bits: i64) {
        self.class_counts_complete = false;
        // Duplicate-handle guard (see register_object).
        if !handle.is_null() && !self.registered_handles.insert(handle.value()) {
            return;
        }
        let pos = self.output.len() as u32;

        // MS (size)
        write_modular_short_bytes(&mut self.output, raw_data.len());

        // R2010+: MC (handle bits) — reproduce the original value
        if self.version.r2010_plus() {
            write_modular_char_bytes(&mut self.output, handle_bits as usize);
        }

        // Merged data bytes
        self.output.extend_from_slice(raw_data);

        // CRC-16
        let crc_val = crc::crc16(crc::CRC16_SEED, &self.output[pos as usize..]);
        self.output.extend_from_slice(&crc_val.to_le_bytes());

        // Handle → offset mapping
        if !handle.is_null() {
            self.handle_map.push((handle.value(), pos));
        }

        // No need to reset writer — we didn't use it
    }

    // ── write_common_data ───────────────────────────────────────────
    /// Object type + handle + extended-data preamble shared by
    /// every object (entities AND non-graphical objects).
    pub fn write_common_data(
        &mut self,
        type_code: i16,
        handle: Handle,
        xdata: &crate::xdata::ExtendedData,
    ) {
        self.pending_type_code = Some(type_code);
        // Object type (BS or MC depending on version)
        self.writer.write_object_type(type_code);

        // R2000..R2007: save position for deferred size field
        if self.version.r2000_plus() && !self.version.r2010_plus() {
            self.writer.save_position_for_size();
        }

        // Handle (absolute)
        self.writer
            .main_mut()
            .write_handle_undefined(handle.value());

        // Extended data
        self.write_extended_data(xdata);
    }

    // ── write_common_entity_data ────────────────────────────────────
    /// Full preamble for an entity: type code, handle, xdata, graphic
    /// flag, entity mode, reactors/xdic, layer/linetype, colour,
    /// line-weight, prev/next entity chain.
    ///
    /// Field order must match the C# reference exactly because both the
    /// main-stream and handle-stream are order-sensitive.
    pub fn write_common_entity_data(
        &mut self,
        type_code: i16,
        handle: Handle,
        owner_handle: Handle,
        layer: &str,
        color: &Color,
        line_weight: &crate::types::LineWeight,
        transparency: &Transparency,
        invisible: bool,
        linetype_scale: f64,
        linetype: &str,
        linetype_handle: &Option<Handle>,
        xdata: &crate::xdata::ExtendedData,
        reactors: &[Handle],
        xdictionary_handle: &Option<Handle>,
        graphic_data: Option<&[u8]>,
        // DWG round-trip fields (None = use defaults/computed)
        entity_mode_override: Option<u8>,
        material_flags: u8,
        material_handle: &Option<Handle>,
        shadow_flags: u8,
        plotstyle_flags: u8,
        plotstyle_handle: &Option<Handle>,
        color_book_handle: &Option<Handle>,
        full_visual_style_handle: &Option<Handle>,
        face_visual_style_handle: &Option<Handle>,
        edge_visual_style_handle: &Option<Handle>,
    ) {
        // ── MAIN + HANDLE: shared preamble (type + handle + xdata) ──
        self.write_common_data(type_code, handle, xdata);

        // ── MAIN: graphic presence flag + optional graphic bytes ──
        let has_graphic = graphic_data.is_some();
        self.writer.write_bit(has_graphic);
        if let Some(gdata) = graphic_data {
            // R2010+: BLL (Bit Long Long); pre-R2010: RL (Raw Long)
            let gsize = gdata.len() as i64;
            if self.version.r2010_plus() {
                self.writer.write_bit_long_long(gsize);
            } else {
                self.writer.write_raw_long(gsize as i32);
            }
            for &b in gdata {
                self.writer.write_byte(b);
            }
        }

        // ── MAIN: R13-R14 save position for size ──
        if self.version.r13_14_only() {
            self.writer.save_position_for_size();
        }

        // ── MAIN: entity mode (2 bits) ──
        let entmode = entity_mode_override.unwrap_or_else(|| self.get_entity_mode(&owner_handle));
        self.writer.write_2bits(entmode);

        // ── HANDLE: owner (if entmode == 0) ──
        if entmode == 0 {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, owner_handle.value());
        }

        // ── MAIN + HANDLE: reactors + xdic ──
        // Reactor count (MAIN)
        self.writer.write_bit_long(reactors.len() as i32);
        // Reactor handles (HANDLE)
        for r in reactors {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, r.value());
        }

        // Filter entity xdic handle: only keep it if the referenced dictionary
        // is actually WRITTEN. `contains_key` is not enough — a dictionary that
        // is present in the document but dropped on this save (e.g. an AEC/
        // version-locked object) would leave a dangling handle that AutoCAD's
        // reader rejects (the object reads as improperly read / erased).
        let effective_entity_xdic =
            xdictionary_handle.filter(|xdic| !xdic.is_null() && self.is_writable_object(xdic));

        // R2004+: no-xdic flag (MAIN) + conditional xdic handle (HANDLE)
        // Pre-R2004: always write xdic handle (0 if none)
        if self.version.r2004_plus() {
            self.writer.write_bit(effective_entity_xdic.is_none());
            // Xdic handle (HANDLE) — only if present
            if let Some(xdic) = effective_entity_xdic {
                self.writer
                    .write_handle(DwgReferenceType::HardOwnership, xdic.value());
            }
        } else {
            // Pre-R2004: always write xdic handle (0 if none)
            let xdic_val = effective_entity_xdic.map(|h| h.value()).unwrap_or(0);
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, xdic_val);
        }

        // Enqueue extension dictionary so its object is written later
        if let Some(xdic) = effective_entity_xdic {
            if !xdic.is_null() {
                self.object_queue.push_back(xdic);
            }
        }

        // R2013+: the data-store flag is present in every entity header. The
        // modeler writer sets it immediately before this call.
        if self.version.r2013_plus(self.dxf_version) {
            let has_ds = self.pending_has_ds_data;
            self.writer.write_bit(has_ds);
        }
        self.pending_has_ds_data = false;

        // ── R13-R14 only: layer + linetype ──
        if self.version.r13_14_only() {
            // Layer handle (HANDLE: hard pointer)
            let layer_h = self
                .document
                .layers
                .get(layer)
                .map(|l| l.handle)
                .unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, layer_h.value());

            let lt_lower = linetype.to_ascii_lowercase();
            let resolved_linetype = self
                .document
                .line_types
                .get(linetype)
                .map(|lt| lt.handle)
                .or(*linetype_handle)
                .filter(|handle| !handle.is_null());
            let has_preserved_handle = linetype_handle.filter(|handle| !handle.is_null()).is_some();
            let is_bylayer =
                (lt_lower == "bylayer" || lt_lower.is_empty()) && !has_preserved_handle;
            self.writer.write_bit(is_bylayer);
            if !is_bylayer {
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    resolved_linetype.unwrap_or(Handle::NULL).value(),
                );
            }
        }

        // ── R13-R2000 (pre-R2004): Nolinks + prev/next entity chain ──
        // In R13/R14/R2000, entities in a block form a doubly-linked list.
        // NOLINKS bit = 1 means handles are sequential (reader infers
        // prev = handle-1, next = handle+1) and prev/next handles are omitted.
        // NOLINKS bit = 0 means prev/next handles are written explicitly.
        if !self.version.r2004_plus() {
            let prev_h = self.prev_handle.unwrap_or(Handle::NULL);
            let next_h = self.next_handle.unwrap_or(Handle::NULL);
            let has_links = !prev_h.is_null()
                && prev_h.value() == handle.value().wrapping_sub(1)
                && !next_h.is_null()
                && next_h.value() == handle.value().wrapping_add(1);

            // MAIN: Nolinks bit (true = sequential, reader infers prev/next)
            self.writer.write_bit(has_links);

            // HANDLE: prev + next entity handles only when NOT sequential
            if !has_links {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, prev_h.value());
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, next_h.value());
            }
        }

        // ── MAIN: Color (EnColor) ──
        if self.version.r2000_plus() {
            let color_book_handle = if self.version.r2004_plus() {
                color_book_handle
                    .filter(|handle| !handle.is_null() && self.is_writable_object(handle))
            } else {
                None
            };
            self.writer
                .write_en_color_with_book(color, transparency, color_book_handle.is_some());
            if let Some(handle) = color_book_handle {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, handle.value());
                self.object_queue.push_back(handle);
            }
        } else {
            // R13-R14: colour as CMC
            self.writer.write_cm_color(color);
        }

        // ── MAIN: Linetype scale ──
        self.writer.write_bit_double(linetype_scale);

        // ── R13-R14 only: invisibility + early return ──
        // DXF group 60 convention (all DWG versions): 0 = visible, non-zero = invisible
        if self.version.r13_14_only() {
            self.writer.write_bit_short(if invisible { 1 } else { 0 });
            return;
        }

        // ── R2000+: Layer handle (HANDLE: hard pointer) ──
        let layer_h = self
            .document
            .layers
            .get(layer)
            .map(|l| l.handle)
            .unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, layer_h.value());

        // ── MAIN: Linetype flags ──
        // 00 = bylayer, 01 = byblock, 10 = continuous, 11 = handle present
        let lt_lower = linetype.to_ascii_lowercase();
        let resolved_linetype = self
            .document
            .line_types
            .get(linetype)
            .map(|lt| lt.handle)
            .or(*linetype_handle)
            .filter(|handle| !handle.is_null());
        let has_preserved_handle = linetype_handle.filter(|handle| !handle.is_null()).is_some();
        let is_builtin = lt_lower == "bylayer"
            || lt_lower.is_empty()
            || lt_lower == "byblock"
            || lt_lower == "continuous";
        if resolved_linetype.is_some() && (has_preserved_handle || !is_builtin) {
            self.writer.write_2bits(0b11);
            self.writer.write_handle(
                DwgReferenceType::HardPointer,
                resolved_linetype.unwrap().value(),
            );
        } else if lt_lower == "bylayer" || lt_lower.is_empty() {
            self.writer.write_2bits(0b00);
        } else if lt_lower == "byblock" {
            self.writer.write_2bits(0b01);
        } else if lt_lower == "continuous" {
            self.writer.write_2bits(0b10);
        } else {
            // Named linetype — write handle
            self.writer.write_2bits(0b11);
            let lt_handle = resolved_linetype.unwrap_or(Handle::NULL);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, lt_handle.value());
        }

        // ── R2007+: material flags + shadow flags ──
        if self.version.r2007_plus() {
            let material_handle = material_handle.filter(|handle| self.is_writable_object(handle));
            let material_flags = if material_flags == 0b11 && material_handle.is_none() {
                0b00
            } else {
                material_flags
            };
            // Material flags BB
            self.writer.write_2bits(material_flags);
            if material_flags == 0b11 {
                if let Some(mh) = material_handle {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, mh.value());
                } else {
                    self.writer.write_handle(DwgReferenceType::HardPointer, 0);
                }
            }
            // Shadow flags RC
            self.writer.write_byte(shadow_flags);
        }

        // ── R2000+: Plotstyle flags ──
        self.writer.write_2bits(plotstyle_flags);
        if plotstyle_flags == 0b11 {
            if let Some(ph) = plotstyle_handle {
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, ph.value());
            } else {
                self.writer.write_handle(DwgReferenceType::HardPointer, 0);
            }
        }

        // ── R2010+: visual-style override handles ──
        if self.version.r2010_plus() {
            let full = full_visual_style_handle
                .filter(|handle| !handle.is_null() && self.is_writable_object(handle));
            let face = face_visual_style_handle
                .filter(|handle| !handle.is_null() && self.is_writable_object(handle));
            let edge = edge_visual_style_handle
                .filter(|handle| !handle.is_null() && self.is_writable_object(handle));
            for handle in [full, face, edge] {
                self.writer.write_bit(handle.is_some());
                if let Some(handle) = handle {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, handle.value());
                    self.object_queue.push_back(handle);
                }
            }
        }

        // ── MAIN: Invisibility ──
        self.writer.write_bit_short(if invisible { 1 } else { 0 });

        // ── R2000+: Lineweight (5-bit DWG index) ──
        self.writer.write_byte(line_weight.to_dwg_index());
    }

    // ── write_common_non_entity_data ────────────────────────────────
    /// Preamble for non-entity objects: type code, handle,
    /// owner handle, reactors, xdictionary.
    pub fn write_common_non_entity_data(
        &mut self,
        type_code: i16,
        handle: Handle,
        owner_handle: Handle,
        reactors: &[Handle],
        xdictionary_handle: &Option<Handle>,
    ) {
        self.write_common_non_entity_data_eed(
            type_code,
            handle,
            owner_handle,
            reactors,
            xdictionary_handle,
            None,
        );
    }

    /// Build the `AcadAnnotative` EED block `(app_handle, bytes)` for a style
    /// record, or `None` when it isn't annotative (or the `AcadAnnotative`
    /// appid is missing). Encodes `AnnotativeData { 1 <flag> }` per the ODA EED
    /// spec, using the version's string width.
    pub fn annotative_eed_block(&self, annotative: bool) -> Option<(u64, Vec<u8>)> {
        if !annotative {
            return None;
        }
        let app = self.document.app_ids.get("AcadAnnotative")?;
        let bytes = crate::io::dwg::annotative_eed::encode(self.version.r2007_plus(), annotative);
        Some((app.handle.value(), bytes))
    }

    /// As [`write_common_non_entity_data`], but merges one extra EED block
    /// (e.g. the annotative marker) with any raw EED preserved by handle.
    /// An existing block for the same application is replaced, so the marker
    /// is written exactly once.
    pub fn write_common_non_entity_data_eed(
        &mut self,
        type_code: i16,
        handle: Handle,
        owner_handle: Handle,
        reactors: &[Handle],
        xdictionary_handle: &Option<Handle>,
        extra_eed: Option<(u64, Vec<u8>)>,
    ) {
        self.write_common_non_entity_data_eed_internal(
            type_code,
            handle,
            owner_handle,
            reactors,
            xdictionary_handle,
            extra_eed,
            false,
        );
    }

    pub(super) fn write_common_non_entity_data_relative_owner(
        &mut self,
        type_code: i16,
        handle: Handle,
        owner_handle: Handle,
        reactors: &[Handle],
        xdictionary_handle: &Option<Handle>,
    ) {
        self.write_common_non_entity_data_eed_internal(
            type_code,
            handle,
            owner_handle,
            reactors,
            xdictionary_handle,
            None,
            true,
        );
    }

    fn write_common_non_entity_data_eed_internal(
        &mut self,
        type_code: i16,
        handle: Handle,
        owner_handle: Handle,
        reactors: &[Handle],
        xdictionary_handle: &Option<Handle>,
        extra_eed: Option<(u64, Vec<u8>)>,
        relative_owner: bool,
    ) {
        // ── writeCommonData portion ──

        self.pending_type_code = Some(type_code);
        // Object type
        self.writer.write_object_type(type_code);

        // R2000..R2007: size placeholder (AC1015..AC1021)
        // C#: this._version >= AC1015 && this._version < AC1024
        // R2010 (AC1024) does NOT get a size placeholder.
        if self.version.r2000_plus() && !self.version.r2010_plus() {
            self.writer.save_position_for_size();
        }

        // Handle (absolute)
        self.writer
            .main_mut()
            .write_handle_undefined(handle.value());

        // Extended data — look up raw EED by handle for round-trip fidelity
        let mut eed = crate::xdata::ExtendedData::default();
        if let Some(raw) = self.document.eed_by_handle.get(&handle) {
            eed.raw_dwg_eed = raw.clone();
        }
        if let Some((app, bytes)) = extra_eed {
            eed.raw_dwg_eed.retain(|(a, _)| *a != app);
            eed.raw_dwg_eed.push((app, bytes));
        }
        self.write_extended_data(&eed);

        // ── R13-R14 Only: size placeholder (after xdata, before owner) ──
        if self.version.r13_14_only() {
            self.writer.save_position_for_size();
        }

        // ── HANDLE: Owner handle (soft pointer) ──
        // Check for owner override (e.g. for ATTRIB extension dictionaries
        // whose parent entity was re-allocated with a new handle)
        let effective_owner = self
            .owner_overrides
            .get(&handle)
            .copied()
            .unwrap_or(owner_handle);
        if relative_owner && !effective_owner.is_null() {
            self.writer
                .write_handle_relative(handle.value(), effective_owner.value());
        } else {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, effective_owner.value());
        }

        // ── writeReactorsAndDictionaryHandle portion ──

        // Use side-channel reactors if struct reactors are empty (roundtrip preservation)
        let effective_reactors: Vec<Handle>;
        let reactors_to_write: &[Handle] = if reactors.is_empty() {
            if let Some(saved) = self.document.reactors_by_handle.get(&handle) {
                effective_reactors = saved.clone();
                &effective_reactors
            } else {
                reactors
            }
        } else {
            reactors
        };

        // MAIN: Reactor count
        self.writer.write_bit_long(reactors_to_write.len() as i32);

        // HANDLE: Reactor handles
        for r in reactors_to_write {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, r.value());
        }

        // Use side-channel xdictionary if struct value is None (roundtrip preservation)
        // Only use it if the xdictionary object actually exists in document.objects,
        // otherwise BricsCAD reports "Object was erased" for the dangling reference.
        let effective_xdic = if xdictionary_handle.is_none() {
            self.document
                .extension_dictionary_handle(handle)
                .filter(|xdic| self.document.objects.contains_key(xdic))
        } else {
            *xdictionary_handle
        };

        // R2004+: MAIN no-xdic flag + conditional HANDLE xdic
        // Pre-R2004: HANDLE xdic always written (0 if null)
        let no_xdic = effective_xdic.is_none();
        if self.version.r2004_plus() {
            self.writer.write_bit(no_xdic);
            if !no_xdic {
                self.writer.write_handle(
                    DwgReferenceType::HardOwnership,
                    effective_xdic.unwrap().value(),
                );
            }
        } else {
            // Pre-R2004: always emit xdic handle (0 if None)
            let xdic_val = effective_xdic.map(|h| h.value()).unwrap_or(0);
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, xdic_val);
        }

        // Enqueue extension dictionary so its object is written later
        if let Some(xdic) = effective_xdic {
            if !xdic.is_null() {
                self.object_queue.push_back(xdic);
            }
        }

        // R2013+: retain data-store links while reusing the source section.
        if self.version.r2013_plus(self.dxf_version) {
            let has_ds_data = self.document.dwg_source_version == Some(self.dxf_version)
                && self.document.dwg_data_store_handles.contains(&handle);
            self.writer.write_bit(has_ds_data);
        }
    }

    // ── write_xref_dependant_bit ────────────────────────────────────
    /// 64-group "xref dependant" flag.
    pub fn write_xref_dependant_bit(&mut self) {
        self.write_xref_dependant_bit_value(false);
    }

    /// 64-group "xref dependant" flag with explicit value.
    pub fn write_xref_dependant_bit_value(&mut self, xref_dep: bool) {
        self.write_xref_table_flags(false, false, xref_dep);
    }

    pub fn write_xref_table_flags(
        &mut self,
        xref_reference: bool,
        xref_resolved: bool,
        xref_dependent: bool,
    ) {
        if self.version.r2007_plus() {
            // R2007+: is_xref_resolved BS — only 0 (not xref-dependent) or
            // 256 (0x100, xref-resolved) are valid; the reader derives
            // is_xref_dep from `== 256`. Writing 0x10 makes AutoCAD reject the
            // record (eDwgObjectImproperlyRead) on xref-dependent table entries.
            let combined: i16 = if xref_resolved || xref_dependent {
                0x100
            } else {
                0
            };
            self.writer.write_bit_short(combined);
        } else {
            // Pre-R2007: 64-flag B (Referenced), xrefindex+1 BS, Xdep B (XrefDependent)
            self.writer.write_bit(xref_reference);
            self.writer
                .write_bit_short(if xref_resolved { 0x100 } else { 0 });
            self.writer.write_bit(xref_dependent);
        }
    }

    // ── write_extended_data ─────────────────────────────────────────
    /// Write registered-application extended data (XDATA) blocks.
    ///
    /// `xdata.raw_dwg_eed` blocks (captured verbatim on a DWG read) are written
    /// back byte-for-byte to preserve round-trip fidelity. Structured
    /// `xdata.records()` — e.g. XDATA a plugin attaches via `write_record` —
    /// are encoded into EED bytes and appended for any application not already
    /// carried verbatim, so they survive a save instead of being dropped.
    /// When neither is present a lone BS 0 terminator is written (no EED).
    pub fn write_extended_data(&mut self, xdata: &crate::xdata::ExtendedData) {
        // EED string entries (code 0) are code-page encoded pre-R2007 and
        // UTF-16 in R2007+, so verbatim `raw_dwg_eed` bytes captured from a
        // different encoding family garble and desync the record. Drop them on
        // an incompatible cross-version save (write no EED) rather than corrupt.
        let eed_compatible = match self.document.dwg_source_version {
            None => true,
            Some(src) => {
                (src >= crate::types::DxfVersion::AC1021)
                    == (self.dxf_version >= crate::types::DxfVersion::AC1021)
            }
        };

        // Verbatim blobs captured on read. Dropped on a cross-version save whose
        // string encoding family differs (they'd garble); freshly-encoded
        // records below are always emitted in the target version's encoding.
        let raw_blocks: &[(u64, Vec<u8>)] = if eed_compatible {
            &xdata.raw_dwg_eed
        } else {
            &[]
        };

        // Encode structured records (e.g. plugin XDATA via write_record) into
        // EED bytes. Only records whose application is registered in the APPID
        // table can be written (the reference below is a handle into it); an
        // unregistered app is skipped rather than emitting a dangling handle.
        // An app already present verbatim in `raw_blocks` wins, so a clean DWG
        // round-trip stays byte-for-byte and we never double-write one app.
        let wide = self.version.r2007_plus();
        let mut record_blocks: Vec<(u64, Vec<u8>)> = Vec::new();
        for rec in xdata.records() {
            let Some(app) = self.document.app_ids.get(&rec.application_name) else {
                continue;
            };
            let app_handle = app.handle.value();
            if raw_blocks.iter().any(|(a, _)| *a == app_handle) {
                continue;
            }
            let code_page =
                crate::io::dxf::code_page::dwg_code_page_index(&self.document.header.code_page);
            let encoding =
                crate::io::dxf::code_page::encoding_from_code_page(&self.document.header.code_page)
                    .unwrap_or(encoding_rs::WINDOWS_1252);
            let bytes = crate::io::dwg::eed_codec::encode_values_with_encoding(
                wide,
                &rec.values,
                encoding,
                code_page,
                |name| {
                    self.document
                        .layers
                        .get(name)
                        .map(|l| l.handle.value())
                        .unwrap_or(0)
                },
            );
            // The block length is framed as a BS (i16); skip a pathologically
            // large record rather than overflow and desync the object stream.
            if bytes.len() > i16::MAX as usize {
                continue;
            }
            record_blocks.push((app_handle, bytes));
        }

        if raw_blocks.is_empty() && record_blocks.is_empty() {
            // No EED: write BS 0 terminator
            self.writer.write_bit_short(0);
            return;
        }
        for (app_handle, raw_bytes) in raw_blocks.iter().chain(record_blocks.iter()) {
            // BS size of this application's data block
            self.writer.write_bit_short(raw_bytes.len() as i16);
            // Application handle — always in main stream (HardPointer reference type)
            self.writer.main_mut().write_handle(
                crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer,
                *app_handle,
            );
            // Raw xdata bytes — to main stream byte-for-byte
            for &b in raw_bytes {
                self.writer.write_byte(b);
            }
        }
        // Final BS 0 terminator
        self.writer.write_bit_short(0);
    }

    // ── sub-entity handle allocator ────────────────────────────────
    /// Allocate a new unique handle for sub-entities (vertices, seqend)
    /// that don't have handles assigned by the document.
    pub fn alloc_handle(&mut self) -> Handle {
        let h = self.next_alloc_handle;
        self.next_alloc_handle += 1;
        Handle::new(h)
    }

    // ── class_type_code ─────────────────────────────────────────────
    /// Look up the DXF class number for an UNLISTED object type.
    ///
    /// In the reference implementation, types not in the `ObjectType` enum (UNLISTED)
    /// **always** use their DXF class number (500+) as the DWG type
    /// code — regardless of version.  Only fixed types (0–82 in the
    /// ODA spec) use literal type codes.
    pub fn class_type_code(&self, dxf_name: &str, fallback: i16) -> i16 {
        self.document
            .classes
            .get_by_name(dxf_name)
            .map(|c| c.class_number)
            .unwrap_or(fallback)
    }

    // ── entity-mode helper ──────────────────────────────────────────
    /// Returns the 2-bit entity-mode value (per ODA spec §19.4.4):
    /// - 0 = owned (owner handle present) — VERTEX, ATTRIB, SEQEND,
    ///       or entity inside a named block
    /// - 1 = paper-space entity (BB 01 → any *Paper_Space block)
    /// - 2 = model-space entity (BB 10 → *Model_Space)
    ///
    /// AutoCAD uses entmode=1 for entities in ALL paper-space blocks,
    /// not just the canonical `*Paper_Space`.  Additional layouts like
    /// `*Paper_Space0`, `*Paper_Space1`, … also get entmode=1.  The
    /// correct owner is determined by the BLOCK_HEADER's entity_handles
    /// list, not by the entity's owner handle (which is omitted for
    /// entmode != 0).
    fn get_entity_mode(&self, owner_handle: &Handle) -> u8 {
        for br in self.document.block_records.iter() {
            if br.handle == *owner_handle {
                let upper = br.name.to_ascii_uppercase();
                if upper == "*MODEL_SPACE" {
                    return 2; // model space (BB 10)
                }
                if upper == "*PAPER_SPACE"
                    || (upper.starts_with("*PAPER_SPACE")
                        && upper.len() > 12
                        && upper[12..].bytes().all(|b| b.is_ascii_digit()))
                {
                    return 1; // paper space (BB 01)
                }
                return 0; // named block → owned
            }
        }
        0
    }
}

// ── Helper: write modular-short to a byte vec ───────────────────────
/// Encode `value` as a DWG modular short (MS) and append to `output`.
/// Each 16-bit word carries 15 data bits; bit 15 = continuation flag.
pub(crate) fn write_modular_short_bytes(output: &mut Vec<u8>, value: usize) {
    let mut remaining = value;
    loop {
        let word = (remaining & 0x7FFF) as u16;
        remaining >>= 15;
        if remaining > 0 {
            output.extend_from_slice(&(word | 0x8000).to_le_bytes());
        } else {
            output.extend_from_slice(&word.to_le_bytes());
            break;
        }
    }
}

/// Encode `value` as a DWG modular char (MC) and append to `output`.
/// Each byte carries 7 data bits; bit 7 = continuation flag.
/// This is used for R2010+ handle-stream bit count in the record header.
pub(crate) fn write_modular_char_bytes(output: &mut Vec<u8>, value: usize) {
    if value == 0 {
        output.push(0);
        return;
    }
    let mut remaining = value;
    while remaining > 0 {
        let b = (remaining & 0x7F) as u8;
        remaining >>= 7;
        if remaining > 0 {
            output.push(b | 0x80);
        } else {
            output.push(b);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modular_short_small() {
        let mut buf = Vec::new();
        write_modular_short_bytes(&mut buf, 100);
        assert_eq!(buf.len(), 2);
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 100);
    }

    #[test]
    fn modular_short_large() {
        let mut buf = Vec::new();
        // 0x8000 requires two words
        write_modular_short_bytes(&mut buf, 0x8000);
        assert_eq!(buf.len(), 4);
        // First word has continuation flag
        let w0 = u16::from_le_bytes([buf[0], buf[1]]);
        assert_ne!(w0 & 0x8000, 0);
        // Second word has no flag
        let w1 = u16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(w1 & 0x8000, 0);
        // Reconstruct
        let lo = (w0 & 0x7FFF) as usize;
        let hi = (w1 & 0x7FFF) as usize;
        assert_eq!(lo | (hi << 15), 0x8000);
    }

    #[test]
    fn modular_short_zero() {
        let mut buf = Vec::new();
        write_modular_short_bytes(&mut buf, 0);
        assert_eq!(buf.len(), 2);
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 0);
    }

    #[test]
    fn modular_short_max_single_word() {
        let mut buf = Vec::new();
        write_modular_short_bytes(&mut buf, 0x7FFF);
        assert_eq!(buf.len(), 2);
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 0x7FFF);
    }
}
