//! DWG handle reference types
//!
//! Handle references in DWG encode both the type of reference (ownership,
//! pointer) and the handle value. The reference type determines the
//! relationship between objects.

/// DWG handle reference type, encoded in the high nibble of the handle header byte.
///
/// See OpenDesign spec section "Handle References".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum DwgReferenceType {
    /// Undefined / absolute handle reference (type 0)
    Undefined = 0x00,
    /// Soft ownership reference (type 2)
    /// The target object is owned by the source, but the reference
    /// doesn't prevent deletion.
    SoftOwnership = 0x02,
    /// Hard ownership reference (type 3)
    /// The target object is owned by the source and cannot exist without it.
    HardOwnership = 0x03,
    /// Soft pointer reference (type 4)
    /// The source points to the target, but doesn't own it.
    SoftPointer = 0x04,
    /// Hard pointer reference (type 5)
    /// The source requires the target to exist.
    HardPointer = 0x05,
}

impl DwgReferenceType {
    /// Get the numeric value for the reference type header nibble.
    pub fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_type_codes() {
        assert_eq!(DwgReferenceType::Undefined.code(), 0);
        assert_eq!(DwgReferenceType::SoftOwnership.code(), 2);
        assert_eq!(DwgReferenceType::HardOwnership.code(), 3);
        assert_eq!(DwgReferenceType::SoftPointer.code(), 4);
        assert_eq!(DwgReferenceType::HardPointer.code(), 5);
    }
}
