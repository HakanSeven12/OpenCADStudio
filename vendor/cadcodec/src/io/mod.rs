//! File I/O for DXF and DWG formats.
//!
//! | Sub-module | Capabilities |
//! |------------|----------------------------------------------------|
//! | [`dxf`]    | Read/write ASCII and Binary DXF (R12 – R2018+) |
//! | [`dwg`]    | Read/write native DWG binary (R13 – R2018) |
//!
//! The top-level re-exports [`DxfReader`], [`DxfWriter`], [`DwgReader`],
//! and [`DwgWriter`] for quick access.

pub mod dwg;
pub mod dxf;
mod loft_parameters;
mod loft_surface_curves;
pub mod read;
mod surface_history;

#[cfg(feature = "import")]
pub mod import;

pub use dwg::{DwgReader, DwgWriter};
pub use dxf::{DxfReader, DxfWriter};
pub use read::{
    push_read_diagnostic, ReadDiagnostic, ReadOutcome, ReadStage, ReadStats, SourceFormat,
    MAX_READ_DIAGNOSTICS,
};
