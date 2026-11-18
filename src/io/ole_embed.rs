// Embed a raster image inside the drawing itself as an OLE2FRAME entity.
//
// The IMAGE mechanism (RasterImage + ImageDefinition) stores only a file path,
// so a drawing copied to another machine loses its pictures. An OLE2FRAME
// instead carries the encoded raster inside an OLE compound file embedded in
// the entity data, so
// the .dwg is self-contained. document both builds the compound storage and
// reads it back, and the renderer already paints OLE2FRAMEs via
// ImageModel::from_ole2frame, so this module only packs and unpacks.

use std::io::Write as _;
use std::path::Path;

use document::compound_file::{
    BinaryRecord, CompoundEntry, CompoundFile, CompoundStorage, CompoundStream,
    CompoundStreamContent, StructuredStoragePayload,
};
use document::entities::{Ole2Frame, OleFrameEnvelope, OleObjectType};
use document::types::Vector3;

/// OLE compound-file stream holding OLE 1.0 native data. document's
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

/// The system Insert Object window, opened on Create from File (links and
/// icons off). The chosen file, or `Err("Cancelled")`; choosing Create New
/// reports that a new object needs its own application.
#[cfg(target_os = "windows")]
pub fn insert_object_dialog() -> Result<std::path::PathBuf, String> {
    use windows_sys::Win32::System::Ole::{
        OleInitialize, OleUIInsertObjectW, IOF_DISABLEDISPLAYASICON, IOF_DISABLELINK,
        IOF_SELECTCREATEFROMFILE, OLEUIINSERTOBJECTW, OLEUI_OK,
    };
    // The window takes at most MAX_PATH characters.
    let mut file = vec![0u16; 260];
    // SAFETY: the window runs in this thread's own OLE apartment, which ends
    // with the thread; the structure and the file buffer outlive the call.
    unsafe {
        OleInitialize(std::ptr::null());
        let mut info: OLEUIINSERTOBJECTW = std::mem::zeroed();
        info.cbStruct = std::mem::size_of::<OLEUIINSERTOBJECTW>() as u32;
        info.dwFlags = IOF_SELECTCREATEFROMFILE | IOF_DISABLELINK | IOF_DISABLEDISPLAYASICON;
        info.lpszFile = file.as_mut_ptr();
        info.cchFile = file.len() as u32;
        // The window writes the chosen mode back into the structure.
        let result = OleUIInsertObjectW(std::ptr::addr_of_mut!(info));
        let from_file = info.dwFlags & IOF_SELECTCREATEFROMFILE != 0;
        // OLEUI_CANCEL is 2; anything else is the window's error code.
        if result == 2 {
            return Err("Cancelled".into());
        }
        if result != OLEUI_OK {
            return Err(format!("Insert Object window error {result}"));
        }
        if !from_file {
            return Err(crate::tr!("ole-props", "create-new"));
        }
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Ok(std::path::PathBuf::from(String::from_utf16_lossy(
        &file[..len],
    )))
}

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
    /// A document embedded whole (an OLE package); `bytes` is then the
    /// picture of it the drawing shows.
    pub source: Option<PackagedFile>,
}

/// A file carried whole inside an OLE package: its name and its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct PackagedFile {
    pub name: String,
    pub bytes: Vec<u8>,
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
            source: None,
        })
    }

    /// A document of any kind, embedded whole as an OLE package and shown by
    /// a picture of its first page. LibreOffice draws that picture, so it
    /// must be installed.
    pub fn from_document(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        let picture = render_first_page(path)?;
        let mut embedded = Self::from_bytes(name.clone(), picture)?;
        embedded.source = Some(PackagedFile { name, bytes });
        Ok(embedded)
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
/// world space. A picture rides in an `\x01Ole10Native` stream as OLE 1.0
/// native data behind the geometry envelope document writes around it; a
/// document is an OLE package with the picture as its presentation.
pub fn build_embedded_ole(
    image: &EmbeddedImage,
    upper_left: Vector3,
    lower_right: Vector3,
) -> Ole2Frame {
    let stream = |name: &str, data: &[u8]| {
        CompoundEntry::Stream(CompoundStream {
            name: name.to_string(),
            class_id: [0; 16],
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            content: CompoundStreamContent::Binary(BinaryRecord::split(data, 4096)),
        })
    };
    let (class_id, entries) = match &image.source {
        Some(file) => (
            CLSID_PACKAGE,
            vec![
                stream(OLE_STREAM, &ole_stream()),
                stream(COMP_OBJ, &comp_obj()),
                stream(OLE10NATIVE, &package_native(file)),
                stream(OLE_PRES, &presentation_dib(image)),
            ],
        ),
        None => {
            // OLE 1.0 native: 4-byte little-endian payload length, then the raster.
            let mut native = Vec::with_capacity(4 + image.bytes.len());
            let _ = native.write(&(image.bytes.len() as u32).to_le_bytes());
            native.extend_from_slice(&image.bytes);
            ([0; 16], vec![stream(OLE10NATIVE, &native)])
        }
    };

    let file = CompoundFile {
        minor_version: 0x003E,
        major_version: 3,
        transaction_signature: 0,
        root: CompoundStorage {
            name: "Root Entry".to_string(),
            class_id,
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            entries,
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

/// CLSID_Package {0003000C-0000-0000-C000-000000000046}, the system's
/// packager: it opens the packaged file with the program for its type.
const CLSID_PACKAGE: [u8; 16] = [
    0x0C, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
const OLE_STREAM: &str = "\u{1}Ole";
const COMP_OBJ: &str = "\u{1}CompObj";
const OLE_PRES: &str = "\u{2}OlePres000";

/// The `\x01Ole` stream (MS-OLEDS 2.3.3) of an embedded object.
fn ole_stream() -> Vec<u8> {
    let mut out = 0x0200_0001u32.to_le_bytes().to_vec();
    out.extend_from_slice(&[0; 16]);
    out
}

/// The `\x01CompObj` stream (MS-OLEDS 2.3.8) naming the packager.
fn comp_obj() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xFFFE_0001u32.to_le_bytes());
    out.extend_from_slice(&0x0000_0A03u32.to_le_bytes());
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&CLSID_PACKAGE);
    let ansi = |out: &mut Vec<u8>, text: &str| {
        out.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.push(0);
    };
    ansi(&mut out, "Package");
    // No clipboard format.
    out.extend_from_slice(&0u32.to_le_bytes());
    ansi(&mut out, "Package");
    out
}

/// A file name as the package's ANSI strings take it.
fn ansi_name(name: &str) -> Vec<u8> {
    name.chars()
        .map(|c| if c.is_ascii() && c != '\0' { c as u8 } else { b'_' })
        .collect()
}

/// The packager's native data: its size, then the label, the file's path,
/// the path it is unpacked to, and the file's bytes.
fn package_native(file: &PackagedFile) -> Vec<u8> {
    let name = ansi_name(&file.name);
    let mut body = 2u16.to_le_bytes().to_vec();
    for _ in 0..2 {
        body.extend_from_slice(&name);
        body.push(0);
    }
    body.extend_from_slice(&[0, 0, 3, 0]);
    body.extend_from_slice(&(name.len() as u32 + 1).to_le_bytes());
    body.extend_from_slice(&name);
    body.push(0);
    body.extend_from_slice(&(file.bytes.len() as u32).to_le_bytes());
    body.extend_from_slice(&file.bytes);
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}

/// The file an OLE object packages, read back from its native data.
pub fn packaged_file(ole: &Ole2Frame) -> Option<PackagedFile> {
    let file = ole.storage.compound_file.as_ref()?;
    let native = file.root.entries.iter().find_map(|entry| match entry {
        CompoundEntry::Stream(stream) if stream.name == OLE10NATIVE => {
            Some(stream.content.encode())
        }
        _ => None,
    })?;
    let body = native.get(4..)?;
    if body.get(..2)? != [2, 0] {
        return None;
    }
    let mut at = 2;
    let z_string = |at: &mut usize| -> Option<Vec<u8>> {
        let end = *at + body.get(*at..)?.iter().position(|b| *b == 0)?;
        let text = body[*at..end].to_vec();
        *at = end + 1;
        Some(text)
    };
    let label = z_string(&mut at)?;
    z_string(&mut at)?;
    at += 4;
    let u32_at = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(body.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    at += 4 + u32_at(at)?;
    let size = u32_at(at)?;
    let bytes = body.get(at + 4..at + 4 + size)?.to_vec();
    let name = String::from_utf8_lossy(&label).into_owned();
    Some(PackagedFile { name, bytes })
}

/// The `\x02OlePres000` stream (MS-OLEDS 2.3.4): the picture as a CF_DIB,
/// sized in HIMETRIC at 96 pixels per inch.
fn presentation_dib(image: &EmbeddedImage) -> Vec<u8> {
    let rgb = image::load_from_memory(&image.bytes)
        .map(|picture| picture.to_rgb8())
        .unwrap_or_default();
    let (width, height) = rgb.dimensions();
    let stride = (width as usize * 3).div_ceil(4) * 4;
    let mut dib = Vec::with_capacity(40 + stride * height as usize);
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&((stride * height as usize) as u32).to_le_bytes());
    dib.extend_from_slice(&3780i32.to_le_bytes());
    dib.extend_from_slice(&3780i32.to_le_bytes());
    dib.extend_from_slice(&[0; 8]);
    // Rows bottom up, each pixel blue, green, red.
    for row in rgb.rows().rev() {
        let start = dib.len();
        for pixel in row {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        dib.resize(start + stride, 0);
    }
    let himetric = |pixels: u32| pixels * 2540 / 96;
    let mut out = Vec::with_capacity(40 + dib.len());
    // CF_DIB, no target device, content aspect, any index.
    for value in [0xFFFF_FFFF, 8, 4, 1, 0xFFFF_FFFF, 0, 0] {
        out.extend_from_slice(&u32::to_le_bytes(value));
    }
    out.extend_from_slice(&himetric(width).to_le_bytes());
    out.extend_from_slice(&himetric(height).to_le_bytes());
    out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
    out.extend(dib);
    out
}

/// LibreOffice's program, on the search path or where its installer puts it.
fn office_program() -> Option<std::path::PathBuf> {
    let installed = [
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ];
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .flat_map(|dir| [dir.join("soffice"), dir.join("soffice.exe")])
        .chain(installed.into_iter().map(std::path::PathBuf::from))
        .find(|path| path.is_file())
}

/// A PNG of the document's first page, drawn by LibreOffice. It runs with a
/// profile of its own, so a LibreOffice the user has open does not take the
/// job.
fn render_first_page(path: &Path) -> Result<Vec<u8>, String> {
    let program = office_program().ok_or_else(|| crate::tr!("ole-props", "needs-office"))?;
    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let work =
        std::env::temp_dir().join(format!("ocs-ole-render-{}-{run}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let profile = work.join("profile");
    let profile_url = format!(
        "file:///{}",
        profile.to_string_lossy().replace('\\', "/").trim_start_matches('/')
    );
    let status = std::process::Command::new(program)
        .arg(format!("-env:UserInstallation={profile_url}"))
        .args(["--headless", "--convert-to", "png", "--outdir"])
        .arg(&work)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let png = std::fs::read(work.join(format!("{stem}.png")));
    let _ = std::fs::remove_dir_all(&work);
    match png {
        Ok(png) if status.success() => Ok(png),
        _ => Err(crate::tr!("ole-props", "render-failed")),
    }
}

/// Non-interactive embed used by the control API: add the entity to `doc` and
/// return its handle. The undo snapshot is the caller's responsibility.
pub fn add_embedded_image(
    doc: &mut document::Document,
    image: &EmbeddedImage,
    origin: Vector3,
    width: f64,
) -> Result<document::Handle, String> {
    let (upper_left, lower_right) = corners_from_placement(origin, width, image.aspect());
    let ole = build_embedded_ole(image, upper_left, lower_right);
    doc.add_entity(document::EntityType::Ole2Frame(Box::new(ole)))
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
            source: None,
        };
        let ole = build_embedded_ole(
            &image,
            Vector3::new(0.0, 3.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
        );
        let payload = ole.encoded_payload();
        match document::entities::extract_presentation(&payload) {
            Some(document::entities::OlePresentation::Raster(bytes)) => assert_eq!(bytes, png),
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
            source: None,
        };
        let mut doc = document::Document::new();
        let handle =
            add_embedded_image(&mut doc, &image, Vector3::new(10.0, 10.0, 0.0), 20.0).unwrap();

        let dir = std::env::temp_dir().join("ocs_ole_embed_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("embedded_roundtrip.dwg");
        document::DwgWriter::write_to_file(&path, &doc).unwrap();

        let mut reader = document::DwgReader::from_file(&path).unwrap();
        let reopened = reader.read().unwrap();
        std::fs::remove_file(&path).ok();

        let ole = match reopened.get_entity(handle) {
            Some(document::EntityType::Ole2Frame(ole)) => ole,
            other => panic!("expected the Ole2Frame back, got {other:?}"),
        };
        // Corners survive: 20 wide at aspect 4:3 → 15 tall, base at (10,10).
        assert_eq!(ole.lower_right_corner, Vector3::new(30.0, 10.0, 0.0));
        assert_eq!(ole.upper_left_corner, Vector3::new(10.0, 25.0, 0.0));
        match document::entities::extract_presentation(&ole.encoded_payload()) {
            Some(document::entities::OlePresentation::Raster(bytes)) => assert_eq!(bytes, png),
            other => panic!("expected the embedded raster back, got {other:?}"),
        }
    }
}

