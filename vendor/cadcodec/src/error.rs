//! Error types for acadrust.
//!
//! The crate uses [`DxfError`] as the single error enum and provides a
//! [`Result<T>`] type alias (`Result<T, DxfError>`) for convenience.

use std::io;
use thiserror::Error;

/// Main error type for acadrust operations
#[derive(Debug, Error)]
pub enum DxfError {
    /// IO error occurred during file operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Unsupported CAD file version
    #[error("Unsupported CAD version: {0:?}")]
    UnsupportedVersion(String),

    /// Error during compression/decompression
    #[error("Compression error: {0}")]
    Compression(String),

    /// Error parsing CAD file format
    #[error("Parse error: {0}")]
    Parse(String),

    /// Invalid DXF code encountered
    #[error("Invalid DXF code: {0}")]
    InvalidDxfCode(i32),

    /// Invalid handle reference
    #[error("Invalid handle: {0:#X}")]
    InvalidHandle(u64),

    /// Object not found in document
    #[error("Object not found: handle {0:#X}")]
    ObjectNotFound(u64),

    /// Invalid entity type
    #[error("Invalid entity type: {0}")]
    InvalidEntityType(String),

    /// CRC checksum mismatch
    #[error("CRC checksum mismatch: expected {expected:#X}, got {actual:#X}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    /// Invalid file header
    #[error("Invalid file header: {0}")]
    InvalidHeader(String),

    /// Invalid file format
    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

    /// Invalid sentinel in file
    #[error("Invalid sentinel: {0}")]
    InvalidSentinel(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Decryption error
    #[error("Decryption error: {0}")]
    Decryption(String),

    /// Encoding error
    #[error("Encoding error: {0}")]
    Encoding(String),

    /// Feature not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Error importing external CAD format (STL, COLLADA, STEP, IGES)
    #[cfg(feature = "import")]
    #[error("Import error: {0}")]
    ImportError(String),

    /// Generic error with custom message
    #[error("{0}")]
    Custom(String),
}

/// Result type alias for acadrust operations
pub type Result<T> = std::result::Result<T, DxfError>;

impl From<String> for DxfError {
    fn from(s: String) -> Self {
        DxfError::Custom(s)
    }
}

impl From<&str> for DxfError {
    fn from(s: &str) -> Self {
        DxfError::Custom(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = DxfError::UnsupportedVersion("AC1009".to_string());
        assert_eq!(err.to_string(), "Unsupported CAD version: \"AC1009\"");
    }

    #[test]
    fn test_checksum_error() {
        let err = DxfError::ChecksumMismatch {
            expected: 0x1234,
            actual: 0x5678,
        };
        assert!(err.to_string().contains("0x1234"));
        assert!(err.to_string().contains("0x5678"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let dxf_err: DxfError = io_err.into();
        assert!(matches!(dxf_err, DxfError::Io(_)));
    }
}
