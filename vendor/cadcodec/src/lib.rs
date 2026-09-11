//! # acadrust
//!
//! A pure Rust library for reading and writing CAD files in **DXF** and **DWG** formats.
//!
//! acadrust provides comprehensive support for both file formats with a focus on
//! correctness, type safety, and completeness — full-featured CAD file
//! manipulation for the Rust ecosystem.
//!
//! ## Highlights
//!
//! - **DXF** — Read and write ASCII and Binary DXF (R12 through R2018+)
//! - **DWG** — Read and write native DWG binary files (R13 through R2018+)
//! - **41 entity types**, 9 table types, 20+ non-graphical objects
//! - **ACIS/SAT/SAB** — Parse and write ACIS solid-model data (SAT text and SAB binary);
//!   parametric primitive builders for box, wedge, pyramid, cylinder, cone, sphere, and torus
//! - **Type safe** — strongly-typed entities, tables, and enums
//! - **Failsafe mode** — error-tolerant parsing that collects diagnostics
//! - **Encoding support** — automatic code page detection for pre-2007 files
//! - **Serde support** — optional `Serialize`/`Deserialize` for all types (enable the `serde` feature)
//!
//! ## Feature Flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `serde` | Enables `serde::Serialize` and `serde::Deserialize` on all document types |
//!
//! ```toml
//! [dependencies]
//! acadrust = { version = "0.5.4", features = ["serde"] }
//! ```
//!
//! ### Serialize an entity to JSON
//!
//! ```rust,ignore
//! use acadrust::entities::Line;
//!
//! let line = Line::from_coords(0.0, 0.0, 0.0, 100.0, 50.0, 0.0);
//! let json = serde_json::to_string_pretty(&line).unwrap();
//! println!("{json}");
//! ```
//!
//! ### Round-trip a full document
//!
//! ```rust,ignore
//! use acadrust::{CadDocument, DxfReader};
//!
//! let doc = DxfReader::from_file("input.dxf")?.read()?;
//!
//! // Serialize
//! let json = serde_json::to_string(&doc).unwrap();
//!
//! // Deserialize
//! let doc2: CadDocument = serde_json::from_str(&json).unwrap();
//! assert_eq!(doc2.entities().count(), doc.entities().count());
//! ```
//!
//! See the [`examples/serde_json.rs`](https://github.com/hakanaktt/acadrust/blob/main/examples/serde_json.rs)
//! example for more patterns including web-API-style entity lists.
//!
//! ## Quick Start — DXF
//!
//! ```rust,ignore
//! use acadrust::{CadDocument, DxfReader, DxfWriter};
//!
//! // Read
//! let doc = DxfReader::from_file("input.dxf")?.read()?;
//! println!("Entities: {}", doc.entities().count());
//!
//! // Write
//! DxfWriter::new(&doc).write_to_file("output.dxf")?;
//! # Ok::<(), acadrust::error::DxfError>(())
//! ```
//!
//! ## Quick Start — DWG
//!
//! ```rust,ignore
//! use acadrust::{CadDocument, DwgWriter};
//! use acadrust::io::dwg::DwgReader;
//! use acadrust::entities::*;
//! use acadrust::types::{Color, Vector3};
//!
//! // Read
//! let mut reader = DwgReader::from_file("input.dwg")?;
//! let doc = reader.read()?;
//!
//! // Create and write
//! let mut doc = CadDocument::new();
//! let mut line = Line::from_coords(0.0, 0.0, 0.0, 100.0, 50.0, 0.0);
//! line.common.color = Color::RED;
//! doc.add_entity(EntityType::Line(line))?;
//! DwgWriter::write_to_file("output.dwg", &doc)?;
//! # Ok::<(), acadrust::error::DxfError>(())
//! ```
//!
//! ## Module Overview
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`document`] | [`CadDocument`] — the central drawing container |
//! | [`entities`] | 41 graphical entity types ([`Line`], [`Circle`], [`Spline`], …) |
//! | [`tables`]   | Table entries ([`Layer`], [`LineType`], [`TextStyle`], [`DimStyle`], …) |
//! | [`objects`]   | Non-graphical objects (dictionaries, layouts, styles) |
//! | [`types`]     | Primitives ([`Vector3`], [`Color`], [`Handle`], [`DxfVersion`], …) |
//! | [`io`]        | Readers and writers for DXF and DWG |
//! | [`entities::acis`] | ACIS/SAT/SAB solid-model parser, writer, and primitive builders |
//! | [`classes`]   | DXF class definitions (CLASSES section) |
//! | [`xdata`]     | Extended data (XData) attached to entities |
//! | [`error`]     | Error types ([`DxfError`]) and [`Result`] alias |
//! | [`notification`] | Structured parse diagnostics |
//!
//! ## File Version Support
//!
//! | Code | AutoCAD | DXF | DWG |
//! |------|---------|-----|-----|
//! | AC1009 | R12     | R/W | —   |
//! | AC1012 | R13     | R/W | R/W |
//! | AC1014 | R14     | R/W | R/W |
//! | AC1015 | 2000    | R/W | R/W |
//! | AC1018 | 2004    | R/W | R/W |
//! | AC1021 | 2007    | R/W | R/W |
//! | AC1024 | 2010    | R/W | R/W |
//! | AC1027 | 2013    | R/W | R/W |
//! | AC1032 | 2018+   | R/W | R/W |

#![allow(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod classes;
pub mod compound_file;
pub mod document;
pub mod entities;
pub mod error;
pub mod fields;
pub mod io;
pub mod layer_state;
pub mod notification;
pub mod objects;
pub mod tables;
pub mod types;
pub mod vba;
pub mod xdata;

// Re-export commonly used types
pub use error::{DxfError, Result};
pub use types::{
    BoundingBox2D, BoundingBox3D, Color, DxfVersion, Handle, LineWeight, Transparency, Vector2,
    Vector3,
};

// Re-export entity types
pub use entities::{
    Arc, Circle, Ellipse, EmbeddedEntity, Entity, EntityType, Line, LwPolyline, MText, Point,
    Polyline, ProxyGraphicRecord, ProxyGraphics, ProxyUnicodeText, Spline, Text,
};

// Re-export table types
pub use tables::{
    AppId, BlockRecord, DimStyle, Layer, LineType, Table, TableEntry, TextStyle, Ucs, VPort, View,
    VxTableRecord,
};

// Re-export document
pub use document::{CadDocument, Preview, PreviewFormat, SolidHistoryGraph};
pub use layer_state::{LayerState, LayerStateLayer, LayerStateMask};

// Re-export I/O types
pub use io::dwg::{DwgReadOptions, DwgReader, DwgWriter};
pub use io::dxf::{DxfReader, DxfReaderConfiguration, DxfWriter};
pub use io::read::{
    push_read_diagnostic, ReadDiagnostic, ReadOutcome, ReadStage, ReadStats, SourceFormat,
    MAX_READ_DIAGNOSTICS,
};

// Re-export ACIS types
pub use entities::acis::primitives;
pub use entities::acis::{SabReader, SabWriter, SatParser, SatWriter};
pub use entities::acis::{SatDocument, SatHeader, SatPointer, SatRecord, SatToken, SatVersion};

// Re-export import types (when `import` feature is enabled)
#[cfg(feature = "import")]
pub use io::import::{
    import_file, ColladaImporter, FbxImporter, GltfImporter, ImportConfig, ImportFormat,
    ObjImporter, StlImporter,
};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_cad_document_creation() {
        let doc = CadDocument::new();
        assert_eq!(doc.version, DxfVersion::AC1032);

        let doc2 = CadDocument::with_version(DxfVersion::AC1015);
        assert_eq!(doc2.version, DxfVersion::AC1015);
    }
}
