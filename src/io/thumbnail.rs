//! DWG preview thumbnails.
//!
//! - [`from_screenshot`] crops the visible drawing area into a small
//!   [`acadrust::Preview`] embedded on save.
//! - [`read_handle`] / [`extract_to_png`] read a DWG's *embedded* preview back
//!   for the Start page and the OS file-manager thumbnailer. Extraction lives in
//!   the shared [`dwg_thumbnailer`] core crate (also used by the Windows/macOS
//!   thumbnail handlers).

use acadrust::{Preview, PreviewFormat};
use iced::Rectangle;
use image::{ImageFormat, RgbImage};
use std::io::Cursor;

/// Build a preview from the visible drawing area of an Iced window screenshot.
pub fn from_screenshot(
    screenshot: &iced::window::Screenshot,
    bounds: Rectangle,
    png: bool,
) -> Option<Preview> {
    let scale = screenshot.scale_factor;
    if !scale.is_finite() || scale <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return None;
    }

    let width = screenshot.size.width as f32;
    let height = screenshot.size.height as f32;
    let left = (bounds.x * scale).floor().clamp(0.0, width) as u32;
    let top = (bounds.y * scale).floor().clamp(0.0, height) as u32;
    let right = ((bounds.x + bounds.width) * scale)
        .ceil()
        .clamp(0.0, width) as u32;
    let bottom = ((bounds.y + bounds.height) * scale)
        .ceil()
        .clamp(0.0, height) as u32;
    if right <= left || bottom <= top {
        return None;
    }

    let cropped = screenshot
        .crop(Rectangle {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
        .ok()?;
    let rgba = image::RgbaImage::from_raw(
        cropped.size.width,
        cropped.size.height,
        cropped.rgba.to_vec(),
    )?;
    let (cw, ch) = canvas_dims(cropped.size.width as f64 / cropped.size.height as f64);
    let resized = image::imageops::resize(&rgba, cw, ch, image::imageops::FilterType::Triangle);
    encode(image::DynamicImage::ImageRgba8(resized).into_rgb8(), png)
}

/// Longest edge of the generated thumbnail, in pixels.
const MAX_DIM: u32 = 256;

/// Canvas dimensions for an aspect ratio, longest edge = [`MAX_DIM`].
fn canvas_dims(aspect: f64) -> (u32, u32) {
    if aspect >= 1.0 {
        (MAX_DIM, ((MAX_DIM as f64 / aspect).round() as u32).clamp(16, MAX_DIM))
    } else {
        (((MAX_DIM as f64 * aspect).round() as u32).clamp(16, MAX_DIM), MAX_DIM)
    }
}

/// Encode PNG for R2013+; older targets receive a BMP/DIB.
fn encode(img: RgbImage, png: bool) -> Option<Preview> {
    if png {
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png).ok()?;
        let data = buf.into_inner();
        return (!data.is_empty()).then_some(Preview { format: PreviewFormat::Png, data });
    }
    let data = rle8_dib(&img).or_else(|| bmp24_dib(&img))?;
    Some(Preview { format: PreviewFormat::Bmp, data })
}

/// Build an 8-bit palettised, `BI_RLE8`-compressed DIB. `None` when the image
/// holds more than 256 distinct colours (the caller then uses 24-bit).
fn rle8_dib(img: &RgbImage) -> Option<Vec<u8>> {
    let (w, h) = (img.width(), img.height());
    // Exact palette + per-pixel index (top-to-bottom, left-to-right).
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut lut: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    let mut idx = Vec::with_capacity((w * h) as usize);
    for px in img.pixels() {
        let c = [px.0[0], px.0[1], px.0[2]];
        let i = if let Some(&i) = lut.get(&c) {
            i
        } else {
            if palette.len() >= 256 {
                return None;
            }
            let i = palette.len() as u8;
            palette.push(c);
            lut.insert(c, i);
            i
        };
        idx.push(i);
    }

    // RLE8 body, rows bottom-up (BMP stores the last image row first). Encoded
    // runs only: 2 bytes per single-colour run — ideal for flat-fill previews.
    let mut rle = Vec::new();
    for (n, row) in (0..h).rev().enumerate() {
        let line = &idx[(row * w) as usize..(row * w + w) as usize];
        let mut x = 0usize;
        while x < line.len() {
            let v = line[x];
            let mut run = 1usize;
            while x + run < line.len() && line[x + run] == v && run < 255 {
                run += 1;
            }
            rle.push(run as u8);
            rle.push(v);
            x += run;
        }
        if n + 1 < h as usize {
            rle.extend_from_slice(&[0, 0]); // end of line
        }
    }
    rle.extend_from_slice(&[0, 1]); // end of bitmap

    // BITMAPINFOHEADER (40) + full 256-entry palette (BGRA) + RLE body. The
    // 256-entry palette is required: the reader derives the pixel-data offset as
    // `(1 << bitCount) * 4`, so a short palette would misplace it.
    let mut dib = Vec::with_capacity(40 + 1024 + rle.len());
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(w as i32).to_le_bytes());
    dib.extend_from_slice(&(h as i32).to_le_bytes()); // + = bottom-up
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&8u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&1u32.to_le_bytes()); // biCompression = BI_RLE8
    dib.extend_from_slice(&(rle.len() as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&256u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    for i in 0..256 {
        let c = palette.get(i).copied().unwrap_or([0, 0, 0]);
        dib.extend_from_slice(&[c[2], c[1], c[0], 0]); // BGRA
    }
    dib.extend_from_slice(&rle);
    Some(dib)
}

/// 24-bit uncompressed DIB (fallback): the `image` BMP minus its file header.
fn bmp24_dib(img: &RgbImage) -> Option<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Bmp).ok()?;
    let bmp = buf.into_inner();
    (bmp.len() > 14).then(|| bmp[14..].to_vec())
}

/// Read a DWG's embedded preview and write it as a PNG at `output`, scaled so
/// its longest edge is at most `size`. Returns `false` on any failure (no
/// preview, undecodable, write error) so the OS thumbnailer falls back to a
/// generic icon. Backs the hidden `--dwg-thumbnail` mode the installed
/// freedesktop `.thumbnailer` invokes. Extraction lives in the shared
/// [`dwg_thumbnailer`] core (also used by the Windows/macOS handlers).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn extract_to_png(input: &std::path::Path, output: &std::path::Path, size: u32) -> bool {
    match dwg_thumbnailer::extract(input, size) {
        Some(mut img) => {
            // Bottom-left "DWG" ribbon so the format reads at a glance in the
            // file manager (the Start-page `read_handle` stays unbadged).
            dwg_thumbnailer::badge_dwg(&mut img);
            img.save_with_format(output, ImageFormat::Png).is_ok()
        }
        None => false,
    }
}

/// Read a DWG's embedded preview and decode it to an iced image handle for the
/// Start page's recent-file thumbnails. `None` for DXF/other files, a missing
/// preview, or an undecodable format (WMF).
#[cfg(not(target_arch = "wasm32"))]
pub fn read_handle(path: &std::path::Path) -> Option<iced::widget::image::Handle> {
    let img = dwg_thumbnailer::extract(path, MAX_DIM)?;
    let (w, h) = (img.width(), img.height());
    Some(iced::widget::image::Handle::from_rgba(w, h, img.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prepend a `BITMAPFILEHEADER` so `image` can decode the DIB. Mirrors the
    /// palette-aware offset the shared `dwg_thumbnailer::dib_to_bmp` computes.
    fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
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

    #[test]
    fn canvas_keeps_aspect_with_max_dim_edge() {
        assert_eq!(canvas_dims(1.0), (MAX_DIM, MAX_DIM));
        assert_eq!(canvas_dims(2.0), (MAX_DIM, MAX_DIM / 2)); // wide
        assert_eq!(canvas_dims(0.5), (MAX_DIM / 2, MAX_DIM)); // tall
    }

    #[test]
    fn rle8_bmp_is_8bit_compressed_and_round_trips() {
        let mut image = RgbImage::from_pixel(MAX_DIM, MAX_DIM, image::Rgb([255, 255, 255]));
        for i in 10..90 {
            image.put_pixel(i, i, image::Rgb([0, 0, 0]));
        }
        let bmp = encode(image, false).unwrap();
        assert_eq!(bmp.format, PreviewFormat::Bmp);
        // 8-bit, BI_RLE8.
        assert_eq!(u16::from_le_bytes([bmp.data[14], bmp.data[15]]), 8, "bitcount");
        assert_eq!(
            u32::from_le_bytes([bmp.data[16], bmp.data[17], bmp.data[18], bmp.data[19]]),
            1,
            "compression = BI_RLE8"
        );
        // Far under a 24-bit DIB of the same canvas (256·256·3 = 196 608).
        assert!(bmp.data.len() < 196_608 / 10, "rle8 {} not << 24-bit", bmp.data.len());
        // Decodes through the exact path the reader uses, line preserved.
        let img = image::load_from_memory(&dib_to_bmp(&bmp.data)).expect("rle8 decodes").to_rgb8();
        assert_eq!((img.width(), img.height()), (MAX_DIM, MAX_DIM));
        assert!(img.pixels().any(|px| px.0 == [0, 0, 0]), "black line missing");
    }

    #[test]
    fn png_preview_decodes() {
        let image = RgbImage::from_pixel(MAX_DIM, MAX_DIM, image::Rgb([255, 255, 255]));
        let p = encode(image, true).unwrap();
        assert_eq!(p.format, PreviewFormat::Png);
        let img = image::load_from_memory_with_format(&p.data, ImageFormat::Png).expect("png decodes");
        assert_eq!((img.width(), img.height()), (MAX_DIM, MAX_DIM));
    }
}
