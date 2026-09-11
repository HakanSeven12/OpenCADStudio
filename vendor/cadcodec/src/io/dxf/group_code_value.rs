//! Group code value types
//!
//! Determines how to interpret the value associated with a DXF group code.

use super::DxfCode;

/// Type of value associated with a group code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GroupCodeValueType {
    /// No value or unknown
    None,

    /// String value
    String,

    /// Boolean value (0 or 1)
    Bool,

    /// 8-bit integer
    Byte,

    /// 16-bit signed integer
    Int16,

    /// 32-bit signed integer
    Int32,

    /// 64-bit signed integer
    Int64,

    /// Double-precision floating-point
    Double,

    /// 3D point (three doubles: X, Y, Z)
    Point3D,

    /// Binary data (hex string)
    BinaryData,

    /// Object handle (hex string)
    Handle,
}

impl GroupCodeValueType {
    /// Determine the value type from a DXF group code enum.
    /// NOTE: This goes through the DxfCode enum, which may not cover all codes.
    /// For binary reading where correctness is critical, use `from_raw_code` instead.
    pub fn from_code(code: DxfCode) -> Self {
        // Delegate to from_raw_code using the raw i32 representation.
        // However, DxfCode::from_i32 can lose unmapped codes (they become Invalid = -9999).
        // So we use the DxfCode's i32 representation which preserves the original value
        // only if the DxfCode was constructed with a known variant.
        Self::from_raw_code(code.to_i32())
    }

    /// Determine the value type from a raw integer group code.
    /// This is the canonical mapping and should be preferred over `from_code`
    /// when the original integer code is available (e.g., binary DXF reading).
    pub fn from_raw_code(code_num: i32) -> Self {
        match code_num {
            // String values (0-9, 100-109, 300-309, 999)
            0..=9 | 100..=109 | 300..=309 | 999 => GroupCodeValueType::String,

            // Floating-point values (10-59, 110-149, 210-239, 460-469)
            10..=59 | 110..=149 | 210..=239 | 460..=469 => GroupCodeValueType::Double,

            // 16-bit integers (60-79, 170-179, 270-279, 370-389, 400-409)
            60..=79 | 170..=179 | 270..=279 | 370..=389 | 400..=409 => GroupCodeValueType::Int16,

            // 8-bit integers (280-289)
            280..=289 => GroupCodeValueType::Byte,

            // 32-bit integers (90-99, 420-429, 440-449)
            90..=99 | 420..=429 | 440..=449 => GroupCodeValueType::Int32,

            // 64-bit integers (160-169)
            160..=169 => GroupCodeValueType::Int64,

            // 32-bit integers (450-459) — DXF "Long" = 4-byte int
            450..=459 => GroupCodeValueType::Int32,

            // Boolean values (290-299)
            290..=299 => GroupCodeValueType::Bool,

            // Binary data (310-319)
            310..=319 => GroupCodeValueType::BinaryData,

            // Handle values (320-369, 390-399, 480-481)
            320..=369 | 390..=399 | 480..=481 => GroupCodeValueType::Handle,

            // String handles (410-419, 430-439, 470-479)
            410..=419 | 430..=439 | 470..=479 => GroupCodeValueType::String,

            // Extended data
            1004 => GroupCodeValueType::BinaryData,
            1005 => GroupCodeValueType::Handle,
            1000..=1009 => GroupCodeValueType::String,
            1040..=1042 => GroupCodeValueType::Double,
            1010..=1059 => GroupCodeValueType::Double,
            1060..=1070 => GroupCodeValueType::Int16,
            1071 => GroupCodeValueType::Int32,

            // Default to None for unknown codes
            _ => GroupCodeValueType::None,
        }
    }

    /// Check if this is a coordinate value (part of a 3D point)
    pub fn is_coordinate(code: DxfCode) -> bool {
        let code_num = code.to_i32();

        // X coordinates: 10, 11, 12, 13, 14, 15, 16, 17, 18, 110, 111, 112, 1010, 1011, 1012, 1013
        // Y coordinates: 20, 21, 22, 23, 24, 25, 26, 27, 28, 120, 121, 122, 1020, 1021, 1022, 1023
        // Z coordinates: 30, 31, 32, 33, 34, 35, 36, 37, 38, 130, 131, 132, 1030, 1031, 1032, 1033
        // Extrusion: 210, 220, 230

        matches!(
            code_num,
            10..=18 | 20..=28 | 30..=38 |
            110..=112 | 120..=122 | 130..=132 |
            210 | 220 | 230 |
            1010..=1013 | 1020..=1023 | 1030..=1033
        )
    }

    /// Get the coordinate axis (0=X, 1=Y, 2=Z) for a coordinate code
    pub fn coordinate_axis(code: DxfCode) -> Option<usize> {
        let code_num = code.to_i32();

        // X coordinates (10-18, 110-112, 210, 1010-1013)
        if matches!(code_num, 10..=18 | 110..=112 | 210 | 1010..=1013) {
            return Some(0);
        }

        // Y coordinates (20-28, 120-122, 220, 1020-1023)
        if matches!(code_num, 20..=28 | 120..=122 | 220 | 1020..=1023) {
            return Some(1);
        }

        // Z coordinates (30-38, 130-132, 230, 1030-1033)
        if matches!(code_num, 30..=38 | 130..=132 | 230 | 1030..=1033) {
            return Some(2);
        }

        None
    }

    /// Get the coordinate group index (0=primary, 1=secondary, etc.)
    pub fn coordinate_group(code: DxfCode) -> Option<usize> {
        let code_num = code.to_i32();

        match code_num {
            10 | 20 | 30 => Some(0), // Primary point
            11 | 21 | 31 => Some(1), // Secondary point
            12 | 22 | 32 => Some(2), // Tertiary point
            13 | 23 | 33 => Some(3), // Quaternary point
            14 | 24 | 34 => Some(4),
            15 | 25 | 35 => Some(5),
            16 | 26 | 36 => Some(6),
            17 | 27 | 37 => Some(7),
            18 | 28 | 38 => Some(8),
            110 | 120 | 130 => Some(10),     // UCS origin
            111 | 121 | 131 => Some(11),     // UCS X-axis
            112 | 122 | 132 => Some(12),     // UCS Y-axis
            210 | 220 | 230 => Some(21),     // Extrusion direction
            1010 | 1020 | 1030 => Some(100), // XData point
            1011 | 1021 | 1031 => Some(101), // XData world position
            1012 | 1022 | 1032 => Some(102), // XData world displacement
            1013 | 1023 | 1033 => Some(103), // XData world direction
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_type_from_code() {
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Start),
            GroupCodeValueType::String
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Text),
            GroupCodeValueType::String
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::XCoordinate),
            GroupCodeValueType::Double
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Int70),
            GroupCodeValueType::Int16
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Int90),
            GroupCodeValueType::Int32
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Int160),
            GroupCodeValueType::Int64
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::Bool290),
            GroupCodeValueType::Bool
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::BinaryData310),
            GroupCodeValueType::BinaryData
        );
        assert_eq!(
            GroupCodeValueType::from_code(DxfCode::SoftPointerId330),
            GroupCodeValueType::Handle
        );
    }

    #[test]
    fn test_is_coordinate() {
        assert!(GroupCodeValueType::is_coordinate(DxfCode::XCoordinate));
        assert!(GroupCodeValueType::is_coordinate(DxfCode::YCoordinate));
        assert!(GroupCodeValueType::is_coordinate(DxfCode::ZCoordinate));
        assert!(GroupCodeValueType::is_coordinate(DxfCode::ExtrusionX));
        assert!(!GroupCodeValueType::is_coordinate(DxfCode::Real40));
        assert!(!GroupCodeValueType::is_coordinate(DxfCode::Int70));
    }

    #[test]
    fn test_coordinate_axis() {
        assert_eq!(
            GroupCodeValueType::coordinate_axis(DxfCode::XCoordinate),
            Some(0)
        );
        assert_eq!(
            GroupCodeValueType::coordinate_axis(DxfCode::YCoordinate),
            Some(1)
        );
        assert_eq!(
            GroupCodeValueType::coordinate_axis(DxfCode::ZCoordinate),
            Some(2)
        );
        assert_eq!(GroupCodeValueType::coordinate_axis(DxfCode::Real40), None);
    }

    #[test]
    fn test_coordinate_group() {
        assert_eq!(
            GroupCodeValueType::coordinate_group(DxfCode::XCoordinate),
            Some(0)
        );
        assert_eq!(
            GroupCodeValueType::coordinate_group(DxfCode::XCoordinate1),
            Some(1)
        );
        assert_eq!(
            GroupCodeValueType::coordinate_group(DxfCode::XCoordinate2),
            Some(2)
        );
        assert_eq!(
            GroupCodeValueType::coordinate_group(DxfCode::ExtrusionX),
            Some(21)
        );
    }
}
