// Embed a raster image inside the drawing itself as an OLE2FRAME entity.
//
// The IMAGE mechanism (RasterImage + ImageDefinition) stores only a file path,
// so a drawing copied to another machine loses its pictures. An OLE2FRAME
// instead carries the encoded raster inside an OLE compound file embedded in
// the entity data, so
// the .dwg is self-contained. cadcodec both builds the compound storage and
// reads it back, and the renderer already paints OLE2FRAMEs via
// ImageModel::from_ole2frame, so this module only packs and unpacks.

use std::io::Write as _;
use std::path::Path;

use acadrust::compound_file::{
    BinaryRecord, CompoundEntry, CompoundFile, CompoundStorage, CompoundStream,
    CompoundStreamContent, StructuredStoragePayload,
};
use acadrust::entities::{Ole2Frame, OleFrameEnvelope, OleObjectType};
use acadrust::types::Vector3;

/// OLE compound-file stream holding OLE 1.0 native data. cadcodec's
/// presentation extractor looks here first; the body is the encoded raster
/// behind a 4-byte little-endian length.
const OLE10NATIVE: &str = "\u{1}Ole10Native";
/// Leading header marker carried verbatim by the geometry envelope; readers
/// use it to recognise the envelope around the compound file.
const GEOMETRY_MARKER: u16 = 1;
/// Encoded formats the extractor recognises by magic and every DWG consumer
/// can decode. Anything else is re-encoded as PNG before embedding.
const PORTABLE_FORMATS: [image::ImageFormat; 3] = [
    image::ImageFormat::Png,
    image::ImageFormat::Jpeg,
    image::ImageFormat::Bmp,
];

/// A raster image prepared for embedding: encoded bytes plus the metadata the
/// placement prompts need.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedImage {
    /// Encoded raster bytes (PNG / JPEG / BMP as stored or re-encoded).
    pub bytes: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// File name the picture came from — surfaces in prompts and properties.
    pub name: String,
}

impl EmbeddedImage {
    /// Prepare an image file for embedding. Recognised raster formats are
    /// stored as-is; anything else is decoded and re-encoded as PNG so the
    /// payload always stays renderable.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string();
        Self::from_bytes(name, bytes)
    }

    /// Prepare already-read image bytes for embedding (web file handles).
    pub fn from_bytes(name: String, bytes: Vec<u8>) -> Result<Self, String> {
        let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
        let encoded = if PORTABLE_FORMATS.contains(&format) {
            bytes
        } else {
            encode_png(&bytes)?
        };
        let img = image::load_from_memory(&encoded).map_err(|e| e.to_string())?;
        let (pixel_width, pixel_height) = image::GenericImageView::dimensions(&img);
        Ok(Self {
            bytes: encoded,
            pixel_width,
            pixel_height,
            name,
        })
    }

    /// Width : height aspect ratio; a degenerate height falls back to square.
    pub fn aspect(&self) -> f64 {
        if self.pixel_height == 0 {
            1.0
        } else {
            self.pixel_width as f64 / self.pixel_height as f64
        }
    }
}

fn encode_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png.into_inner())
}

/// World-space corners for a picture placed with its bottom-left corner at
/// `origin`, extending `width` right and `width / aspect` up — the same
/// rectangle the IMAGE command previews.
pub fn corners_from_placement(origin: Vector3, width: f64, aspect: f64) -> (Vector3, Vector3) {
    let width = width.max(1e-6);
    let height = width / aspect.max(1e-6);
    (
        Vector3::new(origin.x, origin.y + height, origin.z),
        Vector3::new(origin.x + width, origin.y, origin.z),
    )
}

/// Pack `image` into an OLE2FRAME spanning `upper_left` → `lower_right` in
/// world space. The picture rides in an `\x01Ole10Native` stream as OLE 1.0
/// native data behind the geometry envelope cadcodec writes around it.
pub fn build_embedded_ole(
    image: &EmbeddedImage,
    upper_left: Vector3,
    lower_right: Vector3,
) -> Ole2Frame {
    // OLE 1.0 native: 4-byte little-endian payload length, then the raster.
    let mut native = Vec::with_capacity(4 + image.bytes.len());
    let _ = native.write(&(image.bytes.len() as u32).to_le_bytes());
    native.extend_from_slice(&image.bytes);

    let file = CompoundFile {
        minor_version: 0x003E,
        major_version: 3,
        transaction_signature: 0,
        root: CompoundStorage {
            name: "Root Entry".to_string(),
            class_id: [0; 16],
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            entries: vec![CompoundEntry::Stream(CompoundStream {
                name: OLE10NATIVE.to_string(),
                class_id: [0; 16],
                state_bits: 0,
                creation_time: 0,
                modified_time: 0,
                content: CompoundStreamContent::Binary(BinaryRecord::split(&native, 4096)),
            })],
        },
    };

    let mut ole = Ole2Frame::new();
    ole.ole_object_type = OleObjectType::Embedded;
    ole.source_application = "OpenCADStudio".to_string();
    ole.upper_left_corner = upper_left;
    ole.lower_right_corner = lower_right;
    ole.envelope = OleFrameEnvelope::Geometry {
        marker: GEOMETRY_MARKER,
        extension_records: Vec::new(),
    };
    ole.storage = StructuredStoragePayload {
        leading_records: Vec::new(),
        compound_file: Some(file),
        trailing_records: Vec::new(),
    };
    ole
}

/// Non-interactive embed used by the control API: add the entity to `doc` and
/// return its handle. The undo snapshot is the caller's responsibility.
pub fn add_embedded_image(
    doc: &mut acadrust::CadDocument,
    image: &EmbeddedImage,
    origin: Vector3,
    width: f64,
) -> Result<acadrust::Handle, String> {
    let (upper_left, lower_right) = corners_from_placement(origin, width, image.aspect());
    let ole = build_embedded_ole(image, upper_left, lower_right);
    doc.add_entity(acadrust::EntityType::Ole2Frame(ole))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid PNG: a solid-colour 4×3 image through the `image` crate.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        png.into_inner()
    }

    #[test]
    fn from_bytes_keeps_png_and_measures_pixels() {
        let embedded = EmbeddedImage::from_bytes("t.png".into(), png_bytes(4, 3)).unwrap();
        assert_eq!(embedded.pixel_width, 4);
        assert_eq!(embedded.pixel_height, 3);
        assert_eq!(embedded.bytes, png_bytes(4, 3));
    }

    #[test]
    fn from_bytes_reencodes_foreign_formats_as_png() {
        // TIFF starts with II*/MM* and is not in the portable set, so it must
        // come back re-encoded as PNG.
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        let mut tiff = std::io::Cursor::new(Vec::new());
        img.write_to(&mut tiff, image::ImageFormat::Tiff).unwrap();
        let embedded = EmbeddedImage::from_bytes("t.tif".into(), tiff.into_inner()).unwrap();
        assert!(embedded.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn packed_payload_roundtrips_through_the_extractor() {
        let png = png_bytes(4, 3);
        let image = EmbeddedImage {
            bytes: png.clone(),
            pixel_width: 4,
            pixel_height: 3,
            name: "t.png".into(),
        };
        let ole = build_embedded_ole(
            &image,
            Vector3::new(0.0, 3.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
        );
        let payload = ole.encoded_payload();
        match acadrust::entities::extract_presentation(&payload) {
            Some(acadrust::entities::OlePresentation::Raster(bytes)) => assert_eq!(bytes, png),
            other => panic!("expected the embedded raster back, got {other:?}"),
        }
    }

    #[test]
    fn dwg_roundtrip_preserves_the_embedded_picture() {
        let png = png_bytes(4, 3);
        let image = EmbeddedImage {
            bytes: png.clone(),
            pixel_width: 4,
            pixel_height: 3,
            name: "t.png".into(),
        };
        let mut doc = acadrust::CadDocument::new();
        let handle =
            add_embedded_image(&mut doc, &image, Vector3::new(10.0, 10.0, 0.0), 20.0).unwrap();

        let dir = std::env::temp_dir().join("ocs_ole_embed_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("embedded_roundtrip.dwg");
        acadrust::DwgWriter::write_to_file(&path, &doc).unwrap();

        let mut reader = acadrust::io::dwg::DwgReader::from_file(&path).unwrap();
        let reopened = reader.read().unwrap();
        std::fs::remove_file(&path).ok();

        let ole = match reopened.get_entity(handle) {
            Some(acadrust::EntityType::Ole2Frame(ole)) => ole,
            other => panic!("expected the Ole2Frame back, got {other:?}"),
        };
        // Corners survive: 20 wide at aspect 4:3 → 15 tall, base at (10,10).
        assert_eq!(ole.lower_right_corner, Vector3::new(30.0, 10.0, 0.0));
        assert_eq!(ole.upper_left_corner, Vector3::new(10.0, 25.0, 0.0));
        match acadrust::entities::extract_presentation(&ole.encoded_payload()) {
            Some(acadrust::entities::OlePresentation::Raster(bytes)) => assert_eq!(bytes, png),
            other => panic!("expected the embedded raster back, got {other:?}"),
        }
    }
}
