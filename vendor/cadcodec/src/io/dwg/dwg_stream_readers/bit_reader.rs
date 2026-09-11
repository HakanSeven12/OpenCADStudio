//! DWG bit-level binary reader
//!
//! The DWG format operates at bit granularity, not byte granularity.
//! This reader tracks the current bit position and handles all the
//! DWG-specific variable-length decodings.
//!
//! Based on the reference `DwgStreamReaderBase` and version-specific subclasses
//! (AC12, AC15, AC18, AC21, AC24).

use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::{Color, DxfVersion, Transparency, Vector2, Vector3};
use std::sync::Arc;

/// Bit-level reader for the DWG binary format.
///
/// Reads bits from a byte buffer using MSB-first packing within each byte.
/// Mirrors `DwgBitWriter` on the write side.
///
/// # Internal state
/// - `data`: The source byte buffer
/// - `position`: Current byte read position in the stream
/// - `bit_shift`: Current bit offset within the current byte (0–7)
/// - `last_byte`: The most recently read byte from the stream
pub struct DwgBitReader {
    /// Source data buffer.
    data: Arc<[u8]>,
    /// Current stream byte position (points past the last byte read via advance_byte).
    position: usize,
    /// Current bit offset within `last_byte` (0 = no bits consumed yet from last_byte)
    bit_shift: u8,
    /// The most recently read byte from the stream.
    last_byte: u8,
    /// DWG stream version (determines decoding behavior).
    version: DwgVersion,
    /// Exact DXF version (for R2013+/R2018+ sub-version checks).
    dxf_version: DxfVersion,
    /// Text encoding for non-Unicode strings.
    encoding: &'static encoding_rs::Encoding,
    /// Whether the text stream is empty (R2007+ flag).
    pub is_empty: bool,
    /// Text stream bit position for R2007+ three-stream merge.
    /// -1 = no separate text stream (read inline).
    text_stream_pos: i64,
    /// One-past-the-end bit position of the R2007+ text stream.
    text_stream_end_pos: i64,
}

impl DwgBitReader {
    /// Create a new bit reader over the given data buffer.
    pub fn new(data: Vec<u8>, version: DwgVersion, dxf_version: DxfVersion) -> Self {
        Self::from_shared(data.into(), version, dxf_version)
    }

    /// Create a reader sharing immutable bytes with sibling stream readers.
    /// Object records have main/text/handle cursors over identical storage;
    /// sharing avoids three full record copies per cursor set.
    pub fn from_shared(data: Arc<[u8]>, version: DwgVersion, dxf_version: DxfVersion) -> Self {
        Self {
            data,
            position: 0,
            bit_shift: 0,
            last_byte: 0,
            version,
            dxf_version,
            encoding: encoding_rs::WINDOWS_1252,
            is_empty: false,
            text_stream_pos: -1,
            text_stream_end_pos: -1,
        }
    }

    pub fn from_shared_with_encoding(
        data: Arc<[u8]>,
        version: DwgVersion,
        dxf_version: DxfVersion,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        let mut reader = Self::from_shared(data, version, dxf_version);
        reader.encoding = encoding;
        reader
    }

    /// Create a new bit reader with a specific encoding.
    pub fn with_encoding(
        data: Vec<u8>,
        version: DwgVersion,
        dxf_version: DxfVersion,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        let mut r = Self::new(data, version, dxf_version);
        r.encoding = encoding;
        r
    }

    /// Decode bytes using the document's legacy text code page.
    pub fn decode_legacy_text(&self, bytes: &[u8]) -> String {
        let (decoded, _, _) = self.encoding.decode(bytes);
        crate::io::dxf::code_page::decode_mif_escapes(&decoded)
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
        // After advance_byte, position points past the byte we read.
        // The actual bit position is (position * 8) adjusted for bit_shift.
        let byte_pos = self.position as i64 * 8;
        if self.bit_shift > 0 {
            byte_pos + self.bit_shift as i64 - 8
        } else {
            byte_pos
        }
    }

    /// Current byte position in the stream.
    pub fn stream_position(&self) -> usize {
        self.position
    }

    /// Current byte position (alias for stream_position).
    pub fn position(&self) -> usize {
        self.position
    }

    /// Total length of the underlying data buffer.
    pub fn data_len(&self) -> usize {
        self.data.len()
    }

    /// Clone the entire underlying data buffer.
    ///
    /// Used to extract the raw merged-stream record bytes for
    /// round-trip preservation of unknown entities.
    pub fn data_bytes(&self) -> Vec<u8> {
        self.data.as_ref().to_vec()
    }

    /// Set the stream position (byte-level) and reset bit shift.
    pub fn set_position(&mut self, pos: usize) {
        self.position = pos;
        self.bit_shift = 0;
    }

    /// Set position by bit offset.
    pub fn set_position_in_bits(&mut self, pos: i64) {
        self.position = (pos >> 3) as usize;
        self.bit_shift = (pos & 7) as u8;

        if self.bit_shift > 0 {
            self.advance_byte();
        }
    }

    /// Length of the underlying data buffer.
    pub fn length(&self) -> usize {
        self.data.len()
    }

    /// Read the next raw byte from the stream into `last_byte`.
    pub fn advance_byte(&mut self) {
        if self.position < self.data.len() {
            self.last_byte = self.data[self.position];
            self.position += 1;
        } else {
            self.last_byte = 0;
        }
    }

    /// Advance by `offset` bytes, reading the last one into `last_byte`.
    pub fn advance(&mut self, offset: usize) {
        if offset > 1 {
            self.position += offset - 1;
        }
        self.advance_byte(); // reads byte at position and increments
    }

    /// Read a single byte, applying the current bit shift.
    ///
    /// Port of `DwgStreamReaderBase.ReadByte`.
    pub fn read_byte(&mut self) -> u8 {
        if self.bit_shift == 0 {
            self.advance_byte();
            self.last_byte
        } else {
            let last_values = (self.last_byte as u32) << self.bit_shift;
            self.advance_byte();
            (last_values | ((self.last_byte as u32) >> (8 - self.bit_shift))) as u8
        }
    }

    /// Read `length` bytes, applying the current bit shift.
    pub fn read_bytes(&mut self, length: usize) -> Vec<u8> {
        let mut arr = vec![0u8; length];
        self.apply_shift_to_arr(&mut arr);
        arr
    }

    /// Read 16 sentinel bytes.
    pub fn read_sentinel(&mut self) -> Vec<u8> {
        self.read_bytes(16)
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Bit-level primitives
    // ════════════════════════════════════════════════════════════════════════

    /// Read a single bit (B type).
    pub fn read_bit(&mut self) -> bool {
        if self.bit_shift == 0 {
            self.advance_byte();
            let result = (self.last_byte & 128) == 128;
            self.bit_shift = 1;
            result
        } else {
            let value = ((self.last_byte as u32) << self.bit_shift & 128) == 128;
            self.bit_shift += 1;
            self.bit_shift &= 7;
            value
        }
    }

    /// Read a single bit and return as i16 (0 or 1).
    pub fn read_bit_as_short(&mut self) -> i16 {
        if self.read_bit() {
            1
        } else {
            0
        }
    }

    /// Read a 2-bit value (BB type).
    pub fn read_2bits(&mut self) -> u8 {
        if self.bit_shift == 0 {
            self.advance_byte();
            let value = self.last_byte >> 6;
            self.bit_shift = 2;
            value
        } else if self.bit_shift == 7 {
            let last_value = (self.last_byte << 1) & 2;
            self.advance_byte();
            let value = last_value | (self.last_byte >> 7);
            self.bit_shift = 1;
            value
        } else {
            let value = (self.last_byte >> (6 - self.bit_shift)) & 3;
            self.bit_shift += 2;
            self.bit_shift &= 7;
            value
        }
    }

    /// Read a 3-bit value (3B type, used for BLL size prefix).
    fn read_3bits(&mut self) -> u8 {
        let mut b = 0u8;
        if self.read_bit() {
            b = 1;
        }
        b <<= 1;
        if self.read_bit() {
            b |= 1;
        }
        b <<= 1;
        if self.read_bit() {
            b |= 1;
        }
        b
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Variable-length integer types
    // ════════════════════════════════════════════════════════════════════════

    /// Read a BitShort (BS type) — variable-length i16.
    pub fn read_bit_short(&mut self) -> i16 {
        match self.read_2bits() {
            0 => {
                // 00: A short (2 bytes) follows, little-endian
                self.read_raw_short()
            }
            1 => {
                // 01: An unsigned char (1 byte) follows
                self.read_byte() as i16
            }
            2 => {
                // 10: 0
                0
            }
            3 => {
                // 11: 256
                256
            }
            _ => unreachable!(),
        }
    }

    /// Read a BitShort and interpret as bool (non-zero = true).
    pub fn read_bit_short_as_bool(&mut self) -> bool {
        self.read_bit_short() != 0
    }

    /// Read a BitLong (BL type) — variable-length i32.
    pub fn read_bit_long(&mut self) -> i32 {
        match self.read_2bits() {
            0 => {
                // 00: A long (4 bytes) follows, little-endian
                self.read_raw_long() as i32
            }
            1 => {
                // 01: An unsigned char (1 byte) follows
                self.read_byte() as i32
            }
            2 => {
                // 10: 0
                0
            }
            _ => {
                // 11: not used
                0 // graceful fallback
            }
        }
    }

    /// Read a BitLongLong (BLL type) — variable-length i64.
    pub fn read_bit_long_long(&mut self) -> i64 {
        let size = self.read_3bits();
        let mut value: u64 = 0;
        for i in 0..size {
            let b = self.read_byte() as u64;
            value += b << ((i as u64) << 3);
        }
        value as i64
    }

    /// Read a BitDouble (BD type) — variable-length f64.
    pub fn read_bit_double(&mut self) -> f64 {
        match self.read_2bits() {
            0 => {
                // 00: A full IEEE double follows
                self.read_raw_double()
            }
            1 => {
                // 01: 1.0
                1.0
            }
            2 => {
                // 10: 0.0
                0.0
            }
            _ => {
                // 11: not used
                0.0
            }
        }
    }

    /// Read a BitDouble with a default value (DD type).
    ///
    /// Differential encoding that compares with a default, patching bytes.
    pub fn read_bit_double_with_default(&mut self, def: f64) -> f64 {
        let mut arr = def.to_le_bytes();
        match self.read_2bits() {
            0 => {
                // 00: No data, use default
                def
            }
            1 => {
                // 01: 4 bytes follow, replacing bytes 0–3 of default
                arr[0] = self.read_byte();
                arr[1] = self.read_byte();
                arr[2] = self.read_byte();
                arr[3] = self.read_byte();
                f64::from_le_bytes(arr)
            }
            2 => {
                // 10: 6 bytes follow — 2 replace bytes 4–5, 4 replace bytes 0–3
                arr[4] = self.read_byte();
                arr[5] = self.read_byte();
                arr[0] = self.read_byte();
                arr[1] = self.read_byte();
                arr[2] = self.read_byte();
                arr[3] = self.read_byte();
                f64::from_le_bytes(arr)
            }
            3 => {
                // 11: A full RD follows
                self.read_raw_double()
            }
            _ => unreachable!(),
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Raw types (no variable-length encoding)
    // ════════════════════════════════════════════════════════════════════════

    /// Read a raw char (RC type) — same as read_byte but returns as char.
    pub fn read_raw_char(&mut self) -> u8 {
        self.read_byte()
    }

    /// Read a raw short (RS type) — 2 bytes, little-endian.
    pub fn read_raw_short(&mut self) -> i16 {
        let b0 = self.read_byte() as u16;
        let b1 = self.read_byte() as u16;
        (b0 | (b1 << 8)) as i16
    }

    /// Read a raw unsigned short.
    pub fn read_raw_ushort(&mut self) -> u16 {
        let b0 = self.read_byte() as u16;
        let b1 = self.read_byte() as u16;
        b0 | (b1 << 8)
    }

    /// Read a raw long (RL type) — 4 bytes, little-endian, returns as i64 (matching the reference ReadRawLong → long).
    pub fn read_raw_long(&mut self) -> i64 {
        let b0 = self.read_byte() as u32;
        let b1 = self.read_byte() as u32;
        let b2 = self.read_byte() as u32;
        let b3 = self.read_byte() as u32;
        (b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)) as i32 as i64
    }

    /// Read a raw unsigned long (RD is actually 4 bytes in the reference ReadRawULong).
    pub fn read_raw_ulong(&mut self) -> u64 {
        let b0 = self.read_byte() as u64;
        let b1 = self.read_byte() as u64;
        let b2 = self.read_byte() as u64;
        let b3 = self.read_byte() as u64;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Read a raw double (RD type) — 8 bytes, little-endian IEEE 754.
    pub fn read_raw_double(&mut self) -> f64 {
        let bytes = self.read_bytes(8);
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&bytes);
        f64::from_le_bytes(arr)
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Composite point types
    // ════════════════════════════════════════════════════════════════════════

    /// Read 2 BitDoubles as an XY point (2BD type).
    pub fn read_2bit_double(&mut self) -> Vector2 {
        let x = self.read_bit_double();
        let y = self.read_bit_double();
        Vector2::new(x, y)
    }

    /// Read 3 BitDoubles as an XYZ point (3BD type).
    pub fn read_3bit_double(&mut self) -> Vector3 {
        let x = self.read_bit_double();
        let y = self.read_bit_double();
        let z = self.read_bit_double();
        Vector3::new(x, y, z)
    }

    /// Read 2 raw doubles as an XY point (2RD type).
    pub fn read_2raw_double(&mut self) -> Vector2 {
        let x = self.read_raw_double();
        let y = self.read_raw_double();
        Vector2::new(x, y)
    }

    /// Read 3 raw doubles as an XYZ point (3RD type).
    pub fn read_3raw_double(&mut self) -> Vector3 {
        let x = self.read_raw_double();
        let y = self.read_raw_double();
        let z = self.read_raw_double();
        Vector3::new(x, y, z)
    }

    /// Read 2 BitDoubles with defaults (2DD type).
    pub fn read_2bit_double_with_default(&mut self, def: Vector2) -> Vector2 {
        let x = self.read_bit_double_with_default(def.x);
        let y = self.read_bit_double_with_default(def.y);
        Vector2::new(x, y)
    }

    /// Read 3 BitDoubles with defaults (3DD type).
    pub fn read_3bit_double_with_default(&mut self, def: Vector3) -> Vector3 {
        let x = self.read_bit_double_with_default(def.x);
        let y = self.read_bit_double_with_default(def.y);
        let z = self.read_bit_double_with_default(def.z);
        Vector3::new(x, y, z)
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Extrusion & Thickness (version-dependent)
    // ════════════════════════════════════════════════════════════════════════

    /// Read a BitExtrusion (BE type).
    ///
    /// - R13/R14: 3BD
    /// - R2000+: single bit; if 1 → (0,0,1), if 0 → 3BD
    pub fn read_bit_extrusion(&mut self) -> Vector3 {
        if self.dxf_version >= DxfVersion::AC1015 {
            // R2000+: optimized
            if self.read_bit() {
                Vector3::new(0.0, 0.0, 1.0)
            } else {
                self.read_3bit_double()
            }
        } else {
            // R13/R14: always 3BD
            self.read_3bit_double()
        }
    }

    /// Read a BitThickness (BT type).
    ///
    /// - R13/R14: BD
    /// - R2000+: single bit; if 1 → 0.0, if 0 → BD
    pub fn read_bit_thickness(&mut self) -> f64 {
        if self.dxf_version >= DxfVersion::AC1015 {
            // R2000+: optimized
            if self.read_bit() {
                0.0
            } else {
                self.read_bit_double()
            }
        } else {
            // R13/R14: always BD
            self.read_bit_double()
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Modular encodings
    // ════════════════════════════════════════════════════════════════════════

    /// Read a ModularChar (MC type) — unsigned variable-length integer.
    pub fn read_modular_char(&mut self) -> u64 {
        let mut shift = 0;
        let last_byte = self.read_byte();
        let mut value = (last_byte & 0x7F) as u64;

        if (last_byte & 0x80) != 0 {
            loop {
                shift += 7;
                let b = self.read_byte();
                value |= ((b & 0x7F) as u64) << shift;
                if (b & 0x80) == 0 {
                    break;
                }
            }
        }

        value
    }

    /// Read a SignedModularChar (SMC type) — signed variable-length integer.
    ///
    /// Used in handle section for file offset deltas.
    pub fn read_signed_modular_char(&mut self) -> i64 {
        let first_byte = self.read_byte();

        if (first_byte & 0x80) == 0 {
            // Single byte: bits 0–5 = value, bit 6 = sign
            let value = (first_byte & 0x3F) as i64;
            if (first_byte & 0x40) != 0 {
                -value
            } else {
                value
            }
        } else {
            // Multi-byte
            let mut total_shift = 0;
            let mut sum = (first_byte & 0x7F) as i64;

            loop {
                total_shift += 7;
                let b = self.read_byte();
                if (b & 0x80) != 0 {
                    sum |= ((b & 0x7F) as i64) << total_shift;
                } else {
                    // Last byte: bits 0–5 = value, bit 6 = sign
                    let value = sum | (((b & 0x3F) as i64) << total_shift);
                    return if (b & 0x40) != 0 { -value } else { value };
                }
            }
        }
    }

    /// Read a ModularShort (MS type) — 2-byte chunks with continuation flag.
    pub fn read_modular_short(&mut self) -> i32 {
        let mut shift = 15; // starts at bit 15 for 2nd chunk onwards

        let b1 = self.read_byte();
        let b2 = self.read_byte();

        let mut flag = (b2 & 0x80) == 0;
        let mut value = (b1 as i32) | (((b2 & 0x7F) as i32) << 8);

        while !flag {
            let b1 = self.read_byte();
            let b2 = self.read_byte();
            flag = (b2 & 0x80) == 0;
            value |= (b1 as i32) << shift;
            shift += 8;
            value |= ((b2 & 0x7F) as i32) << shift;
            shift += 7;
        }

        value
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Handle references
    // ════════════════════════════════════════════════════════════════════════

    /// Read a handle reference (H type) as an absolute handle.
    pub fn read_handle(&mut self) -> u64 {
        self.read_handle_reference(0, &mut DwgReferenceType::SoftOwnership)
    }

    /// Read a handle reference relative to a reference handle.
    pub fn read_handle_relative(&mut self, reference_handle: u64) -> u64 {
        self.read_handle_reference(reference_handle, &mut DwgReferenceType::SoftOwnership)
    }

    /// Read a handle reference, returning the resolved handle and reference type.
    pub fn read_handle_reference(
        &mut self,
        reference_handle: u64,
        ref_type: &mut DwgReferenceType,
    ) -> u64 {
        // |CODE (4 bits)|COUNTER (4 bits)|HANDLE or OFFSET|
        let form = self.read_byte();
        let code = form >> 4;
        let counter = (form & 0x0F) as usize;

        *ref_type = match code {
            0 => DwgReferenceType::Undefined,
            2 => DwgReferenceType::SoftOwnership,
            3 => DwgReferenceType::HardOwnership,
            4 => DwgReferenceType::SoftPointer,
            5 => DwgReferenceType::HardPointer,
            _ => DwgReferenceType::Undefined,
        };

        if code <= 0x5 {
            // 0x2..0x5: just read offset and use it as result
            self.read_handle_bytes(counter)
        } else if code == 0x6 {
            // result is reference_handle + 1
            reference_handle.wrapping_add(1)
        } else if code == 0x8 {
            // result is reference_handle - 1
            reference_handle.wrapping_sub(1)
        } else if code == 0xA {
            // result is reference_handle + offset
            let offset = self.read_handle_bytes(counter);
            reference_handle.wrapping_add(offset)
        } else if code == 0xC {
            // result is reference_handle - offset
            let offset = self.read_handle_bytes(counter);
            reference_handle.wrapping_sub(offset)
        } else {
            // Invalid code — return 0
            0
        }
    }

    /// Read `length` big-endian handle bytes and convert to u64.
    fn read_handle_bytes(&mut self, length: usize) -> u64 {
        // Handle values are at most 8 bytes (u64). If counter > 8
        // we're reading corrupt data; clamp to avoid panics.
        let length = length.min(8);
        let raw = self.read_bytes(length);
        // Convert from big-endian to u64
        let mut arr = [0u8; 8];
        for i in 0..length {
            arr[length - 1 - i] = raw[i];
        }
        u64::from_le_bytes(arr)
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Text / String types (version-dependent)
    // ════════════════════════════════════════════════════════════════════════

    /// Read a TextUnicode (TU type).
    ///
    /// - Pre-R2007: RS length + RC encoding key + encoded bytes
    /// - R2007+: RS char count + UTF-16LE (char_count * 2 bytes)
    pub fn read_text_unicode(&mut self) -> String {
        if self.dxf_version >= DxfVersion::AC1021 {
            // R2007+: RS char_count, then char_count * 2 bytes of UTF-16LE
            let char_count = self.read_raw_short();
            if char_count <= 0 {
                return String::new();
            }
            let byte_count = (char_count as usize) * 2;
            let bytes = self.read_bytes(byte_count);
            // Decode UTF-16LE
            let utf16: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&utf16).replace('\0', "")
        } else {
            // Pre-R2007: RS length + RC encoding + bytes
            let text_length = self.read_raw_short();
            if text_length <= 0 {
                return String::new();
            }
            let _encoding_key = self.read_byte();
            let bytes = self.read_bytes(text_length as usize);
            // Decode using the reader's encoding; legacy strings may embed
            // MIF \U+XXXX escapes for characters outside the code page.
            let (decoded, _, _) = self.encoding.decode(&bytes);
            crate::io::dxf::code_page::decode_mif_escapes(&decoded)
        }
    }

    /// Read a VariableText (TV type).
    ///
    /// - Pre-R2007: BS length + encoded bytes
    /// - R2007+: BS char_count + UTF-16LE (char_count * 2 bytes)
    pub fn read_variable_text(&mut self) -> String {
        if self.dxf_version >= DxfVersion::AC1021 {
            // R2007+: If we have a separate text stream, read from it.
            // The ENTIRE variable text (BS char_count + UTF-16LE) is in the text stream.
            if self.text_stream_pos >= 0 {
                // Save main stream position
                let saved_pos = self.position_in_bits();
                // Switch to text stream
                self.set_position_in_bits(self.text_stream_pos);
                // Read BS char_count + UTF-16LE data from text stream
                let char_count = self.read_bit_short();
                let result = if char_count <= 0 {
                    String::new()
                } else {
                    let byte_count = (char_count as usize) * 2;
                    let bytes = self.read_bytes(byte_count);
                    let utf16: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&utf16).replace('\0', "")
                };
                // Save updated text stream position
                self.text_stream_pos = self.position_in_bits();
                // Restore main stream position
                self.set_position_in_bits(saved_pos);
                result
            } else {
                // No separate text stream — read inline
                let char_count = self.read_bit_short();
                if char_count <= 0 {
                    return String::new();
                }
                let byte_count = (char_count as usize) * 2;
                let bytes = self.read_bytes(byte_count);
                let utf16: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&utf16).replace('\0', "")
            }
        } else {
            // Pre-R2007: BS length + encoded bytes
            let length = self.read_bit_short();
            if length <= 0 {
                return String::new();
            }
            let bytes = self.read_bytes(length as usize);
            let (decoded, _, _) = self.encoding.decode(&bytes);
            // Legacy strings may embed MIF \U+XXXX escapes for characters
            // outside the code page — decode them into Unicode chars.
            crate::io::dxf::code_page::decode_mif_escapes(&decoded.replace('\0', ""))
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Color types (version-dependent)
    // ════════════════════════════════════════════════════════════════════════

    /// Read a CmColor (CMC type).
    ///
    /// - Pre-R2004: BS color index
    /// - R2004+: BS color_index + BL rgb + RC flags + optional TV names
    pub fn read_cm_color(&mut self) -> Color {
        if self.dxf_version >= DxfVersion::AC1018 {
            // R2004+ (AC18+)
            let _color_index = self.read_bit_short();
            let rgb = self.read_bit_long() as u32;
            let arr = rgb.to_le_bytes();

            let color = if rgb == 0xC000_0000 {
                Color::ByLayer
            } else if rgb == 0xC800_0000 {
                Color::None
            } else if (rgb & 0x0100_0000) != 0 {
                // Indexed color
                Color::from_index(arr[0] as i16)
            } else {
                // True color
                Color::from_rgb(arr[2], arr[1], arr[0])
            };

            // RC: color byte flags
            let id = self.read_byte();

            // &1 => color name follows (TV)
            if (id & 1) == 1 {
                let _color_name = self.read_variable_text();
            }
            // &2 => book name follows (TV)
            if (id & 2) == 2 {
                let _book_name = self.read_variable_text();
            }

            color
        } else {
            // Pre-R2004: BS color index
            let index = self.read_bit_short();
            Color::from_index(index)
        }
    }

    /// Read an EnColor (entity color with transparency).
    ///
    /// Returns (Color, Transparency, bool has_color_handle).
    pub fn read_en_color(&mut self) -> (Color, Transparency, bool) {
        if self.dxf_version >= DxfVersion::AC1018 {
            // R2004+
            let size = self.read_bit_short();
            let mut transparency = Transparency::BY_LAYER;

            if size != 0 {
                let flags = (size as u16) & 0xFF00;

                // 0x4000: AcDbColor handle reference (read from the handle
                // section by the caller). When present, NO rgb value follows
                // in the data stream.
                let is_book_color = (flags & 0x4000) > 0;

                // A true/complex color rgb (BL) is present ONLY when the color
                // is not a book-color reference (0x4000) but has the 0x8000
                // flag. A book color carries its rgb via the handle instead.
                //
                // Field order in the data stream is rgb/color FIRST, then the
                // transparency BL. (An entity with both a true colour and a
                // transparency has flags 0x8000|0x2000 = 0xA000; the rgb long
                // precedes the transparency long.)
                let color = if is_book_color {
                    Color::from_index((size & 0x0FFF) as i16)
                } else if (flags & 0x8000) > 0 {
                    // Complex/true color BL follows in main stream.
                    // The BL uses the same packed format as CMC:
                    //   0xC0000000 = ByLayer, flag byte bit 0 set = ACI index,
                    //   otherwise = true RGB color.
                    let rgb = self.read_bit_long() as u32;
                    let arr = rgb.to_le_bytes();
                    if rgb == 0xC000_0000 {
                        Color::ByLayer
                    } else if (rgb & 0x0100_0000) != 0 {
                        // Indexed color (flag byte 0xC3 has bit 0 set)
                        Color::from_index(arr[0] as i16)
                    } else {
                        // True color RGB (flag byte 0xC2)
                        Color::from_rgb(arr[2], arr[1], arr[0])
                    }
                } else {
                    // ACI color index (lower 12 bits)
                    Color::from_index((size & 0x0FFF) as i16)
                };

                // 0x2000: transparency (BL) — read AFTER the rgb/color-book
                // value, matching the entity ENC field order.
                if (flags & 0x2000) > 0 {
                    let value = self.read_bit_long();
                    transparency = Transparency::from_alpha_value(value as u32);
                }

                (color, transparency, is_book_color)
            } else {
                (Color::ByBlock, Transparency::BY_LAYER, false)
            }
        } else {
            // Pre-R2004
            let color_number = self.read_bit_short();
            (
                Color::from_index(color_number),
                Transparency::BY_LAYER,
                false,
            )
        }
    }

    /// Read a color by index (BS → Color).
    pub fn read_color_by_index(&mut self) -> Color {
        Color::from_index(self.read_bit_short())
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Object type (version-dependent)
    // ════════════════════════════════════════════════════════════════════════

    /// Read an ObjectType (OT type).
    ///
    /// - Pre-R2007: BS
    /// - R2010+: BB + 1 or 2 bytes
    pub fn read_object_type(&mut self) -> i16 {
        if self.dxf_version >= DxfVersion::AC1024 {
            // R2010+ (AC24+): BB + conditional bytes
            let pair = self.read_2bits();
            match pair {
                0 => {
                    // Read following byte
                    self.read_byte() as i16
                }
                1 => {
                    // Read following byte + 0x1F0
                    0x1F0 + self.read_byte() as i16
                }
                2 | 3 => {
                    // Read following two bytes (raw short)
                    self.read_raw_short()
                }
                _ => unreachable!(),
            }
        } else {
            // Pre-R2010: BS
            self.read_bit_short()
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Date/Time types
    // ════════════════════════════════════════════════════════════════════════

    /// Read a DateTime as two BitLongs (julian date + milliseconds).
    pub fn read_datetime_julian(&mut self) -> f64 {
        let jdate = self.read_bit_long();
        let millis = self.read_bit_long();
        // Convert julian to unix-like and add millis
        let unix_time = (jdate as f64 - 2440587.5) * 86400.0;
        unix_time + (millis as f64 / 1000.0)
    }

    /// Read a TimeSpan as two BitLongs (hours + milliseconds).
    pub fn read_timespan(&mut self) -> f64 {
        let hours = self.read_bit_long() as f64;
        let millis = self.read_bit_long() as f64;
        hours * 3600.0 + millis / 1000.0
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Stream position control (R2007+ string stream support)
    // ════════════════════════════════════════════════════════════════════════

    /// Set position by flag — reads string stream flag and computes
    /// the start of the string data section.
    ///
    /// Returns the bit position where the string data starts.
    pub fn set_position_by_flag(&mut self, position: i64) -> i64 {
        self.set_position_in_bits(position);

        // String stream present bit
        let flag = self.read_bit();

        if flag {
            // String stream present
            let (length, size) = self.apply_flag_to_position(position);
            let start_pos = length - size;
            self.text_stream_pos = start_pos;
            self.text_stream_end_pos = length;
            self.set_position_in_bits(start_pos);
            start_pos
        } else {
            // No string stream — mark as empty and go to end
            self.is_empty = true;
            self.text_stream_pos = -1;
            self.text_stream_end_pos = position;
            self.position = self.data.len();
            self.bit_shift = 0;
            position
        }
    }

    /// Bits remaining in the separate R2007+ text stream.
    pub fn text_stream_remaining_bits(&self) -> i64 {
        if self.text_stream_pos < 0 || self.text_stream_end_pos < 0 {
            0
        } else {
            (self.text_stream_end_pos - self.text_stream_pos).max(0)
        }
    }

    /// Read one opaque bit from the current text-stream cursor.
    pub fn read_text_stream_bit(&mut self) -> bool {
        if self.text_stream_pos < 0 {
            return false;
        }
        let saved_pos = self.position_in_bits();
        self.set_position_in_bits(self.text_stream_pos);
        let value = self.read_bit();
        self.text_stream_pos = self.position_in_bits();
        self.set_position_in_bits(saved_pos);
        value
    }

    /// Reset the bit shift and read a CRC (2 bytes, little-endian).
    pub fn reset_shift(&mut self) -> u16 {
        if self.bit_shift > 0 {
            self.bit_shift = 0;
        }
        self.advance_byte();
        let lo = self.last_byte as u16;
        self.advance_byte();
        lo | ((self.last_byte as u16) << 8)
    }

    /// Read a raw short using big-endian order (for handle section chunk sizes).
    pub fn read_short_big_endian(&mut self) -> i16 {
        let b0 = self.read_byte() as i16;
        let b1 = self.read_byte() as i16;
        (b0 << 8) | b1
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Internal helpers
    // ════════════════════════════════════════════════════════════════════════

    /// Apply the current bit shift to a raw byte array read from the stream.
    fn apply_shift_to_arr(&mut self, arr: &mut [u8]) {
        let length = arr.len();
        // Read raw bytes from stream
        let start = self.position;
        if start >= self.data.len() {
            // Past end of data — fill with zeros
            arr.iter_mut().for_each(|b| *b = 0);
            return;
        }
        let end = (start + length).min(self.data.len());
        let available = end - start;
        arr[..available].copy_from_slice(&self.data[start..end]);
        self.position = end;

        if self.bit_shift == 0 {
            if available > 0 {
                self.last_byte = arr[available - 1];
            }
            return;
        }

        let shift = 8 - self.bit_shift;
        for i in 0..available {
            let last_value = (self.last_byte as u32) << self.bit_shift;
            self.last_byte = arr[i];
            arr[i] = (last_value | ((self.last_byte as u32) >> shift)) as u8;
        }
    }

    /// Compute the string data section boundaries from the flag position.
    ///
    /// Returns (length_in_bits, str_data_size_in_bits).
    fn apply_flag_to_position(&mut self, last_pos: i64) -> (i64, i64) {
        // Decrement by 16 bytes (128 bits)
        let mut length = last_pos - 16;
        self.set_position_in_bits(length);

        // Read the unsigned short at endbit − 128
        let mut str_data_size = self.read_raw_ushort() as i64;

        // If 0x8000 bit set, decrement another 16 bytes and read hiSize
        if (str_data_size & 0x8000) != 0 {
            length -= 16;
            self.set_position_in_bits(length);
            str_data_size &= 0x7FFF;
            let hi_size = self.read_raw_ushort() as i64;
            str_data_size += (hi_size & 0xFFFF) << 15;
        }

        (length, str_data_size)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests — round-trip with DwgBitWriter
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;

    fn make_writer(dxf: DxfVersion) -> DwgBitWriter {
        let dwg = DwgVersion::from_dxf_version(dxf).unwrap();
        DwgBitWriter::new(dwg, dxf)
    }

    fn make_reader(writer: DwgBitWriter) -> DwgBitReader {
        let dxf = writer.dxf_version();
        let dwg = writer.version();
        let data = writer.into_bytes();
        DwgBitReader::new(data, dwg, dxf)
    }

    #[test]
    fn test_bit_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        let mut r = make_reader(w);
        assert!(r.read_bit());
        assert!(!r.read_bit());
        assert!(r.read_bit());
    }

    #[test]
    fn test_2bits_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_2bits(0);
        w.write_2bits(1);
        w.write_2bits(2);
        w.write_2bits(3);
        let mut r = make_reader(w);
        assert_eq!(r.read_2bits(), 0);
        assert_eq!(r.read_2bits(), 1);
        assert_eq!(r.read_2bits(), 2);
        assert_eq!(r.read_2bits(), 3);
    }

    #[test]
    fn test_bit_short_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_short(0);
        w.write_bit_short(42);
        w.write_bit_short(256);
        w.write_bit_short(-1);
        w.write_bit_short(1000);
        let mut r = make_reader(w);
        assert_eq!(r.read_bit_short(), 0);
        assert_eq!(r.read_bit_short(), 42);
        assert_eq!(r.read_bit_short(), 256);
        assert_eq!(r.read_bit_short(), -1);
        assert_eq!(r.read_bit_short(), 1000);
    }

    #[test]
    fn test_bit_long_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_long(0);
        w.write_bit_long(42);
        w.write_bit_long(100_000);
        w.write_bit_long(-1);
        let mut r = make_reader(w);
        assert_eq!(r.read_bit_long(), 0);
        assert_eq!(r.read_bit_long(), 42);
        assert_eq!(r.read_bit_long(), 100_000);
        assert_eq!(r.read_bit_long(), -1);
    }

    #[test]
    fn test_bit_double_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_double(0.0);
        w.write_bit_double(1.0);
        w.write_bit_double(3.14159);
        let mut r = make_reader(w);
        assert_eq!(r.read_bit_double(), 0.0);
        assert_eq!(r.read_bit_double(), 1.0);
        assert!((r.read_bit_double() - 3.14159).abs() < 1e-10);
    }

    #[test]
    fn test_raw_short_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_raw_short(0);
        w.write_raw_short(12345);
        w.write_raw_short(-32768);
        let mut r = make_reader(w);
        assert_eq!(r.read_raw_short(), 0);
        assert_eq!(r.read_raw_short(), 12345);
        assert_eq!(r.read_raw_short(), -32768);
    }

    #[test]
    fn test_byte_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_byte(0);
        w.write_byte(0xFF);
        w.write_byte(0x42);
        let mut r = make_reader(w);
        assert_eq!(r.read_byte(), 0);
        assert_eq!(r.read_byte(), 0xFF);
        assert_eq!(r.read_byte(), 0x42);
    }

    #[test]
    fn test_variable_text_roundtrip_pre2007() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_variable_text("Hello");
        w.write_variable_text("");
        w.write_variable_text("Test123");
        let mut r = make_reader(w);
        assert_eq!(r.read_variable_text(), "Hello");
        assert_eq!(r.read_variable_text(), "");
        assert_eq!(r.read_variable_text(), "Test123");
    }

    #[test]
    fn test_variable_text_roundtrip_r2007() {
        let mut w = make_writer(DxfVersion::AC1021);
        w.write_variable_text("Hello");
        w.write_variable_text("");
        w.write_variable_text("Ünîcödé");
        let mut r = make_reader(w);
        assert_eq!(r.read_variable_text(), "Hello");
        assert_eq!(r.read_variable_text(), "");
        assert_eq!(r.read_variable_text(), "Ünîcödé");
    }

    #[test]
    fn test_handle_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_handle(DwgReferenceType::SoftOwnership, 0x1A);
        let mut r = make_reader(w);
        let h = r.read_handle();
        assert_eq!(h, 0x1A);
    }

    #[test]
    fn test_bit_extrusion_r2000() {
        let mut w = make_writer(DxfVersion::AC1015);
        // Default extrusion (0,0,1) should write single bit = 1
        w.write_bit_extrusion(Vector3::new(0.0, 0.0, 1.0));
        // Custom extrusion should write bit = 0 + 3BD
        w.write_bit_extrusion(Vector3::new(1.0, 0.0, 0.0));
        let mut r = make_reader(w);
        let ext1 = r.read_bit_extrusion();
        assert_eq!(ext1, Vector3::new(0.0, 0.0, 1.0));
        let ext2 = r.read_bit_extrusion();
        assert_eq!(ext2, Vector3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn test_bit_thickness_r2000() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_thickness(0.0);
        w.write_bit_thickness(2.5);
        let mut r = make_reader(w);
        assert_eq!(r.read_bit_thickness(), 0.0);
        assert!((r.read_bit_thickness() - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_bit_long_long_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_long_long(0);
        w.write_bit_long_long(255);
        w.write_bit_long_long(100_000);
        let mut r = make_reader(w);
        assert_eq!(r.read_bit_long_long(), 0);
        assert_eq!(r.read_bit_long_long(), 255);
        assert_eq!(r.read_bit_long_long(), 100_000);
    }

    #[test]
    fn test_object_type_pre_r2010() {
        let mut w = make_writer(DxfVersion::AC1015);
        w.write_bit_short(17); // Entity type 17 = ARC
        w.write_bit_short(500); // Class type
        let mut r = make_reader(w);
        assert_eq!(r.read_object_type(), 17);
        assert_eq!(r.read_object_type(), 500);
    }

    #[test]
    fn test_3bit_double_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        let pt = Vector3::new(1.5, 2.5, 3.5);
        w.write_3bit_double(pt);
        let mut r = make_reader(w);
        let result = r.read_3bit_double();
        assert!((result.x - 1.5).abs() < 1e-10);
        assert!((result.y - 2.5).abs() < 1e-10);
        assert!((result.z - 3.5).abs() < 1e-10);
    }

    #[test]
    fn test_bit_double_with_default_roundtrip() {
        let mut w = make_writer(DxfVersion::AC1015);
        let reference = 3.14;
        // Case 00: stored == reference → 2 bits
        // Writer: write_bit_double_with_default(stored=3.14, reference=3.14)
        w.write_bit_double_with_default(3.14, reference);
        // Case 11: stored=100.5 is very different from reference=3.14 → full RD
        w.write_bit_double_with_default(100.5, reference);
        let mut r = make_reader(w);
        // Reader: read_bit_double_with_default(reference=3.14)
        let v1 = r.read_bit_double_with_default(reference);
        assert!((v1 - 3.14).abs() < 1e-10, "v1 = {}", v1);
        let v2 = r.read_bit_double_with_default(reference);
        assert!((v2 - 100.5).abs() < 1e-10, "v2 = {}", v2);
    }
}
