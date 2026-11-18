//! Pictures IMPORT, WMFIN, SVGIN and SVG paste bring in: Windows metafiles
//! and SVG drawings, read into the objects of a block. The import commands
//! run these readers off the UI thread.

use document::EntityType;

use super::{svg_import, wmf_import};

/// The reader a picture goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFormat {
    Wmf,
    Svg,
}

/// What a picture read gives: its objects, or why there are none.
#[derive(Debug, Clone)]
pub enum PictureRead {
    /// A metafile's objects and the faces its text uses.
    Wmf(wmf_import::Picture),
    /// An SVG drawing's objects.
    Svg(Vec<EntityType>),
    /// Nothing readable at the path.
    Missing,
    /// The bytes are not of the format.
    Unreadable,
    /// An SVG drawing with no shapes to import.
    Empty,
}

/// Reads the file at `path` (a source registered this session, else the
/// disk) as a picture of `format`, fitted to `fit`.
pub fn load(format: PictureFormat, path: &str, fit: [f64; 2]) -> PictureRead {
    match crate::scene::model::pdf_raster::source_bytes(path) {
        Some(bytes) => read(format, &bytes, fit),
        None => PictureRead::Missing,
    }
}

/// Reads `bytes` as a picture of `format`, fitted to `fit`.
pub fn read(format: PictureFormat, bytes: &[u8], fit: [f64; 2]) -> PictureRead {
    match format {
        PictureFormat::Wmf => {
            wmf_import::read(bytes, fit).map_or(PictureRead::Unreadable, PictureRead::Wmf)
        }
        PictureFormat::Svg => match svg_import::read(bytes, fit) {
            Ok(picture) => PictureRead::Svg(picture.entities),
            Err(svg_import::Error::NotSvg) => PictureRead::Unreadable,
            Err(svg_import::Error::Empty) => PictureRead::Empty,
        },
    }
}
