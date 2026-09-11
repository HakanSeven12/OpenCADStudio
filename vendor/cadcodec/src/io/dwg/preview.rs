//! DWG preview / thumbnail image container codec (shared by reader + writer).
//!
//! Across every DWG version the preview lives at a raw file offset recorded in
//! the file header's "preview seeker" (byte `0x0D`). The image data itself is
//! stored **uncompressed**, wrapped in a sentinel-bracketed container:
//!
//! ```text
//! [start sentinel : 16 bytes]
//! [overall size   : RL]            // bytes from here (count) to end of image data
//! [images present : RC]            // number of image descriptors that follow
//! images present × {
//!     [code  : RC]                 // 1 = header, 2 = BMP(DIB), 3 = WMF, 6 = PNG
//!     [start : RL]                 // ABSOLUTE file offset of this image's data
//!     [size  : RL]
//! }
//! [image data ...]                 // descriptors' data, contiguous
//! [end sentinel   : 16 bytes]
//! ```
//!
//! `start` is an absolute file offset, so the writer needs to know where the
//! container will be placed (`base`) before it can emit the descriptors.
//! Verified against a real AC1024 file (preview at 0x1C0, header+BMP images).

use crate::document::{Preview, PreviewFormat};
use crate::io::dwg::file_headers::section_definition::{end_sentinels, start_sentinels};

/// Image descriptor codes.
const CODE_HEADER: u8 = 1;
const CODE_BMP: u8 = 2;
const CODE_WMF: u8 = 3;
const CODE_PNG: u8 = 6;

/// Fixed 80-byte reserved "header data" (code 1) AutoCAD writes ahead of a BMP
/// image. Observed to be all zeros in real files.
const HEADER_LEN: usize = 80;

/// Total byte length of a preview container given its `overall size` field.
///
/// `overall size` spans the count byte through the end of the image data, so
/// the whole container is `sentinel(16) + size(4) + overall + end sentinel(16)`.
pub fn container_len(overall_size: usize) -> usize {
    16 + 4 + overall_size + 16
}

/// Read the `overall size` field from the start of a container, if `bytes`
/// begins with the preview start sentinel. Lets the reader learn how many bytes
/// to read before parsing.
pub fn overall_size(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 20 || bytes[..16] != start_sentinels::PREVIEW {
        return None;
    }
    Some(u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize)
}

/// Parse a preview container beginning at file offset `base` (the preview
/// seeker value). `bytes` must start at `base` and cover the image data.
/// Returns the first BMP/WMF/PNG image found, or `None`.
pub fn parse_preview(bytes: &[u8], base: u64) -> Option<Preview> {
    if bytes.len() < 21 || bytes[..16] != start_sentinels::PREVIEW {
        return None;
    }
    let count = bytes[20] as usize;
    let mut off = 21usize;
    for _ in 0..count {
        if off + 9 > bytes.len() {
            break;
        }
        let code = bytes[off];
        let start = u32::from_le_bytes([
            bytes[off + 1],
            bytes[off + 2],
            bytes[off + 3],
            bytes[off + 4],
        ]) as u64;
        let size = u32::from_le_bytes([
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
            bytes[off + 8],
        ]) as usize;
        off += 9;
        let format = match code {
            CODE_BMP => PreviewFormat::Bmp,
            CODE_WMF => PreviewFormat::Wmf,
            CODE_PNG => PreviewFormat::Png,
            _ => continue, // header (1) or unknown — skip
        };
        // `start` is absolute; translate to a slice offset within `bytes`.
        let rel = start.checked_sub(base)? as usize;
        let end = rel.checked_add(size)?;
        if size == 0 || end > bytes.len() {
            continue;
        }
        return Some(Preview {
            format,
            data: bytes[rel..end].to_vec(),
        });
    }
    None
}

/// Build a preview container to be placed at file offset `base`.
///
/// With `None` (or empty data) returns the minimal empty preview — the
/// long-standing default that AutoCAD accepts as "no thumbnail". A BMP image is
/// preceded by the 80-byte reserved header (code 1) exactly as AutoCAD writes
/// it; PNG/WMF use a single image descriptor.
pub fn build_preview(preview: Option<&Preview>, base: u64) -> Vec<u8> {
    let p = match preview {
        Some(p) if !p.data.is_empty() => p,
        _ => return empty_preview(),
    };

    let (code, with_header) = match p.format {
        PreviewFormat::Bmp => (CODE_BMP, true),
        PreviewFormat::Wmf => (CODE_WMF, false),
        PreviewFormat::Png => (CODE_PNG, false),
    };
    let count = if with_header { 2usize } else { 1usize };
    let descriptors_len = 9 * count;
    // Container offset where image data begins (right after the descriptors).
    let data_rel = 16 + 4 + 1 + descriptors_len;
    let header_bytes = if with_header { HEADER_LEN } else { 0 };
    // overall size = count byte + descriptors + all image data.
    let overall = 1 + descriptors_len + header_bytes + p.data.len();

    let mut out = Vec::with_capacity(container_len(overall));
    out.extend_from_slice(&start_sentinels::PREVIEW);
    out.extend_from_slice(&(overall as u32).to_le_bytes());
    out.push(count as u8);

    if with_header {
        let hstart = base + data_rel as u64;
        push_descriptor(&mut out, CODE_HEADER, hstart, HEADER_LEN as u32);
        push_descriptor(
            &mut out,
            code,
            hstart + HEADER_LEN as u64,
            p.data.len() as u32,
        );
        out.extend_from_slice(&[0u8; HEADER_LEN]);
        out.extend_from_slice(&p.data);
    } else {
        push_descriptor(&mut out, code, base + data_rel as u64, p.data.len() as u32);
        out.extend_from_slice(&p.data);
    }

    out.extend_from_slice(&end_sentinels::PREVIEW);
    out
}

fn push_descriptor(out: &mut Vec<u8>, code: u8, start: u64, size: u32) {
    out.push(code);
    out.extend_from_slice(&(start as u32).to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
}

/// The minimal valid preview: sentinels wrapping `overall size = 1`, `count = 0`.
pub fn empty_preview() -> Vec<u8> {
    let mut out = Vec::with_capacity(37);
    out.extend_from_slice(&start_sentinels::PREVIEW);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&end_sentinels::PREVIEW);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_roundtrips_to_none() {
        let base = 0x1c0;
        let bytes = build_preview(None, base);
        assert_eq!(bytes, empty_preview());
        assert_eq!(parse_preview(&bytes, base), None);
    }

    #[test]
    fn png_roundtrips() {
        let base = 0x300;
        let img = Preview {
            format: PreviewFormat::Png,
            data: vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4, 5],
        };
        let bytes = build_preview(Some(&img), base);
        // one descriptor, no header
        assert_eq!(bytes[20], 1);
        assert_eq!(parse_preview(&bytes, base), Some(img));
    }

    #[test]
    fn bmp_roundtrips_with_header() {
        let base = 0x1c0;
        let dib = vec![0x28u8; 1200];
        let img = Preview {
            format: PreviewFormat::Bmp,
            data: dib,
        };
        let bytes = build_preview(Some(&img), base);
        // two descriptors: header + BMP
        assert_eq!(bytes[20], 2);
        // overall size == container minus the 36 wrapper bytes
        let overall = overall_size(&bytes).unwrap();
        assert_eq!(container_len(overall), bytes.len());
        assert_eq!(parse_preview(&bytes, base), Some(img));
    }

    #[test]
    fn absolute_start_offsets() {
        // Mirrors the real AC1024 file: header at data_rel, BMP after it.
        let base = 0x1c0;
        let img = Preview {
            format: PreviewFormat::Bmp,
            data: vec![7u8; 100],
        };
        let bytes = build_preview(Some(&img), base);
        // descriptor[1] (BMP) start field at container offset 21 + 9 + 1.
        let s = u32::from_le_bytes([bytes[31], bytes[32], bytes[33], bytes[34]]) as u64;
        // BMP data starts after: sentinel16 + size4 + count1 + 2*desc(18) + header80
        assert_eq!(s, base + 16 + 4 + 1 + 18 + 80);
    }
}
