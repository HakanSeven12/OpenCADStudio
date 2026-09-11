//! Handle type for CAD objects
//!
//! Handles are unique 64-bit identifiers for all CAD objects in a document.

use std::fmt;

/// A unique identifier for CAD objects
///
/// Handles are 64-bit unsigned integers that uniquely identify
/// objects within a CAD document. Handle 0 is reserved and invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Handle(u64);

impl Handle {
    /// The null/invalid handle (0)
    pub const NULL: Handle = Handle(0);

    /// Create a new handle from a u64 value
    #[inline]
    pub const fn new(value: u64) -> Self {
        Handle(value)
    }

    /// Get the raw u64 value
    #[inline]
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// Check if this is a null/invalid handle
    #[inline]
    pub const fn is_null(&self) -> bool {
        self.0 == 0
    }

    /// Check if this is a valid handle
    #[inline]
    pub const fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

impl Default for Handle {
    fn default() -> Self {
        Handle::NULL
    }
}

impl From<u64> for Handle {
    fn from(value: u64) -> Self {
        Handle(value)
    }
}

impl From<Handle> for u64 {
    fn from(handle: Handle) -> Self {
        handle.0
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#X}", self.0)
    }
}

impl fmt::LowerHex for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_creation() {
        let handle = Handle::new(0x1234);
        assert_eq!(handle.value(), 0x1234);
    }

    #[test]
    fn test_null_handle() {
        let null = Handle::NULL;
        assert!(null.is_null());
        assert!(!null.is_valid());
        assert_eq!(null.value(), 0);
    }

    #[test]
    fn test_valid_handle() {
        let handle = Handle::new(42);
        assert!(!handle.is_null());
        assert!(handle.is_valid());
    }

    #[test]
    fn test_handle_display() {
        let handle = Handle::new(0xABCD);
        assert_eq!(format!("{}", handle), "0xABCD");
        assert_eq!(format!("{:x}", handle), "abcd");
        assert_eq!(format!("{:X}", handle), "ABCD");
    }

    #[test]
    fn test_handle_conversion() {
        let value: u64 = 12345;
        let handle: Handle = value.into();
        let back: u64 = handle.into();
        assert_eq!(value, back);
    }

    #[test]
    fn test_handle_ordering() {
        let h1 = Handle::new(100);
        let h2 = Handle::new(200);
        assert!(h1 < h2);
        assert!(h2 > h1);
    }
}
