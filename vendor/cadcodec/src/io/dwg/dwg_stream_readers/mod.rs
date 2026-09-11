//! DWG stream readers — bit-level deserialization for the DWG binary format.
//!
//! This mirrors the writer-side `dwg_stream_writers` module.

pub mod bit_reader;
pub mod classes_reader;
pub mod handle_reader;
pub mod header_reader;
pub mod merged_reader;
pub mod object_reader;
