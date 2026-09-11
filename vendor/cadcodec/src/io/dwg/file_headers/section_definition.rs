//! DWG section name constants, sentinels, and locator mappings
//!
//! Based on the reference `DwgSectionDefinition` class.
//! These constants define the known section names used in DWG files,
//! along with sentinel byte arrays that bracket section data in AC15 format.

/// Section name constants used in DWG file format.
///
/// These are used as keys in section descriptors (AC18) and
/// to identify section locator records (AC15).
pub mod names {
    pub const ACDB_OBJECTS: &str = "AcDb:AcDbObjects";
    pub const ACDS_PROTOTYPE: &str = "AcDb:AcDsPrototype_1b";
    pub const APP_INFO: &str = "AcDb:AppInfo";
    pub const AUX_HEADER: &str = "AcDb:AuxHeader";
    pub const HEADER: &str = "AcDb:Header";
    pub const CLASSES: &str = "AcDb:Classes";
    pub const HANDLES: &str = "AcDb:Handles";
    pub const OBJ_FREE_SPACE: &str = "AcDb:ObjFreeSpace";
    pub const TEMPLATE: &str = "AcDb:Template";
    pub const SUMMARY_INFO: &str = "AcDb:SummaryInfo";
    pub const FILE_DEP_LIST: &str = "AcDb:FileDepList";
    pub const PREVIEW: &str = "AcDb:Preview";
    pub const REV_HISTORY: &str = "AcDb:RevHistory";
    pub const SECURITY: &str = "AcDb:Security";
    pub const VBA_PROJECT: &str = "AcDb:VBAProject";
    pub const SIGNATURE: &str = "AcDb:Signature";
}

/// Start sentinels for sections (16 bytes each).
///
/// These mark the beginning of a section in AC15 format.
pub mod start_sentinels {
    pub const HEADER: [u8; 16] = [
        0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33,
        0x5F,
    ];
    pub const CLASSES: [u8; 16] = [
        0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6,
        0x8A,
    ];
    pub const PREVIEW: [u8; 16] = [
        0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10,
        0x2B,
    ];
}

/// End sentinels for sections (16 bytes each).
///
/// These mark the end of a section in AC15 format.
pub mod end_sentinels {
    pub const HEADER: [u8; 16] = [
        0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC,
        0xA0,
    ];
    pub const CLASSES: [u8; 16] = [
        0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49,
        0x75,
    ];
    pub const PREVIEW: [u8; 16] = [
        0xE0, 0xDA, 0x92, 0xF8, 0x2B, 0xC9, 0xD7, 0xD7, 0x62, 0xA8, 0x35, 0xC0, 0x62, 0xBB, 0xEF,
        0xD4,
    ];
    /// File header end sentinel (AC15 format)
    pub const FILE_HEADER: [u8; 16] = [
        0x95, 0xA0, 0x4E, 0x28, 0x99, 0x82, 0x1A, 0xE5, 0x5E, 0x41, 0xE0, 0x5F, 0x9D, 0x3A, 0x4D,
        0x00,
    ];
}

/// Section hash values for AC18/AC21 format.
///
/// These are the hashcodes used in the section map to identify sections.
/// The same values are returned by [`ac21_section_info::hash_code()`].
#[allow(dead_code)]
#[repr(u32)]
pub enum DwgSectionHash {
    Unknown = 0x00000000,
    Security = 0x4A0204EA,
    FileDepList = 0x6C4205CA,
    VbaProject = 0x586E0544,
    AppInfo = 0x3FA0043E,
    Preview = 0x40AA0473,
    SummaryInfo = 0x717A060F,
    RevHistory = 0x60A205B3,
    AcDbObjects = 0x674C05A9,
    ObjFreeSpace = 0x77E2061F,
    Template = 0x4A1404CE,
    Handles = 0x3F6E0450,
    Classes = 0x3F54045F,
    AuxHeader = 0x54F0050A,
    Header = 0x32B803D9,
}

/// AC21 section metadata for the writer (spec §5.2 section table).
///
/// Each AC21 section has a fixed hashcode, a default page size, an encoding
/// mode (1 = uncompressed, 4 = compressed), and an encryption mode (0, 1, or 2).
/// These values come from the ODA spec and are validated against real R2007 DWG files.
pub mod ac21_section_info {
    use super::names;

    /// Look up the known hashcode for an AC21 section name.
    ///
    /// Returns the 32-bit hashcode used in the section map, or `None`
    /// for unknown section names.
    pub fn hash_code(name: &str) -> Option<u32> {
        match name {
            names::HEADER => Some(0x32B803D9),
            names::CLASSES => Some(0x3F54045F),
            names::HANDLES => Some(0x3F6E0450),
            names::ACDB_OBJECTS => Some(0x674C05A9),
            names::OBJ_FREE_SPACE => Some(0x77E2061F),
            names::TEMPLATE => Some(0x4A1404CE),
            names::AUX_HEADER => Some(0x54F0050A),
            names::REV_HISTORY => Some(0x60A205B3),
            names::SUMMARY_INFO => Some(0x717A060F),
            names::PREVIEW => Some(0x40AA0473),
            names::APP_INFO => Some(0x3FA0043E),
            names::FILE_DEP_LIST => Some(0x6C4205CA),
            names::SECURITY => Some(0x4A0204EA),
            names::VBA_PROJECT => Some(0x586E0544),
            _ => None,
        }
    }

    /// Look up the default data page size for an AC21 section.
    ///
    /// Returns the default page size in bytes, or `None` for unknown sections.
    /// `AcDb:VBAProject` has no fixed default — its page size varies with content.
    pub fn page_size(name: &str) -> Option<u64> {
        match name {
            names::HEADER => Some(0x800),
            names::CLASSES => Some(0xF800),
            names::HANDLES => Some(0xF800),
            names::ACDB_OBJECTS => Some(0xF800),
            names::OBJ_FREE_SPACE => Some(0xF800),
            names::TEMPLATE => Some(0x400),
            names::AUX_HEADER => Some(0x800),
            names::REV_HISTORY => Some(0x1000),
            names::SUMMARY_INFO => Some(0x80),
            names::PREVIEW => Some(0x400),
            names::APP_INFO => Some(0x300),
            names::FILE_DEP_LIST => Some(0x100),
            names::SECURITY => Some(0xF800),
            // VBAProject has variable page size — caller must supply it
            names::VBA_PROJECT => None,
            _ => None,
        }
    }

    /// Look up the encoding mode for an AC21 section.
    ///
    /// - `4` = compressed (LZ77 AC21 + RS encoding)
    /// - `1` = uncompressed (RS encoding only)
    ///
    /// Returns `None` for unknown section names.
    pub fn encoding(name: &str) -> Option<u64> {
        match name {
            // Compressed sections (encoding=4)
            names::HEADER
            | names::CLASSES
            | names::HANDLES
            | names::ACDB_OBJECTS
            | names::OBJ_FREE_SPACE
            | names::TEMPLATE
            | names::AUX_HEADER
            | names::REV_HISTORY => Some(4),

            // Uncompressed sections (encoding=1)
            names::SUMMARY_INFO
            | names::PREVIEW
            | names::APP_INFO
            | names::FILE_DEP_LIST
            | names::SECURITY
            | names::VBA_PROJECT => Some(1),

            _ => None,
        }
    }

    /// Look up the default encryption mode for an AC21 section (spec §5.2).
    ///
    /// - `0` = no encryption
    /// - `1` = XOR encryption (password-protected files)
    /// - `2` = fixed obfuscation (always applied for FileDepList, VBAProject)
    ///
    /// Some sections (Header, Classes, Handles, AcDbObjects, Preview,
    /// SummaryInfo) can have `encryption=1` when the file is
    /// password-protected. This function returns the **default** value
    /// for unencrypted files.
    ///
    /// Returns `None` for unknown section names.
    pub fn encryption(name: &str) -> Option<u64> {
        match name {
            // Sections with fixed obfuscation (encryption=2, spec §5.2)
            names::FILE_DEP_LIST | names::VBA_PROJECT => Some(2),

            // All other sections default to no encryption
            names::HEADER
            | names::CLASSES
            | names::HANDLES
            | names::ACDB_OBJECTS
            | names::OBJ_FREE_SPACE
            | names::TEMPLATE
            | names::AUX_HEADER
            | names::REV_HISTORY
            | names::SUMMARY_INFO
            | names::PREVIEW
            | names::APP_INFO
            | names::SECURITY => Some(0),

            _ => None,
        }
    }

    /// All known AC21 section names in the order they typically appear in DWG files.
    pub const ALL_SECTION_NAMES: &[&str] = &[
        names::HEADER,
        names::CLASSES,
        names::HANDLES,
        names::ACDB_OBJECTS,
        names::OBJ_FREE_SPACE,
        names::TEMPLATE,
        names::AUX_HEADER,
        names::REV_HISTORY,
        names::SUMMARY_INFO,
        names::PREVIEW,
        names::APP_INFO,
        names::FILE_DEP_LIST,
        names::SECURITY,
        names::VBA_PROJECT,
    ];

    /// Section map entry order per spec §5.2 section table.
    ///
    /// The spec explicitly states: "The section map may contain the following
    /// sections (in this order, the order in the file stream is different)".
    /// This order is used when serializing section map entries; it differs
    /// from the physical stream order defined in §5.1.
    pub const SECTION_MAP_ORDER: &[&str] = &[
        names::SECURITY,
        names::FILE_DEP_LIST,
        names::VBA_PROJECT,
        names::APP_INFO,
        names::PREVIEW,
        names::SUMMARY_INFO,
        names::REV_HISTORY,
        names::ACDB_OBJECTS,
        names::OBJ_FREE_SPACE,
        names::TEMPLATE,
        names::HANDLES,
        names::CLASSES,
        names::AUX_HEADER,
        names::HEADER,
    ];
}

/// AC18 page type constants
pub const PAGE_TYPE_DATA_SECTION: i32 = 0x4163043B;
pub const PAGE_TYPE_SECTION_MAP: i32 = 0x4163003B;
pub const PAGE_TYPE_SECTION_PAGE_MAP: i32 = 0x41630E3B;

/// Get the section locator record number for a given section name (AC15 format).
///
/// Returns `None` for sections that don't have a locator record number
/// (AcDbObjects, Preview — they exist in the file but not in the header locator table).
pub fn section_locator_number(name: &str) -> Option<u8> {
    match name {
        names::HEADER => Some(0),
        names::CLASSES => Some(1),
        names::HANDLES => Some(2),
        names::OBJ_FREE_SPACE => Some(3),
        names::TEMPLATE => Some(4),
        names::AUX_HEADER => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_locator_numbers() {
        assert_eq!(section_locator_number(names::HEADER), Some(0));
        assert_eq!(section_locator_number(names::CLASSES), Some(1));
        assert_eq!(section_locator_number(names::HANDLES), Some(2));
        assert_eq!(section_locator_number(names::OBJ_FREE_SPACE), Some(3));
        assert_eq!(section_locator_number(names::TEMPLATE), Some(4));
        assert_eq!(section_locator_number(names::AUX_HEADER), Some(5));
        assert_eq!(section_locator_number(names::ACDB_OBJECTS), None);
        assert_eq!(section_locator_number(names::PREVIEW), None);
    }

    #[test]
    fn test_sentinels_are_complement() {
        // Start and end sentinels for the same section should be bitwise complements
        for i in 0..16 {
            assert_eq!(
                start_sentinels::HEADER[i] ^ end_sentinels::HEADER[i],
                0xFF,
                "Header sentinel mismatch at index {i}"
            );
            assert_eq!(
                start_sentinels::CLASSES[i] ^ end_sentinels::CLASSES[i],
                0xFF,
                "Classes sentinel mismatch at index {i}"
            );
            assert_eq!(
                start_sentinels::PREVIEW[i] ^ end_sentinels::PREVIEW[i],
                0xFF,
                "Preview sentinel mismatch at index {i}"
            );
        }
    }

    // ─── AC21 section info tests ────────────────────────────────────

    #[test]
    fn test_hash_code_matches_enum() {
        // Verify that ac21_section_info::hash_code() returns the same values
        // as the DwgSectionHash enum discriminants.
        assert_eq!(
            ac21_section_info::hash_code(names::HEADER),
            Some(DwgSectionHash::Header as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::CLASSES),
            Some(DwgSectionHash::Classes as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::HANDLES),
            Some(DwgSectionHash::Handles as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::ACDB_OBJECTS),
            Some(DwgSectionHash::AcDbObjects as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::OBJ_FREE_SPACE),
            Some(DwgSectionHash::ObjFreeSpace as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::TEMPLATE),
            Some(DwgSectionHash::Template as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::AUX_HEADER),
            Some(DwgSectionHash::AuxHeader as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::REV_HISTORY),
            Some(DwgSectionHash::RevHistory as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::SUMMARY_INFO),
            Some(DwgSectionHash::SummaryInfo as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::PREVIEW),
            Some(DwgSectionHash::Preview as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::APP_INFO),
            Some(DwgSectionHash::AppInfo as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::FILE_DEP_LIST),
            Some(DwgSectionHash::FileDepList as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::SECURITY),
            Some(DwgSectionHash::Security as u32)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::VBA_PROJECT),
            Some(DwgSectionHash::VbaProject as u32)
        );
    }

    #[test]
    fn test_hash_code_unknown_returns_none() {
        assert_eq!(ac21_section_info::hash_code("AcDb:Unknown"), None);
        assert_eq!(ac21_section_info::hash_code(""), None);
        assert_eq!(ac21_section_info::hash_code("NotASection"), None);
    }

    #[test]
    fn test_hash_code_exact_values() {
        // Cross-check against ODA spec §5.2 table values
        assert_eq!(
            ac21_section_info::hash_code(names::HEADER),
            Some(0x32B803D9)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::CLASSES),
            Some(0x3F54045F)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::HANDLES),
            Some(0x3F6E0450)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::ACDB_OBJECTS),
            Some(0x674C05A9)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::OBJ_FREE_SPACE),
            Some(0x77E2061F)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::TEMPLATE),
            Some(0x4A1404CE)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::AUX_HEADER),
            Some(0x54F0050A)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::REV_HISTORY),
            Some(0x60A205B3)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::SUMMARY_INFO),
            Some(0x717A060F)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::PREVIEW),
            Some(0x40AA0473)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::APP_INFO),
            Some(0x3FA0043E)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::FILE_DEP_LIST),
            Some(0x6C4205CA)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::SECURITY),
            Some(0x4A0204EA)
        );
        assert_eq!(
            ac21_section_info::hash_code(names::VBA_PROJECT),
            Some(0x586E0544)
        );
    }

    #[test]
    fn test_page_size_values() {
        assert_eq!(ac21_section_info::page_size(names::HEADER), Some(0x800));
        assert_eq!(ac21_section_info::page_size(names::CLASSES), Some(0xF800));
        assert_eq!(ac21_section_info::page_size(names::HANDLES), Some(0xF800));
        assert_eq!(
            ac21_section_info::page_size(names::ACDB_OBJECTS),
            Some(0xF800)
        );
        assert_eq!(
            ac21_section_info::page_size(names::OBJ_FREE_SPACE),
            Some(0xF800)
        );
        assert_eq!(ac21_section_info::page_size(names::TEMPLATE), Some(0x400));
        assert_eq!(ac21_section_info::page_size(names::AUX_HEADER), Some(0x800));
        assert_eq!(
            ac21_section_info::page_size(names::REV_HISTORY),
            Some(0x1000)
        );
        assert_eq!(
            ac21_section_info::page_size(names::SUMMARY_INFO),
            Some(0x80)
        );
        assert_eq!(ac21_section_info::page_size(names::PREVIEW), Some(0x400));
        assert_eq!(ac21_section_info::page_size(names::APP_INFO), Some(0x300));
        assert_eq!(
            ac21_section_info::page_size(names::FILE_DEP_LIST),
            Some(0x100)
        );
        assert_eq!(ac21_section_info::page_size(names::SECURITY), Some(0xF800));
        // VBAProject has variable page size
        assert_eq!(ac21_section_info::page_size(names::VBA_PROJECT), None);
    }

    #[test]
    fn test_page_size_unknown_returns_none() {
        assert_eq!(ac21_section_info::page_size("AcDb:Unknown"), None);
    }

    #[test]
    fn test_encoding_compressed_sections() {
        // These sections use encoding=4 (compressed)
        for name in &[
            names::HEADER,
            names::CLASSES,
            names::HANDLES,
            names::ACDB_OBJECTS,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::REV_HISTORY,
        ] {
            assert_eq!(
                ac21_section_info::encoding(name),
                Some(4),
                "{name} should be compressed (encoding=4)"
            );
        }
    }

    #[test]
    fn test_encoding_uncompressed_sections() {
        // These sections use encoding=1 (uncompressed)
        for name in &[
            names::SUMMARY_INFO,
            names::PREVIEW,
            names::APP_INFO,
            names::FILE_DEP_LIST,
            names::SECURITY,
            names::VBA_PROJECT,
        ] {
            assert_eq!(
                ac21_section_info::encoding(name),
                Some(1),
                "{name} should be uncompressed (encoding=1)"
            );
        }
    }

    #[test]
    fn test_encoding_unknown_returns_none() {
        assert_eq!(ac21_section_info::encoding("AcDb:Unknown"), None);
    }

    #[test]
    fn test_encryption_values() {
        // Most sections default to encryption=0 (spec §5.2)
        for name in &[
            names::HEADER,
            names::CLASSES,
            names::HANDLES,
            names::ACDB_OBJECTS,
            names::OBJ_FREE_SPACE,
            names::TEMPLATE,
            names::AUX_HEADER,
            names::REV_HISTORY,
            names::SUMMARY_INFO,
            names::PREVIEW,
            names::APP_INFO,
            names::SECURITY,
        ] {
            assert_eq!(
                ac21_section_info::encryption(name),
                Some(0),
                "{name} should have encryption=0"
            );
        }

        // FileDepList and VBAProject use fixed obfuscation (encryption=2)
        assert_eq!(ac21_section_info::encryption(names::FILE_DEP_LIST), Some(2));
        assert_eq!(ac21_section_info::encryption(names::VBA_PROJECT), Some(2));
    }

    #[test]
    fn test_encryption_unknown_returns_none() {
        assert_eq!(ac21_section_info::encryption("AcDb:Unknown"), None);
    }

    #[test]
    fn test_all_section_names_completeness() {
        // ALL_SECTION_NAMES should contain exactly 14 entries
        assert_eq!(ac21_section_info::ALL_SECTION_NAMES.len(), 14);

        // Every name in ALL_SECTION_NAMES should have valid hash_code and encoding
        for name in ac21_section_info::ALL_SECTION_NAMES {
            assert!(
                ac21_section_info::hash_code(name).is_some(),
                "Missing hash_code for {name}"
            );
            assert!(
                ac21_section_info::encoding(name).is_some(),
                "Missing encoding for {name}"
            );
            assert!(
                ac21_section_info::encryption(name).is_some(),
                "Missing encryption for {name}"
            );
        }
    }

    #[test]
    fn test_all_section_names_have_unique_hashcodes() {
        let hashcodes: Vec<u32> = ac21_section_info::ALL_SECTION_NAMES
            .iter()
            .filter_map(|name| ac21_section_info::hash_code(name))
            .collect();

        // Check for uniqueness
        let mut sorted = hashcodes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            hashcodes.len(),
            sorted.len(),
            "Duplicate hashcodes detected"
        );
    }

    #[test]
    fn test_page_sizes_are_power_of_two_or_known() {
        // Page sizes should be reasonable values (powers of 2 or commonly used sizes)
        for name in ac21_section_info::ALL_SECTION_NAMES {
            if let Some(size) = ac21_section_info::page_size(name) {
                assert!(size > 0, "{name} has zero page size");
                assert!(
                    size <= 0x10000,
                    "{name} has unexpectedly large page size: {size:#X}"
                );
            }
        }
    }
}
