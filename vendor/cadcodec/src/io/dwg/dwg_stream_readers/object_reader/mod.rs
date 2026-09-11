//! DWG Object Reader — Core Framework
//!
//! Reads the AcDb:AcDbObjects section, parsing each object record
//! from the handle map. Each record has the framing:
//!
//! ```text
//! [ModularShort(size)][R2010+: ModularChar(handle_bits)][merged_data][CRC16]
//! ```
//!
//! The reader dispatches by type code to specific entity/table/object
//! readers (implemented in sibling modules in later phases).
//!
//! Based on the reference `DwgObjectReader.cs`.

pub mod associative;
pub mod class_object;
pub mod common;
pub mod data_objects;
pub mod dgn_linestyle;
pub mod dynamic_block;
pub mod entities;
pub mod field;
pub mod objects;
pub mod tables;

use crate::error::{DxfError, Result};
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::{Color, DxfVersion, Transparency};
use std::collections::HashMap;
use std::sync::Arc;

/// Maximum number of items in any array read from the stream.
/// Prevents infinite loops when reading corrupt data.
const MAX_ARRAY_COUNT: i32 = 100_000;
const MAX_MESH_FACES: usize = 250_000;
const MAX_MESH_FACE_INDICES: usize = 1_000_000;

/// Cap a loop count read from the stream to a safe maximum.
#[inline]
fn safe_count(raw: i32) -> i32 {
    raw.max(0).min(MAX_ARRAY_COUNT)
}

/// Result of reading a single object record's common data.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ObjectCommonData {
    /// DWG type code (fixed 0–82 or class number 500+)
    pub type_code: i16,
    /// Object handle
    pub handle: u64,
    /// Raw EED blobs: (app_handle_value, raw_bytes_for_that_app)
    #[cfg_attr(feature = "serde", serde(skip))]
    pub eed_raw: Vec<(u64, Vec<u8>)>,
}

/// Common entity data read from the entity preamble.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityCommonData {
    /// Base common data (type + handle + xdata)
    pub common: ObjectCommonData,
    /// Has graphic data flag
    pub has_graphic: bool,
    /// Raw graphic data bytes (stored for DWG round-trip; present only when has_graphic was true).
    /// Serde-skipped — not serialized.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub graphic_data: Option<Vec<u8>>,
    /// Entity mode (0=owned, 1=paper, 2=model, 3=unused)
    pub entity_mode: u8,
    /// Owner handle (only if entity_mode == 0)
    pub owner_handle: u64,
    /// Reactor handles
    pub reactors: Vec<u64>,
    /// XDictionary handle (if present)
    pub xdictionary_handle: Option<u64>,
    /// Color
    pub color: Color,
    /// Transparency
    pub transparency: Transparency,
    /// Line weight (raw DWG index)
    pub line_weight: u8,
    /// Linetype scale
    pub linetype_scale: f64,
    /// Invisibility flag
    pub invisible: bool,
    /// Layer handle
    pub layer_handle: u64,
    /// Linetype flags (00=bylayer, 01=byblock, 10=continuous, 11=handle)
    pub linetype_flags: u8,
    /// Linetype handle (only valid if linetype_flags == 0b11)
    pub linetype_handle: u64,
    /// AcDbColor object handle for a color-book color — R2004+
    pub color_book_handle: Option<u64>,
    /// Previous entity handle (pre-R2004)
    pub prev_entity_handle: Option<u64>,
    /// Next entity handle (pre-R2004)
    pub next_entity_handle: Option<u64>,
    /// Material flags (BB: 00=bylayer, 01=byblock, 10=reserved, 11=handle) — R2007+
    pub material_flags: u8,
    /// Material handle (if material_flags == 0b11)
    pub material_handle: Option<u64>,
    /// Shadow flags (RC) — R2007+
    pub shadow_flags: u8,
    /// Plotstyle flags (BB: 00=bylayer, 01=byblock, 10=reserved, 11=handle)
    pub plotstyle_flags: u8,
    /// Plotstyle handle (if plotstyle_flags == 0b11)
    pub plotstyle_handle: Option<u64>,
    /// Full visual-style override handle — R2010+
    pub full_visual_style_handle: Option<u64>,
    /// Face visual-style override handle — R2010+
    pub face_visual_style_handle: Option<u64>,
    /// Edge visual-style override handle — R2010+
    pub edge_visual_style_handle: Option<u64>,
    /// R2013+ `has_ds_data` bit: the entity's geometry lives in the AcDs
    /// (Autodesk Data Store) section. For a 3DSOLID/REGION/BODY/SURFACE this
    /// signals that a SAB blob in `AcDb:AcDsPrototype_1b` belongs to it; the
    /// blobs pair with these entities in object-stream (file-offset) order.
    pub has_ds_data: bool,
}

/// Common non-entity object data.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NonEntityCommonData {
    /// Base common data (type + handle + xdata)
    pub common: ObjectCommonData,
    /// Owner handle
    pub owner_handle: u64,
    /// Reactor handles
    pub reactors: Vec<u64>,
    /// XDictionary handle (if present)
    pub xdictionary_handle: Option<u64>,
    pub has_ds_data: bool,
}

/// DWG Object Reader — iterates the object section by handle map.
pub struct DwgObjectReader {
    /// Raw object section data
    data: Vec<u8>,
    /// DWG version
    version: DwgVersion,
    /// DXF version
    dxf_version: DxfVersion,
    /// Handle → byte-offset map (from handle section)
    handle_map: HashMap<u64, i64>,
    /// Document code page used by pre-R2007 object strings.
    encoding: &'static encoding_rs::Encoding,
}

impl DwgObjectReader {
    /// Create a new object reader.
    ///
    /// # Arguments
    /// * `data` — Raw AcDb:AcDbObjects section data
    /// * `dxf_version` — DXF version for version-specific parsing
    /// * `handle_map` — Handle → file offset map from the handle section
    pub fn new(
        data: Vec<u8>,
        dxf_version: DxfVersion,
        handle_map: HashMap<u64, i64>,
    ) -> Result<Self> {
        Self::with_encoding(data, dxf_version, handle_map, encoding_rs::WINDOWS_1252)
    }

    pub fn with_encoding(
        data: Vec<u8>,
        dxf_version: DxfVersion,
        handle_map: HashMap<u64, i64>,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<Self> {
        let version = DwgVersion::from_dxf_version(dxf_version)?;
        Ok(DwgObjectReader {
            data,
            version,
            dxf_version,
            handle_map,
            encoding,
        })
    }

    /// Read only the type prefix of an object record.
    /// Pass-1 cataloguing needs no payload, text, or handle cursor; avoiding a
    /// full record allocation here removes one complete copy of every entity.
    pub fn type_code_at(&self, offset: usize) -> Result<i16> {
        if offset >= self.data.len() {
            return Err(DxfError::Parse(format!(
                "Object offset {} out of range (data len {})",
                offset,
                self.data.len()
            )));
        }
        let (size, ms_len) = read_modular_short(&self.data[offset..]);
        let mut pos = offset + ms_len;
        if self.version.r2010_plus() {
            let (_, mc_len) = read_modular_char(&self.data[pos..]);
            pos += mc_len;
        }
        if size == 0 || pos >= self.data.len() {
            return Err(DxfError::Parse(format!(
                "Object record at {} has no type prefix",
                offset
            )));
        }
        let prefix_len = size.min(32).min(self.data.len() - pos);
        Ok(PrefixBitReader::new(&self.data[pos..pos + prefix_len])
            .read_object_type(self.dxf_version))
    }

    /// Read a single object record at the given byte offset.
    ///
    /// Returns the type code and a `DwgMergedReader` positioned at the
    /// start of the object's merged data (after framing).
    ///
    /// Record framing:
    /// ```text
    /// [MS(size)][R2010+: MC(handle_bits)][merged_data][CRC16]
    /// ```
    pub fn read_record_at(&self, offset: usize) -> Result<(i16, DwgMergedReader)> {
        if offset >= self.data.len() {
            return Err(DxfError::Parse(format!(
                "Object offset {} out of range (data len {})",
                offset,
                self.data.len()
            )));
        }

        let mut pos = offset;

        // 1. Read ModularShort (MS) — object size in bytes
        let (size, ms_len) = read_modular_short(&self.data[pos..]);
        pos += ms_len;

        // 2. R2010+: Read ModularChar (MC) — handle stream bit count
        let handle_bits = if self.version.r2010_plus() {
            let (hb, mc_len) = read_modular_char(&self.data[pos..]);
            pos += mc_len;
            hb as i64
        } else {
            0
        };

        // 3. Extract merged data
        if pos + size > self.data.len() {
            return Err(DxfError::Parse(format!(
                "Object record extends past data: offset={}, size={}, data_len={}",
                offset,
                size,
                self.data.len()
            )));
        }
        let merged_data: Arc<[u8]> = Arc::from(&self.data[pos..pos + size]);

        // 4. For R2007+ (ThreeStream): read type_code from a temp reader,
        //    then manually construct the three sub-readers with correct
        //    stream positions.
        //
        //    R2007 (AC1021): record contains an RL (total_size_bits) field
        //      after the type code.  Stream positions are derived from RL.
        //
        //    R2010+ (AC1024+): NO embedded RL field.  Stream positions are
        //      computed from handle_bits (MC in the record framing).
        //      handle_start = total_data_bits - handle_bits
        //      flag_position = handle_start - 1
        if self.version.r2007_plus() {
            let dwg = DwgVersion::from_dxf_version(self.dxf_version).unwrap_or(DwgVersion::AC15);

            // Read type_code from temp reader
            let mut temp = crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                Arc::clone(&merged_data),
                dwg,
                self.dxf_version,
                self.encoding,
            );
            let type_code = temp.read_object_type();

            // Compute flag_position and handle_start
            let (flag_position, handle_start, data_start_bits);

            if !self.version.r2010_plus() {
                // R2007: RL is embedded after type_code
                let total_size_bits = temp.read_raw_long() as i64;
                data_start_bits = temp.position_in_bits();

                // Per-object RL convention (matches the reference implementation):
                // RL = absolute bit position of handle stream start (from bit 0).
                // Flag bit is at RL - 1.  Handles start at bit RL (NOT byte-aligned).
                flag_position = total_size_bits - 1;
                handle_start = total_size_bits;
            } else {
                // R2010+: No RL field.  Compute from handle_bits (MC).
                // handle_bits = total_data_bits - handle_start_bits (writer formula)
                // So: handle_start = total_data_bits - handle_bits
                data_start_bits = temp.position_in_bits();
                let total_data_bits = (size as i64) * 8;
                handle_start = total_data_bits - handle_bits;
                flag_position = handle_start - 1;
            }

            // Main reader: starts at data_start_bits (after type_code [+ RL])
            let mut main_reader = crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                Arc::clone(&merged_data),
                dwg,
                self.dxf_version,
                self.encoding,
            );
            main_reader.set_position_in_bits(data_start_bits);

            // Text reader: positioned by flag at flag_position
            let mut text_reader = crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                Arc::clone(&merged_data),
                dwg,
                self.dxf_version,
                self.encoding,
            );
            let main_data_end = text_reader.set_position_by_flag(flag_position);

            // Handle reader: starts at bit position handle_start (NOT byte-aligned).
            let mut handle_reader =
                crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                    Arc::clone(&merged_data),
                    dwg,
                    self.dxf_version,
                    self.encoding,
                );
            handle_reader.set_position_in_bits(handle_start);

            let mut reader = DwgMergedReader::from_readers(
                main_reader,
                Some(text_reader),
                Some(handle_reader),
                self.dxf_version,
            );
            reader.set_handle_bits(handle_bits);
            reader.set_handle_start(handle_start);
            reader.set_main_data_end(main_data_end);
            return Ok((type_code, reader));
        }

        // 5. Create merged reader (pre-R2007, TwoStream)
        //
        //    For R2000–R2004 the merged data layout is:
        //        [BS type_code][RL main_size_bits][H handle][xdata]
        //        [...entity/object data...][handle references]
        //    The RL value (main_size_bits) tells us the bit position
        //    where the handle references begin in the merged data.
        //    The writer writes handle bytes at this exact bit position
        //    (which may NOT be byte-aligned).
        //
        //    For R13–R14 there is no top-level RL; we pass 0 and
        //    handle reads fall back to position 0.
        let dwg = DwgVersion::from_dxf_version(self.dxf_version).unwrap_or(DwgVersion::AC15);

        let handle_start_bits = if self.version.r2000_plus() {
            // Read BS + RL from a disposable temp reader to discover
            // the split point without consuming from the final reader.
            let mut temp = crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                Arc::clone(&merged_data),
                dwg,
                self.dxf_version,
                self.encoding,
            );
            let _tc = temp.read_object_type(); // BS
            temp.read_raw_long() as i64 // RL = main_size_bits
        } else {
            // R13–R14: no top-level RL.
            0
        };

        // Create main reader (reads from bit 0)
        let main_reader =
            crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                Arc::clone(&merged_data),
                dwg,
                self.dxf_version,
                self.encoding,
            );

        // Create handle reader positioned at handle_start_bits
        let mut handle_reader =
            crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::from_shared_with_encoding(
                merged_data,
                dwg,
                self.dxf_version,
                self.encoding,
            );
        handle_reader.set_position_in_bits(handle_start_bits);

        let mut reader = DwgMergedReader::from_readers(
            main_reader,
            None, // no text stream for pre-R2007
            Some(handle_reader),
            self.dxf_version,
        );
        reader.set_handle_start(handle_start_bits);
        reader.set_main_data_end(handle_start_bits);

        // 6. Read the type code from the reader
        let type_code = reader.read_object_type();

        Ok((type_code, reader))
    }

    /// Read common data shared by all objects (entities and non-entities).
    ///
    /// This reads: object_type (already read), size placeholder, handle, xdata.
    /// The type_code is passed in since it was already read during dispatch.
    pub fn read_common_data(
        &self,
        reader: &mut DwgMergedReader,
        type_code: i16,
    ) -> ObjectCommonData {
        // R2000..R2004: deferred size field (BL placeholder — skip it)
        // For R2007+, this was already consumed during record parsing.
        if self.version.r2000_plus() && !self.version.r2007_plus() {
            let _size_in_bits = reader.read_raw_long();
        }

        // Handle (absolute, from main stream)
        let handle = reader.main_mut().read_handle();

        // Set this object's handle as the reference for subsequent
        // offset-based handle codes (6/8/A/C) in the handle stream.
        reader.set_ref_handle(handle);

        // Extended data — read raw bytes for round-trip
        let eed_raw = self.read_extended_data_raw(reader);

        ObjectCommonData {
            type_code,
            handle,
            eed_raw,
        }
    }

    /// Read common entity data (the full entity preamble).
    ///
    /// Field order matches the writer exactly.
    pub fn read_common_entity_data(
        &self,
        reader: &mut DwgMergedReader,
        type_code: i16,
    ) -> EntityCommonData {
        let common = self.read_common_data(reader, type_code);

        // Graphic presence flag
        let has_graphic = reader.read_bit();
        let graphic_data = if has_graphic {
            // Read graphic data bytes for round-trip preservation.
            // R2010+: BLL (Bit Long Long); pre-R2010: RL (Raw Long)
            let graphic_size = if self.version.r2010_plus() {
                reader.read_bit_long_long()
            } else {
                reader.read_raw_long() as i64
            };
            let mut gdata = Vec::with_capacity(graphic_size.max(0) as usize);
            for _ in 0..graphic_size.max(0) {
                gdata.push(reader.read_byte());
            }
            Some(gdata)
        } else {
            None
        };

        // R13-R14: size field (RL = main_size_bits) — reposition handle reader
        if self.version.r13_14_only() {
            let main_size_bits = reader.read_raw_long();
            reader.reposition_handle_reader(main_size_bits);
        }

        // Entity mode (2 bits)
        let entity_mode = reader.main_mut().read_2bits();

        // Owner handle (if entity_mode == 0)
        let owner_handle = if entity_mode == 0 {
            reader.read_handle()
        } else {
            0
        };

        // Reactor count + handles
        // Every handle consumes at least one byte. Never synthesize reactors
        // from the zero values returned after the source record is exhausted.
        let reactor_count = safe_count(reader.read_bit_long())
            .min((reader.handle_remaining_bits() / 8).min(i32::MAX as i64) as i32);
        let mut reactors = Vec::new();
        for _ in 0..reactor_count {
            reactors.push(reader.read_handle());
        }

        // XDictionary
        // R2004+: xdic_missing_flag (B) + conditional handle
        // Pre-R2004: always read handle (0 = none)
        let xdictionary_handle = if self.version.r2004_plus() {
            let no_xdic = reader.read_bit();
            if !no_xdic {
                Some(reader.read_handle())
            } else {
                None
            }
        } else {
            let h = reader.read_handle();
            if h != 0 {
                Some(h)
            } else {
                None
            }
        };

        // R2013+: modeler geometry may live in the AcDs data store. The flag is
        // part of every entity header, including entities with preview data.
        let mut has_ds_data = false;
        if self.version.r2013_plus(self.dxf_version) {
            has_ds_data = reader.read_bit();
        }

        // R13-R14: layer + linetype
        let mut layer_handle = 0u64;
        let mut linetype_flags = 0u8;
        let mut linetype_handle = 0u64;
        if self.version.r13_14_only() {
            layer_handle = reader.read_handle();
            let is_bylayer_lt = reader.read_bit();
            if !is_bylayer_lt {
                // Linetype handle (hard pointer) — present if NOT by-layer
                linetype_flags = 0b11;
                linetype_handle = reader.read_handle();
            }
        }

        // Pre-R2004: Nolinks + prev/next (R13/R14 and R2000-R2002)
        let mut prev_entity_handle = None;
        let mut next_entity_handle = None;
        if !self.version.r2004_plus() {
            let nolinks = reader.read_bit();
            if !nolinks {
                prev_entity_handle = Some(reader.read_handle());
                next_entity_handle = Some(reader.read_handle());
            }
        }

        // Color
        let (color, transparency, has_color_handle) = if self.version.r2000_plus() {
            reader.read_en_color()
        } else {
            (reader.read_cm_color(), Transparency::default(), false)
        };

        // R2004+: Color book color handle (hard pointer) — only if flagged
        let color_book_handle = if self.version.r2004_plus() && has_color_handle {
            Some(reader.read_handle())
        } else {
            None
        };

        // Linetype scale
        let linetype_scale = reader.read_bit_double();

        // R13-R14: invisibility + return early
        // DXF group 60 convention (all DWG versions): 0 = visible, non-zero = invisible
        let invisible;
        if self.version.r13_14_only() {
            invisible = reader.read_bit_short() != 0;
            return EntityCommonData {
                common,
                has_graphic,
                graphic_data,
                entity_mode,
                owner_handle,
                reactors,
                xdictionary_handle,
                color,
                transparency,
                line_weight: 0,
                linetype_scale,
                invisible,
                layer_handle,
                linetype_flags,
                linetype_handle,
                color_book_handle,
                prev_entity_handle,
                next_entity_handle,
                material_flags: 0,
                material_handle: None,
                shadow_flags: 0,
                plotstyle_flags: 0,
                plotstyle_handle: None,
                full_visual_style_handle: None,
                face_visual_style_handle: None,
                edge_visual_style_handle: None,
                has_ds_data,
            };
        }

        // R2000+: Layer handle
        if self.version.r2000_plus() {
            layer_handle = reader.read_handle();
        }

        // Linetype flags (2 bits): 00=bylayer, 01=byblock, 10=continuous, 11=handle present
        linetype_handle = 0;
        linetype_flags = reader.main_mut().read_2bits();
        if linetype_flags == 0b11 {
            // Linetype handle (hard pointer) — present when flags == 11
            linetype_handle = reader.read_handle();
        }

        // R2007+: material flags + shadow flags
        let mut material_flags = 0u8;
        let mut material_handle: Option<u64> = None;
        let mut shadow_flags = 0u8;
        if self.version.r2007_plus() {
            material_flags = reader.main_mut().read_2bits();
            // Material handle (hard pointer) — present when flags == 0b11
            if material_flags == 0b11 {
                material_handle = Some(reader.read_handle());
            }
            shadow_flags = reader.read_byte();
        }

        // R2000+: Plotstyle flags (00=bylayer, 01=byblock, 11=handle present)
        let mut plotstyle_flags = 0u8;
        let mut plotstyle_handle: Option<u64> = None;
        if self.version.r2000_plus() {
            plotstyle_flags = reader.main_mut().read_2bits();
            if plotstyle_flags == 0b11 {
                // Plotstyle handle (hard pointer)
                plotstyle_handle = Some(reader.read_handle());
            }
        }

        // R2010+: visual style bits — each bit conditionally followed by a handle
        let mut full_visual_style_handle = None;
        let mut face_visual_style_handle = None;
        let mut edge_visual_style_handle = None;
        if self.version.r2010_plus() {
            if reader.read_bit() {
                full_visual_style_handle = Some(reader.read_handle());
            }
            if reader.read_bit() {
                face_visual_style_handle = Some(reader.read_handle());
            }
            if reader.read_bit() {
                edge_visual_style_handle = Some(reader.read_handle());
            }
        }

        // Invisibility
        invisible = reader.read_bit_short() != 0;

        // R2000+: Lineweight (5-bit DWG index → raw byte value)
        let line_weight = if self.version.r2000_plus() {
            reader.read_byte()
        } else {
            0
        };

        EntityCommonData {
            common,
            has_graphic,
            graphic_data,
            entity_mode,
            owner_handle,
            reactors,
            xdictionary_handle,
            color,
            transparency,
            line_weight,
            linetype_scale,
            invisible,
            layer_handle,
            linetype_flags,
            linetype_handle,
            color_book_handle,
            prev_entity_handle,
            next_entity_handle,
            material_flags,
            material_handle,
            shadow_flags,
            plotstyle_flags,
            plotstyle_handle,
            full_visual_style_handle,
            face_visual_style_handle,
            edge_visual_style_handle,
            has_ds_data,
        }
    }

    /// Read common non-entity object data.
    pub fn read_common_non_entity_data(
        &self,
        reader: &mut DwgMergedReader,
        type_code: i16,
    ) -> NonEntityCommonData {
        // Read common data (type + size + handle + xdata)
        let common = self.read_common_data(reader, type_code);

        // R13-R14: size field (RL = main_size_bits) — reposition handle reader
        if self.version.r13_14_only() {
            let main_size_bits = reader.read_raw_long();
            reader.reposition_handle_reader(main_size_bits);
        }

        // Owner handle (soft pointer)
        let owner_handle = reader.read_handle();

        // Reactor count + handles
        let reactor_count = safe_count(reader.read_bit_long())
            .min((reader.handle_remaining_bits() / 8).min(i32::MAX as i64) as i32);
        let mut reactors = Vec::new();
        for _ in 0..reactor_count {
            reactors.push(reader.read_handle());
        }

        // XDictionary
        // R2004+: xdic_missing_flag (B) + conditional handle
        // Pre-R2004: always read handle (0 = none)
        let xdictionary_handle = if self.version.r2004_plus() {
            let no_xdic = reader.read_bit();
            if !no_xdic {
                Some(reader.read_handle())
            } else {
                None
            }
        } else {
            let h = reader.read_handle();
            if h != 0 {
                Some(h)
            } else {
                None
            }
        };

        // R2013+: binary data flag
        let has_ds_data = self.version.r2013_plus(self.dxf_version) && reader.read_bit();

        NonEntityCommonData {
            common,
            owner_handle,
            reactors,
            xdictionary_handle,
            has_ds_data,
        }
    }

    /// Read extended data and return raw bytes per application.
    ///
    /// EED format: repeating [BS size | H app_handle (main!) | RC×size data] until size==0.
    /// The application handle is ALWAYS in the main stream (even for R2007+).
    fn read_extended_data_raw(&self, reader: &mut DwgMergedReader) -> Vec<(u64, Vec<u8>)> {
        let mut result: Vec<(u64, Vec<u8>)> = Vec::new();
        loop {
            let size = reader.read_bit_short();
            if size <= 0 {
                break;
            }
            let size_u = size as usize;
            // Application handle — always from MAIN stream.
            let app_handle = reader.main_mut().read_handle();
            // Raw xdata bytes (from main stream, one byte at a time).
            // These bytes are NOT bit-packed; read them through the bit
            // reader to respect any outstanding bit shift.
            let mut data = Vec::with_capacity(size_u);
            for _ in 0..size_u {
                data.push(reader.read_byte());
            }
            result.push((app_handle, data));
        }
        result
    }

    /// Get the list of all handles in the handle map.
    pub fn handles(&self) -> Vec<u64> {
        self.handle_map.keys().copied().collect()
    }

    /// Get the byte offset for a given handle.
    pub fn offset_for(&self, handle: u64) -> Option<i64> {
        self.handle_map.get(&handle).copied()
    }

    /// Get the DWG version.
    pub fn version(&self) -> DwgVersion {
        self.version
    }

    /// Get the DXF version.
    pub fn dxf_version(&self) -> DxfVersion {
        self.dxf_version
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Modular encoding readers (byte-level, for record framing)
// ════════════════════════════════════════════════════════════════════════════

/// Allocation-free reader for the few leading bits that encode an object type.
/// A full `DwgBitReader` owns its backing bytes, which is useful for persistent
/// merged streams but wasteful for the pass-1 type catalog.
struct PrefixBitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> PrefixBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn read_bit(&mut self) -> u8 {
        let shift = 7 - self.bit % 8;
        let value = (self.data.get(self.bit / 8).copied().unwrap_or(0) >> shift) & 1;
        self.bit += 1;
        value
    }

    fn read_2bits(&mut self) -> u8 {
        (self.read_bit() << 1) | self.read_bit()
    }

    fn read_byte(&mut self) -> u8 {
        let mut value = 0u8;
        for _ in 0..8 {
            value = (value << 1) | self.read_bit();
        }
        value
    }

    fn read_raw_short(&mut self) -> i16 {
        i16::from_le_bytes([self.read_byte(), self.read_byte()])
    }

    fn read_object_type(&mut self, version: DxfVersion) -> i16 {
        let pair = self.read_2bits();
        if version >= DxfVersion::AC1024 {
            match pair {
                0 => self.read_byte() as i16,
                1 => 0x1F0 + self.read_byte() as i16,
                2 | 3 => self.read_raw_short(),
                _ => unreachable!(),
            }
        } else {
            match pair {
                0 => self.read_raw_short(),
                1 => self.read_byte() as i16,
                2 => 0,
                3 => 256,
                _ => unreachable!(),
            }
        }
    }
}

/// Read a ModularShort (MS) from a byte slice.
/// Returns (value, bytes_consumed).
fn read_modular_short(data: &[u8]) -> (usize, usize) {
    let mut value: usize = 0;
    let mut shift = 0;
    let mut i = 0;

    loop {
        if i + 1 >= data.len() {
            break;
        }
        let word = u16::from_le_bytes([data[i], data[i + 1]]);
        i += 2;

        value |= ((word & 0x7FFF) as usize) << shift;
        shift += 15;

        if (word & 0x8000) == 0 {
            break;
        }
    }

    (value, i)
}

/// Read a ModularChar (MC) from a byte slice.
/// Returns (value, bytes_consumed).
fn read_modular_char(data: &[u8]) -> (usize, usize) {
    let mut value: usize = 0;
    let mut shift = 0;
    let mut i = 0;

    loop {
        if i >= data.len() {
            break;
        }
        let b = data[i];
        i += 1;

        value |= ((b & 0x7F) as usize) << shift;
        shift += 7;

        if (b & 0x80) == 0 {
            break;
        }
    }

    (value, i)
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modular_short_read() {
        // Single word: value 42 (0x002A), no continuation
        let data = [0x2A, 0x00];
        let (val, len) = read_modular_short(&data);
        assert_eq!(val, 42);
        assert_eq!(len, 2);
    }

    #[test]
    fn test_modular_short_multi_word() {
        // Two words: 0x8001 0x0001 → (1) | (1 << 15) = 1 + 32768 = 32769
        let data = [0x01, 0x80, 0x01, 0x00];
        let (val, len) = read_modular_short(&data);
        assert_eq!(val, 32769);
        assert_eq!(len, 4);
    }

    #[test]
    fn test_modular_char_read() {
        // Single byte: value 42 (no continuation)
        let data = [42u8];
        let (val, len) = read_modular_char(&data);
        assert_eq!(val, 42);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_modular_char_multi_byte() {
        // Two bytes: 0x81 0x01 → (1) | (1 << 7) = 1 + 128 = 129
        let data = [0x81, 0x01];
        let (val, len) = read_modular_char(&data);
        assert_eq!(val, 129);
        assert_eq!(len, 2);
    }

    #[test]
    fn test_modular_short_roundtrip() {
        // Verify roundtrip with the writer's encoding
        use crate::io::dwg::dwg_stream_writers::object_writer::common::{
            write_modular_char_bytes, write_modular_short_bytes,
        };

        for &val in &[0, 1, 42, 127, 128, 255, 1000, 32767, 32768, 65535, 100000] {
            let mut buf = Vec::new();
            write_modular_short_bytes(&mut buf, val);
            let (read_val, _) = read_modular_short(&buf);
            assert_eq!(read_val, val, "MS roundtrip failed for {}", val);
        }

        for &val in &[0, 1, 42, 127, 128, 255, 1000, 32767, 65535, 100000] {
            let mut buf = Vec::new();
            write_modular_char_bytes(&mut buf, val);
            let (read_val, _) = read_modular_char(&buf);
            assert_eq!(read_val, val, "MC roundtrip failed for {}", val);
        }
    }

    #[test]
    fn test_read_record_roundtrip() {
        // Write a simple object record using the writer, then read it back
        use crate::io::dwg::crc;
        use crate::io::dwg::dwg_stream_writers::merged_writer::DwgMergedWriter;
        use crate::io::dwg::dwg_stream_writers::object_writer::common::write_modular_short_bytes;

        let dwg_ver = DwgVersion::AC15;
        let dxf_ver = DxfVersion::AC1015;

        // Build merged data: type_code(BS) + size_placeholder(BL) + handle
        let mut writer = DwgMergedWriter::new(dwg_ver, dxf_ver);
        writer.write_object_type(common::OBJ_LINE); // type = 19
        writer.save_position_for_size();
        writer.main_mut().write_handle_undefined(0x42);
        // XData: count=0
        writer.write_bit_short(0);

        let merged = writer.merge();

        // Build the record frame: MS(size) + merged + CRC16
        let mut record = Vec::new();
        write_modular_short_bytes(&mut record, merged.len());
        record.extend_from_slice(&merged);
        let crc_val = crc::crc16(crc::CRC16_SEED, &record);
        record.extend_from_slice(&crc_val.to_le_bytes());

        // Create the reader
        let handle_map = HashMap::new();
        let reader = DwgObjectReader::new(record, dxf_ver, handle_map).unwrap();

        // Read the record at offset 0
        let (type_code, mut merged_reader) = reader.read_record_at(0).unwrap();
        assert_eq!(type_code, common::OBJ_LINE);

        // Read common data (size BL + handle + xdata)
        let common_data = reader.read_common_data(&mut merged_reader, type_code);
        assert_eq!(common_data.type_code, common::OBJ_LINE);
        assert_eq!(common_data.handle, 0x42);
        assert!(common_data.eed_raw.is_empty());
    }
}
