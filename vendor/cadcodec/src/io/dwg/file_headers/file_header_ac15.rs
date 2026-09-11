//! AC15 (R13/R14/R2000) file header writer
//!
//! Implements the linear DWG file format used by R13 through R2000.
//! The file layout is sequential: a file header with section locator
//! records followed by section data blocks in a fixed order.
//!
//! ## File Layout
//!
//! ```text
//! [File Header - 0x61 bytes]
//!   Version string (6 bytes) + padding (7 bytes)
//!   Preview seeker (4 bytes) + magic bytes (2)
//!   Code page (2 bytes) + record count (4 bytes)
//!   6 × Section locator records (9 bytes each)
//!   CRC-16 (2 bytes) + end sentinel (16 bytes)
//! [Header Variables]
//! [Classes]
//! [ObjFreeSpace]
//! [Template]
//! [AuxHeader]
//! [AcDbObjects]
//! [Handles]
//! [Preview]
//! ```
//!
//! Based on the reference `DwgFileHeaderWriterAC15`.

use byteorder::{LittleEndian, WriteBytesExt};
use indexmap::IndexMap;
use std::io::{Seek, Write};

use super::section_definition::{end_sentinels, names};
use super::section_descriptor::DwgSectionLocatorRecord;
use crate::error::DxfError;
use crate::io::dwg::crc;
use crate::types::DxfVersion;

const FILE_HEADER_SIZE_R13_R14: usize = 0x58;
const FILE_HEADER_SIZE_R2000: usize = 0x61;
#[cfg(test)]
const FILE_HEADER_SIZE: usize = FILE_HEADER_SIZE_R2000;

/// AC15 file header writer for the linear DWG format.
///
/// Collects section data, computes section seekers (file offsets),
/// then writes the complete file header followed by section data.
pub struct DwgFileHeaderWriterAC15 {
    /// DXF version being written.
    version: DxfVersion,
    /// AutoCAD maintenance release version at file-header offset 0x0B.
    maintenance_version: u8,
    /// Compact DWG code-page index.
    code_page: u16,
    /// Ordered map of section name → (locator record, section data).
    /// Data is `None` until the section is added.
    records: IndexMap<String, (DwgSectionLocatorRecord, Option<Vec<u8>>)>,
}

impl DwgFileHeaderWriterAC15 {
    /// Create a new AC15 file header writer.
    ///
    /// Initializes the section record table with the standard AC15 section
    /// ordering. All sections start with no data; use `add_section` to
    /// populate them before calling `write_file`.
    pub fn new(version: DxfVersion) -> Self {
        let mut records = IndexMap::new();

        if version >= DxfVersion::AC1015 {
            // Preserve the established R2000 physical section order.
            records.insert(
                names::HEADER.to_string(),
                (DwgSectionLocatorRecord::new(Some(0)), None),
            );
            records.insert(
                names::CLASSES.to_string(),
                (DwgSectionLocatorRecord::new(Some(1)), None),
            );
            records.insert(
                names::OBJ_FREE_SPACE.to_string(),
                (DwgSectionLocatorRecord::new(Some(3)), None),
            );
            records.insert(
                names::TEMPLATE.to_string(),
                (DwgSectionLocatorRecord::new(Some(4)), None),
            );
            records.insert(
                names::AUX_HEADER.to_string(),
                (DwgSectionLocatorRecord::new(Some(5)), None),
            );
            records.insert(
                names::ACDB_OBJECTS.to_string(),
                (DwgSectionLocatorRecord::new(None), None),
            );
            records.insert(
                names::HANDLES.to_string(),
                (DwgSectionLocatorRecord::new(Some(2)), None),
            );
        } else {
            // R13/R14 have five locator records and no AuxHeader. Their
            // object stream begins immediately after Classes.
            records.insert(
                names::HEADER.to_string(),
                (DwgSectionLocatorRecord::new(Some(0)), None),
            );
            records.insert(
                names::CLASSES.to_string(),
                (DwgSectionLocatorRecord::new(Some(1)), None),
            );
            records.insert(
                names::ACDB_OBJECTS.to_string(),
                (DwgSectionLocatorRecord::new(None), None),
            );
            records.insert(
                names::HANDLES.to_string(),
                (DwgSectionLocatorRecord::new(Some(2)), None),
            );
            records.insert(
                names::OBJ_FREE_SPACE.to_string(),
                (DwgSectionLocatorRecord::new(Some(3)), None),
            );
            records.insert(
                names::TEMPLATE.to_string(),
                (DwgSectionLocatorRecord::new(Some(4)), None),
            );
        }
        records.insert(
            names::PREVIEW.to_string(),
            (DwgSectionLocatorRecord::new(None), None),
        );

        Self {
            version,
            maintenance_version: 15,
            code_page: 30,
            records,
        }
    }

    /// Preserve the source document's maintenance release version.
    pub fn set_maintenance_version(&mut self, maintenance_version: u8) {
        self.maintenance_version = maintenance_version;
    }

    pub fn set_code_page(&mut self, code_page: u16) {
        self.code_page = code_page;
    }

    /// Add section data for the given section name.
    ///
    /// The `compressed` and `decomp_size` parameters are ignored for AC15
    /// (no compression in the linear format).
    pub fn add_section(&mut self, name: &str, data: Vec<u8>) {
        if let Some(entry) = self.records.get_mut(name) {
            entry.0.size = data.len() as i64;
            entry.1 = Some(data);
        }
    }

    /// File offset where the next section added will begin.
    ///
    /// Equals the file header size plus every section already added. Because the
    /// Preview is added last, calling this right before adding it yields the
    /// Preview's file offset — the `base` its container's absolute image offsets
    /// are relative to.
    pub fn pending_section_offset(&self) -> usize {
        self.file_header_size()
            + self
                .records
                .values()
                .map(|(_, d)| d.as_ref().map_or(0, |d| d.len()))
                .sum::<usize>()
    }

    /// Get the file offset where the AcDbObjects section starts.
    ///
    /// This is used by the object writer to compute absolute handle→offset
    /// mappings. Returns the sum of the file header size and all section
    /// sizes before AcDbObjects.
    pub fn handle_section_offset(&self) -> usize {
        let mut offset = self.file_header_size();
        for (name, (_, data)) in &self.records {
            if name == names::ACDB_OBJECTS {
                break;
            }
            offset += data.as_ref().map_or(0, |d| d.len());
        }
        offset
    }

    /// Write the complete DWG file to the output stream.
    ///
    /// Computes section seekers, writes the file header with locator records
    /// and CRC, then appends all section data in order.
    pub fn write_file<W: Write + Seek>(&mut self, output: &mut W) -> Result<(), DxfError> {
        self.set_record_seekers();
        self.write_file_header(output)?;
        self.write_record_streams(output)?;
        Ok(())
    }

    /// Calculate absolute file offsets for each section.
    fn set_record_seekers(&mut self) {
        let mut curr_offset = self.file_header_size() as i64;
        for (_, (record, data)) in self.records.iter_mut() {
            record.seeker = curr_offset;
            curr_offset += data.as_ref().map_or(0, |d| d.len()) as i64;
        }
    }

    /// Write the 0x61-byte file header to the output stream.
    fn write_file_header<W: Write>(&self, output: &mut W) -> Result<(), DxfError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.file_header_size());

        // 0x00: Version string (6 bytes, e.g., "AC1015")
        let version_str = self.version.as_str();
        buf.extend_from_slice(version_str.as_bytes());

        // 0x06: 5 zero bytes + maintenance version + 1 (7 bytes total)
        // In R14, bytes 0x06..0x0B are zeros and ACADMAINTVER, then 0x0C = 1
        buf.extend_from_slice(&[0, 0, 0, 0, 0, self.maintenance_version, 1]);

        // 0x0D: Preview seeker (4-byte absolute address)
        let preview_seeker = self
            .records
            .get(names::PREVIEW)
            .map_or(0i32, |(r, _)| r.seeker as i32);
        buf.write_i32::<LittleEndian>(preview_seeker)?;

        // 0x11: application/version bytes. The AC15 reader and known-good
        // R2000 fixtures use this fixed pair for the whole R13-R2000 family.
        buf.extend_from_slice(&[0x1B, 0x19]);

        // 0x13: Compact DWG code-page index.
        buf.write_u16::<LittleEndian>(self.code_page)?;

        let locator_count = self
            .records
            .values()
            .filter(|(record, _)| record.number.is_some())
            .count();
        buf.write_i32::<LittleEndian>(locator_count as i32)?;

        // 0x19: 6 × Section locator records (9 bytes each = 54 bytes)
        // Locator records must be written by section number, independently of
        // the dictionary order that defines the physical section layout.
        let mut locator_records: Vec<_> = self
            .records
            .values()
            .filter_map(|(record, _)| record.number.map(|number| (number, record)))
            .collect();
        locator_records.sort_unstable_by_key(|(number, _)| *number);
        for (number, record) in locator_records {
            self.write_record(&mut buf, number, record)?;
        }

        // CRC-16 over everything written so far
        let crc_value = crc::crc16(crc::CRC16_SEED, &buf);
        buf.write_i16::<LittleEndian>(crc_value as i16)?;

        // End sentinel (16 bytes)
        buf.extend_from_slice(&end_sentinels::FILE_HEADER);

        let file_header_size = self.file_header_size();
        debug_assert_eq!(
            buf.len(),
            file_header_size,
            "File header size mismatch: expected {file_header_size}, got {}",
            buf.len()
        );

        output.write_all(&buf)?;
        Ok(())
    }

    fn file_header_size(&self) -> usize {
        if self.version >= DxfVersion::AC1015 {
            FILE_HEADER_SIZE_R2000
        } else {
            FILE_HEADER_SIZE_R13_R14
        }
    }

    /// Write a single section locator record (9 bytes).
    fn write_record(
        &self,
        buf: &mut Vec<u8>,
        number: u8,
        record: &DwgSectionLocatorRecord,
    ) -> Result<(), DxfError> {
        buf.push(number);
        buf.write_i32::<LittleEndian>(record.seeker as i32)?;
        buf.write_i32::<LittleEndian>(record.size as i32)?;
        Ok(())
    }

    /// Write all section data streams to the output.
    fn write_record_streams<W: Write>(&self, output: &mut W) -> Result<(), DxfError> {
        for (_, (_, data)) in &self.records {
            if let Some(data) = data {
                output.write_all(data)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_new_creates_all_sections() {
        let writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);
        assert_eq!(writer.records.len(), 8);

        // Verify numbered records
        assert_eq!(writer.records[names::HEADER].0.number, Some(0));
        assert_eq!(writer.records[names::CLASSES].0.number, Some(1));
        assert_eq!(writer.records[names::HANDLES].0.number, Some(2));
        assert_eq!(writer.records[names::OBJ_FREE_SPACE].0.number, Some(3));
        assert_eq!(writer.records[names::TEMPLATE].0.number, Some(4));
        assert_eq!(writer.records[names::AUX_HEADER].0.number, Some(5));

        // Non-numbered records
        assert_eq!(writer.records[names::ACDB_OBJECTS].0.number, None);
        assert_eq!(writer.records[names::PREVIEW].0.number, None);
    }

    #[test]
    fn test_handle_section_offset() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);
        // Add sections before AcDbObjects
        writer.add_section(names::HEADER, vec![0; 100]);
        writer.add_section(names::CLASSES, vec![0; 50]);
        writer.add_section(names::OBJ_FREE_SPACE, vec![0; 10]);
        writer.add_section(names::TEMPLATE, vec![0; 20]);
        writer.add_section(names::AUX_HEADER, vec![0; 30]);

        // Offset = FILE_HEADER_SIZE + 100 + 50 + 10 + 20 + 30 = 0x61 + 210 = 307
        assert_eq!(writer.handle_section_offset(), FILE_HEADER_SIZE + 210);
    }

    #[test]
    fn test_write_file_header_size() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);

        // Add empty sections for all records
        for name in [
            names::HEADER,
            names::CLASSES,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::ACDB_OBJECTS,
            names::HANDLES,
            names::PREVIEW,
        ] {
            writer.add_section(name, vec![]);
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();
        // With all empty sections, file should be exactly FILE_HEADER_SIZE
        assert_eq!(data.len(), FILE_HEADER_SIZE);
    }

    #[test]
    fn test_write_file_version_string() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);
        for name in [
            names::HEADER,
            names::CLASSES,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::ACDB_OBJECTS,
            names::HANDLES,
            names::PREVIEW,
        ] {
            writer.add_section(name, vec![]);
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();
        assert_eq!(&data[0..6], b"AC1015");
        assert_eq!(data[0x0B], 15);
        assert_eq!(&data[0x11..0x13], &[0x1B, 0x19]);
    }

    #[test]
    fn test_write_file_with_data() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);

        let header_data = vec![0xAA; 100];
        let classes_data = vec![0xBB; 50];
        let objects_data = vec![0xCC; 200];

        writer.add_section(names::HEADER, header_data.clone());
        writer.add_section(names::CLASSES, classes_data.clone());
        writer.add_section(names::OBJ_FREE_SPACE, vec![]);
        writer.add_section(names::TEMPLATE, vec![]);
        writer.add_section(names::AUX_HEADER, vec![]);
        writer.add_section(names::ACDB_OBJECTS, objects_data.clone());
        writer.add_section(names::HANDLES, vec![]);
        writer.add_section(names::PREVIEW, vec![]);

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();

        // File = header (0x61) + all section data
        assert_eq!(data.len(), FILE_HEADER_SIZE + 100 + 50 + 200);

        // Verify section data appears in order after header
        assert_eq!(
            &data[FILE_HEADER_SIZE..FILE_HEADER_SIZE + 100],
            &[0xAA; 100]
        );
        assert_eq!(
            &data[FILE_HEADER_SIZE + 100..FILE_HEADER_SIZE + 150],
            &[0xBB; 50]
        );
        assert_eq!(
            &data[FILE_HEADER_SIZE + 150..FILE_HEADER_SIZE + 350],
            &[0xCC; 200]
        );
    }

    #[test]
    fn test_write_file_seekers_correct() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);

        writer.add_section(names::HEADER, vec![0; 100]);
        writer.add_section(names::CLASSES, vec![0; 50]);
        writer.add_section(names::OBJ_FREE_SPACE, vec![]);
        writer.add_section(names::TEMPLATE, vec![]);
        writer.add_section(names::AUX_HEADER, vec![]);
        writer.add_section(names::ACDB_OBJECTS, vec![0; 200]);
        writer.add_section(names::HANDLES, vec![0; 30]);
        writer.add_section(names::PREVIEW, vec![]);

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();

        // Parse locator records from the file header
        // At offset 0x19, we have 6 records of 9 bytes each
        let mut offset = 0x19;
        let mut records: Vec<(u8, i32, i32)> = Vec::new();
        for _ in 0..6 {
            let number = data[offset];
            let seeker = i32::from_le_bytes([
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
            ]);
            let size = i32::from_le_bytes([
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
                data[offset + 8],
            ]);
            records.push((number, seeker, size));
            offset += 9;
        }

        let locator_numbers: Vec<_> = records.iter().map(|record| record.0).collect();
        assert_eq!(locator_numbers, [0, 1, 2, 3, 4, 5]);

        // Record 0 (Header): seeker = 0x61, size = 100
        let header_rec = records.iter().find(|r| r.0 == 0).unwrap();
        assert_eq!(header_rec.1, 0x61);
        assert_eq!(header_rec.2, 100);

        // Record 1 (Classes): seeker = 0x61 + 100 = 197, size = 50
        let classes_rec = records.iter().find(|r| r.0 == 1).unwrap();
        assert_eq!(classes_rec.1, 197);
        assert_eq!(classes_rec.2, 50);
    }

    #[test]
    fn test_file_header_crc() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);
        for name in [
            names::HEADER,
            names::CLASSES,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::ACDB_OBJECTS,
            names::HANDLES,
            names::PREVIEW,
        ] {
            writer.add_section(name, vec![]);
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();

        // CRC is at offset 0x4F (right after 6 records: 0x19 + 54 = 0x4F)
        let crc_offset = 0x19 + 54;
        let stored_crc = u16::from_le_bytes([data[crc_offset], data[crc_offset + 1]]);

        // Compute CRC over the header data (before CRC)
        let computed_crc = crc::crc16(crc::CRC16_SEED, &data[..crc_offset]);
        assert_eq!(stored_crc, computed_crc, "File header CRC mismatch");
    }

    #[test]
    fn test_file_header_end_sentinel() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1015);
        for name in [
            names::HEADER,
            names::CLASSES,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::ACDB_OBJECTS,
            names::HANDLES,
            names::PREVIEW,
        ] {
            writer.add_section(name, vec![]);
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();

        // End sentinel is the last 16 bytes of the file header
        let sentinel_offset = FILE_HEADER_SIZE - 16;
        assert_eq!(
            &data[sentinel_offset..FILE_HEADER_SIZE],
            &end_sentinels::FILE_HEADER,
            "File header end sentinel mismatch"
        );
    }

    #[test]
    fn test_r13_version_string() {
        let mut writer = DwgFileHeaderWriterAC15::new(DxfVersion::AC1012);
        for name in [
            names::HEADER,
            names::CLASSES,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::ACDB_OBJECTS,
            names::HANDLES,
            names::PREVIEW,
        ] {
            writer.add_section(name, vec![]);
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();
        assert_eq!(&data[0..6], b"AC1012");
    }
}
