//! AC1021 (R2007) compressed metadata structure
//!
//! The `Dwg21CompressedMetadata` struct holds the fields from the
//! 0x110-byte decompressed metadata block in the AC1021 file header.
//!
//! This block is obtained by:
//! 1. Reading 0x400 bytes of Reed-Solomon encoded data
//! 2. Decoding with factor=3, block_size=239
//! 3. LZ77 AC21 decompressing into a 0x110-byte buffer
//!
//! Based on the reference `Dwg21CompressedMetadata` class.

use crate::error::DxfError;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// Decompressed metadata from the AC1021 file header.
///
/// All fields are 8 bytes (u64) in the binary format, stored as
/// little-endian unsigned 64-bit integers. The total structure
/// is 0x110 (272) bytes = 34 × 8 bytes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dwg21CompressedMetadata {
    /// Header size (expected: 0x70)
    pub header_size: u64,
    /// Total file size
    pub file_size: u64,
    /// CRC of compressed pages map data
    pub pages_map_crc_compressed: u64,
    /// Reed-Solomon correction factor for pages map
    pub pages_map_correction_factor: u64,
    /// CRC seed for pages map
    pub pages_map_crc_seed: u64,
    /// Offset to the second map (Map2)
    pub map2_offset: u64,
    /// ID of the second map
    pub map2_id: u64,
    /// Offset to the pages map (relative to 0x480)
    pub pages_map_offset: u64,
    /// ID of the pages map
    pub pages_map_id: u64,
    /// Offset to the second file header copy
    pub header2_offset: u64,
    /// Compressed size of the pages map
    pub pages_map_size_compressed: u64,
    /// Uncompressed size of the pages map
    pub pages_map_size_uncompressed: u64,
    /// Total number of pages
    pub pages_amount: u64,
    /// Maximum page ID
    pub pages_max_id: u64,
    /// Unknown constant (expected: 0x20 = 32)
    pub unknown_0x20: u64,
    /// Unknown constant (expected: 0x40 = 64)
    pub unknown_0x40: u64,
    /// CRC of uncompressed pages map
    pub pages_map_crc_uncompressed: u64,
    /// Unknown constant (expected: 0xF800)
    pub unknown_0xf800: u64,
    /// Unknown constant (expected: 4)
    pub unknown_4: u64,
    /// Unknown constant (expected: 1)
    pub unknown_1: u64,
    /// Number of sections
    pub sections_amount: u64,
    /// CRC of uncompressed sections map
    pub sections_map_crc_uncompressed: u64,
    /// Compressed size of sections map
    pub sections_map_size_compressed: u64,
    /// ID of second sections map
    pub sections_map2_id: u64,
    /// ID of sections map
    pub sections_map_id: u64,
    /// Uncompressed size of sections map
    pub sections_map_size_uncompressed: u64,
    /// CRC of compressed sections map
    pub sections_map_crc_compressed: u64,
    /// Reed-Solomon correction factor for sections map
    pub sections_map_correction_factor: u64,
    /// CRC seed for sections map
    pub sections_map_crc_seed: u64,
    /// Stream version (expected: 0x60100, spec §5.2)
    pub stream_version: u64,
    /// CRC seed
    pub crc_seed: u64,
    /// Encoded CRC seed
    pub crc_seed_encoded: u64,
    /// Random seed
    pub random_seed: u64,
    /// **Header CRC-64** — the 64-bit integrity checksum for the header
    ///
    /// This is the key value for CRC64 extraction. It is positioned at
    /// offset 0x108 in the decompressed metadata block.
    pub header_crc64: u64,
}

impl Dwg21CompressedMetadata {
    /// Parse the compressed metadata from a decompressed 0x110-byte buffer.
    ///
    /// # Arguments
    /// * `data` - Decompressed buffer (must be at least 0x110 = 272 bytes)
    ///
    /// # Returns
    /// Parsed metadata structure with all fields including `header_crc64`.
    pub fn from_bytes(data: &[u8]) -> Result<Self, DxfError> {
        if data.len() < 0x110 {
            return Err(DxfError::InvalidFormat(format!(
                "AC1021 metadata buffer too small: {} bytes, expected at least 0x110 (272)",
                data.len()
            )));
        }

        let mut cursor = Cursor::new(data);

        Ok(Self {
            header_size: cursor.read_u64::<LittleEndian>()?,
            file_size: cursor.read_u64::<LittleEndian>()?,
            pages_map_crc_compressed: cursor.read_u64::<LittleEndian>()?,
            pages_map_correction_factor: cursor.read_u64::<LittleEndian>()?,
            pages_map_crc_seed: cursor.read_u64::<LittleEndian>()?,
            map2_offset: cursor.read_u64::<LittleEndian>()?,
            map2_id: cursor.read_u64::<LittleEndian>()?,
            pages_map_offset: cursor.read_u64::<LittleEndian>()?,
            pages_map_id: cursor.read_u64::<LittleEndian>()?,
            header2_offset: cursor.read_u64::<LittleEndian>()?,
            pages_map_size_compressed: cursor.read_u64::<LittleEndian>()?,
            pages_map_size_uncompressed: cursor.read_u64::<LittleEndian>()?,
            pages_amount: cursor.read_u64::<LittleEndian>()?,
            pages_max_id: cursor.read_u64::<LittleEndian>()?,
            unknown_0x20: cursor.read_u64::<LittleEndian>()?,
            unknown_0x40: cursor.read_u64::<LittleEndian>()?,
            pages_map_crc_uncompressed: cursor.read_u64::<LittleEndian>()?,
            unknown_0xf800: cursor.read_u64::<LittleEndian>()?,
            unknown_4: cursor.read_u64::<LittleEndian>()?,
            unknown_1: cursor.read_u64::<LittleEndian>()?,
            sections_amount: cursor.read_u64::<LittleEndian>()?,
            sections_map_crc_uncompressed: cursor.read_u64::<LittleEndian>()?,
            sections_map_size_compressed: cursor.read_u64::<LittleEndian>()?,
            sections_map2_id: cursor.read_u64::<LittleEndian>()?,
            sections_map_id: cursor.read_u64::<LittleEndian>()?,
            sections_map_size_uncompressed: cursor.read_u64::<LittleEndian>()?,
            sections_map_crc_compressed: cursor.read_u64::<LittleEndian>()?,
            sections_map_correction_factor: cursor.read_u64::<LittleEndian>()?,
            sections_map_crc_seed: cursor.read_u64::<LittleEndian>()?,
            stream_version: cursor.read_u64::<LittleEndian>()?,
            crc_seed: cursor.read_u64::<LittleEndian>()?,
            crc_seed_encoded: cursor.read_u64::<LittleEndian>()?,
            random_seed: cursor.read_u64::<LittleEndian>()?,
            header_crc64: cursor.read_u64::<LittleEndian>()?,
        })
    }
}

/// Size of the serialized metadata buffer in bytes (34 × 8 = 272 = 0x110).
pub const METADATA_SIZE: usize = 0x110;

impl Default for Dwg21CompressedMetadata {
    /// Create metadata with standard constants from spec §5.2.
    ///
    /// All address/size/CRC fields are zeroed — they must be filled in
    /// by the writer once the actual layout is known.
    fn default() -> Self {
        Self {
            header_size: 0x70,
            file_size: 0,
            pages_map_crc_compressed: 0,
            pages_map_correction_factor: 0,
            pages_map_crc_seed: 0,
            map2_offset: 0,
            map2_id: 0,
            pages_map_offset: 0,
            pages_map_id: 0,
            header2_offset: 0,
            pages_map_size_compressed: 0,
            pages_map_size_uncompressed: 0,
            pages_amount: 0,
            pages_max_id: 0,
            unknown_0x20: 0x20,
            unknown_0x40: 0x40,
            pages_map_crc_uncompressed: 0,
            unknown_0xf800: 0xF800,
            unknown_4: 4,
            unknown_1: 1,
            sections_amount: 0,
            sections_map_crc_uncompressed: 0,
            sections_map_size_compressed: 0,
            sections_map2_id: 0,
            sections_map_id: 0,
            sections_map_size_uncompressed: 0,
            sections_map_crc_compressed: 0,
            sections_map_correction_factor: 0,
            sections_map_crc_seed: 0,
            stream_version: 0x60100,
            crc_seed: 0,
            crc_seed_encoded: 0,
            random_seed: 0,
            header_crc64: 0,
        }
    }
}

impl Dwg21CompressedMetadata {
    /// Serialize all 34 fields as little-endian u64 into a 0x110-byte buffer.
    ///
    /// This is the exact inverse of [`from_bytes()`](Self::from_bytes).
    /// The caller is responsible for computing and writing the `header_crc64`
    /// field before calling this method (or patching byte offset 0x108
    /// in the returned buffer afterwards).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(METADATA_SIZE);
        // Write all 34 fields in order — must match from_bytes() exactly
        buf.write_u64::<LittleEndian>(self.header_size).unwrap();
        buf.write_u64::<LittleEndian>(self.file_size).unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_crc_compressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_correction_factor)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_crc_seed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.map2_offset).unwrap();
        buf.write_u64::<LittleEndian>(self.map2_id).unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_offset)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_id).unwrap();
        buf.write_u64::<LittleEndian>(self.header2_offset).unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_size_compressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_size_uncompressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.pages_amount).unwrap();
        buf.write_u64::<LittleEndian>(self.pages_max_id).unwrap();
        buf.write_u64::<LittleEndian>(self.unknown_0x20).unwrap();
        buf.write_u64::<LittleEndian>(self.unknown_0x40).unwrap();
        buf.write_u64::<LittleEndian>(self.pages_map_crc_uncompressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.unknown_0xf800).unwrap();
        buf.write_u64::<LittleEndian>(self.unknown_4).unwrap();
        buf.write_u64::<LittleEndian>(self.unknown_1).unwrap();
        buf.write_u64::<LittleEndian>(self.sections_amount).unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_crc_uncompressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_size_compressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map2_id)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_id).unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_size_uncompressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_crc_compressed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_correction_factor)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.sections_map_crc_seed)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.stream_version).unwrap();
        buf.write_u64::<LittleEndian>(self.crc_seed).unwrap();
        buf.write_u64::<LittleEndian>(self.crc_seed_encoded)
            .unwrap();
        buf.write_u64::<LittleEndian>(self.random_seed).unwrap();
        buf.write_u64::<LittleEndian>(self.header_crc64).unwrap();
        debug_assert_eq!(buf.len(), METADATA_SIZE);
        buf
    }
}

impl std::fmt::Display for Dwg21CompressedMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "AC1021 Compressed Metadata:")?;
        writeln!(f, "  Header Size:                {:#X}", self.header_size)?;
        writeln!(f, "  File Size:                  {}", self.file_size)?;
        writeln!(
            f,
            "  Pages Map CRC (compressed): {:#018X}",
            self.pages_map_crc_compressed
        )?;
        writeln!(
            f,
            "  Pages Map Correction Factor: {}",
            self.pages_map_correction_factor
        )?;
        writeln!(
            f,
            "  Pages Map CRC Seed:         {:#018X}",
            self.pages_map_crc_seed
        )?;
        writeln!(f, "  Map2 Offset:                {:#X}", self.map2_offset)?;
        writeln!(f, "  Map2 ID:                    {}", self.map2_id)?;
        writeln!(
            f,
            "  Pages Map Offset:           {:#X}",
            self.pages_map_offset
        )?;
        writeln!(f, "  Pages Map ID:               {}", self.pages_map_id)?;
        writeln!(
            f,
            "  Header2 Offset:             {:#X}",
            self.header2_offset
        )?;
        writeln!(
            f,
            "  Pages Map Size Compressed:  {}",
            self.pages_map_size_compressed
        )?;
        writeln!(
            f,
            "  Pages Map Size Uncompressed: {}",
            self.pages_map_size_uncompressed
        )?;
        writeln!(f, "  Pages Amount:               {}", self.pages_amount)?;
        writeln!(f, "  Pages Max ID:               {}", self.pages_max_id)?;
        writeln!(f, "  Sections Amount:            {}", self.sections_amount)?;
        writeln!(f, "  Sections Map ID:            {}", self.sections_map_id)?;
        writeln!(f, "  Stream Version:             {}", self.stream_version)?;
        writeln!(f, "  CRC Seed:                   {:#018X}", self.crc_seed)?;
        writeln!(
            f,
            "  CRC Seed Encoded:           {:#018X}",
            self.crc_seed_encoded
        )?;
        writeln!(
            f,
            "  Random Seed:                {:#018X}",
            self.random_seed
        )?;
        writeln!(
            f,
            "  Header CRC-64:              {:#018X}",
            self.header_crc64
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_too_small() {
        let data = vec![0u8; 100];
        let result = Dwg21CompressedMetadata::from_bytes(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_parse_zeros() {
        let data = vec![0u8; 0x110];
        let meta = Dwg21CompressedMetadata::from_bytes(&data).unwrap();
        assert_eq!(meta.header_size, 0);
        assert_eq!(meta.header_crc64, 0);
    }

    #[test]
    fn test_metadata_crc64_position() {
        // Place a known value at offset 0x108 (the CRC64 field)
        let mut data = vec![0u8; 0x110];
        // Offset 0x108 = byte 264
        let crc_value: u64 = 0xDEADBEEF_CAFEBABE;
        data[0x108..0x110].copy_from_slice(&crc_value.to_le_bytes());
        let meta = Dwg21CompressedMetadata::from_bytes(&data).unwrap();
        assert_eq!(meta.header_crc64, 0xDEADBEEF_CAFEBABE);
    }

    // ─── Default impl tests ─────────────────────────────────────────

    #[test]
    fn test_default_constants() {
        let meta = Dwg21CompressedMetadata::default();
        // Spec §5.2 "normally" values
        assert_eq!(meta.header_size, 0x70);
        assert_eq!(meta.unknown_0x20, 0x20);
        assert_eq!(meta.unknown_0x40, 0x40);
        assert_eq!(meta.unknown_0xf800, 0xF800);
        assert_eq!(meta.unknown_4, 4);
        assert_eq!(meta.unknown_1, 1);
        assert_eq!(meta.stream_version, 0x60100);
    }

    #[test]
    fn test_default_zeroed_fields() {
        let meta = Dwg21CompressedMetadata::default();
        // All address/size/CRC fields must be zero by default
        assert_eq!(meta.file_size, 0);
        assert_eq!(meta.pages_map_crc_compressed, 0);
        assert_eq!(meta.pages_map_correction_factor, 0);
        assert_eq!(meta.pages_map_crc_seed, 0);
        assert_eq!(meta.map2_offset, 0);
        assert_eq!(meta.map2_id, 0);
        assert_eq!(meta.pages_map_offset, 0);
        assert_eq!(meta.pages_map_id, 0);
        assert_eq!(meta.header2_offset, 0);
        assert_eq!(meta.pages_map_size_compressed, 0);
        assert_eq!(meta.pages_map_size_uncompressed, 0);
        assert_eq!(meta.pages_amount, 0);
        assert_eq!(meta.pages_max_id, 0);
        assert_eq!(meta.pages_map_crc_uncompressed, 0);
        assert_eq!(meta.sections_amount, 0);
        assert_eq!(meta.sections_map_crc_uncompressed, 0);
        assert_eq!(meta.sections_map_size_compressed, 0);
        assert_eq!(meta.sections_map2_id, 0);
        assert_eq!(meta.sections_map_id, 0);
        assert_eq!(meta.sections_map_size_uncompressed, 0);
        assert_eq!(meta.sections_map_crc_compressed, 0);
        assert_eq!(meta.sections_map_correction_factor, 0);
        assert_eq!(meta.sections_map_crc_seed, 0);
        assert_eq!(meta.crc_seed, 0);
        assert_eq!(meta.crc_seed_encoded, 0);
        assert_eq!(meta.random_seed, 0);
        assert_eq!(meta.header_crc64, 0);
    }

    // ─── to_bytes() tests ───────────────────────────────────────────

    #[test]
    fn test_to_bytes_size() {
        let meta = Dwg21CompressedMetadata::default();
        let bytes = meta.to_bytes();
        assert_eq!(bytes.len(), METADATA_SIZE);
        assert_eq!(bytes.len(), 0x110);
    }

    #[test]
    fn test_to_bytes_from_bytes_roundtrip_default() {
        let original = Dwg21CompressedMetadata::default();
        let bytes = original.to_bytes();
        let parsed = Dwg21CompressedMetadata::from_bytes(&bytes).unwrap();

        // Verify all 34 fields survive the roundtrip
        assert_eq!(original.header_size, parsed.header_size);
        assert_eq!(original.file_size, parsed.file_size);
        assert_eq!(
            original.pages_map_crc_compressed,
            parsed.pages_map_crc_compressed
        );
        assert_eq!(
            original.pages_map_correction_factor,
            parsed.pages_map_correction_factor
        );
        assert_eq!(original.pages_map_crc_seed, parsed.pages_map_crc_seed);
        assert_eq!(original.map2_offset, parsed.map2_offset);
        assert_eq!(original.map2_id, parsed.map2_id);
        assert_eq!(original.pages_map_offset, parsed.pages_map_offset);
        assert_eq!(original.pages_map_id, parsed.pages_map_id);
        assert_eq!(original.header2_offset, parsed.header2_offset);
        assert_eq!(
            original.pages_map_size_compressed,
            parsed.pages_map_size_compressed
        );
        assert_eq!(
            original.pages_map_size_uncompressed,
            parsed.pages_map_size_uncompressed
        );
        assert_eq!(original.pages_amount, parsed.pages_amount);
        assert_eq!(original.pages_max_id, parsed.pages_max_id);
        assert_eq!(original.unknown_0x20, parsed.unknown_0x20);
        assert_eq!(original.unknown_0x40, parsed.unknown_0x40);
        assert_eq!(
            original.pages_map_crc_uncompressed,
            parsed.pages_map_crc_uncompressed
        );
        assert_eq!(original.unknown_0xf800, parsed.unknown_0xf800);
        assert_eq!(original.unknown_4, parsed.unknown_4);
        assert_eq!(original.unknown_1, parsed.unknown_1);
        assert_eq!(original.sections_amount, parsed.sections_amount);
        assert_eq!(
            original.sections_map_crc_uncompressed,
            parsed.sections_map_crc_uncompressed
        );
        assert_eq!(
            original.sections_map_size_compressed,
            parsed.sections_map_size_compressed
        );
        assert_eq!(original.sections_map2_id, parsed.sections_map2_id);
        assert_eq!(original.sections_map_id, parsed.sections_map_id);
        assert_eq!(
            original.sections_map_size_uncompressed,
            parsed.sections_map_size_uncompressed
        );
        assert_eq!(
            original.sections_map_crc_compressed,
            parsed.sections_map_crc_compressed
        );
        assert_eq!(
            original.sections_map_correction_factor,
            parsed.sections_map_correction_factor
        );
        assert_eq!(original.sections_map_crc_seed, parsed.sections_map_crc_seed);
        assert_eq!(original.stream_version, parsed.stream_version);
        assert_eq!(original.crc_seed, parsed.crc_seed);
        assert_eq!(original.crc_seed_encoded, parsed.crc_seed_encoded);
        assert_eq!(original.random_seed, parsed.random_seed);
        assert_eq!(original.header_crc64, parsed.header_crc64);
    }

    #[test]
    fn test_to_bytes_from_bytes_roundtrip_populated() {
        // Fill every field with a distinct non-zero value to catch
        // any field ordering mismatch between to_bytes and from_bytes
        let original = Dwg21CompressedMetadata {
            header_size: 0x70,
            file_size: 0x1234_5678_9ABC_DEF0,
            pages_map_crc_compressed: 0xAAAA_BBBB_CCCC_DDDD,
            pages_map_correction_factor: 3,
            pages_map_crc_seed: 0x1111_2222_3333_4444,
            map2_offset: 0x0800,
            map2_id: 2,
            pages_map_offset: 0x0400,
            pages_map_id: 1,
            header2_offset: 0x5000,
            pages_map_size_compressed: 0x300,
            pages_map_size_uncompressed: 0x500,
            pages_amount: 15,
            pages_max_id: 20,
            unknown_0x20: 0x20,
            unknown_0x40: 0x40,
            pages_map_crc_uncompressed: 0x5555_6666_7777_8888,
            unknown_0xf800: 0xF800,
            unknown_4: 4,
            unknown_1: 1,
            sections_amount: 14,
            sections_map_crc_uncompressed: 0x9999_AAAA_BBBB_CCCC,
            sections_map_size_compressed: 0x200,
            sections_map2_id: 4,
            sections_map_id: 3,
            sections_map_size_uncompressed: 0x400,
            sections_map_crc_compressed: 0xDDDD_EEEE_FFFF_0000,
            sections_map_correction_factor: 5,
            sections_map_crc_seed: 0x0101_0202_0303_0404,
            stream_version: 0x60100,
            crc_seed: 0xFEDC_BA98_7654_3210,
            crc_seed_encoded: 0xABCD_EF01_2345_6789,
            random_seed: 0x0F0E_0D0C_0B0A_0908,
            header_crc64: 0xDEAD_BEEF_CAFE_BABE,
        };

        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), METADATA_SIZE);

        let parsed = Dwg21CompressedMetadata::from_bytes(&bytes).unwrap();

        // Verify every field roundtrips correctly
        assert_eq!(parsed.header_size, 0x70);
        assert_eq!(parsed.file_size, 0x1234_5678_9ABC_DEF0);
        assert_eq!(parsed.pages_map_crc_compressed, 0xAAAA_BBBB_CCCC_DDDD);
        assert_eq!(parsed.pages_map_correction_factor, 3);
        assert_eq!(parsed.pages_map_crc_seed, 0x1111_2222_3333_4444);
        assert_eq!(parsed.map2_offset, 0x0800);
        assert_eq!(parsed.map2_id, 2);
        assert_eq!(parsed.pages_map_offset, 0x0400);
        assert_eq!(parsed.pages_map_id, 1);
        assert_eq!(parsed.header2_offset, 0x5000);
        assert_eq!(parsed.pages_map_size_compressed, 0x300);
        assert_eq!(parsed.pages_map_size_uncompressed, 0x500);
        assert_eq!(parsed.pages_amount, 15);
        assert_eq!(parsed.pages_max_id, 20);
        assert_eq!(parsed.unknown_0x20, 0x20);
        assert_eq!(parsed.unknown_0x40, 0x40);
        assert_eq!(parsed.pages_map_crc_uncompressed, 0x5555_6666_7777_8888);
        assert_eq!(parsed.unknown_0xf800, 0xF800);
        assert_eq!(parsed.unknown_4, 4);
        assert_eq!(parsed.unknown_1, 1);
        assert_eq!(parsed.sections_amount, 14);
        assert_eq!(parsed.sections_map_crc_uncompressed, 0x9999_AAAA_BBBB_CCCC);
        assert_eq!(parsed.sections_map_size_compressed, 0x200);
        assert_eq!(parsed.sections_map2_id, 4);
        assert_eq!(parsed.sections_map_id, 3);
        assert_eq!(parsed.sections_map_size_uncompressed, 0x400);
        assert_eq!(parsed.sections_map_crc_compressed, 0xDDDD_EEEE_FFFF_0000);
        assert_eq!(parsed.sections_map_correction_factor, 5);
        assert_eq!(parsed.sections_map_crc_seed, 0x0101_0202_0303_0404);
        assert_eq!(parsed.stream_version, 0x60100);
        assert_eq!(parsed.crc_seed, 0xFEDC_BA98_7654_3210);
        assert_eq!(parsed.crc_seed_encoded, 0xABCD_EF01_2345_6789);
        assert_eq!(parsed.random_seed, 0x0F0E_0D0C_0B0A_0908);
        assert_eq!(parsed.header_crc64, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[test]
    fn test_to_bytes_field_offsets() {
        // Verify specific fields land at their spec-defined byte offsets
        let mut meta = Dwg21CompressedMetadata::default();
        meta.header_size = 0x70;
        meta.header_crc64 = 0xDEAD_BEEF_CAFE_BABE;
        meta.file_size = 0x1234;

        let bytes = meta.to_bytes();

        // header_size at offset 0x00
        assert_eq!(
            u64::from_le_bytes(bytes[0x00..0x08].try_into().unwrap()),
            0x70
        );
        // file_size at offset 0x08
        assert_eq!(
            u64::from_le_bytes(bytes[0x08..0x10].try_into().unwrap()),
            0x1234
        );
        // unknown_0x20 at offset 0x70
        assert_eq!(
            u64::from_le_bytes(bytes[0x70..0x78].try_into().unwrap()),
            0x20
        );
        // unknown_0x40 at offset 0x78
        assert_eq!(
            u64::from_le_bytes(bytes[0x78..0x80].try_into().unwrap()),
            0x40
        );
        // unknown_0xf800 at offset 0x88
        assert_eq!(
            u64::from_le_bytes(bytes[0x88..0x90].try_into().unwrap()),
            0xF800
        );
        // stream_version at offset 0xE8
        assert_eq!(
            u64::from_le_bytes(bytes[0xE8..0xF0].try_into().unwrap()),
            0x60100
        );
        // header_crc64 at offset 0x108
        assert_eq!(
            u64::from_le_bytes(bytes[0x108..0x110].try_into().unwrap()),
            0xDEAD_BEEF_CAFE_BABE
        );
    }

    #[test]
    fn test_to_bytes_byte_level_roundtrip() {
        // Verify that to_bytes → from_bytes → to_bytes produces identical bytes
        let meta = Dwg21CompressedMetadata {
            header_size: 0x70,
            file_size: 999999,
            pages_map_crc_compressed: 0x42,
            pages_map_correction_factor: 3,
            pages_map_crc_seed: 0xFF,
            map2_offset: 0x800,
            map2_id: 2,
            pages_map_offset: 0,
            pages_map_id: 1,
            header2_offset: 0x5000,
            pages_map_size_compressed: 100,
            pages_map_size_uncompressed: 200,
            pages_amount: 10,
            pages_max_id: 12,
            unknown_0x20: 0x20,
            unknown_0x40: 0x40,
            pages_map_crc_uncompressed: 0xAB,
            unknown_0xf800: 0xF800,
            unknown_4: 4,
            unknown_1: 1,
            sections_amount: 14,
            sections_map_crc_uncompressed: 0xCD,
            sections_map_size_compressed: 50,
            sections_map2_id: 4,
            sections_map_id: 3,
            sections_map_size_uncompressed: 80,
            sections_map_crc_compressed: 0xEF,
            sections_map_correction_factor: 5,
            sections_map_crc_seed: 0x11,
            stream_version: 0x60100,
            crc_seed: 0x22,
            crc_seed_encoded: 0x33,
            random_seed: 0x44,
            header_crc64: 0x55,
        };

        let bytes1 = meta.to_bytes();
        let parsed = Dwg21CompressedMetadata::from_bytes(&bytes1).unwrap();
        let bytes2 = parsed.to_bytes();
        assert_eq!(bytes1, bytes2, "byte-level roundtrip failed");
    }
}
