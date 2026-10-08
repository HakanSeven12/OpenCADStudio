// OLE2FRAME binary data → presentation RGBA image.
//
// Extraction (compound-file walk, OlePres header parse, WMFC → EMF
// reassembly) lives in opencadcodec — `extract_presentation` returns the picture
// bytes tagged by kind. This module only decodes them: rasters through the
// `image` crate, metafiles through the `gdi` player.

use codec::entities::{extract_presentation, OlePresentation};

use super::gdi;

/// Decode the picture for an OLE2FRAME data blob. Returns RGBA + dimensions.
pub fn decode(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    match extract_presentation(data)? {
        OlePresentation::Raster(bytes) => {
            let img = image::load_from_memory(&bytes).ok()?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if w == 0 || h == 0 {
                return None;
            }
            Some((rgba.into_raw(), w, h))
        }
        OlePresentation::Dib(bytes) => gdi::dib::decode_packed(&bytes),
        OlePresentation::Emf(bytes) => gdi::render_emf(&bytes),
        OlePresentation::Wmf(bytes) => gdi::render_wmf(&bytes),
    }
}
