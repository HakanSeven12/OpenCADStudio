//! Extract the embedded preview image from a DWG file, for OS file-manager
//! thumbnails.
//!
//! Every DWG version stores an (uncompressed) preview at the raw file offset
//! recorded in the file header's preview seeker (byte `0x0D`). This crate reads
//! *only* that — no full document parse — and decodes it to an RGBA image. The
//! preview container is a fixed byte format, so this crate depends only on
//! `image` (no CAD library). Shared by OpenCADStudio (Linux, via its
//! `--dwg-thumbnail` mode), the Windows `IThumbnailProvider`, and the macOS
//! QuickLook extension.

use std::path::Path;

use image::ImageFormat;
// Re-exported so downstream crates (the Windows/macOS handlers) can name the
// returned image type without taking their own direct `image` dependency.
pub use image::RgbaImage;

/// Read the DWG at `path`, extract its embedded preview, and scale it so the
/// longest edge is at most `max_dim` pixels (aspect preserved). Returns `None`
/// for a DXF/other file, a missing or empty preview, or a preview in a format
/// this crate can't decode (WMF).
pub fn extract(path: &Path, max_dim: u32) -> Option<RgbaImage> {
    let (format, data) = read_preview(path)?;
    let img = decode(format, &data)?;
    Some(downscale(img, max_dim.max(1)))
}

/// Extract an embedded preview from an in-memory DWG. This is the browser
/// counterpart of [`extract`], where the file picker supplies bytes instead of
/// a reusable filesystem path.
pub fn extract_bytes(bytes: &[u8], max_dim: u32) -> Option<RgbaImage> {
    if bytes.get(..2)? != b"AC" {
        return None;
    }
    let base = i32::from_le_bytes(bytes.get(0x0D..0x11)?.try_into().ok()?);
    if base <= 0 {
        return None;
    }
    let base = base as u64;
    let start = usize::try_from(base).ok()?;
    let total = preview_container_len(bytes.get(start..start.checked_add(20)?)?)?;
    let container = bytes.get(start..start.checked_add(total)?)?;
    let (format, data) = parse_preview_container(container, base)?;
    let img = decode(format, data)?;
    Some(downscale(img, max_dim.max(1)))
}

/// White "DWG" wordmark, composited (centered) onto the format band.
static DWG_LABEL_PNG: &[u8] = include_bytes!("../assets/dwg-label.png");
/// Format band colour — OCS brand red.
const BAND_RGBA: [u8; 4] = [0xB0, 0x30, 0x20, 0xFF];

/// Append a full-width "DWG" band below a thumbnail (rectangular, spanning the
/// whole width) so DWG files read at a glance in the file manager. Grows the
/// image height by the band. Only DWG files ever produce a thumbnail (DXF has no
/// embedded preview and falls back to its file-type icon), so the label is
/// always "DWG".
pub fn badge_dwg(img: &mut RgbaImage) {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return;
    }
    // Band height ~20% of the thumbnail width (min 18 px).
    let band_h = ((w as f32 * 0.20) as u32).max(18);

    // New canvas prefilled with the band colour; the thumbnail covers the top,
    // leaving the bottom `band_h` rows as the band.
    let mut out = RgbaImage::from_pixel(w, h + band_h, image::Rgba(BAND_RGBA));
    image::imageops::overlay(&mut out, img, 0, 0);

    // Centre the white "DWG" wordmark in the band (~55% of the band height).
    if let Ok(label) = image::load_from_memory_with_format(DWG_LABEL_PNG, ImageFormat::Png) {
        let label = label.to_rgba8();
        if label.width() > 0 && label.height() > 0 {
            let mut target_h = ((band_h as f32 * 0.72) as u32).max(1);
            let mut target_w = ((label.width() as f32 * target_h as f32 / label.height() as f32)
                as u32)
                .max(1);
            // Don't let a wide wordmark spill past the thumbnail edges.
            let max_w = ((w as f32 * 0.90) as u32).max(1);
            if target_w > max_w {
                target_w = max_w;
                target_h = ((label.height() as f32 * target_w as f32 / label.width() as f32) as u32)
                    .max(1);
            }
            let label = image::imageops::thumbnail(&label, target_w, target_h);
            let x = ((w as i64 - target_w as i64) / 2).max(0);
            let y = h as i64 + (band_h as i64 - target_h as i64) / 2;
            image::imageops::overlay(&mut out, &label, x, y);
        }
    }

    *img = out;
}

// ── Preview extraction ───────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Fmt {
    Bmp,
    Png,
}

/// DWG preview container start sentinel — the same 16 bytes across all versions.
const PREVIEW_SENTINEL: [u8; 16] = [
    0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B,
];
/// Image descriptor codes (1 = header, 3 = WMF — both skipped).
const CODE_BMP: u8 = 2;
const CODE_PNG: u8 = 6;

/// Parse the preview container at the file's raw preview offset and return the
/// first BMP/PNG image. Self-contained byte parsing — the container is a fixed
/// DWG format, so this needs no CAD-library dependency (only `image`).
///
/// Layout: `[sentinel 16][overall_size RL][count RC] count×[code RC, start RL,
/// size RL] [image data][end sentinel 16]`, where `start` is an ABSOLUTE file
/// offset.
fn read_preview(path: &Path) -> Option<(Fmt, Vec<u8>)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut ver = [0u8; 6];
    f.read_exact(&mut ver).ok()?;
    if &ver[..2] != b"AC" {
        return None; // not a DWG (DXF/other)
    }
    // Preview seeker: absolute file offset at header byte 0x0D.
    f.seek(SeekFrom::Start(0x0D)).ok()?;
    let mut a = [0u8; 4];
    f.read_exact(&mut a).ok()?;
    let base = i32::from_le_bytes(a);
    if base <= 0 {
        return None;
    }
    let base = base as u64;

    // Sentinel + overall size, to learn the container length.
    f.seek(SeekFrom::Start(base)).ok()?;
    let mut head = [0u8; 20];
    f.read_exact(&mut head).ok()?;
    let total = preview_container_len(&head)?;
    f.seek(SeekFrom::Start(base)).ok()?;
    let mut buf = vec![0u8; total];
    f.read_exact(&mut buf).ok()?;
    let (format, data) = parse_preview_container(&buf, base)?;
    Some((format, data.to_vec()))
}

fn preview_container_len(head: &[u8]) -> Option<usize> {
    if head.get(..16)? != PREVIEW_SENTINEL {
        return None;
    }
    let overall = u32::from_le_bytes(head.get(16..20)?.try_into().ok()?) as usize;
    if overall == 0 || overall > 64 * 1024 * 1024 {
        return None;
    }
    // Whole container = sentinel(16) + size(4) + overall + end sentinel(16).
    36usize.checked_add(overall)
}

fn parse_preview_container(buf: &[u8], base: u64) -> Option<(Fmt, &[u8])> {
    let total = preview_container_len(buf.get(..20)?)?;
    if buf.len() < total {
        return None;
    }
    let count = *buf.get(20)? as usize;
    let mut off = 21usize;
    for _ in 0..count {
        if off + 9 > buf.len() {
            break;
        }
        let code = buf[off];
        let start =
            u32::from_le_bytes([buf[off + 1], buf[off + 2], buf[off + 3], buf[off + 4]]) as u64;
        let size =
            u32::from_le_bytes([buf[off + 5], buf[off + 6], buf[off + 7], buf[off + 8]]) as usize;
        off += 9;
        let fmt = match code {
            CODE_BMP => Fmt::Bmp,
            CODE_PNG => Fmt::Png,
            _ => continue, // header / WMF / unknown
        };
        // `start` is absolute; translate to a slice offset within `buf`.
        let rel = start.checked_sub(base)? as usize;
        let end = rel.checked_add(size)?;
        if size == 0 || end > buf.len() {
            continue;
        }
        return Some((fmt, &buf[rel..end]));
    }
    None
}

// ── Decode + scale ───────────────────────────────────────────────────────────

fn decode(format: Fmt, data: &[u8]) -> Option<RgbaImage> {
    let img = match format {
        Fmt::Png => image::load_from_memory_with_format(data, ImageFormat::Png).ok()?,
        Fmt::Bmp => image::load_from_memory_with_format(&dib_to_bmp(data), ImageFormat::Bmp).ok()?,
    };
    Some(img.to_rgba8())
}

fn downscale(img: RgbaImage, max_dim: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    if w <= max_dim && h <= max_dim {
        return img;
    }
    let (nw, nh) = if w >= h {
        (max_dim, ((h * max_dim) / w).max(1))
    } else {
        (((w * max_dim) / h).max(1), max_dim)
    };
    image::imageops::thumbnail(&img, nw, nh)
}

// ── C ABI (for the macOS QuickLook extension and other FFI consumers) ────────

/// Extract a DWG preview and encode it as a PNG. Writes a freshly-allocated
/// buffer to `*out_ptr` / `*out_len`; free it with [`dwg_thumbnail_free`].
/// Returns `true` on success. `path_utf8` is a NUL-terminated UTF-8 path.
///
/// # Safety
/// `path_utf8` must be a valid NUL-terminated string; `out_ptr`/`out_len` must
/// be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn dwg_thumbnail_png(
    path_utf8: *const std::os::raw::c_char,
    max_dim: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> bool {
    if path_utf8.is_null() || out_ptr.is_null() || out_len.is_null() {
        return false;
    }
    let cstr = std::ffi::CStr::from_ptr(path_utf8);
    let Ok(path) = cstr.to_str() else { return false };
    let Some(mut img) = extract(Path::new(path), max_dim) else {
        return false;
    };
    badge_dwg(&mut img); // full-width "DWG" band, same as every file-manager path
    let mut buf = std::io::Cursor::new(Vec::new());
    if img.write_to(&mut buf, ImageFormat::Png).is_err() {
        return false;
    }
    let mut bytes = buf.into_inner().into_boxed_slice();
    *out_ptr = bytes.as_mut_ptr();
    *out_len = bytes.len();
    std::mem::forget(bytes);
    true
}

/// Free a buffer returned by [`dwg_thumbnail_png`].
///
/// # Safety
/// `ptr`/`len` must be exactly the values written by a prior successful
/// `dwg_thumbnail_png`, and must be freed at most once.
#[no_mangle]
pub unsafe extern "C" fn dwg_thumbnail_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len != 0 {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}

/// Prepend the 14-byte `BITMAPFILEHEADER` a stored DIB lacks so a BMP decoder
/// can read it.
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 16 {
        return Vec::new();
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let palette = if (1..=8).contains(&bpp) { (1usize << bpp) * 4 } else { 0 };
    let mut v = Vec::with_capacity(14 + dib.len());
    v.extend_from_slice(b"BM");
    v.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&((14 + bi_size + palette) as u32).to_le_bytes());
    v.extend_from_slice(dib);
    v
}
