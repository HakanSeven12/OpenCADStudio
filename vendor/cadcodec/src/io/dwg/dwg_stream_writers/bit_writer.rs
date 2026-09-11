//! DWG bit-level binary writer
//!
//! The DWG format operates at bit granularity, not byte granularity.
//! This writer tracks the current bit position and handles all the
//! DWG-specific variable-length encodings.
//!
//! Based on the reference `DwgStreamWriterBase` and version-specific subclasses.

use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::Transparency;
use crate::types::{Color, DxfVersion, Vector2, Vector3};

/// Bit-level writer for the DWG binary format.
///
/// Accumulates bits into bytes using MSB-first packing within each byte.
/// When 8 bits accumulate, the byte is flushed to the internal buffer.
///
/// # Internal state
/// - `buffer`: The accumulated output bytes
/// - `bit_shift`: Current bit offset within the partial byte (0–7)
/// - `last_byte`: Accumulator for the partial byte being built
pub struct DwgBitWriter {
    /// Output buffer holding completed bytes.
    /// After seeking backward, buffer.len() may be > write_pos.
    buffer: Vec<u8>,
    /// Current byte write position (index where the next full byte goes).
    /// Equal to buffer.len() during append-only usage. May be < buffer.len()
    /// after `set_position_in_bits()` seeks backward.
    write_pos: usize,
    /// Current bit offset within the partial byte (0 = next bit goes to MSB)
    bit_shift: u8,
    /// Partial byte being built (only bits 0..bit_shift are written)
    last_byte: u8,
    /// DWG stream version (determines encoding behavior)
    version: DwgVersion,
    /// Exact DXF version (for R2013+/R2018+ sub-version checks)
    dxf_version: DxfVersion,
    /// Saved bit position for size patching
    saved_position_in_bits: i64,
    /// Text encoding for non-Unicode strings
    encoding: &'static encoding_rs::Encoding,
}

impl DwgBitWriter {
    /// Create a new bit writer for the given DWG version.
    pub fn new(version: DwgVersion, dxf_version: DxfVersion) -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            write_pos: 0,
            bit_shift: 0,
            last_byte: 0,
            version,
            dxf_version,
            saved_position_in_bits: -1,
            encoding: encoding_rs::WINDOWS_1252,
        }
    }

    /// Create a new bit writer with a specific encoding.
    pub fn with_encoding(
        version: DwgVersion,
        dxf_version: DxfVersion,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        let mut w = Self::new(version, dxf_version);
        w.encoding = encoding;
        w
    }

    /// Encode text using the document's legacy text code page.
    ///
    /// Characters the code page cannot represent become MIF `\U+XXXX`
    /// escapes instead of `&#NNNNN;` references.
    pub fn encode_legacy_text(&self, text: &str) -> Vec<u8> {
        crate::io::dxf::code_page::encode_legacy_string(text, self.encoding)
    }

    /// Get the DWG version.
    pub fn version(&self) -> DwgVersion {
        self.version
    }

    /// Get the DXF version.
    pub fn dxf_version(&self) -> DxfVersion {
        self.dxf_version
    }

    /// Current position in bits.
    pub fn position_in_bits(&self) -> i64 {
        self.write_pos as i64 * 8 + self.bit_shift as i64
    }

    /// Current byte position (partial byte not counted).
    pub fn position(&self) -> usize {
        self.write_pos
    }

    /// Get the saved position in bits.
    pub fn saved_position_in_bits(&self) -> i64 {
        self.saved_position_in_bits
    }

    /// Get a reference to the output buffer.
    ///
    /// Note: If `bit_shift > 0`, there is an unflushed partial byte in `last_byte`.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Get a snapshot of the current bytes (including partial byte) without mutating.
    pub fn to_bytes_snapshot(&self) -> Vec<u8> {
        let mut result = self.buffer.clone();
        if self.bit_shift > 0 {
            if self.write_pos < result.len() {
                let mask = !((1u8 << (8 - self.bit_shift)) - 1);
                result[self.write_pos] = (self.last_byte & mask) | (result[self.write_pos] & !mask);
            } else {
                while result.len() < self.write_pos {
                    result.push(0);
                }
                result.push(self.last_byte);
            }
        }
        result
    }

    /// Get the total length in bytes (including partial byte if any).
    pub fn length(&self) -> usize {
        self.buffer.len() + if self.bit_shift > 0 { 1 } else { 0 }
    }

    /// Flush any remaining partial byte and return the completed buffer.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.flush();
        self.buffer
    }

    /// Get a copy of the buffer with any partial byte flushed.
    pub fn to_bytes(&mut self) -> Vec<u8> {
        self.flush();
        self.buffer.clone()
    }

    /// Flush and move out the completed buffer without cloning it.
    /// The writer remains valid and can be reset/reused by its owner.
    pub fn take_bytes(&mut self) -> Vec<u8> {
        self.flush();
        self.write_pos = 0;
        std::mem::take(&mut self.buffer)
    }

    /// Flush the partial byte (pad with zeros).
    ///
    /// If the write position is within the existing buffer (after seeking),
    /// the partial byte is merged with the existing data (lower bits preserved).
    pub fn flush(&mut self) {
        if self.bit_shift > 0 {
            if self.write_pos < self.buffer.len() {
                // Merge: upper bit_shift bits from last_byte, lower bits preserved
                let mask = !((1u8 << (8 - self.bit_shift)) - 1); // e.g., shift=3 → 0xE0
                self.buffer[self.write_pos] =
                    (self.last_byte & mask) | (self.buffer[self.write_pos] & !mask);
            } else {
                // Extend buffer to reach write_pos, then push partial byte
                while self.buffer.len() < self.write_pos {
                    self.buffer.push(0);
                }
                self.buffer.push(self.last_byte);
            }
            self.write_pos += 1;
            self.last_byte = 0;
            self.bit_shift = 0;
        }
    }

    /// Reset the writer, clearing all data.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.write_pos = 0;
        self.bit_shift = 0;
        self.last_byte = 0;
        self.saved_position_in_bits = -1;
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Internal byte emission (seekable)
    // ════════════════════════════════════════════════════════════════════════

    /// Emit a complete byte at the current write position.
    ///
    /// If `write_pos < buffer.len()` (after seeking backward), the existing
    /// byte is overwritten. Otherwise the buffer is extended.
    #[inline]
    fn emit_byte(&mut self, value: u8) {
        if self.write_pos < self.buffer.len() {
            self.buffer[self.write_pos] = value;
        } else {
            // Extend to reach write_pos if there is a gap
            while self.buffer.len() < self.write_pos {
                self.buffer.push(0);
            }
            self.buffer.push(value);
        }
        self.write_pos += 1;
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Primitive bit operations
    // ════════════════════════════════════════════════════════════════════════

    /// Write a single bit (B type).
    ///
    /// Bits are packed MSB-first within each byte. When the 8th bit is
    /// written, the byte is flushed to the buffer.
    #[inline]
    pub fn write_bit(&mut self, value: bool) {
        if self.bit_shift < 7 {
            if value {
                self.last_byte |= 1 << (7 - self.bit_shift);
            }
            self.bit_shift += 1;
        } else {
            // bit_shift == 7: this is the last bit of the byte
            if value {
                self.last_byte |= 1;
            }
            self.emit_byte(self.last_byte);
            self.last_byte = 0;
            self.bit_shift = 0;
        }
    }

    /// Write a 2-bit value (BB type).
    ///
    /// Handles the case where the 2 bits straddle a byte boundary.
    #[inline]
    pub fn write_2bits(&mut self, value: u8) {
        debug_assert!(value <= 3, "2-bit value must be 0–3");
        match self.bit_shift {
            0..=5 => {
                // Both bits fit in the current byte
                self.last_byte |= (value & 0x03) << (6 - self.bit_shift);
                self.bit_shift += 2;
            }
            6 => {
                // Both bits fill the remaining byte exactly
                self.last_byte |= value & 0x03;
                self.emit_byte(self.last_byte);
                self.last_byte = 0;
                self.bit_shift = 0;
            }
            7 => {
                // Bits straddle the byte boundary
                // High bit goes into current byte's LSB
                self.last_byte |= (value >> 1) & 0x01;
                self.emit_byte(self.last_byte);
                // Low bit starts the next byte's MSB
                self.last_byte = (value & 0x01) << 7;
                self.bit_shift = 1;
            }
            _ => unreachable!(),
        }
    }

    /// Write a 3-bit value (3B type, used for BLL size prefix).
    fn write_3bits(&mut self, value: u8) {
        debug_assert!(value <= 7, "3-bit value must be 0–7");
        self.write_bit(value & 4 != 0);
        self.write_bit(value & 2 != 0);
        self.write_bit(value & 1 != 0);
    }

    /// Write a full byte (RC type).
    ///
    /// If not at a byte boundary, the byte straddles two output bytes.
    #[inline]
    pub fn write_byte(&mut self, value: u8) {
        if self.bit_shift == 0 {
            self.emit_byte(value);
        } else {
            // Straddle: merge upper bits with last_byte, lower bits start new byte
            self.emit_byte(self.last_byte | (value >> self.bit_shift));
            self.last_byte = value << (8 - self.bit_shift);
        }
    }

    /// Write a slice of bytes.
    pub fn write_bytes(&mut self, data: &[u8]) {
        if self.bit_shift == 0 {
            for &b in data {
                self.emit_byte(b);
            }
        } else {
            for &b in data {
                self.emit_byte(self.last_byte | (b >> self.bit_shift));
                self.last_byte = b << (8 - self.bit_shift);
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Raw typed writers (no bit coding)
    // ════════════════════════════════════════════════════════════════════════

    /// Write a 16-bit signed integer, little-endian (RS type).
    pub fn write_raw_short(&mut self, value: i16) {
        let bytes = value.to_le_bytes();
        self.write_bytes(&bytes);
    }

    /// Write a 16-bit unsigned integer, little-endian.
    pub fn write_raw_ushort(&mut self, value: u16) {
        let bytes = value.to_le_bytes();
        self.write_bytes(&bytes);
    }

    /// Write a 32-bit signed integer, little-endian (RL type).
    pub fn write_raw_long(&mut self, value: i32) {
        let bytes = value.to_le_bytes();
        self.write_bytes(&bytes);
    }

    /// Write a 64-bit double, little-endian (RD type).
    pub fn write_raw_double(&mut self, value: f64) {
        let bytes = value.to_le_bytes();
        self.write_bytes(&bytes);
    }

    /// Write a 32-bit integer as 4 raw bytes, little-endian.
    pub fn write_int(&mut self, value: i32) {
        self.write_raw_long(value);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Bit-coded variable-length encodings
    // ════════════════════════════════════════════════════════════════════════

    /// Write a bit-coded short (BS type).
    ///
    /// | Condition     | 2-bit code | Data           |
    /// |---------------|-----------|----------------|
    /// | value == 0    | 10 (2)    | nothing        |
    /// | 0 < v < 256   | 01 (1)    | 1 byte         |
    /// | value == 256  | 11 (3)    | nothing        |
    /// | otherwise     | 00 (0)    | 2 bytes LE     |
    pub fn write_bit_short(&mut self, value: i16) {
        if value == 0 {
            self.write_2bits(2);
        } else if value > 0 && value < 256 {
            self.write_2bits(1);
            self.write_byte(value as u8);
        } else if value == 256 {
            self.write_2bits(3);
        } else {
            self.write_2bits(0);
            self.write_raw_short(value);
        }
    }

    /// Write a bit-coded long (BL type).
    ///
    /// | Condition     | 2-bit code | Data           |
    /// |---------------|-----------|----------------|
    /// | value == 0    | 10 (2)    | nothing        |
    /// | 0 < v < 256   | 01 (1)    | 1 byte         |
    /// | otherwise     | 00 (0)    | 4 bytes LE     |
    pub fn write_bit_long(&mut self, value: i32) {
        if value == 0 {
            self.write_2bits(2);
        } else if value > 0 && value < 256 {
            self.write_2bits(1);
            self.write_byte(value as u8);
        } else {
            self.write_2bits(0);
            self.write_raw_long(value);
        }
    }

    /// Write an unsigned bit-coded long.
    pub fn write_bit_long_unsigned(&mut self, value: u32) {
        self.write_bit_long(value as i32);
    }

    /// Write a bit-coded double (BD type).
    ///
    /// | Condition       | 2-bit code | Data           |
    /// |-----------------|-----------|----------------|
    /// | value == 0.0    | 10 (2)    | nothing        |
    /// | value == 1.0    | 01 (1)    | nothing        |
    /// | otherwise       | 00 (0)    | 8 bytes LE     |
    pub fn write_bit_double(&mut self, value: f64) {
        if value == 0.0 {
            self.write_2bits(2);
        } else if value == 1.0 {
            self.write_2bits(1);
        } else {
            self.write_2bits(0);
            self.write_raw_double(value);
        }
    }

    /// Write a bit-coded long long (BLL type).
    ///
    /// 3-bit size prefix (0–8) indicating how many bytes follow,
    /// then N bytes least-significant first.
    pub fn write_bit_long_long(&mut self, value: i64) {
        // Count how many bytes are needed
        let mut size: u8 = 0;
        let mut temp = value as u64;
        while temp > 0 {
            size += 1;
            temp >>= 8;
        }
        debug_assert!(size <= 8);

        self.write_3bits(size);
        let bytes = (value as u64).to_le_bytes();
        for i in 0..size as usize {
            self.write_byte(bytes[i]);
        }
    }

    /// Write a bit-coded double with a default value (DD type).
    ///
    /// Differential encoding that compares the value byte-by-byte with
    /// a default value, writing only the differing bytes.
    ///
    /// The first parameter `def` is the value to STORE in the stream.
    /// The second parameter `value` is the REFERENCE (default) value
    /// that the reader already knows. This matches the reference
    /// parameter convention: `WriteBitDoubleWithDefault(double def, double value)`.
    ///
    /// | Case                            | 2-bit code | Data            |
    /// |---------------------------------|-----------|-----------------|
    /// | def == value                    | 00 (0)    | nothing         |
    /// | last 4 bytes match (4,5,6,7)    | 01 (1)    | 4 bytes (0..4)  |
    /// | last 2 bytes match (6,7)        | 10 (2)    | 6 bytes         |
    /// | otherwise                       | 11 (3)    | 8 bytes (full)  |
    pub fn write_bit_double_with_default(&mut self, def: f64, value: f64) {
        if def == value {
            self.write_2bits(0); // Use default — no data
            return;
        }

        let def_bytes = def.to_le_bytes();
        let val_bytes = value.to_le_bytes();

        // Count matching bytes from the end inward (C# "symmetry" loop)
        let mut matching_from_end = 0i32;
        let mut last = 7i32;
        while last >= 0 && def_bytes[last as usize] == val_bytes[last as usize] {
            matching_from_end += 1;
            last -= 1;
        }

        if matching_from_end >= 4 {
            // 01: bytes 4,5,6,7 match — write def bytes 0,1,2,3
            self.write_2bits(1);
            self.write_byte(def_bytes[0]);
            self.write_byte(def_bytes[1]);
            self.write_byte(def_bytes[2]);
            self.write_byte(def_bytes[3]);
        } else if matching_from_end >= 2 {
            // 10: bytes 6,7 match — write def bytes 4,5 then 0,1,2,3
            self.write_2bits(2);
            self.write_byte(def_bytes[4]);
            self.write_byte(def_bytes[5]);
            self.write_byte(def_bytes[0]);
            self.write_byte(def_bytes[1]);
            self.write_byte(def_bytes[2]);
            self.write_byte(def_bytes[3]);
        } else {
            // 11: no match — write full double
            self.write_2bits(3);
            self.write_raw_double(def);
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Compound geometric types
    // ════════════════════════════════════════════════════════════════════════

    /// Write two bit-doubles (2BD type).
    pub fn write_2bit_double(&mut self, value: Vector2) {
        self.write_bit_double(value.x);
        self.write_bit_double(value.y);
    }

    /// Write three bit-doubles (3BD type).
    pub fn write_3bit_double(&mut self, value: Vector3) {
        self.write_bit_double(value.x);
        self.write_bit_double(value.y);
        self.write_bit_double(value.z);
    }

    /// Write two raw doubles (2RD type).
    pub fn write_2raw_double(&mut self, value: Vector2) {
        self.write_raw_double(value.x);
        self.write_raw_double(value.y);
    }

    /// Write three raw doubles (3RD type).
    pub fn write_3raw_double(&mut self, value: Vector3) {
        self.write_raw_double(value.x);
        self.write_raw_double(value.y);
        self.write_raw_double(value.z);
    }

    /// Write two bit-doubles with defaults (2DD type).
    pub fn write_2bit_double_with_default(&mut self, def: Vector2, value: Vector2) {
        self.write_bit_double_with_default(def.x, value.x);
        self.write_bit_double_with_default(def.y, value.y);
    }

    /// Write three bit-doubles with defaults (3DD type).
    pub fn write_3bit_double_with_default(&mut self, def: Vector3, value: Vector3) {
        self.write_bit_double_with_default(def.x, value.x);
        self.write_bit_double_with_default(def.y, value.y);
        self.write_bit_double_with_default(def.z, value.z);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Version-dependent typed writers
    // ════════════════════════════════════════════════════════════════════════

    /// Write thickness with version-specific optimization (BT type).
    ///
    /// - R13/R14 (AC12): Always writes full BD
    /// - R2000+ (AC15+): If thickness == 0.0, writes single `true` bit; otherwise `false` + BD
    pub fn write_bit_thickness(&mut self, thickness: f64) {
        if self.version.r13_14_only() {
            self.write_bit_double(thickness);
        } else {
            // R2000+ optimization
            if thickness == 0.0 {
                self.write_bit(true);
            } else {
                self.write_bit(false);
                self.write_bit_double(thickness);
            }
        }
    }

    /// Write extrusion normal with version-specific optimization (BE type).
    ///
    /// - R13/R14 (AC12): Always writes full 3BD
    /// - R2000+ (AC15+): If normal == (0,0,1), writes single `true` bit; otherwise `false` + 3BD
    pub fn write_bit_extrusion(&mut self, normal: Vector3) {
        if self.version.r13_14_only() {
            self.write_3bit_double(normal);
        } else {
            // R2000+ optimization
            if normal.x == 0.0 && normal.y == 0.0 && normal.z == 1.0 {
                self.write_bit(true);
            } else {
                self.write_bit(false);
                self.write_3bit_double(normal);
            }
        }
    }

    /// Write object type with version-specific encoding (OT type).
    ///
    /// - Pre-R2010: Uses `write_bit_short`
    /// - R2010+ (AC24): Compact 2-bit prefix encoding
    pub fn write_object_type(&mut self, value: i16) {
        if self.version.uses_compact_object_type() {
            // R2010+ compact encoding
            if value >= 0 && value <= 255 {
                self.write_2bits(0);
                self.write_byte(value as u8);
            } else if value >= 0x1F0 && value <= 0x2EF {
                self.write_2bits(1);
                self.write_byte((value - 0x1F0) as u8);
            } else {
                self.write_2bits(2);
                self.write_raw_short(value);
            }
        } else {
            self.write_bit_short(value);
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Text / String writers
    // ════════════════════════════════════════════════════════════════════════

    /// Write a variable-length text string (TV type).
    ///
    /// - Pre-R2007: BS length (byte count) + encoded bytes using document encoding
    /// - R2007+ (AC21): BS length (char count) + UTF-16LE bytes
    pub fn write_variable_text(&mut self, text: &str) {
        if text.is_empty() {
            self.write_bit_short(0);
            return;
        }

        if self.version.uses_unicode_text() {
            // R2007+: Write character count, then UTF-16LE
            let utf16: Vec<u16> = text.encode_utf16().collect();
            self.write_bit_short(utf16.len() as i16);
            for code_unit in &utf16 {
                self.write_bytes(&code_unit.to_le_bytes());
            }
        } else {
            // Pre-R2007: Write byte count, then encoded bytes
            let encoded = crate::io::dxf::code_page::encode_legacy_string(text, self.encoding);
            self.write_bit_short(encoded.len() as i16);
            self.write_bytes(&encoded);
        }
    }

    /// Write a Unicode text string (TU type, R2007+ only).
    ///
    /// RS length (char count + 1 for null) + UTF-16LE bytes + 2-byte null terminator.
    pub fn write_text_unicode(&mut self, text: &str) {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        self.write_raw_short((utf16.len() + 1) as i16); // char count + 1 for null
        for code_unit in &utf16 {
            self.write_bytes(&code_unit.to_le_bytes());
        }
        // Null terminator (2 bytes)
        self.write_bytes(&[0, 0]);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Handle reference encoding
    // ════════════════════════════════════════════════════════════════════════

    /// Write a handle reference.
    ///
    /// Header byte: `(reference_type << 4) | byte_count`
    /// Handle bytes: **big-endian** (unlike everything else which is LE).
    pub fn write_handle(&mut self, ref_type: DwgReferenceType, handle: u64) {
        let byte_count = handle_byte_count(handle);
        let header = (ref_type.code() << 4) | byte_count;
        self.write_byte(header);

        // Write handle bytes in big-endian order
        let bytes = handle.to_be_bytes();
        let start = (8 - byte_count) as usize;
        for i in start..8 {
            self.write_byte(bytes[i]);
        }
    }

    /// Write a handle reference using the compact offset form relative to the
    /// current object's handle.
    pub fn write_handle_relative(&mut self, reference_handle: u64, handle: u64) {
        if handle == reference_handle.wrapping_add(1) {
            self.write_byte(0x60);
            return;
        }
        if handle == reference_handle.wrapping_sub(1) {
            self.write_byte(0x80);
            return;
        }

        let (code, offset) = if handle >= reference_handle {
            (0xA0, handle - reference_handle)
        } else {
            (0xC0, reference_handle - handle)
        };
        let byte_count = handle_byte_count(offset);
        self.write_byte(code | byte_count);
        let bytes = offset.to_be_bytes();
        let start = (8 - byte_count) as usize;
        for i in start..8 {
            self.write_byte(bytes[i]);
        }
    }

    /// Write a handle reference with `Undefined` type (absolute handle).
    pub fn write_handle_undefined(&mut self, handle: u64) {
        self.write_handle(DwgReferenceType::Undefined, handle);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Color encoding (version-dependent)
    // ════════════════════════════════════════════════════════════════════════

    /// Write CMC color (used in header variables, table entries).
    ///
    /// - R13/R14 (AC12), R2000 (AC15): Color index as BS
    /// - R2004+ (AC18): BS(0) + BL(color bytes) + RC(0)
    pub fn write_cm_color(&mut self, color: &Color) {
        if self.version.r2004_plus() {
            self.write_cm_true_color(color);
        } else {
            // R13–R2000: Write color index as BS
            let index = color.approximate_index();
            self.write_bit_short(index);
        }
    }

    /// Write the full R2004 CMC payload regardless of the file version.
    ///
    /// TABLESTYLE legacy row colors use this CMTC encoding in AC1015 too.
    pub fn write_cm_true_color(&mut self, color: &Color) {
        // The BS index is a legacy slot; the BL carries the full color data.
        self.write_bit_short(0);
        let color_long = match color {
            Color::Rgb { r, g, b } => {
                (*b as u32) | ((*g as u32) << 8) | ((*r as u32) << 16) | (0xC2u32 << 24)
            }
            Color::ByLayer => 0xC0u32 << 24,
            Color::None => 0xC8u32 << 24,
            Color::ByBlock => 0xC1u32 << 24,
            Color::Index(idx) => (*idx as u32) | (0xC3u32 << 24),
        };
        self.write_bit_long(color_long as i32);
        self.write_byte(0);
    }

    /// Write entity color with transparency (ENC type).
    ///
    /// - R13/R14: Just writes color index as BS (no transparency)
    /// - R2004+: Flags bitfield with optional true color BL and transparency BL
    pub fn write_en_color(&mut self, color: &Color, transparency: &Transparency) {
        self.write_en_color_with_book(color, transparency, false);
    }

    /// Write entity color with transparency and optional book color flag.
    pub fn write_en_color_with_book(
        &mut self,
        color: &Color,
        transparency: &Transparency,
        is_book_color: bool,
    ) {
        if !self.version.supports_true_color() {
            // R13–R2000: Just CMC color
            self.write_cm_color(color);
            return;
        }

        // R2004+: Build flags bitfield
        let mut flags: u16 = 0;
        let is_true_color = matches!(color, Color::Rgb { .. });
        let has_transparency = !transparency.is_by_layer();

        // Autodesk writes book colors with both ENC color bits set.  The
        // AcDbColor handle still supplies the RGB value, so no RGB BL follows.
        if is_book_color {
            flags |= 0xC000; // AcDbColor reference (handle written separately)
                             // ENC still carries the ACI fallback in the low 13 bits. Omitting
                             // it turns any indexed book color into ByBlock when the record is
                             // read before (or without) resolving the AcDbColor object.
            flags |= color.approximate_index() as u16 & 0x1FFF;
        } else if is_true_color {
            flags |= 0x8000; // Complex/true color follows
        }
        if has_transparency {
            flags |= 0x2000; // Transparency follows
        }

        if !is_true_color && !is_book_color {
            // Simple ACI color — include index in the flags BS
            let index = color.approximate_index();
            flags |= index as u16 & 0x1FFF;
        }

        self.write_bit_short(flags as i16);

        // Field order matches the entity ENC encoding: the true-color rgb BL
        // FIRST, then the transparency BL (book colors emit no rgb — it comes
        // from the AcDbColor handle — but still emit the transparency last).
        if is_true_color && !is_book_color {
            let color_long = match color {
                Color::Rgb { r, g, b } => {
                    (*b as u32) | ((*g as u32) << 8) | ((*r as u32) << 16) | (0xC2u32 << 24)
                }
                Color::Index(idx) => (*idx as u32) | (0xC3u32 << 24),
                Color::ByLayer => 0xC0u32 << 24,
                Color::None => 0xC8u32 << 24,
                Color::ByBlock => 0xC3u32 << 24,
            };
            self.write_bit_long(color_long as i32);
        }

        if has_transparency {
            self.write_bit_long(transparency.to_alpha_value());
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Modular encodings
    // ════════════════════════════════════════════════════════════════════════

    /// Write a modular short value (MS type).
    ///
    /// Two-byte-chunk encoding used for object sizes.  Byte layout matches
    /// C#'s `DwgObjectWriter.writeSize()`:
    ///
    /// - `value < 0x8000`: 2 bytes LE (no flag)
    /// - `value >= 0x8000`: 4 bytes — byte 1 bit 7 is continuation flag,
    ///   lower 15 bits in bytes 0-1, upper bits in bytes 2-3
    pub fn write_modular_short(&mut self, value: usize) {
        if value < 0x8000 {
            // Single word: [low_byte, high_byte]
            self.write_raw_ushort(value as u16);
        } else {
            // Two words with continuation flag in byte 1 bit 7
            let lo = (value & 0xFF) as u8;
            let mid = (((value >> 8) & 0x7F) as u8) | 0x80; // continuation flag
            let hi_lo = ((value >> 15) & 0xFF) as u8;
            let hi_hi = ((value >> 23) & 0xFF) as u8;
            self.write_byte(lo);
            self.write_byte(mid);
            self.write_byte(hi_lo);
            self.write_byte(hi_hi);
        }
    }

    /// Write a modular char value.
    ///
    /// Multi-byte encoding where each byte uses 7 bits for data and the
    /// high bit as a continuation flag. Used for handle-stream bit-size in R2010+.
    pub fn write_modular_char(&mut self, value: usize) {
        let mut v = value;
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.write_byte(byte);
                break;
            } else {
                self.write_byte(byte | 0x80);
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Date/time
    // ════════════════════════════════════════════════════════════════════════

    /// Write a date/time as two BL values (Julian day + milliseconds).
    pub fn write_datetime(&mut self, julian_day: i32, milliseconds: i32) {
        self.write_bit_long(julian_day);
        self.write_bit_long(milliseconds);
    }

    /// Write a date/time as two raw longs (8-bit Julian date format).
    pub fn write_8bit_julian_date(&mut self, julian_day: i32, milliseconds: i32) {
        self.write_raw_long(julian_day);
        self.write_raw_long(milliseconds);
    }

    /// Write a timespan as two BL values (days + milliseconds).
    pub fn write_timespan(&mut self, days: i32, milliseconds: i32) {
        self.write_bit_long(days);
        self.write_bit_long(milliseconds);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Position / alignment
    // ════════════════════════════════════════════════════════════════════════

    /// Pad remaining bits in the current byte with zeros (align to byte boundary).
    pub fn write_spear_shift(&mut self) {
        while self.bit_shift > 0 {
            self.write_bit(false);
        }
    }

    /// Save the current bit position and write 4 zero bytes as a size placeholder.
    ///
    /// Later, `set_position_in_bits` + writing can patch the actual size.
    pub fn save_position_for_size(&mut self) {
        self.saved_position_in_bits = self.position_in_bits();
        self.write_int(0); // 4-byte placeholder
    }

    /// Set the write position to a specific bit offset (seekable).
    ///
    /// This allows seeking back to patch previously written data (e.g., size fields).
    /// Unlike N truncating approach, this preserves all buffer data so that
    /// subsequent writes overwrite in-place, matching C#'s MemoryStream behavior.
    pub fn set_position_in_bits(&mut self, pos_in_bits: i64) {
        let byte_pos = (pos_in_bits / 8) as usize;
        let new_shift = (pos_in_bits % 8) as u8;

        // Commit any pending partial byte at the current write position
        if self.bit_shift > 0 {
            if self.write_pos < self.buffer.len() {
                // Merge: upper bits from last_byte, lower bits preserved
                let mask = !((1u8 << (8 - self.bit_shift)) - 1);
                self.buffer[self.write_pos] =
                    (self.last_byte & mask) | (self.buffer[self.write_pos] & !mask);
            } else {
                while self.buffer.len() < self.write_pos {
                    self.buffer.push(0);
                }
                self.buffer.push(self.last_byte);
            }
        }

        // Move write cursor to new position (no truncation!)
        self.write_pos = byte_pos;
        self.bit_shift = new_shift;

        if new_shift > 0 && byte_pos < self.buffer.len() {
            // Load existing byte's upper bits so subsequent writes merge correctly
            self.last_byte = self.buffer[byte_pos] & (0xFF << (8 - new_shift));
        } else {
            self.last_byte = 0;
        }

        // Extend buffer with zeros if seeking beyond current end
        if byte_pos > self.buffer.len() {
            self.buffer.resize(byte_pos, 0);
        }
    }

    /// Write a flag-encoded position value for the 3-stream text boundary.
    ///
    /// This matches C#'s `SetPositionByFlag` — the continuation flag (0x8000)
    /// is placed on the LAST word(s), not the first. The reader parses backward
    /// from the end of the object data to find the text boundary.
    ///
    /// **Format**:
    /// - `pos < 0x8000`: single 16-bit LE word (no flag)
    /// - `pos < 0x40000000`: two words — upper bits first (no flag), then lower | 0x8000
    /// - `pos >= 0x40000000`: three words — highest (no flag), middle | 0x8000, lowest | 0x8000
    pub fn set_position_by_flag(&mut self, pos: i64) {
        let pos = pos as u64;
        if pos >= 0x8000 {
            if pos >= 0x40000000 {
                // Three words
                self.write_raw_ushort(((pos >> 30) & 0xFFFF) as u16);
                self.write_raw_ushort((((pos >> 15) & 0x7FFF) | 0x8000) as u16);
            } else {
                // Two words — upper portion first (no flag)
                self.write_raw_ushort(((pos >> 15) & 0xFFFF) as u16);
            }
            // Lower portion with flag
            self.write_raw_ushort(((pos & 0x7FFF) | 0x8000) as u16);
        } else {
            // Single word — no flag
            self.write_raw_ushort(pos as u16);
        }
    }

    /// Merge the partial `last_byte` bits with the existing byte at the
    /// current write position. Used after patching size fields.
    ///
    /// Matches C#'s `WriteShiftValue()`: reads the byte at the current
    /// stream position, OR's `_lastByte` with its lower bits, and writes back.
    /// The write position advances by 1 byte. `last_byte` and `bit_shift`
    /// are reset to prevent stale data from being committed by the subsequent
    /// `set_position_in_bits()` call.
    pub fn write_shift_value(&mut self) {
        if self.bit_shift > 0 && self.write_pos < self.buffer.len() {
            let existing = self.buffer[self.write_pos];
            let lower_mask = (1u8 << (8 - self.bit_shift)) - 1; // e.g., shift=3 → 0x1F
            self.buffer[self.write_pos] = self.last_byte | (existing & lower_mask);
            self.write_pos += 1;
        }
        // Always clear partial byte state after shift merge.
        // The data has been committed to the buffer; leaving stale values
        // would cause set_position_in_bits() to re-commit them incorrectly.
        self.last_byte = 0;
        self.bit_shift = 0;
    }

    /// Append another bit writer's buffer to this one.
    ///
    /// Handles the case where both writers may have partial bytes.
    pub fn append(&mut self, other: &DwgBitWriter) {
        if other.buffer.is_empty() && other.bit_shift == 0 {
            return; // Nothing to append
        }

        if self.bit_shift == 0 {
            // Simple case: we're at a byte boundary
            for &b in &other.buffer {
                self.emit_byte(b);
            }
            self.last_byte = other.last_byte;
            self.bit_shift = other.bit_shift;
        } else {
            // Complex case: merge at bit level
            for &b in &other.buffer {
                self.write_byte(b);
            }
            // Handle other's partial byte
            if other.bit_shift > 0 {
                for i in 0..other.bit_shift {
                    let bit = (other.last_byte >> (7 - i)) & 1 != 0;
                    self.write_bit(bit);
                }
            }
        }
    }
}

/// Determine how many bytes are needed to encode a handle value.
fn handle_byte_count(handle: u64) -> u8 {
    if handle == 0 {
        0
    } else if handle < 0x100 {
        1
    } else if handle < 0x10000 {
        2
    } else if handle < 0x100_0000 {
        3
    } else if handle < 0x1_0000_0000 {
        4
    } else if handle < 0x100_0000_0000 {
        5
    } else if handle < 0x1_0000_0000_0000 {
        6
    } else if handle < 0x100_0000_0000_0000 {
        7
    } else {
        8
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> DwgBitWriter {
        DwgBitWriter::new(DwgVersion::AC18, DxfVersion::AC1018)
    }

    // ── Bit operations ──

    #[test]
    fn test_write_single_bits() {
        let mut w = writer();
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(false);
        assert_eq!(w.buffer, vec![0b10110010]);
    }

    #[test]
    fn test_write_bit_position_tracking() {
        let mut w = writer();
        assert_eq!(w.position_in_bits(), 0);
        w.write_bit(true);
        assert_eq!(w.position_in_bits(), 1);
        w.write_bit(false);
        assert_eq!(w.position_in_bits(), 2);
        // Fill the byte
        for _ in 0..6 {
            w.write_bit(false);
        }
        assert_eq!(w.position_in_bits(), 8);
    }

    #[test]
    fn test_write_2bits() {
        let mut w = writer();
        w.write_2bits(3); // 11
        w.write_2bits(1); // 01
        w.write_2bits(2); // 10
        w.write_2bits(0); // 00
        assert_eq!(w.buffer, vec![0b11011000]);
    }

    #[test]
    fn test_write_2bits_straddling() {
        let mut w = writer();
        // Write 7 bits to set bit_shift=7
        for _ in 0..7 {
            w.write_bit(true);
        }
        // Now write 2 bits that straddle the boundary
        w.write_2bits(3); // 11
        assert_eq!(w.buffer, vec![0b11111111]); // First 7 bits + high bit of 3
        assert_eq!(w.bit_shift, 1);
        assert_eq!(w.last_byte, 0b10000000); // Low bit of 3 in MSB
    }

    #[test]
    fn test_write_byte_aligned() {
        let mut w = writer();
        w.write_byte(0xAB);
        assert_eq!(w.buffer, vec![0xAB]);
    }

    #[test]
    fn test_write_byte_misaligned() {
        let mut w = writer();
        w.write_bit(true); // bit_shift=1
        w.write_byte(0xFF);
        // First byte: 1 (our bit) + top 7 bits of 0xFF
        assert_eq!(w.buffer, vec![0xFF]); // 1_1111111
                                          // Remaining bit of 0xFF is in last_byte
        assert_eq!(w.last_byte, 0x80); // 1_0000000
        assert_eq!(w.bit_shift, 1);
    }

    #[test]
    fn test_write_bytes_slice() {
        let mut w = writer();
        w.write_bytes(&[0x01, 0x02, 0x03]);
        assert_eq!(w.buffer, vec![0x01, 0x02, 0x03]);
    }

    // ── Bit-coded types ──

    #[test]
    fn test_bit_short_zero() {
        let mut w = writer();
        w.write_bit_short(0);
        // 2-bit code = 10
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x80); // Top 2 bits = 10
    }

    #[test]
    fn test_bit_short_small() {
        let mut w = writer();
        w.write_bit_short(42);
        // 2-bit code = 01, then 1 byte = 42
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x40); // Top 2 bits = 01
    }

    #[test]
    fn test_bit_short_256() {
        let mut w = writer();
        w.write_bit_short(256);
        // 2-bit code = 11
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0xC0); // Top 2 bits = 11
    }

    #[test]
    fn test_bit_short_full() {
        let mut w = writer();
        w.write_bit_short(1000);
        // 2-bit code = 00, then 2 bytes LE
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x00); // Top 2 bits = 00
    }

    #[test]
    fn test_bit_double_zero() {
        let mut w = writer();
        w.write_bit_double(0.0);
        // 2-bit code = 10
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x80);
    }

    #[test]
    fn test_bit_double_one() {
        let mut w = writer();
        w.write_bit_double(1.0);
        // 2-bit code = 01
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x40);
    }

    #[test]
    fn test_bit_double_other() {
        let mut w = writer();
        w.write_bit_double(3.14);
        // 2-bit code = 00, then 8 bytes
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x00);
        // Total: 2 bits + 64 bits = 66 bits = 9 bytes (with padding)
    }

    #[test]
    fn test_bit_long_zero() {
        let mut w = writer();
        w.write_bit_long(0);
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x80); // code = 10
    }

    #[test]
    fn test_bit_long_small() {
        let mut w = writer();
        w.write_bit_long(100);
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x40); // code = 01
    }

    #[test]
    fn test_bit_long_long_zero() {
        let mut w = writer();
        w.write_bit_long_long(0);
        // 3-bit size = 000
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 1); // Just 3 bits → 1 byte
    }

    #[test]
    fn test_bit_long_long_small() {
        let mut w = writer();
        w.write_bit_long_long(0xFF);
        // 3-bit size = 001, then 1 byte
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 2); // 3 bits + 8 bits = 11 bits → 2 bytes
    }

    // ── Handle encoding ──

    #[test]
    fn test_handle_zero() {
        let mut w = writer();
        w.write_handle(DwgReferenceType::Undefined, 0);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![0x00]); // header: type=0, count=0
    }

    #[test]
    fn test_handle_small() {
        let mut w = writer();
        w.write_handle(DwgReferenceType::SoftPointer, 0x42);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![0x41, 0x42]); // header: type=4, count=1
    }

    #[test]
    fn test_handle_two_bytes() {
        let mut w = writer();
        w.write_handle(DwgReferenceType::HardPointer, 0x1234);
        let bytes = w.into_bytes();
        // header: type=5, count=2 → 0x52, then big-endian: 0x12, 0x34
        assert_eq!(bytes, vec![0x52, 0x12, 0x34]);
    }

    #[test]
    fn test_handle_byte_count() {
        assert_eq!(handle_byte_count(0), 0);
        assert_eq!(handle_byte_count(1), 1);
        assert_eq!(handle_byte_count(0xFF), 1);
        assert_eq!(handle_byte_count(0x100), 2);
        assert_eq!(handle_byte_count(0xFFFF), 2);
        assert_eq!(handle_byte_count(0x10000), 3);
        assert_eq!(handle_byte_count(0x1000000), 4);
        assert_eq!(handle_byte_count(0x100000000), 5);
    }

    // ── Version-specific ──

    #[test]
    fn test_thickness_r2000_zero() {
        let mut w = DwgBitWriter::new(DwgVersion::AC15, DxfVersion::AC1015);
        w.write_bit_thickness(0.0);
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0x80, 0x80); // Single true bit
    }

    #[test]
    fn test_thickness_r2000_nonzero() {
        let mut w = DwgBitWriter::new(DwgVersion::AC15, DxfVersion::AC1015);
        w.write_bit_thickness(1.5);
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0x80, 0x00); // false bit, then BD
    }

    #[test]
    fn test_thickness_r14_always_writes() {
        let mut w = DwgBitWriter::new(DwgVersion::AC12, DxfVersion::AC1014);
        w.write_bit_thickness(0.0);
        let bytes = w.into_bytes();
        // AC12 always writes BD, 0.0 → code 10 → 1 byte
        assert_eq!(bytes[0] & 0xC0, 0x80);
    }

    #[test]
    fn test_extrusion_r2000_default_normal() {
        let mut w = DwgBitWriter::new(DwgVersion::AC15, DxfVersion::AC1015);
        w.write_bit_extrusion(Vector3::new(0.0, 0.0, 1.0));
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0x80, 0x80); // Single true bit
    }

    // ── Object type ──

    #[test]
    fn test_object_type_ac24_small() {
        let mut w = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1024);
        w.write_object_type(17); // LINE
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x00); // 2-bit code = 00
    }

    #[test]
    fn test_object_type_ac24_range() {
        let mut w = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1024);
        w.write_object_type(0x200); // In 0x1F0..=0x2EF range
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x40); // 2-bit code = 01
    }

    // ── Spear shift / alignment ──

    #[test]
    fn test_spear_shift_no_op_at_boundary() {
        let mut w = writer();
        w.write_byte(0xFF);
        w.write_spear_shift();
        assert_eq!(w.buffer, vec![0xFF]);
        assert_eq!(w.bit_shift, 0);
    }

    #[test]
    fn test_spear_shift_pads_partial() {
        let mut w = writer();
        w.write_bit(true);
        w.write_bit(true);
        w.write_spear_shift();
        assert_eq!(w.buffer, vec![0xC0]); // 11000000
        assert_eq!(w.bit_shift, 0);
    }

    // ── Default double ──

    #[test]
    fn test_bit_double_with_default_same() {
        let mut w = writer();
        w.write_bit_double_with_default(3.14, 3.14);
        let bytes = w.into_bytes();
        assert_eq!(bytes[0] & 0xC0, 0x00); // code = 00 (use default)
    }

    #[test]
    fn test_bit_double_with_default_different() {
        let mut w = writer();
        w.write_bit_double_with_default(0.0, 999.999);
        let bytes = w.into_bytes();
        // Should write code 11 (full double) since 0.0 and 999.999 share no bytes
        assert_eq!(bytes[0] & 0xC0, 0xC0);
    }

    // ── Modular encodings ──

    #[test]
    fn test_modular_char_small() {
        let mut w = writer();
        w.write_modular_char(42);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![42]); // < 128, single byte
    }

    #[test]
    fn test_modular_char_large() {
        let mut w = writer();
        w.write_modular_char(0x100);
        let bytes = w.into_bytes();
        // 0x100 = 256 = 0b10_0000000
        // First byte: 0b10000000 | 0 = 0x80 (continuation + 0)
        // Second byte: 0b00000010 = 2
        assert_eq!(bytes, vec![0x80, 0x02]);
    }

    // ── Text ──

    #[test]
    fn test_variable_text_empty() {
        let mut w = writer();
        w.write_variable_text("");
        let bytes = w.into_bytes();
        // BS(0) = code 10 → 1 byte
        assert_eq!(bytes[0] & 0xC0, 0x80);
    }

    #[test]
    fn test_variable_text_simple() {
        let mut w = DwgBitWriter::new(DwgVersion::AC15, DxfVersion::AC1015);
        w.write_variable_text("Hi");
        let bytes = w.into_bytes();
        // BS(2) = code 01 + byte(2), then 'H'=0x48, 'i'=0x69
        assert!(bytes.len() >= 4);
    }

    // ── Append ──

    #[test]
    fn test_append_aligned() {
        let mut w1 = writer();
        w1.write_byte(0xAA);

        let mut w2 = writer();
        w2.write_byte(0xBB);

        w1.append(&w2);
        assert_eq!(w1.buffer, vec![0xAA, 0xBB]);
    }

    #[test]
    fn test_append_misaligned() {
        let mut w1 = writer();
        w1.write_bit(true); // 1 bit

        let mut w2 = writer();
        w2.write_byte(0xFF); // 8 bits

        w1.append(&w2);
        // Total: 9 bits → 2 bytes
        let bytes = w1.into_bytes();
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[0], 0xFF); // 1 + top 7 of 0xFF
    }

    // ── Save/seek ──

    #[test]
    fn test_save_position_for_size() {
        let mut w = writer();
        w.write_bit_short(42);
        w.save_position_for_size();
        assert!(w.saved_position_in_bits() >= 0);
        // Should have written 4 zero bytes as placeholder
        w.write_bit_short(99);
        let total_bits = w.position_in_bits();
        assert!(total_bits > 32); // At least the placeholder + some data
    }

    #[test]
    fn test_set_position_preserves_data() {
        // Critical test: seeking backward must NOT destroy buffer data.
        let mut w = writer();
        w.write_byte(0xAA); // byte 0
        w.write_byte(0xBB); // byte 1
        w.write_byte(0xCC); // byte 2
        w.write_byte(0xDD); // byte 3

        // Seek back to byte 1
        w.set_position_in_bits(8);
        // Write a different value
        w.write_byte(0xFF);
        // Seek forward to byte 4 (end)
        w.set_position_in_bits(32);

        let bytes = w.to_bytes();
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes[0], 0xAA); // unchanged
        assert_eq!(bytes[1], 0xFF); // overwritten
        assert_eq!(bytes[2], 0xCC); // preserved!
        assert_eq!(bytes[3], 0xDD); // preserved!
    }

    #[test]
    fn test_seek_patch_size_roundtrip() {
        // Simulates the merge size-patching flow:
        // 1. Write some data
        // 2. Save position, write 4-byte placeholder
        // 3. Write more data
        // 4. Seek back to saved position, write actual size
        // 5. Seek forward to data end
        // 6. Verify all data is intact
        let mut w = writer();
        w.write_byte(0x01); // byte 0
        let saved = w.position_in_bits(); // bit 8
        w.write_int(0); // bytes 1-4: placeholder
        w.write_byte(0x02); // byte 5
        w.write_byte(0x03); // byte 6
        let end_pos = w.position_in_bits(); // bit 56

        // Patch the size
        w.set_position_in_bits(saved);
        w.write_raw_long(0x12345678);
        w.write_shift_value(); // no-op (byte-aligned)

        // Seek back to end
        w.set_position_in_bits(end_pos);

        let bytes = w.to_bytes();
        assert_eq!(bytes.len(), 7);
        assert_eq!(bytes[0], 0x01);
        // Size field (LE): 0x78, 0x56, 0x34, 0x12
        assert_eq!(bytes[1], 0x78);
        assert_eq!(bytes[2], 0x56);
        assert_eq!(bytes[3], 0x34);
        assert_eq!(bytes[4], 0x12);
        assert_eq!(bytes[5], 0x02); // preserved
        assert_eq!(bytes[6], 0x03); // preserved
    }

    #[test]
    fn test_set_position_by_flag_small() {
        // pos < 0x8000: single 16-bit LE word, no flag
        let mut w = writer();
        w.set_position_by_flag(100);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![100, 0]); // 100 as u16 LE
    }

    #[test]
    fn test_set_position_by_flag_large() {
        // pos >= 0x8000: two words.
        // Upper portion first (no flag), then lower | 0x8000.
        let mut w = writer();
        w.set_position_by_flag(0x10000);
        let bytes = w.into_bytes();
        // 0x10000 >> 15 = 2, 0x10000 & 0x7FFF = 0
        // Word 1: 0x0002 LE → [0x02, 0x00]
        // Word 2: 0x0000 | 0x8000 = 0x8000 LE → [0x00, 0x80]
        assert_eq!(bytes, vec![0x02, 0x00, 0x00, 0x80]);
    }

    #[test]
    fn test_modular_short_small() {
        let mut w = writer();
        w.write_modular_short(100);
        let bytes = w.into_bytes();
        // < 0x8000: simple 16-bit LE
        assert_eq!(bytes, vec![100, 0]);
    }

    #[test]
    fn test_modular_short_large() {
        let mut w = writer();
        w.write_modular_short(0x10000); // 65536
        let bytes = w.into_bytes();
        // >= 0x8000: 4 bytes with continuation flag in byte 1.
        // lo = 0x10000 & 0xFF = 0x00
        // mid = ((0x10000 >> 8) & 0x7F) | 0x80 = (0x00 & 0x7F) | 0x80 = 0x80
        // hi_lo = (0x10000 >> 15) & 0xFF = 0x02
        // hi_hi = (0x10000 >> 23) & 0xFF = 0x00
        assert_eq!(bytes, vec![0x00, 0x80, 0x02, 0x00]);
    }

    #[test]
    fn test_write_shift_value_resets_state() {
        let mut w = writer();
        w.write_byte(0xAA);
        w.write_byte(0xBB);
        // Seek to byte 0, bit 4 (mid-byte)
        w.set_position_in_bits(4);
        // Write something
        w.write_byte(0xFF);
        // Now write_shift_value to merge partial byte at current position
        w.write_shift_value();
        // After write_shift_value, bit_shift should be 0
        assert_eq!(w.bit_shift, 0);
        assert_eq!(w.last_byte, 0);
    }
}
