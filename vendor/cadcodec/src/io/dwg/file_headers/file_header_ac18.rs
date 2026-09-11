//! AC18 (R2004+) file header writer
//!
//! Implements the page-based DWG file format used by R2004 and later
//! (except R2007, which uses a different compression scheme).
//!
//! ## File Layout
//!
//! ```text
//! [0x100 bytes: File metadata]
//!   Version string (6 bytes), preview/summary addresses, etc.
//!   Inner file header (0x6C bytes, CRC-32, XOR with magic sequence)
//! [Data pages: one or more per section]
//!   Per page: magic alignment + 32-byte XOR-masked header + LZ77 data + padding
//! [Section Map page (type 0x4163003B)]
//!   Compressed descriptor table for all sections
//! [Section Page Map page (type 0x41630E3B)]
//!   Compressed page list (pageNumber → size pairs)
//! [Second file header copy]
//! ```
//!
//! Based on the reference `DwgFileHeaderWriterAC18`.

use byteorder::{LittleEndian, WriteBytesExt};
use indexmap::IndexMap;
use std::io::{Cursor, Seek, SeekFrom, Write};

use super::section_definition::{names, PAGE_TYPE_SECTION_MAP, PAGE_TYPE_SECTION_PAGE_MAP};
use super::section_descriptor::{DwgLocalSectionMap, DwgSectionDescriptor};
use crate::error::DxfError;
use crate::io::dwg::checksum::{
    adler32_checksum, apply_magic_sequence, apply_mask, compression_padding, magic_sequence,
};
use crate::io::dwg::compression::DwgLZ77AC18Compressor;
use crate::io::dwg::crc;
use crate::types::DxfVersion;

/// File header/metadata size in bytes for AC18 format.
const FILE_HEADER_SIZE: usize = 0x100;

/// Maximum decompressed page size (default for `add_section`).
pub const DEFAULT_DECOMP_SIZE: usize = 0x7400;

/// Inner file header identifier string.
const INNER_HEADER_ID: &[u8; 12] = b"AcFssFcAJMB\0";

/// AC18 file header writer for the page-based DWG format.
///
/// Sections are written incrementally to the output stream as they are
/// added. After all sections are added, `write_file` writes the section
/// map, page map, and file metadata.
pub struct DwgFileHeaderWriterAC18 {
    /// DXF version being written.
    version: DxfVersion,
    /// AutoCAD maintenance release version (preserved from source file).
    maintenance_version: u8,
    /// Compact DWG code-page index.
    code_page: u16,
    /// Section descriptors, keyed by section name.
    descriptors: IndexMap<String, DwgSectionDescriptor>,
    /// All local section map entries (pages) across all sections.
    local_section_maps: Vec<DwgLocalSectionMap>,
    /// LZ77 compressor instance (reused across pages).
    compressor: DwgLZ77AC18Compressor,
    /// Section ID counter for assigning sequential IDs.
    next_section_id: i32,

    // ── File header metadata (filled during write_file) ──
    /// Number of page slots allocated.
    section_array_page_size: u32,
    /// Page ID of the section page map.
    section_page_map_id: u32,
    /// Page ID of the section map.
    section_map_id: u32,
    /// ID of the last page written.
    last_page_id: i32,
    /// Address of the end of the last section page.
    last_section_addr: u64,
    /// Address of the second file header copy.
    second_header_addr: u64,
    /// Number of active sections (excluding page map / section map).
    section_amount: u32,
    /// Address of the section page map.
    page_map_address: u64,
    /// Gap amount (0 for new files).
    gap_amount: u32,
}

impl DwgFileHeaderWriterAC18 {
    /// Create a new AC18 file header writer and reserve file metadata space.
    ///
    /// Writes `0x100` zero bytes to the output stream to reserve space
    /// for the file metadata that will be filled in by `write_file`.
    pub fn new<W: Write + Seek>(
        version: DxfVersion,
        maintenance_version: u8,
        output: &mut W,
    ) -> Result<Self, DxfError> {
        // Reserve 0x100 bytes at the start for file metadata
        let zeroes = [0u8; FILE_HEADER_SIZE];
        output.write_all(&zeroes)?;

        Ok(Self {
            version,
            maintenance_version,
            code_page: 30,
            descriptors: IndexMap::new(),
            local_section_maps: Vec::new(),
            compressor: DwgLZ77AC18Compressor::new(),
            next_section_id: 0,
            section_array_page_size: 0,
            section_page_map_id: 0,
            section_map_id: 0,
            last_page_id: 0,
            last_section_addr: 0,
            second_header_addr: 0,
            section_amount: 0,
            page_map_address: 0,
            gap_amount: 0,
        })
    }

    pub fn set_code_page(&mut self, code_page: u16) {
        self.code_page = code_page;
    }

    /// Get the file offset where the AcDbObjects section starts.
    ///
    /// For AC18, this is always 0 because object handles are relative
    /// to the decompressed section data, not the file.
    pub fn handle_section_offset(&self) -> usize {
        0
    }

    /// Add a section to the file.
    ///
    /// The section data is split into pages of `decomp_size` bytes,
    /// each page is optionally compressed and written to the output stream.
    ///
    /// # Arguments
    /// * `output` - Seekable output stream
    /// * `name` - Section name (e.g., "AcDb:Header")
    /// * `data` - Raw section data bytes
    /// * `compressed` - Whether to LZ77-compress the section pages
    /// * `decomp_size` - Maximum decompressed page size (default: 0x7400)
    pub fn add_section<W: Write + Seek>(
        &mut self,
        output: &mut W,
        name: &str,
        data: &[u8],
        compressed: bool,
        decomp_size: usize,
    ) -> Result<(), DxfError> {
        let perf = std::env::var_os("PERF").is_some();
        let started = web_time::Instant::now();
        let mut descriptor = DwgSectionDescriptor::new(name);
        descriptor.decompressed_size = decomp_size as u64;
        descriptor.compressed_size = data.len() as u64;
        descriptor.compressed_code = if compressed { 2 } else { 1 };
        descriptor.section_id = self.next_section_id;
        self.next_section_id += 1;

        let n_full_pages = data.len() / decomp_size;
        let mut page_inputs: Vec<_> = (0..n_full_pages)
            .map(|index| {
                let offset = index * decomp_size;
                (offset, decomp_size, &data[offset..offset + decomp_size])
            })
            .collect();
        let remainder = data.len() % decomp_size;
        let offset = n_full_pages * decomp_size;
        if remainder > 0 && !is_all_zeros(&data[offset..]) {
            page_inputs.push((offset, remainder, &data[offset..]));
        }

        use crate::io::dwg::parallel::{map_slice, worker_count};
        let batch_pages = worker_count().saturating_mul(2);
        for page_batch in page_inputs.chunks(batch_pages) {
            let encoded: Vec<_> = map_slice(page_batch, |&(offset, total_size, bytes)| {
                let mut padded = vec![0u8; decomp_size];
                padded[..total_size].copy_from_slice(bytes);
                let encoded = if compressed {
                    let mut compressor = DwgLZ77AC18Compressor::new();
                    compressor.compress(&padded, 0, decomp_size)
                } else {
                    padded
                };
                (offset, total_size, encoded)
            });

            for (offset, total_size, encoded) in encoded {
                self.create_local_section(
                    output,
                    &mut descriptor,
                    offset,
                    total_size,
                    compressed,
                    encoded,
                )?;
            }
        }

        let pages = descriptor.page_count;
        self.descriptors.insert(name.to_string(), descriptor);
        if perf {
            eprintln!(
                "[perf] dwg-write-section name={} format=ac18 time={:.1}ms bytes={} pages={} compressed={}",
                name,
                started.elapsed().as_secs_f64() * 1000.0,
                data.len(),
                pages,
                compressed,
            );
        }
        Ok(())
    }

    /// Finalize the file: write section map, page map, and file metadata.
    pub fn write_file<W: Write + Seek>(&mut self, output: &mut W) -> Result<(), DxfError> {
        self.section_array_page_size = (self.local_section_maps.len() + 2) as u32;
        self.section_page_map_id = self.section_array_page_size;
        self.section_map_id = self.section_array_page_size - 1;

        self.write_descriptors(output)?;
        self.write_records(output)?;
        self.write_file_metadata(output)?;

        Ok(())
    }

    // ── Internal page writing ──

    /// Create and write a single page (local section) to the output stream.
    fn create_local_section<W: Write + Seek>(
        &mut self,
        output: &mut W,
        descriptor: &mut DwgSectionDescriptor,
        offset: usize,
        total_size: usize,
        compressed: bool,
        compressed_data: Vec<u8>,
    ) -> Result<(), DxfError> {
        // Write magic number alignment padding
        self.write_magic_number(output)?;

        // Record page position
        let position = output.seek(SeekFrom::Current(0))? as i64;

        let mut local_map = DwgLocalSectionMap::new();
        local_map.offset = offset as u64;
        local_map.seeker = position;
        local_map.page_number = self.local_section_maps.len() as i32 + 1;

        // ODA checksum: Adler-32 over compressed data only
        local_map.oda = adler32_checksum(0, &compressed_data);

        let compress_diff = compression_padding(compressed_data.len());
        local_map.compressed_size = compressed_data.len() as u64;
        local_map.decompressed_size = total_size as u64;
        local_map.page_size = local_map.compressed_size as i64 + 32 + compress_diff as i64;
        local_map.checksum = 0;

        // First pass: build data section header to compute checksum
        let mut checksum_buf = Vec::with_capacity(32);
        Self::write_data_section_header(
            &mut checksum_buf,
            descriptor,
            &local_map,
            descriptor.page_type,
        )?;

        // Compute checksum: Adler-32 of header seeded with ODA
        local_map.checksum = adler32_checksum(local_map.oda, &checksum_buf);

        // Second pass: rebuild header with correct checksum
        checksum_buf.clear();
        Self::write_data_section_header(
            &mut checksum_buf,
            descriptor,
            &local_map,
            descriptor.page_type,
        )?;

        // Apply XOR mask to the 32-byte header
        apply_mask(&mut checksum_buf, position as u64);

        // Write masked header + compressed data + padding
        output.write_all(&checksum_buf)?;
        output.write_all(&compressed_data)?;

        if compressed {
            let magic = magic_sequence();
            output.write_all(&magic[..compress_diff])?;
        } else if compress_diff != 0 {
            return Err(DxfError::InvalidFormat(
                "Uncompressed page has non-zero compression padding".into(),
            ));
        }

        // Update descriptor and local maps
        if local_map.page_number > 0 {
            descriptor.page_count += 1;
        }
        local_map.size = output.seek(SeekFrom::Current(0))? as i64 - position;

        descriptor.local_sections.push(local_map.clone());
        self.local_section_maps.push(local_map);

        Ok(())
    }

    /// Write the 32-byte data section page header.
    fn write_data_section_header(
        buf: &mut Vec<u8>,
        descriptor: &DwgSectionDescriptor,
        map: &DwgLocalSectionMap,
        page_type: i32,
    ) -> Result<(), DxfError> {
        // 0x00: Section page type (4 bytes)
        buf.write_i32::<LittleEndian>(page_type)?;
        // 0x04: Section number (4 bytes)
        buf.write_i32::<LittleEndian>(descriptor.section_id)?;
        // 0x08: Data size - compressed (4 bytes)
        buf.write_i32::<LittleEndian>(map.compressed_size as i32)?;
        // 0x0C: Page size - decompressed (4 bytes)
        buf.write_i32::<LittleEndian>(map.page_size as i32)?;
        // 0x10: Start offset in decompressed buffer (8 bytes)
        buf.write_i64::<LittleEndian>(map.offset as i64)?;
        // 0x18: Data checksum (4 bytes)
        buf.write_u32::<LittleEndian>(map.checksum)?;
        // 0x1C: ODA (4 bytes)
        buf.write_u32::<LittleEndian>(map.oda)?;

        debug_assert_eq!(buf.len() % 32, 0, "Data section header must be 32 bytes");
        Ok(())
    }

    // ── Section map and page map ──

    /// Write the section descriptors (section map) as a compressed page.
    fn write_descriptors<W: Write + Seek>(&mut self, output: &mut W) -> Result<(), DxfError> {
        let mut stream = Vec::new();

        // 0x00: Number of section descriptions (4 bytes)
        stream.write_i32::<LittleEndian>(self.descriptors.len() as i32)?;
        // 0x04: 0x02 (4 bytes)
        stream.write_i32::<LittleEndian>(2)?;
        // 0x08: 0x00007400 (4 bytes)
        stream.write_i32::<LittleEndian>(0x7400)?;
        // 0x0C: 0x00 (4 bytes)
        stream.write_i32::<LittleEndian>(0)?;
        // 0x10: NumDescriptions again (4 bytes)
        stream.write_i32::<LittleEndian>(self.descriptors.len() as i32)?;

        for descriptor in self.descriptors.values() {
            // 0x00: Size of section (8 bytes)
            stream.write_u64::<LittleEndian>(descriptor.compressed_size)?;
            // 0x08: Page count (4 bytes)
            stream.write_i32::<LittleEndian>(descriptor.page_count)?;
            // 0x0C: Max decompressed page size (4 bytes)
            stream.write_i32::<LittleEndian>(descriptor.decompressed_size as i32)?;
            // 0x10: Unknown (4 bytes, ODA writes 1)
            stream.write_i32::<LittleEndian>(1)?;
            // 0x14: Compressed code (4 bytes)
            stream.write_i32::<LittleEndian>(descriptor.compressed_code)?;
            // 0x18: Section ID (4 bytes)
            stream.write_i32::<LittleEndian>(descriptor.section_id)?;
            // 0x1C: Encrypted (4 bytes)
            stream.write_i32::<LittleEndian>(descriptor.encrypted)?;

            // 0x20: Section name (64 bytes, zero-padded)
            let mut name_buf = [0u8; 64];
            let name_bytes = descriptor.name.as_bytes();
            let copy_len = name_bytes.len().min(64);
            name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            stream.write_all(&name_buf)?;

            // Per-page info
            for local_map in &descriptor.local_sections {
                if local_map.page_number > 0 {
                    // 0x00: Page number (4 bytes)
                    stream.write_i32::<LittleEndian>(local_map.page_number)?;
                    // 0x04: Data size - compressed (4 bytes)
                    stream.write_i32::<LittleEndian>(local_map.compressed_size as i32)?;
                    // 0x08: Start offset (8 bytes)
                    stream.write_u64::<LittleEndian>(local_map.offset)?;
                }
            }
        }

        // Write as section map page (0x4163003B)
        let section_holder = self.set_seeker(output, PAGE_TYPE_SECTION_MAP, &stream)?;
        let padding = compression_padding(
            (output.seek(SeekFrom::Current(0))? as i64 - section_holder.seeker) as usize,
        );
        let magic = magic_sequence();
        output.write_all(&magic[..padding])?;

        let mut holder = section_holder;
        holder.size = output.seek(SeekFrom::Current(0))? as i64 - holder.seeker;

        self.add_local_section(holder);

        Ok(())
    }

    /// Write the section page map (page number → size pairs) as a compressed page.
    fn write_records<W: Write + Seek>(&mut self, output: &mut W) -> Result<(), DxfError> {
        self.write_magic_number(output)?;

        // Create section page map entry
        let mut section = DwgLocalSectionMap::with_section_map(PAGE_TYPE_SECTION_PAGE_MAP);
        self.add_local_section(section.clone());

        let counter = self.local_section_maps.len() * 8;
        section.seeker = output.seek(SeekFrom::Current(0))? as i64;
        let size = counter + compression_padding(counter);
        section.size = size as i64;

        // Sync the list entry with the updated seeker/size values.
        // In C# sections are reference types so the list entry is updated
        // automatically; in Rust we must propagate explicitly.
        if let Some(last_entry) = self.local_section_maps.last_mut() {
            last_entry.seeker = section.seeker;
            last_entry.size = section.size;
        }

        // Build page map entries
        let mut stream = Vec::new();
        for item in &self.local_section_maps {
            stream.write_i32::<LittleEndian>(item.page_number)?;
            stream.write_i32::<LittleEndian>(item.size as i32)?;
        }

        // Compress and write with checksum
        self.compress_checksum(output, &mut section, &stream)?;

        // Update file header metadata
        let last = self.local_section_maps.last().unwrap().clone();
        self.gap_amount = 0;
        self.last_page_id = last.page_number;
        self.last_section_addr = ((last.seeker as u64) + (size as u64)).saturating_sub(256);
        // The section page map records one entry per local section map, including
        // the page-map page itself (appended just above). `section_amount` must
        // equal that entry count (with no gaps) — readers verify
        // `page_entries == gap_amount + section_amount`. An off-by-one here is
        // tolerated by lenient readers but rejected by strict ones.
        self.section_amount = self.local_section_maps.len() as u32;
        self.page_map_address = section.seeker as u64;

        Ok(())
    }

    /// Write the file metadata at offset 0 and append the second file header.
    fn write_file_metadata<W: Write + Seek>(&mut self, output: &mut W) -> Result<(), DxfError> {
        // Second header address = current end of file
        self.second_header_addr = output.seek(SeekFrom::Current(0))?;

        // Build and write inner file header
        let inner_header = self.build_inner_file_header()?;
        output.write_all(&inner_header)?;

        // Seek back to the beginning and write file metadata
        output.seek(SeekFrom::Start(0))?;

        // 0x00: Version string (6 bytes)
        let version_str = self.version.as_str();
        output.write_all(version_str.as_bytes())?;

        // 0x06: 5 bytes of 0x00
        output.write_all(&[0u8; 5])?;

        // 0x0B: Maintenance release version
        output.write_all(&[self.maintenance_version])?;

        // 0x0C: Byte (0x00, 0x01, or 0x03)
        output.write_all(&[3u8])?;

        // 0x0D: Preview address (4 bytes) — points to preview page + 0x20 header
        let preview_addr = self
            .descriptors
            .get(names::PREVIEW)
            .and_then(|d| d.local_sections.first())
            .map_or(0u32, |s| (s.seeker as u32) + 0x20);
        output.write_u32::<LittleEndian>(preview_addr)?;

        // 0x11: DWG version byte (0x21 for AC1021, 0x21 for AC1024+)
        // The ODA spec shows different values per version, but many real-world
        // files (including our reference General.dwg) use 0x21 for AC1024.
        // We preserve the original file's value via maintenance_version context.
        output.write_all(&[33u8])?;

        // 0x12: Maintenance release version (app)
        output.write_all(&[self.maintenance_version])?;

        // 0x13: Codepage (2 bytes)
        output.write_u16::<LittleEndian>(self.code_page)?;

        // 0x15: 3 zero bytes
        output.write_all(&[0u8; 3])?;

        // 0x18: SecurityType (4 bytes)
        output.write_i32::<LittleEndian>(0)?;

        // 0x1C: Unknown (4 bytes)
        output.write_i32::<LittleEndian>(0)?;

        // 0x20: Summary info address (4 bytes) — points to page + 0x20
        let summary_addr = self
            .descriptors
            .get(names::SUMMARY_INFO)
            .and_then(|d| d.local_sections.first())
            .map_or(0u32, |s| (s.seeker as u32) + 0x20);
        output.write_u32::<LittleEndian>(summary_addr)?;

        // 0x24: VBA Project address (4 bytes, 0 if not present)
        output.write_u32::<LittleEndian>(0)?;

        // 0x28: 0x00000080 (4 bytes)
        output.write_i32::<LittleEndian>(0x80)?;

        // 0x2C: App info address (4 bytes) — points to page + 0x20
        let app_info_addr = self
            .descriptors
            .get(names::APP_INFO)
            .and_then(|d| d.local_sections.first())
            .map_or(0u32, |s| (s.seeker as u32) + 0x20);
        output.write_u32::<LittleEndian>(app_info_addr)?;

        // 0x30: 0x50 (80) zero bytes
        output.write_all(&[0u8; 80])?;

        // 0x80: Inner file header copy (0x6C bytes + the rest up to 0x100)
        output.write_all(&inner_header)?;

        // Append trailing magic sequence bytes (20 bytes from offset 236)
        let magic = magic_sequence();
        output.write_all(&magic[236..256])?;

        // Seek to end of file
        output.seek(SeekFrom::End(0))?;

        Ok(())
    }

    /// Build the 0x6C-byte inner file header with CRC-32 and magic sequence XOR.
    fn build_inner_file_header(&self) -> Result<Vec<u8>, DxfError> {
        let mut buf = vec![0u8; 0x6C];

        {
            let mut cursor = Cursor::new(&mut buf[..]);

            // 0x00: "AcFssFcAJMB\0" (12 bytes)
            cursor.write_all(INNER_HEADER_ID)?;

            // 0x0C: 0x00 (4 bytes)
            cursor.write_i32::<LittleEndian>(0)?;
            // 0x10: 0x6C (4 bytes)
            cursor.write_i32::<LittleEndian>(0x6C)?;
            // 0x14: 0x04 (4 bytes)
            cursor.write_i32::<LittleEndian>(0x04)?;
            // 0x18: Root tree node gap (4 bytes)
            cursor.write_i32::<LittleEndian>(0)?;
            // 0x1C: Left gap (4 bytes)
            cursor.write_i32::<LittleEndian>(0)?;
            // 0x20: Right gap (4 bytes)
            cursor.write_i32::<LittleEndian>(0)?;
            // 0x24: Unknown (ODA writes 1) (4 bytes)
            cursor.write_i32::<LittleEndian>(1)?;
            // 0x28: Last page ID (4 bytes)
            cursor.write_i32::<LittleEndian>(self.last_page_id)?;

            // 0x2C: Last section address (8 bytes)
            cursor.write_u64::<LittleEndian>(self.last_section_addr)?;
            // 0x34: Second header address (8 bytes)
            cursor.write_u64::<LittleEndian>(self.second_header_addr)?;

            // 0x3C: Gap amount (4 bytes)
            cursor.write_u32::<LittleEndian>(self.gap_amount)?;
            // 0x40: Section amount (4 bytes)
            cursor.write_u32::<LittleEndian>(self.section_amount)?;

            // 0x44: 0x20 (4 bytes)
            cursor.write_i32::<LittleEndian>(0x20)?;
            // 0x48: 0x80 (4 bytes)
            cursor.write_i32::<LittleEndian>(0x80)?;
            // 0x4C: 0x40 (4 bytes)
            cursor.write_i32::<LittleEndian>(0x40)?;

            // 0x50: Section page map ID (4 bytes)
            cursor.write_u32::<LittleEndian>(self.section_page_map_id)?;
            // 0x54: Page map address - 0x100 (8 bytes)
            cursor.write_u64::<LittleEndian>(self.page_map_address.saturating_sub(256))?;
            // 0x5C: Section map ID (4 bytes)
            cursor.write_u32::<LittleEndian>(self.section_map_id)?;
            // 0x60: Section page array size (4 bytes)
            cursor.write_u32::<LittleEndian>(self.section_array_page_size)?;
            // 0x64: Gap array size (4 bytes)
            cursor.write_u32::<LittleEndian>(0)?;

            // 0x68: CRC-32 placeholder (4 bytes, initially zero)
            cursor.write_u32::<LittleEndian>(0)?;
        }

        // Compute CRC-32 over the entire 0x6C bytes (including zero CRC placeholder)
        let computed_crc = crc::crc32(&buf);

        // Patch CRC at offset 0x68
        buf[0x68] = computed_crc as u8;
        buf[0x69] = (computed_crc >> 8) as u8;
        buf[0x6A] = (computed_crc >> 16) as u8;
        buf[0x6B] = (computed_crc >> 24) as u8;

        // XOR with magic sequence
        apply_magic_sequence(&mut buf);

        Ok(buf)
    }

    // ── Utility methods ──

    /// Write magic sequence bytes to align the stream to a 0x20-byte boundary.
    fn write_magic_number<W: Write + Seek>(&self, output: &mut W) -> Result<(), DxfError> {
        let pos = output.seek(SeekFrom::Current(0))? as usize;
        let padding = pos % 0x20;
        if padding > 0 {
            let magic = magic_sequence();
            output.write_all(&magic[..padding])?;
        }
        Ok(())
    }

    /// Add a local section map entry, assigning the next page number.
    fn add_local_section(&mut self, mut section: DwgLocalSectionMap) {
        section.page_number = self.local_section_maps.len() as i32 + 1;
        self.local_section_maps.push(section);
    }

    /// Compress data, compute checksum, and write a system page (section/page map).
    fn set_seeker<W: Write + Seek>(
        &mut self,
        output: &mut W,
        section_map_type: i32,
        data: &[u8],
    ) -> Result<DwgLocalSectionMap, DxfError> {
        let mut holder = DwgLocalSectionMap::with_section_map(section_map_type);

        self.write_magic_number(output)?;
        holder.seeker = output.seek(SeekFrom::Current(0))? as i64;

        self.compress_checksum(output, &mut holder, data)?;

        Ok(holder)
    }

    /// Compress data and write it with a page header containing checksum.
    fn compress_checksum<W: Write + Seek>(
        &mut self,
        output: &mut W,
        section: &mut DwgLocalSectionMap,
        data: &[u8],
    ) -> Result<(), DxfError> {
        section.decompressed_size = data.len() as u64;

        // Compress
        let compressed = self.compressor.compress(data, 0, data.len());
        section.compressed_size = compressed.len() as u64;

        // First pass: build header to compute checksum
        let mut header = Vec::with_capacity(20);
        Self::write_page_header_data(&mut header, section)?;

        // Checksum = Adler-32 over header, then seeded with that over compressed data
        section.checksum = adler32_checksum(0, &header);
        section.checksum = adler32_checksum(section.checksum, &compressed);

        // Write the page header with checksum, then the compressed data
        Self::write_page_header_data(output, section)?;
        output.write_all(&compressed)?;

        Ok(())
    }

    /// Write a 20-byte page header for system pages (section map, page map).
    fn write_page_header_data<W: Write>(
        output: &mut W,
        section: &DwgLocalSectionMap,
    ) -> Result<(), DxfError> {
        // 0x00: Section page type (4 bytes)
        output.write_i32::<LittleEndian>(section.section_map)?;
        // 0x04: Decompressed size (4 bytes)
        output.write_i32::<LittleEndian>(section.decompressed_size as i32)?;
        // 0x08: Compressed size (4 bytes)
        output.write_i32::<LittleEndian>(section.compressed_size as i32)?;
        // 0x0C: Compression type (4 bytes)
        output.write_i32::<LittleEndian>(section.compression)?;
        // 0x10: Checksum (4 bytes)
        output.write_u32::<LittleEndian>(section.checksum)?;

        Ok(())
    }
}

/// Check if all bytes in a slice are zero.
fn is_all_zeros(data: &[u8]) -> bool {
    data.iter().all(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::super::section_definition::PAGE_TYPE_DATA_SECTION;
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_new_reserves_header_space() {
        let mut output = Cursor::new(Vec::new());
        let _writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        assert_eq!(output.position(), FILE_HEADER_SIZE as u64);
        assert_eq!(output.get_ref().len(), FILE_HEADER_SIZE);
        // All zeros
        assert!(output.get_ref().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_handle_section_offset_always_zero() {
        let mut output = Cursor::new(Vec::new());
        let writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();
        assert_eq!(writer.handle_section_offset(), 0);
    }

    #[test]
    fn test_is_all_zeros() {
        assert!(is_all_zeros(&[]));
        assert!(is_all_zeros(&[0, 0, 0]));
        assert!(!is_all_zeros(&[0, 1, 0]));
    }

    #[test]
    fn test_add_section_small_data() {
        let mut output = Cursor::new(Vec::new());
        let mut writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        // Add a small section (less than one page)
        let data = vec![0xAA; 100];
        writer
            .add_section(&mut output, names::HEADER, &data, true, DEFAULT_DECOMP_SIZE)
            .unwrap();

        // Should have 1 descriptor and 1 local section map
        assert_eq!(writer.descriptors.len(), 1);
        assert_eq!(writer.local_section_maps.len(), 1);

        let desc = &writer.descriptors[names::HEADER];
        assert_eq!(desc.page_count, 1);
        assert_eq!(desc.section_id, 0);
        assert_eq!(desc.compressed_code, 2);
    }

    #[test]
    fn test_add_section_multi_page() {
        let mut output = Cursor::new(Vec::new());
        let mut writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        // Add section larger than one page (2 × 0x7400 + remainder)
        let data = vec![0xBB; 0x7400 * 2 + 1000];
        writer
            .add_section(
                &mut output,
                names::ACDB_OBJECTS,
                &data,
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();

        assert_eq!(writer.descriptors.len(), 1);
        let desc = &writer.descriptors[names::ACDB_OBJECTS];
        assert_eq!(desc.page_count, 3); // 2 full + 1 remainder
        assert_eq!(writer.local_section_maps.len(), 3);
    }

    #[test]
    fn test_add_multiple_sections() {
        let mut output = Cursor::new(Vec::new());
        let mut writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        writer
            .add_section(
                &mut output,
                names::HEADER,
                &vec![0xAA; 100],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::CLASSES,
                &vec![0xBB; 200],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();

        assert_eq!(writer.descriptors.len(), 2);
        assert_eq!(writer.descriptors[names::HEADER].section_id, 0);
        assert_eq!(writer.descriptors[names::CLASSES].section_id, 1);
    }

    #[test]
    fn test_inner_file_header_size() {
        let mut output = Cursor::new(Vec::new());
        let writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        let header = writer.build_inner_file_header().unwrap();
        assert_eq!(header.len(), 0x6C);
    }

    #[test]
    fn test_inner_file_header_magic_xor_roundtrip() {
        let mut output = Cursor::new(Vec::new());
        let writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        let mut header = writer.build_inner_file_header().unwrap();
        // Undo magic sequence XOR
        apply_magic_sequence(&mut header);

        // Now we should see the "AcFssFcAJMB\0" identifier
        assert_eq!(&header[..11], b"AcFssFcAJMB");
        assert_eq!(header[11], 0);
    }

    #[test]
    fn test_write_complete_file() {
        let mut output = Cursor::new(Vec::new());
        let mut writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        // Add minimal sections
        writer
            .add_section(
                &mut output,
                names::HEADER,
                &vec![0xAA; 100],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::CLASSES,
                &vec![0xBB; 50],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::SUMMARY_INFO,
                &vec![0; 10],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::PREVIEW,
                &vec![0xCC; 20],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::APP_INFO,
                &vec![0xDD; 30],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::AUX_HEADER,
                &vec![0; 50],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::ACDB_OBJECTS,
                &vec![0xEE; 300],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::HANDLES,
                &vec![0xFF; 100],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();

        // Write the complete file
        writer.write_file(&mut output).unwrap();

        let data = output.into_inner();

        // File should start with version string
        assert_eq!(&data[0..6], b"AC1018");

        // File should be larger than the reserved header
        assert!(data.len() > FILE_HEADER_SIZE);
    }

    #[test]
    fn test_write_file_version_strings() {
        for version in [
            DxfVersion::AC1018,
            DxfVersion::AC1024,
            DxfVersion::AC1027,
            DxfVersion::AC1032,
        ] {
            let mut output = Cursor::new(Vec::new());
            let mut writer = DwgFileHeaderWriterAC18::new(version, 0, &mut output).unwrap();

            writer
                .add_section(
                    &mut output,
                    names::HEADER,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::CLASSES,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::SUMMARY_INFO,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::PREVIEW,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::APP_INFO,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::AUX_HEADER,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::ACDB_OBJECTS,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();
            writer
                .add_section(
                    &mut output,
                    names::HANDLES,
                    &vec![0; 10],
                    true,
                    DEFAULT_DECOMP_SIZE,
                )
                .unwrap();

            writer.write_file(&mut output).unwrap();

            let data = output.into_inner();
            let expected = version.as_str();
            assert_eq!(
                &data[0..6],
                expected.as_bytes(),
                "Version mismatch for {version:?}"
            );
        }
    }

    #[test]
    fn test_data_section_header_is_32_bytes() {
        let desc = DwgSectionDescriptor::new("test");
        let map = DwgLocalSectionMap::new();
        let mut buf = Vec::new();
        DwgFileHeaderWriterAC18::write_data_section_header(
            &mut buf,
            &desc,
            &map,
            PAGE_TYPE_DATA_SECTION,
        )
        .unwrap();
        assert_eq!(buf.len(), 32);
    }

    #[test]
    fn test_page_header_is_20_bytes() {
        let map = DwgLocalSectionMap::new();
        let mut buf = Vec::new();
        DwgFileHeaderWriterAC18::write_page_header_data(&mut buf, &map).unwrap();
        assert_eq!(buf.len(), 20);
    }

    #[test]
    fn test_write_records_page_map_entry_has_nonzero_size() {
        // Regression test: the page map section's own entry in the page map
        // data must have a non-zero size. Previously, the clone in the list
        // retained stale size=0 due to Rust value semantics.
        let mut output = Cursor::new(Vec::new());
        let mut writer = DwgFileHeaderWriterAC18::new(DxfVersion::AC1018, 0, &mut output).unwrap();

        writer
            .add_section(
                &mut output,
                names::HEADER,
                &vec![0xAA; 100],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::CLASSES,
                &vec![0xBB; 50],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::SUMMARY_INFO,
                &vec![0; 10],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::PREVIEW,
                &vec![0xCC; 20],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::APP_INFO,
                &vec![0xDD; 30],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::AUX_HEADER,
                &vec![0; 50],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::ACDB_OBJECTS,
                &vec![0xEE; 300],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();
        writer
            .add_section(
                &mut output,
                names::HANDLES,
                &vec![0xFF; 100],
                true,
                DEFAULT_DECOMP_SIZE,
            )
            .unwrap();

        writer.write_file(&mut output).unwrap();

        // After write_file, the last local section map (page map) must have non-zero seeker and size
        let page_map_entry = writer.local_section_maps.last().unwrap();
        assert_ne!(
            page_map_entry.seeker, 0,
            "Page map entry seeker should be non-zero"
        );
        assert_ne!(
            page_map_entry.size, 0,
            "Page map entry size should be non-zero"
        );

        // last_section_addr should depend on the actual seeker, not zero
        assert!(
            writer.last_section_addr > 0,
            "last_section_addr should be > 0"
        );
    }
}
