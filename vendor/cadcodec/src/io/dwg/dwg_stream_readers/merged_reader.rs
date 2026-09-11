//! DWG Merged Reader — R2007+ three-stream demultiplexer
//!
//! In DWG R2007+, each object/section record is encoded as three
//! interleaved streams:
//!
//! ```text
//! |---main---|---text---|flag|---handles---|
//! ```
//!
//! `DwgMergedReader` transparently routes reads to the correct sub-reader:
//! - Data reads → main reader
//! - `read_variable_text()` → text reader  
//! - `read_handle()` → handle reader
//!
//! For pre-R2007, all reads go to the main reader (two-stream mode where
//! text is inline in the main stream).
//!
//! Based on the reference `DwgMergedReader`.

use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::{Color, DxfVersion, Vector2, Vector3};

/// Merge mode, determined by DWG version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeMode {
    /// R13–R2004: Two-stream (main + handle). Text is inline in main.
    TwoStream,
    /// R2007+: Three-stream (main + text + handle).
    ThreeStream,
}

/// Merged reader that transparently demultiplexes three-stream R2007+ data.
///
/// For pre-R2007, this acts as a simple passthrough to the main reader.
pub struct DwgMergedReader {
    /// Main data reader
    main: DwgBitReader,
    /// Text reader (R2007+ only; for pre-R2007, text reads from main)
    text: Option<DwgBitReader>,
    /// Handle reader (split from main after handle_start_bits)
    handle: Option<DwgBitReader>,
    /// Merge mode
    _mode: MergeMode,
    /// DXF version
    dxf_version: DxfVersion,
    /// Raw data (kept for lazy text/handle setup in ThreeStream mode)
    raw_data: Option<Vec<u8>>,
    /// Document code page used by lazily-created stream readers.
    encoding: &'static encoding_rs::Encoding,
    /// Handle-stream bit count from the R2010+ MC framing field.
    /// Stored so unknown entities can reproduce the correct framing on write.
    handle_bits: i64,
    /// Reference handle for offset-based handle codes (6/8/A/C).
    ///
    /// In DWG, handle references with codes 6, 8, 0xA, 0xC are relative to
    /// the current object's own handle.  This field should be set via
    /// `set_ref_handle()` right after reading the object's handle from the
    /// main stream.
    ref_handle: u64,
    /// Bit position where the handle stream starts.
    /// For R2007: equals the RL field (total_size_bits).
    /// For R2010+: equals total_data_bits - handle_bits.
    /// For pre-R2007: equals handle_start_bits from the constructor.
    handle_start_bit: i64,
    /// Bit position where the text stream starts; main data ends here.
    text_start_bit: i64,
}

impl DwgMergedReader {
    /// Create a merged reader from raw section data.
    ///
    /// For R2007+, this splits the data into three sub-readers based on
    /// the embedded stream boundaries.
    ///
    /// For pre-R2007, the data is split into main (up to handle_start_bits)
    /// and handle (from handle_start_bits onward).
    ///
    /// # Arguments
    /// * `data` - Raw section data (the merged stream bytes)
    /// * `dxf_version` - DXF version for version-specific parsing
    /// * `handle_start_bits` - Bit position where handle data begins
    ///   (only used for two-stream mode; for three-stream, computed from flags)
    pub fn new(data: Vec<u8>, dxf_version: DxfVersion, handle_start_bits: i64) -> Self {
        Self::new_with_encoding(
            data,
            dxf_version,
            handle_start_bits,
            encoding_rs::WINDOWS_1252,
        )
    }

    pub fn new_with_encoding(
        data: Vec<u8>,
        dxf_version: DxfVersion,
        handle_start_bits: i64,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        let dwg = DwgVersion::from_dxf_version(dxf_version).unwrap_or(DwgVersion::AC15);

        let mode = if dxf_version >= DxfVersion::AC1021 {
            MergeMode::ThreeStream
        } else {
            MergeMode::TwoStream
        };

        match mode {
            MergeMode::TwoStream => {
                // Two-stream: main = data[:handle_start], handle = data[handle_start:]
                let main = DwgBitReader::with_encoding(data.clone(), dwg, dxf_version, encoding);

                // Create handle reader from remaining bytes
                let handle_start_byte = (handle_start_bits / 8) as usize;
                let handle_data = if handle_start_byte < data.len() {
                    data[handle_start_byte..].to_vec()
                } else {
                    Vec::new()
                };
                let handle = DwgBitReader::with_encoding(handle_data, dwg, dxf_version, encoding);

                DwgMergedReader {
                    main,
                    text: None,
                    handle: Some(handle),
                    _mode: mode,
                    dxf_version,
                    raw_data: None,
                    encoding,
                    handle_bits: 0,
                    ref_handle: 0,
                    handle_start_bit: handle_start_bits,
                    text_start_bit: handle_start_bits,
                }
            }
            MergeMode::ThreeStream => {
                // Three-stream: lazy setup.
                // Don't read BL or set up text/handle readers here.
                // The BL is not at position 0 — it comes after the type code.
                // Text and handle readers will be set up later via
                // setup_text_and_handle() after the caller reads the BL.
                let main_reader =
                    DwgBitReader::with_encoding(data.clone(), dwg, dxf_version, encoding);

                DwgMergedReader {
                    main: main_reader,
                    text: None,
                    handle: None,
                    _mode: mode,
                    dxf_version,
                    raw_data: Some(data),
                    encoding,
                    handle_bits: 0,
                    ref_handle: 0,
                    handle_start_bit: 0, // set later when RL is known
                    text_start_bit: 0,
                }
            }
        }
    }

    /// Create a merged reader from separate pre-split streams.
    ///
    /// Used when the caller has already separated the three streams
    /// (e.g., after decompression of individual section pages).
    pub fn from_readers(
        main: DwgBitReader,
        text: Option<DwgBitReader>,
        handle: Option<DwgBitReader>,
        dxf_version: DxfVersion,
    ) -> Self {
        let mode = if text.is_some() {
            MergeMode::ThreeStream
        } else {
            MergeMode::TwoStream
        };
        DwgMergedReader {
            main,
            text,
            handle,
            _mode: mode,
            dxf_version,
            raw_data: None,
            encoding: encoding_rs::WINDOWS_1252,
            handle_bits: 0,
            ref_handle: 0,
            handle_start_bit: 0,
            text_start_bit: 0,
        }
    }

    /// Set up text and handle readers for ThreeStream mode.
    ///
    /// Called after the caller reads the RL (total_size_bits) from the main stream.
    /// The RL is written by `save_position_for_size` in the writer.
    /// It stores one past the text-present flag bit position. The flag is at
    /// RL − 1 and the handle stream starts at RL.
    ///
    /// Layout: `[RL][main_data...][text...][modular_short][flag@RL-1][handles...]`
    pub fn setup_text_and_handle(&mut self, total_size_bits: i64) {
        if let Some(ref data) = self.raw_data {
            let dwg = DwgVersion::from_dxf_version(self.dxf_version).unwrap_or(DwgVersion::AC15);

            // Text reader — the flag bit is at RL − 1 (per-object convention,
            // matching the classes reader and object reader).
            let mut text_reader =
                DwgBitReader::with_encoding(data.clone(), dwg, self.dxf_version, self.encoding);
            self.text_start_bit = text_reader.set_position_by_flag(total_size_bits - 1);
            self.text = Some(text_reader);

            self.handle_start_bit = total_size_bits;
            let mut handle_reader =
                DwgBitReader::with_encoding(data.clone(), dwg, self.dxf_version, self.encoding);
            handle_reader.set_position_in_bits(self.handle_start_bit);
            self.handle = Some(handle_reader);
        }
    }

    /// Return a clone of the full merged-stream record bytes.
    ///
    /// The bytes represent the complete payload between the
    /// ModularShort length prefix and the CRC-16 trailer.
    /// Used to preserve raw data for unknown entity round-trips.
    pub fn raw_merged_data(&self) -> Vec<u8> {
        self.main.data_bytes()
    }

    /// Set the handle-bits value (from R2010+ MC framing).
    pub fn set_handle_bits(&mut self, bits: i64) {
        self.handle_bits = bits;
    }

    /// Get the handle-bits value stored by the reader.
    pub fn get_handle_bits(&self) -> i64 {
        self.handle_bits
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Data reads — always from main reader
    // ════════════════════════════════════════════════════════════════════════

    pub fn read_bit(&mut self) -> bool {
        self.main.read_bit()
    }
    pub fn read_byte(&mut self) -> u8 {
        self.main.read_byte()
    }
    pub fn read_bytes(&mut self, length: usize) -> Vec<u8> {
        self.main.read_bytes(length)
    }
    pub fn decode_legacy_text(&self, bytes: &[u8]) -> String {
        self.main.decode_legacy_text(bytes)
    }
    /// Bytes left in the main data stream from the current position.
    pub fn remaining_bytes(&self) -> usize {
        self.main.data_len().saturating_sub(self.main.position())
    }
    /// Bits remaining in entity main-data stream, excluding text and handles.
    pub fn main_remaining_bits(&self) -> i64 {
        let end = if self.text_start_bit > 0 {
            self.text_start_bit
        } else {
            self.handle_start_bit
        };
        (end - self.main.position_in_bits()).max(0)
    }
    pub fn read_bit_short(&mut self) -> i16 {
        self.main.read_bit_short()
    }
    pub fn read_bit_long(&mut self) -> i32 {
        self.main.read_bit_long()
    }
    pub fn read_bit_long_long(&mut self) -> i64 {
        self.main.read_bit_long_long()
    }
    pub fn read_bit_double(&mut self) -> f64 {
        self.main.read_bit_double()
    }
    pub fn read_raw_long(&mut self) -> i64 {
        self.main.read_raw_long()
    }
    pub fn read_raw_short(&mut self) -> i16 {
        self.main.read_raw_short()
    }
    pub fn read_raw_double(&mut self) -> f64 {
        self.main.read_raw_double()
    }
    pub fn read_2bit_double(&mut self) -> Vector2 {
        self.main.read_2bit_double()
    }
    pub fn read_3bit_double(&mut self) -> Vector3 {
        self.main.read_3bit_double()
    }
    pub fn read_2raw_double(&mut self) -> Vector2 {
        self.main.read_2raw_double()
    }
    pub fn read_3raw_double(&mut self) -> Vector3 {
        self.main.read_3raw_double()
    }
    pub fn read_bit_extrusion(&mut self) -> Vector3 {
        self.main.read_bit_extrusion()
    }
    pub fn read_bit_thickness(&mut self) -> f64 {
        self.main.read_bit_thickness()
    }
    pub fn read_bit_double_with_default(&mut self, default: f64) -> f64 {
        self.main.read_bit_double_with_default(default)
    }
    pub fn read_cm_color(&mut self) -> Color {
        self.main.read_cm_color()
    }
    pub fn read_cm_color_with_names(&mut self) -> (Color, Option<String>, Option<String>) {
        if self.dxf_version < DxfVersion::AC1018 {
            return (Color::from_index(self.main.read_bit_short()), None, None);
        }

        let _color_index = self.main.read_bit_short();
        let rgb = self.main.read_bit_long() as u32;
        let bytes = rgb.to_le_bytes();
        let color = if rgb == 0xC000_0000 {
            Color::ByLayer
        } else if rgb == 0xC800_0000 {
            Color::None
        } else if (rgb & 0x0100_0000) != 0 {
            Color::from_index(bytes[0] as i16)
        } else {
            Color::from_rgb(bytes[2], bytes[1], bytes[0])
        };
        let flags = self.main.read_byte();
        let color_name = if (flags & 1) != 0 {
            Some(self.read_variable_text())
        } else {
            None
        };
        let book_name = if (flags & 2) != 0 {
            Some(self.read_variable_text())
        } else {
            None
        };
        (color, color_name, book_name)
    }
    /// Read a CMTC color.  TABLESTYLE stores the full R2004 CMC payload
    /// even when the containing DWG uses a pre-R2004 file version.
    pub fn read_cm_true_color(&mut self) -> Color {
        let _color_index = self.main.read_bit_short();
        let rgb = self.main.read_bit_long() as u32;
        let arr = rgb.to_le_bytes();
        let color = if rgb == 0xC000_0000 {
            Color::ByLayer
        } else if rgb == 0xC800_0000 {
            Color::None
        } else if (rgb & 0x0100_0000) != 0 {
            Color::from_index(arr[0] as i16)
        } else {
            Color::from_rgb(arr[2], arr[1], arr[0])
        };
        let flags = self.main.read_byte();
        if (flags & 1) != 0 {
            let _ = self.read_variable_text();
        }
        if (flags & 2) != 0 {
            let _ = self.read_variable_text();
        }
        color
    }
    pub fn read_en_color(&mut self) -> (Color, crate::types::Transparency, bool) {
        self.main.read_en_color()
    }
    pub fn read_color_by_index(&mut self) -> Color {
        self.main.read_color_by_index()
    }
    pub fn read_modular_char(&mut self) -> u64 {
        self.main.read_modular_char()
    }
    pub fn read_signed_modular_char(&mut self) -> i64 {
        self.main.read_signed_modular_char()
    }
    pub fn read_modular_short(&mut self) -> i32 {
        self.main.read_modular_short()
    }
    pub fn read_object_type(&mut self) -> i16 {
        self.main.read_object_type()
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Text reads — from text reader for R2007+, main for pre-R2007
    // ════════════════════════════════════════════════════════════════════════

    /// Read a variable-length text string.
    ///
    /// For R2007+, this reads from the separate text stream (UTF-16LE).
    /// For pre-R2007, this reads from the main stream.
    pub fn read_variable_text(&mut self) -> String {
        match &mut self.text {
            Some(text_reader) => text_reader.read_variable_text(),
            None => self.main.read_variable_text(),
        }
    }

    /// Bits remaining after the currently decoded fields in the separate
    /// R2007+ text stream.
    pub fn text_remaining_bits(&self) -> i64 {
        self.text
            .as_ref()
            .map(DwgBitReader::text_stream_remaining_bits)
            .unwrap_or(0)
    }

    /// Read one bit from the separate R2007+ text stream.
    pub fn read_text_bit(&mut self) -> bool {
        self.text
            .as_mut()
            .map(DwgBitReader::read_text_stream_bit)
            .unwrap_or(false)
    }

    /// Read a text string, but always from the main stream.
    ///
    /// Used for fields that are always inline even in R2007+.
    pub fn read_text_inline(&mut self) -> String {
        self.main.read_variable_text()
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Handle reads — from handle reader if available, else main
    // ════════════════════════════════════════════════════════════════════════

    /// Reposition the handle reader to a new bit position.
    ///
    /// Used for R13/R14 where the handle-stream split point (RL) is
    /// discovered inside the entity preamble rather than at the top of
    /// the record.
    pub fn reposition_handle_reader(&mut self, bit_position: i64) {
        if let Some(ref mut handle_reader) = self.handle {
            handle_reader.set_position_in_bits(bit_position);
        }
        self.handle_start_bit = bit_position;
        if self.text.is_none() {
            self.text_start_bit = bit_position;
        }
    }

    /// Read a handle reference.
    ///
    /// For R2007+, this reads from the separate handle stream.
    /// For pre-R2007, this reads from the main stream.
    ///
    /// Offset-type codes (6/8/A/C) are resolved relative to `ref_handle`,
    /// which should be set to the current object's handle via
    /// `set_ref_handle()` after reading the object preface.
    pub fn read_handle(&mut self) -> u64 {
        match &mut self.handle {
            Some(handle_reader) => handle_reader.read_handle_relative(self.ref_handle),
            None => self.main.read_handle_relative(self.ref_handle),
        }
    }

    pub fn handle_remaining_bits(&self) -> i64 {
        match &self.handle {
            Some(reader) => reader.data_len() as i64 * 8 - reader.position_in_bits(),
            None => self.main.data_len() as i64 * 8 - self.main.position_in_bits(),
        }
        .max(0)
    }

    /// Read a handle reference from the MAIN (data) stream, even when a
    /// separate handle stream exists. A few objects store some handle
    /// references inline in the data section rather than in the handle stream
    /// — e.g. the SORTENTSTABLE sort handles. (#146)
    pub fn read_main_handle(&mut self) -> u64 {
        self.main.read_handle_relative(self.ref_handle)
    }

    /// Set the reference handle for offset-based handle codes.
    ///
    /// Must be called after reading the current object's own handle
    /// from the main stream (via `read_common_data`).
    pub fn set_ref_handle(&mut self, handle: u64) {
        self.ref_handle = handle;
    }

    /// Read a handle reference relative to a base handle.
    pub fn read_handle_reference(
        &mut self,
        ref_handle: u64,
    ) -> (u64, crate::io::dwg::dwg_reference_type::DwgReferenceType) {
        match &mut self.handle {
            Some(handle_reader) => {
                let mut ref_type = crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined;
                let h = handle_reader.read_handle_reference(ref_handle, &mut ref_type);
                (h, ref_type)
            }
            None => {
                let mut ref_type = crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined;
                let h = self.main.read_handle_reference(ref_handle, &mut ref_type);
                (h, ref_type)
            }
        }
    }

    /// Read a handle reference using the current object's handle as the base
    /// and return both the resolved handle and its ownership/pointer kind.
    pub fn read_typed_handle(
        &mut self,
    ) -> (u64, crate::io::dwg::dwg_reference_type::DwgReferenceType) {
        self.read_handle_reference(self.ref_handle)
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Position and state queries
    // ════════════════════════════════════════════════════════════════════════

    /// Main reader bit position.
    pub fn position_in_bits(&self) -> i64 {
        self.main.position_in_bits()
    }

    /// Main reader byte position.
    pub fn position(&self) -> usize {
        self.main.position()
    }

    /// Set main reader position.
    pub fn set_position_in_bits(&mut self, pos: i64) {
        self.main.set_position_in_bits(pos);
    }

    /// Get the DXF version.
    pub fn dxf_version(&self) -> DxfVersion {
        self.dxf_version
    }

    /// Get a mutable reference to the main reader (for direct access).
    pub fn main_mut(&mut self) -> &mut DwgBitReader {
        &mut self.main
    }

    /// Get a reference to the main reader.
    pub fn main(&self) -> &DwgBitReader {
        &self.main
    }

    /// Get the bit position where the handle stream starts.
    /// For R2007: this equals the RL (total_size_bits) field.
    /// Returns 0 if not set (pre-R2007 three-stream or no handle reader).
    pub fn handle_start(&self) -> i64 {
        self.handle_start_bit
    }

    /// Set the bit position where the handle stream starts.
    pub fn set_handle_start(&mut self, bit: i64) {
        self.handle_start_bit = bit;
    }

    /// Set the exclusive end of the main-data stream.
    ///
    /// In R2007+ records this is the start of the optional text stream, not
    /// the text-present flag or the handle stream.  Opaque class/proxy payload
    /// readers use this boundary and must never absorb either framing data or
    /// string bytes into the payload.
    pub fn set_main_data_end(&mut self, bit: i64) {
        self.text_start_bit = bit;
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;

    #[test]
    fn test_two_stream_from_readers() {
        // Create main stream with data + text
        let dwg = DwgVersion::AC15;
        let version = DxfVersion::AC1015;
        let mut main_writer = DwgBitWriter::new(dwg, version);
        main_writer.write_bit_short(42);
        main_writer.write_bit_double(3.14);
        main_writer.write_variable_text("hello");

        // Create handle stream
        let mut handle_writer = DwgBitWriter::new(dwg, version);
        handle_writer.write_handle_undefined(0x1A);

        let main_data = main_writer.to_bytes();
        let handle_data = handle_writer.to_bytes();

        let main = DwgBitReader::new(main_data, dwg, version);
        let handle = DwgBitReader::new(handle_data, dwg, version);

        let mut reader = DwgMergedReader::from_readers(main, None, Some(handle), version);

        assert_eq!(reader.read_bit_short(), 42);
        assert!((reader.read_bit_double() - 3.14).abs() < 1e-10);
        assert_eq!(reader.read_variable_text(), "hello");

        let h = reader.read_handle();
        assert_eq!(h, 0x1A);
    }

    #[test]
    fn test_from_readers_passthrough() {
        let dwg = DwgVersion::AC15;
        let version = DxfVersion::AC1015;

        let mut writer = DwgBitWriter::new(dwg, version);
        writer.write_bit_short(99);
        writer.write_bit_double(2.71);
        let data = writer.to_bytes();

        let main = DwgBitReader::new(data, dwg, version);
        let mut reader = DwgMergedReader::from_readers(main, None, None, version);

        assert_eq!(reader.read_bit_short(), 99);
        assert!((reader.read_bit_double() - 2.71).abs() < 1e-10);
    }

    #[test]
    fn test_three_stream_text_routing() {
        // Verify that text reads go to the text reader when one is provided
        let dwg = DwgVersion::AC21;
        let version = DxfVersion::AC1021;

        // Main stream: numeric data
        let mut main_writer = DwgBitWriter::new(dwg, version);
        main_writer.write_bit_short(77);
        main_writer.write_bit_double(1.5);
        let main_data = main_writer.to_bytes();

        // Text stream: also numeric data, but routed separately
        // We write text using the TU format (write_text_unicode)
        let mut text_writer = DwgBitWriter::new(dwg, version);
        text_writer.write_variable_text("world");
        let text_data = text_writer.to_bytes();

        // Handle stream
        let mut handle_writer = DwgBitWriter::new(dwg, version);
        handle_writer.write_handle_undefined(0x42);
        let handle_data = handle_writer.to_bytes();

        let main = DwgBitReader::new(main_data, dwg, version);
        let text = DwgBitReader::new(text_data, dwg, version);
        let handle = DwgBitReader::new(handle_data, dwg, version);

        let mut reader = DwgMergedReader::from_readers(main, Some(text), Some(handle), version);

        // Data from main
        assert_eq!(reader.read_bit_short(), 77);
        assert!((reader.read_bit_double() - 1.5).abs() < 1e-10);

        // Text from separate text reader (uses read_text_unicode internally for R2007+)
        let t = reader.read_variable_text();
        assert_eq!(t, "world");

        // Handle from separate handle reader
        let h = reader.read_handle();
        assert_eq!(h, 0x42);
    }
}
