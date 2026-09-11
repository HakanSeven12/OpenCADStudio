//! DWG merged stream writer
//!
//! In DWG, each object record is composed of up to three sub-streams
//! that are merged into a single byte sequence:
//!
//! 1. **Main** stream — entity/object data fields
//! 2. **Text** stream — string values (R2000+; pre-R2000 text is in main)
//! 3. **Handle** stream — cross-reference handles
//!
//! The `DwgMergedWriter` coordinates these three `DwgBitWriter` instances
//! and implements the merge protocol that combines them into the final
//! record bytes, including size headers and the text-present flag.
//!
//! ## Version variants
//!
//! - **AC12/AC15/AC18** (R13–R2004): Two-stream merge. Text and main share
//!   the same writer (no separate text stream). Handle stream is appended
//!   after main. Size is written as raw BL at a saved position.
//!
//! - **AC21/AC24** (R2007+): Three-stream merge. Text stream is appended
//!   between main and handles with a modular-short size and a `true` bit flag.
//!
//! Based on the reference `DwgMergedStreamWriter` and `DwgmMergedStreamWriterAC14`.

use super::bit_writer::DwgBitWriter;
use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::types::Transparency;
use crate::types::{Color, DxfVersion, Vector2, Vector3};

/// Merged writer mode, determined by DWG version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeMode {
    /// R13–R2004: Two-stream (main + handle). Text goes into main.
    TwoStream,
    /// R2007+: Three-stream (main + text + handle).
    ThreeStream,
}

/// Coordinates three sub-streams (main, text, handle) for writing
/// a single DWG object record.
pub struct DwgMergedWriter {
    /// Main data stream
    main: DwgBitWriter,
    /// Text stream (separate in R2007+, alias of main in earlier versions)
    text: DwgBitWriter,
    /// Handle reference stream
    handle: DwgBitWriter,
    /// Merge mode
    mode: MergeMode,
    /// Whether `save_position_for_size` was called
    saved_position: bool,
    /// Saved bit position (set by `save_position_for_size`)
    position_in_bits: i64,
    /// Bit position in the merged main stream where the handle section starts.
    /// Set during merge, used by R2010+ record framing to compute MC handle_bits.
    handle_start_bits: i64,
}

impl DwgMergedWriter {
    /// Create a merged writer appropriate for the given DWG version.
    pub fn new(version: DwgVersion, dxf_version: DxfVersion) -> Self {
        let mode = if version >= DwgVersion::AC21 {
            MergeMode::ThreeStream
        } else {
            MergeMode::TwoStream
        };

        Self {
            main: DwgBitWriter::new(version, dxf_version),
            text: DwgBitWriter::new(version, dxf_version),
            handle: DwgBitWriter::new(version, dxf_version),
            mode,
            saved_position: false,
            position_in_bits: -1,
            handle_start_bits: -1,
        }
    }

    /// Create a merged writer with a specific encoding.
    pub fn with_encoding(
        version: DwgVersion,
        dxf_version: DxfVersion,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        let mode = if version >= DwgVersion::AC21 {
            MergeMode::ThreeStream
        } else {
            MergeMode::TwoStream
        };

        Self {
            main: DwgBitWriter::with_encoding(version, dxf_version, encoding),
            text: DwgBitWriter::with_encoding(version, dxf_version, encoding),
            handle: DwgBitWriter::with_encoding(version, dxf_version, encoding),
            mode,
            saved_position: false,
            position_in_bits: -1,
            handle_start_bits: -1,
        }
    }

    /// Get the DWG version.
    pub fn version(&self) -> DwgVersion {
        self.main.version()
    }

    /// Get the DXF version.
    pub fn dxf_version(&self) -> DxfVersion {
        self.main.dxf_version()
    }

    /// Get a reference to the main writer (for direct access).
    pub fn main(&self) -> &DwgBitWriter {
        &self.main
    }

    /// Get a mutable reference to the main writer.
    pub fn main_mut(&mut self) -> &mut DwgBitWriter {
        &mut self.main
    }

    /// Get a reference to the text writer.
    pub fn text(&self) -> &DwgBitWriter {
        &self.text
    }

    /// Get a mutable reference to the text writer.
    pub fn text_mut(&mut self) -> &mut DwgBitWriter {
        &mut self.text
    }

    /// Get a reference to the handle writer.
    pub fn handle_writer(&self) -> &DwgBitWriter {
        &self.handle
    }

    /// Get a reference to the main writer (for debug inspection).
    pub fn main_ref(&self) -> &DwgBitWriter {
        &self.main
    }

    /// Check if position was saved.
    pub fn is_saved(&self) -> bool {
        self.saved_position
    }

    /// Get the saved position in bits.
    pub fn saved_pos_bits(&self) -> i64 {
        self.position_in_bits
    }

    /// Get a mutable reference to the handle writer.
    pub fn handle_mut(&mut self) -> &mut DwgBitWriter {
        &mut self.handle
    }

    /// Reset all three streams (for reuse per object).
    pub fn reset(&mut self) {
        self.main.reset();
        self.text.reset();
        self.handle.reset();
        self.saved_position = false;
        self.position_in_bits = -1;
        self.handle_start_bits = -1;
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Size tracking
    // ════════════════════════════════════════════════════════════════════════

    /// Save the current main-stream bit position and write a 4-byte
    /// placeholder for the total object size in bits.
    pub fn save_position_for_size(&mut self) {
        self.saved_position = true;
        self.position_in_bits = self.main.position_in_bits();
        // Write 4 zero bytes as a placeholder
        self.main.write_int(0);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Stream delegation — main writes
    // ════════════════════════════════════════════════════════════════════════

    pub fn write_bit(&mut self, value: bool) {
        self.main.write_bit(value);
    }

    pub fn write_2bits(&mut self, value: u8) {
        self.main.write_2bits(value);
    }

    pub fn write_byte(&mut self, value: u8) {
        self.main.write_byte(value);
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        self.main.write_bytes(data);
    }

    pub fn write_raw_short(&mut self, value: i16) {
        self.main.write_raw_short(value);
    }

    pub fn write_raw_long(&mut self, value: i32) {
        self.main.write_raw_long(value);
    }

    pub fn write_raw_double(&mut self, value: f64) {
        self.main.write_raw_double(value);
    }

    pub fn write_int(&mut self, value: i32) {
        self.main.write_int(value);
    }

    pub fn write_bit_short(&mut self, value: i16) {
        self.main.write_bit_short(value);
    }

    pub fn write_bit_long(&mut self, value: i32) {
        self.main.write_bit_long(value);
    }

    pub fn write_bit_long_unsigned(&mut self, value: u32) {
        self.main.write_bit_long_unsigned(value);
    }

    pub fn write_bit_double(&mut self, value: f64) {
        self.main.write_bit_double(value);
    }

    pub fn write_bit_long_long(&mut self, value: i64) {
        self.main.write_bit_long_long(value);
    }

    pub fn encode_legacy_text(&self, text: &str) -> Vec<u8> {
        self.main.encode_legacy_text(text)
    }

    pub fn write_bit_double_with_default(&mut self, def: f64, value: f64) {
        self.main.write_bit_double_with_default(def, value);
    }

    pub fn write_2bit_double(&mut self, value: Vector2) {
        self.main.write_2bit_double(value);
    }

    pub fn write_3bit_double(&mut self, value: Vector3) {
        self.main.write_3bit_double(value);
    }

    pub fn write_2raw_double(&mut self, value: Vector2) {
        self.main.write_2raw_double(value);
    }

    pub fn write_3raw_double(&mut self, value: Vector3) {
        self.main.write_3raw_double(value);
    }

    pub fn write_2bit_double_with_default(&mut self, def: Vector2, value: Vector2) {
        self.main.write_2bit_double_with_default(def, value);
    }

    pub fn write_3bit_double_with_default(&mut self, def: Vector3, value: Vector3) {
        self.main.write_3bit_double_with_default(def, value);
    }

    pub fn write_bit_thickness(&mut self, thickness: f64) {
        self.main.write_bit_thickness(thickness);
    }

    pub fn write_bit_extrusion(&mut self, normal: Vector3) {
        self.main.write_bit_extrusion(normal);
    }

    pub fn write_object_type(&mut self, value: i16) {
        self.main.write_object_type(value);
    }

    pub fn write_cm_color(&mut self, color: &Color) {
        self.main.write_cm_color(color);
    }

    pub fn write_cm_color_with_names(
        &mut self,
        color: &Color,
        color_name: Option<&str>,
        book_name: Option<&str>,
    ) {
        if !self.version().r2004_plus() {
            self.main.write_cm_color(color);
            return;
        }

        self.main.write_bit_short(0);
        let color_long = match color {
            Color::Rgb { r, g, b } => {
                (*b as u32) | ((*g as u32) << 8) | ((*r as u32) << 16) | (0xC2u32 << 24)
            }
            Color::ByLayer => 0xC0u32 << 24,
            Color::None => 0xC8u32 << 24,
            Color::ByBlock => 0xC1u32 << 24,
            Color::Index(index) => (*index as u32) | (0xC3u32 << 24),
        };
        self.main.write_bit_long(color_long as i32);
        self.main
            .write_byte(u8::from(color_name.is_some()) | (u8::from(book_name.is_some()) << 1));
        if let Some(name) = color_name {
            self.write_variable_text(name);
        }
        if let Some(name) = book_name {
            self.write_variable_text(name);
        }
    }

    pub fn write_cm_true_color(&mut self, color: &Color) {
        self.main.write_cm_true_color(color);
    }

    pub fn write_en_color(&mut self, color: &Color, transparency: &Transparency) {
        self.main.write_en_color(color, transparency);
    }

    pub fn write_en_color_with_book(
        &mut self,
        color: &Color,
        transparency: &Transparency,
        is_book_color: bool,
    ) {
        self.main
            .write_en_color_with_book(color, transparency, is_book_color);
    }

    pub fn write_datetime(&mut self, julian_day: i32, milliseconds: i32) {
        self.main.write_datetime(julian_day, milliseconds);
    }

    pub fn write_8bit_julian_date(&mut self, julian_day: i32, milliseconds: i32) {
        self.main.write_8bit_julian_date(julian_day, milliseconds);
    }

    pub fn write_timespan(&mut self, days: i32, milliseconds: i32) {
        self.main.write_timespan(days, milliseconds);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Stream delegation — text writes (go to text stream in 3-stream mode)
    // ════════════════════════════════════════════════════════════════════════

    /// Write a variable-length text string.
    ///
    /// In three-stream mode (R2007+), text goes to the separate text stream.
    /// In two-stream mode (R13–R2004), text goes directly into main.
    pub fn write_variable_text(&mut self, value: &str) {
        match self.mode {
            MergeMode::ThreeStream => self.text.write_variable_text(value),
            MergeMode::TwoStream => self.main.write_variable_text(value),
        }
    }

    /// Write one preserved opaque bit to the text stream.
    pub fn write_text_bit(&mut self, value: bool) {
        match self.mode {
            MergeMode::ThreeStream => self.text.write_bit(value),
            MergeMode::TwoStream => self.main.write_bit(value),
        }
    }

    /// Write a Unicode text string.
    pub fn write_text_unicode(&mut self, value: &str) {
        match self.mode {
            MergeMode::ThreeStream => self.text.write_text_unicode(value),
            MergeMode::TwoStream => self.main.write_text_unicode(value),
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Stream delegation — handle writes (always go to handle stream)
    // ════════════════════════════════════════════════════════════════════════

    /// Write a handle reference to the handle stream.
    pub fn write_handle(&mut self, ref_type: DwgReferenceType, handle: u64) {
        self.handle.write_handle(ref_type, handle);
    }

    /// Write a compact handle offset relative to the current object's handle.
    pub fn write_handle_relative(&mut self, reference_handle: u64, handle: u64) {
        self.handle.write_handle_relative(reference_handle, handle);
    }

    /// Write a handle reference into the MAIN (data) stream, for objects that
    /// store some handle refs inline in the data section rather than the handle
    /// stream — e.g. the SORTENTSTABLE sort handles. Symmetric with the
    /// reader's `read_main_handle`. (#146)
    pub fn write_main_handle(&mut self, ref_type: DwgReferenceType, handle: u64) {
        self.main.write_handle(ref_type, handle);
    }

    /// Write an undefined-type handle reference.
    pub fn write_handle_undefined(&mut self, handle: u64) {
        self.handle.write_handle_undefined(handle);
    }

    // ════════════════════════════════════════════════════════════════════════
    //  Stream merging
    // ════════════════════════════════════════════════════════════════════════

    /// Merge the three streams into the final object record bytes.
    ///
    /// This is the "spear shift" / finalize operation that combines
    /// main + text + handle according to the version protocol.
    ///
    /// Returns the merged bytes representing one complete object record.
    pub fn merge(&mut self) -> Vec<u8> {
        match self.mode {
            MergeMode::TwoStream => self.merge_two_stream(),
            MergeMode::ThreeStream => self.merge_three_stream(),
        }
    }

    /// Two-stream merge (R13–R2004):
    ///
    /// 1. Record main bit position
    /// 2. Pad main to byte boundary
    /// 3. If saved position, patch the size field (BL at saved position = main size in bits)
    /// 4. Pad handle stream, append handle bytes to main
    /// 5. Pad result to byte boundary
    fn merge_two_stream(&mut self) -> Vec<u8> {
        let main_size_bits = self.main.position_in_bits();

        // Pad main to byte boundary
        self.main.write_spear_shift();

        // Patch size if we saved a position
        if self.saved_position {
            let saved_pos = self.position_in_bits;
            self.main.set_position_in_bits(saved_pos);
            self.main.write_raw_long(main_size_bits as i32);
            self.main.write_shift_value();
            // Seek back to end of main
            self.main.set_position_in_bits(main_size_bits);
        }

        // Append handle stream
        self.handle.write_spear_shift();
        self.handle_start_bits = self.main.position_in_bits();
        self.handle.flush();
        self.main.write_bytes(self.handle.buffer());
        self.main.write_spear_shift();

        self.main.take_bytes()
    }

    /// Three-stream merge (R2007+):
    ///
    /// Layout: `|type_code|RL|---main_data---|---text_data---|flag_words|flag_bit|pad|---handles---|`
    ///
    /// The RL (size in bits) stored at the saved position is an **absolute**
    /// bit position measured from bit 0 of the merged data:
    ///
    /// - **Per-object**: RL = flag_position + 1 (one past the flag bit)
    /// - **Section**: RL = flag_position (position OF the flag bit)
    ///
    /// The reader computes:
    /// - flag at RL − 1 (per-object) or at RL (section)
    /// - handles immediately after the flag bit
    ///
    /// 1. Record main and text bit sizes
    /// 2. If saved position: compute total bits, patch size field
    /// 3. Seek to end of main; if text present:
    ///    a. Pad text and append text bytes to main
    ///    b. Write text-size flag at the text boundary (set_position_by_flag)
    ///    c. Write `true` bit (text present flag)
    /// 4. If text not present: write `false` bit
    /// 5. Append the handle stream
    fn merge_three_stream(&mut self) -> Vec<u8> {
        let main_size_bits = self.main.position_in_bits();
        let text_size_bits = self.text.position_in_bits();

        // Pad main to byte boundary so text and flag writes don't
        // corrupt the last partial byte of entity data
        self.main.write_spear_shift();

        if self.saved_position {
            let saved_pos = self.position_in_bits;

            // RL = mainSizeBits + textSizeBits + 1 (flag bit) + flag_words.
            let mut total_bits = main_size_bits + text_size_bits + 1;
            if text_size_bits > 0 {
                total_bits += 16;
                if text_size_bits >= 0x8000 {
                    total_bits += 16;
                    if text_size_bits >= 0x40000000 {
                        total_bits += 16;
                    }
                }
            }

            self.main.set_position_in_bits(saved_pos);
            self.main.write_raw_long(total_bits as i32);
            self.main.write_shift_value();
        }

        // Seek to end of main (pre-padding position).
        // This effectively "rolls back" the spear-shift padding so that
        // text data is placed immediately after the meaningful main bits.
        self.main.set_position_in_bits(main_size_bits);

        if text_size_bits > 0 {
            // Append text stream bytes (byte-aligned) after main data
            self.text.write_spear_shift();
            self.text.flush();
            self.main.write_bytes(self.text.buffer());

            // Byte-align after text bytes — ensures the buffer is large
            // enough before we seek back to write flag words.
            self.main.write_spear_shift();

            // The text data occupies bits [main_size_bits .. main_size_bits + text_size_bits).
            // At the text boundary we write the text-size flag and the
            // text-present bit.  This overwrites the zero-padding bits
            // that resulted from flushing the text stream.
            self.main
                .set_position_in_bits(main_size_bits + text_size_bits);
            self.main.set_position_by_flag(text_size_bits);
            self.main.write_bit(true); // text present
        } else {
            self.main.write_bit(false); // no text
        }

        // Record handle start position.
        self.handle.write_spear_shift();
        self.handle_start_bits = self.main.position_in_bits();

        // Append handle bytes.
        self.handle.flush();
        self.main.write_bytes(self.handle.buffer());

        // Final byte-alignment for CRC computation.
        self.main.write_spear_shift();

        self.main.take_bytes()
    }

    /// Get the bit position where the handle section starts in the merged data.
    /// Used by R2010+ record framing to compute MC handle_bits.
    pub fn handle_start_bits(&self) -> i64 {
        self.handle_start_bits
    }

    /// Get the current position in bits after the last merge.
    ///
    /// Used by the handle section writer to know the byte offset.
    pub fn last_saved_position_in_bits(&self) -> i64 {
        self.main.saved_position_in_bits()
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_stream_basic() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC15, DxfVersion::AC1015);

        // Write some data to main
        w.write_bit_short(42);
        // Write a handle
        w.write_handle(DwgReferenceType::SoftPointer, 0x10);
        // Text in two-stream mode goes to main
        w.write_variable_text("Hello");

        let bytes = w.merge();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_three_stream_basic() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC24, DxfVersion::AC1024);

        // Data to main
        w.write_bit_short(42);
        // Handle to handle stream
        w.write_handle(DwgReferenceType::SoftPointer, 0x10);
        // Text to text stream
        w.write_variable_text("Hello");

        let bytes = w.merge();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_two_stream_with_size() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC18, DxfVersion::AC1018);

        w.save_position_for_size();
        w.write_bit_short(100);
        w.write_handle(DwgReferenceType::HardPointer, 0x20);

        let bytes = w.merge();
        assert!(!bytes.is_empty());
        // Size should be patched in the first 4 bytes (after start)
    }

    #[test]
    fn test_three_stream_with_size() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC24, DxfVersion::AC1024);

        w.save_position_for_size();
        w.write_bit_short(100);
        w.write_variable_text("Test");
        w.write_handle(DwgReferenceType::SoftOwnership, 0x30);

        let bytes = w.merge();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_three_stream_no_text() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC24, DxfVersion::AC1024);

        w.save_position_for_size();
        w.write_bit_short(7);
        w.write_handle(DwgReferenceType::SoftPointer, 0x01);
        // No text written

        let bytes = w.merge();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_reset() {
        let mut w = DwgMergedWriter::new(DwgVersion::AC15, DxfVersion::AC1015);

        w.write_byte(0xFF);
        w.write_handle(DwgReferenceType::Undefined, 1);
        w.reset();

        assert_eq!(w.main().position_in_bits(), 0);
        assert_eq!(w.text().position_in_bits(), 0);
        assert_eq!(w.handle_writer().position_in_bits(), 0);
    }
}
