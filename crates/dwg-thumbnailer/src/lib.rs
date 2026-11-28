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

use std::io::SeekFrom;
use std::path::Path;

use image::ImageFormat;
// Re-exported so downstream crates (the Windows/macOS handlers) can name the
// returned image type without taking their own direct `image` dependency.
pub use image::RgbaImage;

/// Maximum thumbnail edge length accepted by extraction, scaling, badging, and FFI.
pub const MAX_THUMBNAIL_DIM: u32 = 4096;
/// Upper bound on an embedded DWG preview container payload (64 MiB).
pub const MAX_PREVIEW_CONTAINER_BYTES: usize = 64 * 1024 * 1024;
const MAX_DECODE_DIM: u32 = 8192;
const MAX_DECODE_ALLOC_BYTES: u64 = 128 * 1024 * 1024;

/// Extracted raw DWG preview payload ready for bounded decoding and scaling.
#[derive(Debug, Clone)]
pub struct PreviewData {
    format: Fmt,
    bytes: Vec<u8>,
}

impl PreviewData {
    /// Seek directly to the DWG header preview pointer and read only the bounded
    /// preview container (`<= MAX_PREVIEW_CONTAINER_BYTES`), avoiding loading the
    /// full drawing into memory.
    pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Option<Self> {
        reader.seek(SeekFrom::Start(0)).ok()?;
        let mut ver = [0u8; 6];
        reader.read_exact(&mut ver).ok()?;
        if &ver[..2] != b"AC" {
            return None; // not a DWG (DXF/other)
        }
        // Preview seeker: absolute file offset at header byte 0x0D.
        reader.seek(SeekFrom::Start(0x0D)).ok()?;
        let mut a = [0u8; 4];
        reader.read_exact(&mut a).ok()?;
        let base = i32::from_le_bytes(a);
        if base <= 0 {
            return None;
        }
        let base = base as u64;

        // Sentinel + overall size, to learn the container length.
        reader.seek(SeekFrom::Start(base)).ok()?;
        let mut head = [0u8; 20];
        reader.read_exact(&mut head).ok()?;
        let total = preview_container_len(&head)?;
        let container_end = base.checked_add(u64::try_from(total).ok()?)?;
        if let Ok(stream_len) = reader.seek(SeekFrom::End(0)) {
            if stream_len < container_end {
                return None;
            }
            reader.seek(SeekFrom::Start(base.checked_add(20)?)).ok()?;
        }
        let mut buf = vec![0u8; total];
        buf[..20].copy_from_slice(&head);
        reader.read_exact(&mut buf[20..]).ok()?;
        let (format, data) = parse_preview_container(&buf, base)?;
        Some(Self {
            format,
            bytes: data.to_vec(),
        })
    }

    /// Decode the extracted preview payload under strict dimension/allocation limits
    /// and scale it so the longest edge is at most `max_dim.clamp(1, MAX_THUMBNAIL_DIM)`.
    pub fn decode_scaled(&self, max_dim: u32) -> Option<RgbaImage> {
        let img = decode(self.format, &self.bytes)?;
        Some(downscale(img, max_dim.clamp(1, MAX_THUMBNAIL_DIM)))
    }
}

/// Read the DWG at `path`, extract its embedded preview, and scale it so the
/// longest edge is at most `max_dim` pixels (aspect preserved, clamped to
/// `1..=MAX_THUMBNAIL_DIM`). Returns `None` for a DXF/other file, a missing or
/// empty preview, or a preview in a format this crate can't decode (WMF).
pub fn extract(path: &Path, max_dim: u32) -> Option<RgbaImage> {
    let mut f = std::fs::File::open(path).ok()?;
    PreviewData::read_from(&mut f)?.decode_scaled(max_dim)
}

/// Extract an embedded preview from an in-memory DWG. This is the browser
/// counterpart of [`extract`], where the file picker supplies bytes instead of
/// a reusable filesystem path.
pub fn extract_bytes(bytes: &[u8], max_dim: u32) -> Option<RgbaImage> {
    let mut cursor = std::io::Cursor::new(bytes);
    PreviewData::read_from(&mut cursor)?.decode_scaled(max_dim)
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
    badge_dwg_clear_of(img, 0.0);
}

/// Windows Explorer draws the opening application's icon over the bottom-right
/// corner of a thumbnail; the share of the width it may cover.
pub const WINDOWS_TYPE_OVERLAY: f32 = 0.36;

/// [`badge_dwg`] with the wordmark kept clear of the rightmost `right_clear`
/// share of the width: it is centred in what is left.
pub fn badge_dwg_clear_of(img: &mut RgbaImage, right_clear: f32) {
    let right_clear = if right_clear.is_finite() {
        right_clear.clamp(0.0, 0.9)
    } else {
        0.0
    };
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || w > MAX_THUMBNAIL_DIM || h > MAX_THUMBNAIL_DIM {
        return;
    }
    // Band height ~20% of the thumbnail width (min 18 px).
    let band_h = ((w as f32 * 0.20) as u32).max(18);
    let Some(total_h) = h.checked_add(band_h) else {
        return;
    };
    let within_budget = u64::from(w)
        .checked_mul(u64::from(total_h))
        .and_then(|px| px.checked_mul(4))
        .is_some_and(|bytes| bytes <= MAX_DECODE_ALLOC_BYTES);
    if !within_budget {
        return;
    }

    // New canvas prefilled with the band colour; the thumbnail covers the top,
    // leaving the bottom `band_h` rows as the band.
    let mut out = RgbaImage::from_pixel(w, total_h, image::Rgba(BAND_RGBA));
    image::imageops::overlay(&mut out, img, 0, 0);

    // Centre the white "DWG" wordmark in the band (~55% of the band height).
    if let Some(label) = decode_with_limits(DWG_LABEL_PNG, ImageFormat::Png) {
        if label.width() > 0 && label.height() > 0 {
            let mut target_h = ((band_h as f32 * 0.72) as u32).max(1);
            let mut target_w =
                ((label.width() as f32 * target_h as f32 / label.height() as f32) as u32).max(1);
            // Don't let a wide wordmark spill past the thumbnail edges.
            let free_w = (w as f32 * (1.0 - right_clear)).max(1.0);
            let max_w = ((free_w - w as f32 * 0.10) as u32).max(1);
            if target_w > max_w {
                target_w = max_w;
                target_h = ((label.height() as f32 * target_w as f32 / label.width() as f32)
                    as u32)
                    .max(1);
            }
            let label = image::imageops::thumbnail(&label, target_w, target_h);
            let x = ((free_w as i64 - target_w as i64) / 2).max(0);
            let y = h as i64 + (band_h as i64 - target_h as i64) / 2;
            image::imageops::overlay(&mut out, &label, x, y);
        }
    }

    *img = out;
}

// ── Preview extraction ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
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

fn preview_container_len(head: &[u8]) -> Option<usize> {
    if head.get(..16)? != PREVIEW_SENTINEL {
        return None;
    }
    let overall = usize::try_from(u32::from_le_bytes(head.get(16..20)?.try_into().ok()?)).ok()?;
    if overall == 0 || overall > MAX_PREVIEW_CONTAINER_BYTES {
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
        if off.checked_add(9).is_none_or(|end| end > buf.len()) {
            break;
        }
        let code = buf[off];
        let start =
            u32::from_le_bytes([buf[off + 1], buf[off + 2], buf[off + 3], buf[off + 4]]) as u64;
        let size = usize::try_from(u32::from_le_bytes([
            buf[off + 5],
            buf[off + 6],
            buf[off + 7],
            buf[off + 8],
        ]))
        .ok()?;
        off += 9;
        let fmt = match code {
            CODE_BMP => Fmt::Bmp,
            CODE_PNG => Fmt::Png,
            _ => continue, // header / WMF / unknown
        };
        // `start` is absolute; translate to a slice offset within `buf`.
        let rel = usize::try_from(start.checked_sub(base)?).ok()?;
        let end = rel.checked_add(size)?;
        if size == 0 || end > buf.len() {
            continue;
        }
        return Some((fmt, &buf[rel..end]));
    }
    None
}

// ── Decode + scale ───────────────────────────────────────────────────────────

fn decode_with_limits(bytes: &[u8], img_fmt: ImageFormat) -> Option<RgbaImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIM);
    limits.max_image_height = Some(MAX_DECODE_DIM);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), img_fmt);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let byte_len = u64::from(img.width())
        .checked_mul(u64::from(img.height()))?
        .checked_mul(4)?;
    if byte_len > MAX_DECODE_ALLOC_BYTES {
        return None;
    }
    Some(img.to_rgba8())
}

fn decode(format: Fmt, data: &[u8]) -> Option<RgbaImage> {
    match format {
        Fmt::Png => decode_with_limits(data, ImageFormat::Png),
        Fmt::Bmp => {
            let bmp = dib_to_bmp(data)?;
            decode_with_limits(&bmp, ImageFormat::Bmp)
        }
    }
}

fn downscale(img: RgbaImage, max_dim: u32) -> RgbaImage {
    let max_dim = max_dim.clamp(1, MAX_THUMBNAIL_DIM);
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || (w <= max_dim && h <= max_dim) {
        return img;
    }
    let (nw, nh) = if w >= h {
        let scaled = ((u64::from(h) * u64::from(max_dim)) / u64::from(w)).max(1) as u32;
        (max_dim, scaled)
    } else {
        let scaled = ((u64::from(w) * u64::from(max_dim)) / u64::from(h)).max(1) as u32;
        (scaled, max_dim)
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
    if !out_ptr.is_null() {
        *out_ptr = std::ptr::null_mut();
    }
    if !out_len.is_null() {
        *out_len = 0;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if path_utf8.is_null() || out_ptr.is_null() || out_len.is_null() {
            return false;
        }
        // SAFETY: caller contract guarantees `path_utf8` points to a NUL-terminated C string.
        let cstr = unsafe { std::ffi::CStr::from_ptr(path_utf8) };
        let Ok(path) = cstr.to_str() else {
            return false;
        };
        let Some(mut img) = extract(Path::new(path), max_dim.clamp(1, MAX_THUMBNAIL_DIM)) else {
            return false;
        };
        badge_dwg(&mut img); // full-width "DWG" band, same as every file-manager path
        let mut buf = std::io::Cursor::new(Vec::new());
        if img.write_to(&mut buf, ImageFormat::Png).is_err() {
            return false;
        }
        let mut bytes = buf.into_inner().into_boxed_slice();
        // SAFETY: `out_ptr` and `out_len` are non-null and writable per caller contract.
        unsafe {
            *out_ptr = bytes.as_mut_ptr();
            *out_len = bytes.len();
        }
        std::mem::forget(bytes);
        true
    }))
    .unwrap_or(false)
}

/// Free a buffer returned by [`dwg_thumbnail_png`].
///
/// # Safety
/// `ptr`/`len` must be exactly the values written by a prior successful
/// `dwg_thumbnail_png`, and must be freed at most once.
#[no_mangle]
pub unsafe extern "C" fn dwg_thumbnail_free(ptr: *mut u8, len: usize) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !ptr.is_null() && len != 0 {
            // SAFETY: `ptr`/`len` came from `Box::into_boxed_slice` in `dwg_thumbnail_png`.
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
        }
    }));
}

/// Prepend the 14-byte `BITMAPFILEHEADER` a stored DIB lacks so a BMP decoder
/// can read it.
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if bi_size < 40 || bi_size > dib.len() {
        return None;
    }
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let bi_clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if (1..=8).contains(&bpp) {
        let max_entries = 1usize << bpp;
        let entries = if bi_clr_used > 0 && bi_clr_used <= max_entries {
            bi_clr_used
        } else {
            max_entries
        };
        entries.checked_mul(4)?
    } else {
        0
    };
    let file_size = 14usize.checked_add(dib.len())?;
    let pixel_offset = 14usize.checked_add(bi_size)?.checked_add(palette)?;
    if pixel_offset > file_size {
        return None;
    }
    let file_size_u32 = u32::try_from(file_size).ok()?;
    let pixel_offset_u32 = u32::try_from(pixel_offset).ok()?;
    let mut v = Vec::with_capacity(file_size);
    v.extend_from_slice(b"BM");
    v.extend_from_slice(&file_size_u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&pixel_offset_u32.to_le_bytes());
    v.extend_from_slice(dib);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};

    fn make_synthetic_dwg(base: usize, code: u8, payload: &[u8]) -> Vec<u8> {
        let overall = 1 + 9 + payload.len();
        let total = 36 + overall;
        let mut out = vec![0u8; base + total];
        out[..6].copy_from_slice(b"AC1032");
        out[0x0D..0x11].copy_from_slice(&(base as i32).to_le_bytes());
        out[base..base + 16].copy_from_slice(&PREVIEW_SENTINEL);
        out[base + 16..base + 20].copy_from_slice(&(overall as u32).to_le_bytes());
        out[base + 20] = 1;
        out[base + 21] = code;
        let img_start = base + 21 + 9;
        out[base + 22..base + 26].copy_from_slice(&(img_start as u32).to_le_bytes());
        out[base + 26..base + 30].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        out[img_start..img_start + payload.len()].copy_from_slice(payload);
        out
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut cur = std::io::Cursor::new(Vec::new());
        img.write_to(&mut cur, ImageFormat::Png).unwrap();
        cur.into_inner()
    }

    #[test]
    fn read_from_seeks_directly_without_reading_full_stream() {
        // Virtual DWG whose preview sits after a 20 MiB zero gap; the mock reader
        // refuses sequential reads across the middle and counts bytes read.
        let payload = png_bytes(32, 16);
        let container_base = 20_000_000usize;
        let container = make_synthetic_dwg(128, CODE_PNG, &payload);
        let mut head = vec![0u8; 32];
        head[..6].copy_from_slice(b"AC1032");
        head[0x0D..0x11].copy_from_slice(&(container_base as i32).to_le_bytes());
        let raw_container = &container[128..];
        let virtual_len = (container_base + raw_container.len()) as u64;

        // Relocate the single entry's absolute start offset to match `container_base`.
        let mut relocated = raw_container.to_vec();
        let entry_abs = (container_base + 21 + 9) as u32;
        relocated[22..26].copy_from_slice(&entry_abs.to_le_bytes());

        struct SparseReader<'a> {
            pos: u64,
            total_len: u64,
            head: &'a [u8],
            base: u64,
            tail: &'a [u8],
            bytes_read: usize,
        }
        impl Read for SparseReader<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let p = self.pos as usize;
                let src = if p < self.head.len() {
                    &self.head[p..]
                } else if self.pos >= self.base {
                    let off = (self.pos - self.base) as usize;
                    self.tail.get(off..).unwrap_or(&[])
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "attempted to read skipped gap",
                    ));
                };
                let n = buf.len().min(src.len());
                buf[..n].copy_from_slice(&src[..n]);
                self.pos += n as u64;
                self.bytes_read += n;
                Ok(n)
            }
        }
        impl Seek for SparseReader<'_> {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                let next = match pos {
                    SeekFrom::Start(n) => n as i64,
                    SeekFrom::Current(d) => self.pos as i64 + d,
                    SeekFrom::End(d) => self.total_len as i64 + d,
                };
                if next < 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "neg"));
                }
                self.pos = next as u64;
                Ok(self.pos)
            }
        }

        let mut reader = SparseReader {
            pos: 0,
            total_len: virtual_len,
            head: &head,
            base: container_base as u64,
            tail: &relocated,
            bytes_read: 0,
        };
        let preview = PreviewData::read_from(&mut reader).expect("preview extracted");
        assert!(
            reader.bytes_read <= relocated.len() + 64,
            "read {} bytes",
            reader.bytes_read
        );
        let scaled = preview.decode_scaled(16).expect("decoded");
        assert_eq!((scaled.width(), scaled.height()), (16, 8));
    }

    #[test]
    fn malformed_headers_oversized_containers_and_short_dibs_are_rejected() {
        assert!(extract_bytes(b"NOT_A_DWG", 256).is_none());
        assert!(dib_to_bmp(&[0u8; 20]).is_none());

        let mut bad_bi = vec![0u8; 40];
        bad_bi[0..4].copy_from_slice(&12u32.to_le_bytes()); // bi_size < 40
        assert!(dib_to_bmp(&bad_bi).is_none());

        let mut huge_overall = make_synthetic_dwg(64, CODE_PNG, &[1, 2, 3]);
        huge_overall[64 + 16..64 + 20]
            .copy_from_slice(&((MAX_PREVIEW_CONTAINER_BYTES as u32) + 1).to_le_bytes());
        assert!(extract_bytes(&huge_overall, 256).is_none());
    }

    #[test]
    fn downscale_and_badge_clamp_dimensions_and_nan_clear_factor() {
        let img = RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
        let clamped_zero = downscale(img.clone(), 0);
        assert_eq!((clamped_zero.width(), clamped_zero.height()), (1, 1));
        let clamped_huge = downscale(img.clone(), u32::MAX);
        assert_eq!((clamped_huge.width(), clamped_huge.height()), (40, 20));

        let mut badged = img;
        badge_dwg_clear_of(&mut badged, f32::NAN);
        assert_eq!(badged.width(), 40);
        assert!(badged.height() >= 38);
    }

    #[test]
    fn c_abi_handles_nulls_and_valid_round_trip() {
        let mut out_ptr: *mut u8 = std::ptr::dangling_mut();
        let mut out_len: usize = 999;
        // SAFETY: testing null path rejection with writable stack out pointers.
        let ok = unsafe { dwg_thumbnail_png(std::ptr::null(), 256, &mut out_ptr, &mut out_len) };
        assert!(!ok);
        assert!(out_ptr.is_null());
        assert_eq!(out_len, 0);

        let payload = png_bytes(24, 24);
        let dwg = make_synthetic_dwg(64, CODE_PNG, &payload);
        let tmp = std::env::temp_dir().join(format!("ocs-thumb-test-{}.dwg", std::process::id()));
        std::fs::write(&tmp, &dwg).unwrap();
        let cpath = std::ffi::CString::new(tmp.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: `cpath` is a valid NUL-terminated path and out pointers are writable.
        let ok = unsafe { dwg_thumbnail_png(cpath.as_ptr(), u32::MAX, &mut out_ptr, &mut out_len) };
        let _ = std::fs::remove_file(&tmp);
        assert!(ok);
        assert!(!out_ptr.is_null() && out_len > 8);
        // SAFETY: `out_ptr` holds `out_len` PNG bytes allocated by `dwg_thumbnail_png`.
        unsafe {
            let slice = std::slice::from_raw_parts(out_ptr, out_len);
            assert_eq!(&slice[..8], b"\x89PNG\r\n\x1a\n");
            dwg_thumbnail_free(out_ptr, out_len);
        }
    }
}
