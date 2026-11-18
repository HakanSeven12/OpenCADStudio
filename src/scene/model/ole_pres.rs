// OLE2FRAME binary data → presentation RGBA image.
//
// Extraction (compound-file walk, OlePres header parse, WMFC → EMF
// reassembly) lives in document — `extract_presentation` returns the picture
// bytes tagged by kind. This module only decodes them: rasters through the
// `image` crate, metafiles through the `gdi` player.

use document::entities::{extract_presentation, OlePresentation};

use super::gdi;

/// An OLE2FRAME's picture: pixels as stored, or a metafile the `gdi` player
/// can draw at any size.
pub enum Picture {
    Pixels(Vec<u8>, u32, u32),
    Emf(Vec<u8>),
    Wmf(Vec<u8>),
}

/// The picture of an OLE2FRAME data blob.
pub fn picture(data: &[u8]) -> Option<Picture> {
    match extract_presentation(data)? {
        OlePresentation::Raster(bytes) => {
            let img = image::load_from_memory(&bytes).ok()?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if w == 0 || h == 0 {
                return None;
            }
            Some(Picture::Pixels(rgba.into_raw(), w, h))
        }
        OlePresentation::Dib(bytes) => {
            let (pixels, w, h) = gdi::dib::decode_packed(&bytes)?;
            Some(Picture::Pixels(pixels, w, h))
        }
        OlePresentation::Emf(bytes) => Some(Picture::Emf(bytes)),
        OlePresentation::Wmf(bytes) => Some(Picture::Wmf(bytes)),
    }
}

impl Picture {
    /// Whether the picture can be drawn at another size than one it stores.
    pub fn is_metafile(&self) -> bool {
        !matches!(self, Picture::Pixels(..))
    }

    /// The picture drawn at `target`. Stored pixels come as they are, whole,
    /// whatever the target asks.
    pub fn render(&self, target: gdi::Target) -> Option<gdi::Rendered> {
        match self {
            Picture::Pixels(pixels, width, height) => Some(gdi::Rendered {
                pixels: pixels.clone(),
                width: *width,
                height: *height,
                region: [0.0, 0.0, 1.0, 1.0],
            }),
            Picture::Emf(bytes) => gdi::render_emf_at(bytes, target),
            Picture::Wmf(bytes) => gdi::render_wmf_at(bytes, target),
        }
    }
}

/// Decode the picture for an OLE2FRAME data blob. Returns RGBA + dimensions.
pub fn decode(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let rendered = picture(data)?.render(gdi::Target::DEFAULT)?;
    Some((rendered.pixels, rendered.width, rendered.height))
}
