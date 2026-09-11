//! DWG Classes section reader
//!
//! Reads the AcDb:Classes section from a DWG file, producing a
//! `DxfClassCollection`. Mirrors the reference `DwgClassesReader`.
//!
//! ## Section layout
//!
//! ```text
//! ┌──────────────────┐
//! │ Start sentinel   │ 16 bytes
//! │ Section size (RL)│ 4 bytes
//! │ Class entries... │ variable
//! │ CRC-16           │ 2 bytes
//! │ End sentinel     │ 16 bytes
//! └──────────────────┘
//! ```

use crate::classes::{DxfClass, DxfClassCollection, ProxyFlags};
use crate::error::{DxfError, Result};
use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::io::dwg::file_headers::section_definition::{end_sentinels, start_sentinels};
use crate::types::DxfVersion;

/// Read the Classes section from raw section bytes (already decompressed).
///
/// # Arguments
/// * `data` - Complete section buffer (includes sentinels)
/// * `version` - DXF version for version-specific parsing
///
/// # Returns
/// `DxfClassCollection` containing all parsed class definitions.
pub fn read_classes(
    data: &[u8],
    version: DxfVersion,
    maintenance_version: u8,
) -> Result<DxfClassCollection> {
    read_classes_with_encoding(
        data,
        version,
        maintenance_version,
        encoding_rs::WINDOWS_1252,
    )
}

pub fn read_classes_with_encoding(
    data: &[u8],
    version: DxfVersion,
    maintenance_version: u8,
    encoding: &'static encoding_rs::Encoding,
) -> Result<DxfClassCollection> {
    let dwg = DwgVersion::from_dxf_version(version)?;

    // ── Verify start sentinel ──
    if data.len() < 34 {
        return Err(DxfError::Parse("Classes section too short".to_string()));
    }
    if &data[..16] != &start_sentinels::CLASSES {
        return Err(DxfError::InvalidSentinel(
            "Classes section start sentinel mismatch".to_string(),
        ));
    }

    // ── Read section size (RL at offset 16) ──
    let section_size = i32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;

    // Section data starts after sentinel (16) + size field (4).
    // Extra 4 zero bytes when: (AC1024+ && maintenance > 3) || AC1032+
    let mut data_start = 20;
    if DwgVersion::has_section_extra_rl(version, maintenance_version) {
        data_start += 4;
    }
    if data.len() < data_start + section_size + 2 + 16 {
        return Err(DxfError::Parse(
            "Classes section data truncated".to_string(),
        ));
    }
    let section_data = &data[data_start..data_start + section_size];

    // ── Create the bit reader over the section data ──
    let mut reader = DwgBitReader::with_encoding(section_data.to_vec(), dwg, version, encoding);

    // R2007+: The section data has an RL prefix (total data size in bits)
    // from save_position_for_size. Text is INLINE (not in a separate stream).
    // R2007+: Set up text stream for three-stream merge.
    // The writer puts text strings (dxf_name, cpp_class_name, application_name)
    // in a separate text sub-stream. The RL stores the total bit count;
    // the flag bit is at RL − 1 (same convention as per-object records).
    let end_bit;
    if version >= DxfVersion::AC1021 {
        let total_size_bits = reader.read_raw_long() as i64;
        let data_start = reader.position_in_bits();
        let text_start = reader.set_position_by_flag(total_size_bits - 1);
        reader.set_position_in_bits(data_start);
        end_bit = text_start;
    } else {
        end_bit = (section_size * 8) as i64;
    }

    // ── R2004+: section header ──
    let mut max_class_number: i16 = i16::MAX;
    if version >= DxfVersion::AC1018 {
        max_class_number = reader.read_bit_short(); // BS: max class number
        let _rc1 = reader.read_byte(); // RC: 0x00
        let _rc2 = reader.read_byte(); // RC: 0x00
        let _flag = reader.read_bit(); // B: true
    }

    // ── Read class entries until we consume all section data ──
    let mut classes = DxfClassCollection::new();

    while reader.position_in_bits() < end_bit {
        let class_number = reader.read_bit_short();

        // Sanity check — class numbering starts at 500 and can't exceed max
        if class_number < 500 || class_number > max_class_number {
            break;
        }

        let proxy_flags_raw = reader.read_bit_short() as i32;
        let application_name = reader.read_variable_text();
        let cpp_class_name = reader.read_variable_text();
        let dxf_name = reader.read_variable_text();
        let was_zombie = reader.read_bit();
        let item_class_id = reader.read_bit_short();

        let mut class = DxfClass::new(&dxf_name, &cpp_class_name);
        class.application_name = application_name;
        class.class_number = class_number;
        class.proxy_flags = ProxyFlags::from(proxy_flags_raw);
        class.was_zombie = was_zombie;
        class.item_class_id = item_class_id;
        // 0x1F2 marks a graphical entity, 0x1F3 a non-graphical object. Without
        // this the flag stayed at its `false` default for every class, so any
        // class-based entity the reader has no explicit type for (e.g. an
        // application's custom entity) was classified as an object and decoded
        // with the wrong layout — the entity, and whatever it drew, vanished.
        class.is_an_entity = item_class_id == 0x1F2;

        // R2004+: instance count + 4 extra BL fields
        if version >= DxfVersion::AC1018 {
            let instance_count = reader.read_bit_long();
            class.instance_count = instance_count;
            class.dwg_version = reader.read_bit_long();
            class.maintenance_version = reader.read_bit_long();
            class.unknown1 = reader.read_bit_long();
            class.unknown2 = reader.read_bit_long();
        }

        // Preserve every entry in order: the DWG classes section is positional
        // (object type = 500 + index), so deduping by dxf name would drop a
        // legitimate duplicate-name class and shift all later class numbers.
        classes.push_preserving(class);
    }

    // ── Verify end sentinel ──
    let end_sentinel_start = data_start + section_size + 2; // +2 for CRC
    if data.len() >= end_sentinel_start + 16 {
        if &data[end_sentinel_start..end_sentinel_start + 16] != &end_sentinels::CLASSES {
            // Non-fatal: log warning but don't fail
            // Some writers produce slightly off sentinels
        }
    }

    Ok(classes)
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_stream_writers::classes_writer;

    #[test]
    fn test_classes_roundtrip_r2000() {
        let mut classes = DxfClassCollection::new();
        classes.update_defaults();

        // Write classes section
        let class_vec: Vec<DxfClass> = classes.iter().cloned().collect();
        let written = classes_writer::write_classes(DxfVersion::AC1015, &class_vec, 0);

        // Read it back
        let read_classes = read_classes(&written, DxfVersion::AC1015, 0).unwrap();

        // Should have the same number of classes
        assert_eq!(
            read_classes.len(),
            classes.len(),
            "Class count mismatch: wrote {}, read {}",
            classes.len(),
            read_classes.len()
        );
    }

    #[test]
    fn test_classes_roundtrip_r2004() {
        let mut classes = DxfClassCollection::new();
        classes.update_defaults();

        let class_vec: Vec<DxfClass> = classes.iter().cloned().collect();
        let written = classes_writer::write_classes(DxfVersion::AC1018, &class_vec, 0);
        let read_classes = read_classes(&written, DxfVersion::AC1018, 0).unwrap();

        assert_eq!(
            read_classes.len(),
            classes.len(),
            "Class count mismatch: wrote {}, read {}",
            classes.len(),
            read_classes.len()
        );

        // Verify a specific class and preserve its positional class number.
        let acdb_placeholder = read_classes.get_by_name("ACDBPLACEHOLDER");
        assert!(
            acdb_placeholder.is_some(),
            "Should find ACDBPLACEHOLDER class"
        );
        let cls = acdb_placeholder.unwrap();
        let expected = classes.get_by_name("ACDBPLACEHOLDER").unwrap();
        assert_eq!(cls.cpp_class_name, "AcDbPlaceHolder");
        assert_eq!(cls.class_number, expected.class_number);
    }

    #[test]
    fn test_classes_bad_sentinel_fails() {
        let mut bad_data = vec![0u8; 50];
        // Wrong sentinel
        bad_data[..16].fill(0xFF);
        let result = read_classes(&bad_data, DxfVersion::AC1015, 0);
        assert!(result.is_err());
    }
}
