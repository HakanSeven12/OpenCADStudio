//! DWG stream writers for bit-level binary output
//!
//! These writers handle the low-level bit manipulation required by
//! the DWG format, including variable-length encodings and the
//! triple-stream (main/text/handle) object record architecture.
//!
//! ## Section writers (Sprint 3)
//!
//! Higher-level writers that produce complete DWG sections:
//! - `app_info_writer` — application information section
//! - `aux_header_writer` — auxiliary header (version, timestamps, HANDSEED)
//! - `classes_writer` — DXF class definitions section
//! - `handle_writer` — handle-to-offset mapping section
//! - `header_writer` — header variables section (~200 fields)

pub mod bit_writer;
pub mod merged_writer;

pub mod app_info_writer;
pub mod aux_header_writer;
pub mod classes_writer;
pub mod handle_writer;
pub mod header_writer;
pub mod object_writer;

pub use bit_writer::DwgBitWriter;
pub use merged_writer::DwgMergedWriter;
pub use object_writer::DwgObjectWriter;
