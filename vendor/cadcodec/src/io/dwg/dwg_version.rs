//! DWG version enum for binary DWG format versioning
//!
//! Maps the DXF version strings (AC10xx) to DWG-specific stream writer
//! behavior tiers. The DWG format has version-specific differences at
//! the bit-stream level, file header format, and entity encoding.

use crate::error::DxfError;
use crate::types::DxfVersion;

/// DWG format version, determining which stream writer features are used.
///
/// This maps to the C# inheritance chain:
/// `DwgStreamWriterAC12 → AC15 → AC18 → AC21 → AC24`
///
/// Each version adds or overrides specific encoding behaviors:
/// - AC12: Base bit-level I/O (R13/R14)
/// - AC15: Optimized thickness/extrusion encoding (R2000)
/// - AC18: True color RGB, transparency support (R2004)
/// - AC21: Unicode text encoding (R2007) — RS-encoded pages, LZ77 AC21 compression, CRC-64
/// - AC24: Compact object type encoding (R2010+)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DwgVersion {
    /// R13/R14 baseline
    AC12,
    /// R2000 — optimized thickness/extrusion
    AC15,
    /// R2004 — true color, transparency, page-based file format
    AC18,
    /// R2007 — Unicode text, RS-encoded pages, LZ77 AC21 compression, CRC-64
    AC21,
    /// R2010/R2013/R2018 — compact object type encoding
    AC24,
}

impl DwgVersion {
    /// Convert from `DxfVersion` to `DwgVersion`.
    ///
    /// Returns an error for `Unknown` version.
    pub fn from_dxf_version(version: DxfVersion) -> Result<Self, DxfError> {
        match version {
            DxfVersion::AC1012 | DxfVersion::AC1014 => Ok(DwgVersion::AC12),
            DxfVersion::AC1015 => Ok(DwgVersion::AC15),
            DxfVersion::AC1018 => Ok(DwgVersion::AC18),
            DxfVersion::AC1021 => Ok(DwgVersion::AC21),
            DxfVersion::AC1024 | DxfVersion::AC1027 | DxfVersion::AC1032 => Ok(DwgVersion::AC24),
            DxfVersion::Unknown => Err(DxfError::UnsupportedVersion("Unknown".to_string())),
        }
    }

    /// Get the DXF version string for the file header (e.g., "AC1015").
    pub fn version_string(&self, dxf_version: DxfVersion) -> &'static str {
        dxf_version.as_str()
    }

    /// Whether this version uses the AC18 page-based file format (R2004+).
    pub fn uses_page_format(&self) -> bool {
        matches!(self, DwgVersion::AC18 | DwgVersion::AC21 | DwgVersion::AC24)
    }

    /// Whether this version supports true color (R2004+).
    pub fn supports_true_color(&self) -> bool {
        *self >= DwgVersion::AC18
    }

    /// Whether this version uses Unicode text encoding (R2007+).
    pub fn uses_unicode_text(&self) -> bool {
        *self >= DwgVersion::AC21
    }

    /// Whether this version uses compact object type encoding (R2010+).
    pub fn uses_compact_object_type(&self) -> bool {
        *self >= DwgVersion::AC24
    }

    // ── Version-conditional helpers matching C# DwgSectionIO ──

    /// R13/R14 only
    pub fn r13_14_only(&self) -> bool {
        *self == DwgVersion::AC12
    }

    /// R13 through R2000 (AC1012–AC1015)
    pub fn r13_15_only(&self) -> bool {
        *self <= DwgVersion::AC15
    }

    /// R2000 and later (AC1015+)
    pub fn r2000_plus(&self) -> bool {
        *self >= DwgVersion::AC15
    }

    /// Before R2004 (AC1012–AC1015)
    pub fn r2004_pre(&self) -> bool {
        *self < DwgVersion::AC18
    }

    /// R2004 and later (AC1018+)
    pub fn r2004_plus(&self) -> bool {
        *self >= DwgVersion::AC18
    }

    /// Before R2007 (AC1012–AC1018)
    pub fn r2007_pre(&self) -> bool {
        *self < DwgVersion::AC21
    }

    /// R2007 and later (AC1021+)
    pub fn r2007_plus(&self) -> bool {
        *self >= DwgVersion::AC21
    }

    /// R2010 and later (AC1024+)
    pub fn r2010_plus(&self) -> bool {
        *self >= DwgVersion::AC24
    }

    /// R2013 and later — approximated as AC24 with DxfVersion check
    pub fn r2013_plus(&self, dxf: DxfVersion) -> bool {
        dxf >= DxfVersion::AC1027
    }

    /// R2018 and later — approximated as AC24 with DxfVersion check
    pub fn r2018_plus(&self, dxf: DxfVersion) -> bool {
        dxf >= DxfVersion::AC1032
    }

    /// Whether the Classes/Header sections contain an extra 4-byte RL field
    /// after the section-size RL.
    ///
    /// The full condition is:
    ///   `(version >= AC1024 && maintenance_version > 3) || version > AC1027`
    ///
    /// In practice this means:
    /// - AC1032+ (R2018): always
    /// - AC1024/AC1027 (R2010/R2013): only when maintenance version > 3
    /// - Older versions: never
    pub fn has_section_extra_rl(dxf: DxfVersion, maintenance_version: u8) -> bool {
        (dxf >= DxfVersion::AC1024 && maintenance_version > 3) || dxf > DxfVersion::AC1027
    }

    /// Parse DWG version from the 6-byte version string in the file header.
    ///
    /// Returns `None` for unrecognized version strings.
    pub fn from_version_string(s: &str) -> Option<Self> {
        match s {
            "AC1012" | "AC1014" | "AD1012" | "AD1014" => Some(DwgVersion::AC12),
            "AC1015" | "AD1015" => Some(DwgVersion::AC15),
            "AC1018" | "AD1018" => Some(DwgVersion::AC18),
            "AC1021" | "AD1021" => Some(DwgVersion::AC21),
            "AC1024" | "AC1027" | "AC1032" | "AD1024" | "AD1027" | "AD1032" => {
                Some(DwgVersion::AC24)
            }
            _ => None,
        }
    }

    /// Convert back to `DxfVersion` for the most representative value.
    pub fn to_dxf_version_string(&self) -> &'static str {
        match self {
            DwgVersion::AC12 => "AC1012",
            DwgVersion::AC15 => "AC1015",
            DwgVersion::AC18 => "AC1018",
            DwgVersion::AC21 => "AC1021",
            DwgVersion::AC24 => "AC1024",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_dxf_version() {
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1012).unwrap(),
            DwgVersion::AC12
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1014).unwrap(),
            DwgVersion::AC12
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1015).unwrap(),
            DwgVersion::AC15
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1018).unwrap(),
            DwgVersion::AC18
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1021).unwrap(),
            DwgVersion::AC21
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1024).unwrap(),
            DwgVersion::AC24
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1027).unwrap(),
            DwgVersion::AC24
        );
        assert_eq!(
            DwgVersion::from_dxf_version(DxfVersion::AC1032).unwrap(),
            DwgVersion::AC24
        );
        assert!(DwgVersion::from_dxf_version(DxfVersion::Unknown).is_err());
    }

    #[test]
    fn test_version_conditionals() {
        assert!(DwgVersion::AC12.r13_14_only());
        assert!(!DwgVersion::AC15.r13_14_only());

        assert!(DwgVersion::AC15.r2000_plus());
        assert!(!DwgVersion::AC12.r2000_plus());

        assert!(DwgVersion::AC18.r2004_plus());
        assert!(!DwgVersion::AC15.r2004_plus());

        assert!(DwgVersion::AC21.r2007_plus());
        assert!(!DwgVersion::AC18.r2007_plus());

        assert!(DwgVersion::AC24.r2010_plus());
        assert!(!DwgVersion::AC21.r2010_plus());
    }

    #[test]
    fn test_ordering() {
        assert!(DwgVersion::AC12 < DwgVersion::AC15);
        assert!(DwgVersion::AC15 < DwgVersion::AC18);
        assert!(DwgVersion::AC18 < DwgVersion::AC21);
        assert!(DwgVersion::AC21 < DwgVersion::AC24);
    }
}
