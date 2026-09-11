//! Core types used throughout acadrust.
//!
//! Re-exports the fundamental value types that appear across entities,
//! tables, and I/O:
//!
//! - [`Vector2`] / [`Vector3`] — 2-D and 3-D coordinate types
//! - [`Color`] — ACI index, true-color RGB, or ByLayer/ByBlock
//! - [`Handle`] — unique 64-bit object identifier
//! - [`DxfVersion`] — file version enum (AC1009 … AC1032)
//! - [`LineWeight`] — line thickness
//! - [`Transform`] / [`Matrix4`] — affine transformation types
//! - [`BoundingBox2D`] / [`BoundingBox3D`] — axis-aligned bounds

pub mod aci_table;
pub mod bounds;
pub mod color;
pub mod handle;
pub mod line_weight;
pub mod transform;
pub mod transparency;
pub mod vector;

pub use aci_table::{aci_to_rgb, nearest_aci, ACI_COLORS};
pub use bounds::{BoundingBox2D, BoundingBox3D};
pub use color::Color;
pub use handle::Handle;
pub use line_weight::LineWeight;
pub use transform::{is_zero_angle, normalize_angle, rotate_point_2d, Matrix3, Matrix4, Transform};
pub use transparency::Transparency;
pub use vector::{Vector2, Vector3};

/// DXF version enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DxfVersion {
    /// Unknown version
    Unknown,
    /// AutoCAD R13 (AC1012)
    AC1012,
    /// AutoCAD R14 (AC1014)
    AC1014,
    /// AutoCAD 2000 (AC1015)
    AC1015,
    /// AutoCAD 2004 (AC1018)
    AC1018,
    /// AutoCAD 2007 (AC1021)
    AC1021,
    /// AutoCAD 2010 (AC1024)
    AC1024,
    /// AutoCAD 2013 (AC1027)
    AC1027,
    /// AutoCAD 2018 (AC1032)
    AC1032,
}

impl DxfVersion {
    /// Get the version string (e.g., "AC1015")
    pub fn as_str(&self) -> &'static str {
        match self {
            DxfVersion::Unknown => "UNKNOWN",
            DxfVersion::AC1012 => "AC1012",
            DxfVersion::AC1014 => "AC1014",
            DxfVersion::AC1015 => "AC1015",
            DxfVersion::AC1018 => "AC1018",
            DxfVersion::AC1021 => "AC1021",
            DxfVersion::AC1024 => "AC1024",
            DxfVersion::AC1027 => "AC1027",
            DxfVersion::AC1032 => "AC1032",
        }
    }

    /// Get the DXF version string for the HEADER section
    pub fn to_dxf_string(&self) -> &'static str {
        self.as_str()
    }

    /// Parse version from string (e.g., "AC1015" or "AD1015")
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "AC1012" | "AD1012" => Some(DxfVersion::AC1012),
            "AC1014" | "AD1014" => Some(DxfVersion::AC1014),
            "AC1015" | "AD1015" => Some(DxfVersion::AC1015),
            "AC1018" | "AD1018" => Some(DxfVersion::AC1018),
            "AC1021" | "AD1021" => Some(DxfVersion::AC1021),
            "AC1024" | "AD1024" => Some(DxfVersion::AC1024),
            "AC1027" | "AD1027" => Some(DxfVersion::AC1027),
            "AC1032" | "AD1032" => Some(DxfVersion::AC1032),
            _ => None,
        }
    }

    /// Parse version from version string (e.g., "AC1015")
    pub fn from_version_string(s: &str) -> Self {
        Self::parse(s).unwrap_or(DxfVersion::Unknown)
    }

    /// Get the numeric version code
    pub fn version_code(&self) -> u16 {
        match self {
            DxfVersion::Unknown => 0,
            DxfVersion::AC1012 => 1012,
            DxfVersion::AC1014 => 1014,
            DxfVersion::AC1015 => 1015,
            DxfVersion::AC1018 => 1018,
            DxfVersion::AC1021 => 1021,
            DxfVersion::AC1024 => 1024,
            DxfVersion::AC1027 => 1027,
            DxfVersion::AC1032 => 1032,
        }
    }

    /// Create version from numeric code
    pub fn from_version_code(code: u16) -> Self {
        match code {
            1012 => DxfVersion::AC1012,
            1014 => DxfVersion::AC1014,
            1015 => DxfVersion::AC1015,
            1018 => DxfVersion::AC1018,
            1021 => DxfVersion::AC1021,
            1024 => DxfVersion::AC1024,
            1027 => DxfVersion::AC1027,
            1032 => DxfVersion::AC1032,
            _ => DxfVersion::Unknown,
        }
    }
}

impl std::fmt::Display for DxfVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_string() {
        assert_eq!(DxfVersion::AC1015.as_str(), "AC1015");
        assert_eq!(DxfVersion::AC1032.to_string(), "AC1032");
    }

    #[test]
    fn test_version_parse() {
        assert_eq!(DxfVersion::parse("AC1018"), Some(DxfVersion::AC1018));
        assert_eq!(DxfVersion::parse("INVALID"), None);
    }

    #[test]
    fn test_version_code() {
        assert_eq!(DxfVersion::AC1021.version_code(), 1021);
    }
}
