//! DWG section descriptor and locator record structs
//!
//! These structs describe section layout within a DWG file:
//! - `DwgSectionLocatorRecord`: Simple offset+size record for AC15 format
//! - `DwgSectionDescriptor`: Full descriptor for AC18+ format with compression info
//! - `DwgLocalSectionMap`: Per-page metadata within an AC18 section

/// Section locator record for AC15 (R13–R2000) format.
///
/// Each locator record describes one section in the file header's
/// locator table. Contains the section number, absolute file offset,
/// and size in bytes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgSectionLocatorRecord {
    /// Section number (0=Header, 1=Classes, 2=Handles, 3=ObjFreeSpace, 4=Template, 5=AuxHeader).
    /// `None` for sections not in the locator table (AcDbObjects, Preview).
    pub number: Option<u8>,
    /// Absolute file offset where the section data starts.
    pub seeker: i64,
    /// Size in bytes of the section data.
    pub size: i64,
}

impl DwgSectionLocatorRecord {
    /// Create a new locator record with the given section number.
    pub fn new(number: Option<u8>) -> Self {
        Self {
            number,
            seeker: 0,
            size: 0,
        }
    }
}

/// Section descriptor for AC18 (R2004+) format.
///
/// Describes a section with compression settings, page layout,
/// and references to local section map entries.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgSectionDescriptor {
    /// Section name (e.g., "AcDb:Header").
    pub name: String,
    /// Section page type constant (normally 0x4163043B for data sections).
    pub page_type: i32,
    /// Total compressed size of the section data.
    pub compressed_size: u64,
    /// Number of pages written to file (excludes zero pages).
    pub page_count: i32,
    /// Maximum decompressed size of a single page (normally 0x7400 = 29696 bytes).
    pub decompressed_size: u64,
    /// Compression code: 1 = uncompressed, 2 = LZ77 compressed.
    pub compressed_code: i32,
    /// Section ID (sequential, starting at 0).
    pub section_id: i32,
    /// Encryption flag: 0 = not encrypted, 1 = encrypted.
    pub encrypted: i32,
    /// Per-page metadata for this section.
    pub local_sections: Vec<DwgLocalSectionMap>,
}

impl DwgSectionDescriptor {
    /// Create a new section descriptor with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            page_type: super::section_definition::PAGE_TYPE_DATA_SECTION,
            compressed_size: 0,
            page_count: 0,
            decompressed_size: 0x7400,
            compressed_code: 2, // Compressed by default
            section_id: 0,
            encrypted: 0,
            local_sections: Vec::new(),
        }
    }

    /// Whether this section uses LZ77 compression.
    pub fn is_compressed(&self) -> bool {
        self.compressed_code == 2
    }
}

/// Per-page metadata within an AC18 section.
///
/// Each section can span multiple pages. This struct holds the
/// compression, checksum, and offset information for a single page.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DwgLocalSectionMap {
    /// Compression type (1 = none, 2 = LZ77).
    pub compression: i32,
    /// Page number (1-based, unique per file).
    pub page_number: i32,
    /// Offset within the decompressed section data.
    pub offset: u64,
    /// Compressed size of this page's data.
    pub compressed_size: u64,
    /// Decompressed size of this page's data.
    pub decompressed_size: u64,
    /// Absolute file offset where this page starts.
    pub seeker: i64,
    /// Total page size including header and padding.
    pub size: i64,
    /// Page checksum (Adler-32 of header + data).
    pub checksum: u32,
    /// ODA checksum (Adler-32 of compressed data only).
    pub oda: u32,
    /// Section map type identifier.
    /// Data sections: 0x4163043B, Section map: 0x4163003B, Page map: 0x41630E3B.
    pub section_map: i32,
    /// Page size as written (for data section headers).
    pub page_size: i64,
}

impl DwgLocalSectionMap {
    /// Create a new empty local section map entry.
    pub fn new() -> Self {
        Self {
            compression: 2,
            page_number: 0,
            offset: 0,
            compressed_size: 0,
            decompressed_size: 0,
            seeker: 0,
            size: 0,
            checksum: 0,
            oda: 0,
            section_map: 0,
            page_size: 0,
        }
    }

    /// Create a new local section map with a given section map type.
    pub fn with_section_map(section_map: i32) -> Self {
        let mut map = Self::new();
        map.section_map = section_map;
        map
    }
}

impl Default for DwgLocalSectionMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_locator_record() {
        let record = DwgSectionLocatorRecord::new(Some(2));
        assert_eq!(record.number, Some(2));
        assert_eq!(record.seeker, 0);
        assert_eq!(record.size, 0);
    }

    #[test]
    fn test_section_descriptor_defaults() {
        let desc = DwgSectionDescriptor::new("AcDb:Header");
        assert_eq!(desc.name, "AcDb:Header");
        assert_eq!(desc.decompressed_size, 0x7400);
        assert_eq!(desc.compressed_code, 2);
        assert!(desc.is_compressed());
        assert_eq!(desc.page_type, 0x4163043B);
    }

    #[test]
    fn test_section_descriptor_uncompressed() {
        let mut desc = DwgSectionDescriptor::new("AcDb:Header");
        desc.compressed_code = 1;
        assert!(!desc.is_compressed());
    }

    #[test]
    fn test_local_section_map_defaults() {
        let map = DwgLocalSectionMap::new();
        assert_eq!(map.compression, 2);
        assert_eq!(map.page_number, 0);
    }
}
