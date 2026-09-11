//! DWG file header infrastructure
//!
//! Provides file header writers for DWG file format variants:
//! - **AC15** (R13–R2000): Linear format with sequential sections
//! - **AC18** (R2004+): Page-based format with LZ77 compression
//! - **AC21** (R2007): Page-based format with RS encoding + LZ77 AC21 compression
//!
//! Also provides section definition constants, descriptor structs,
//! and locator record structs used by all formats.

pub mod file_header_ac15;
pub mod file_header_ac18;
pub mod file_header_ac21;
pub mod section_definition;
pub mod section_descriptor;

pub use file_header_ac15::DwgFileHeaderWriterAC15;
pub use file_header_ac18::DwgFileHeaderWriterAC18;
pub use file_header_ac21::DwgFileHeaderWriterAC21;
pub use section_definition::names as section_names;
pub use section_descriptor::{DwgLocalSectionMap, DwgSectionDescriptor, DwgSectionLocatorRecord};
