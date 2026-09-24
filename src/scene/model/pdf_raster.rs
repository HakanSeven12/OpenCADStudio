//! Cross-platform PDF underlay rasterisation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::{RenderCache, RenderSettings};

/// One rasterised PDF page. `dpi` ties its pixel size to its physical size.
pub struct PdfPage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
}

const RASTER_DPI: f32 = 150.0;

/// Source path, page, DPI bits and whether the page background is transparent.
type PageKey = (String, String, u32, bool);

fn page_cache() -> &'static Mutex<HashMap<PageKey, Option<Arc<PdfPage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PageKey, Option<Arc<PdfPage>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn source_cache() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Keep bytes selected through a file dialog. Browsers expose no reusable path,
/// while native builds also benefit by avoiding a second disk read.
pub fn register_source(path: &str, bytes: Arc<Vec<u8>>) {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(path.to_string(), bytes);
    page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|(cached_path, _, _, _), _| cached_path != path);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|key, _| key.0 != path);
}

/// Rasterise a 1-based PDF page, memoised by source path and page name.
pub fn rasterize_page(path: &str, page: &str) -> Option<Arc<PdfPage>> {
    rasterize_page_at_dpi(path, page, RASTER_DPI)
}

/// Rasterise a 1-based PDF page at a caller-chosen DPI, memoised per source
/// path, page and DPI (print quality differs from the on-screen 150 DPI).
pub fn rasterize_page_at_dpi(path: &str, page: &str, dpi: f32) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, dpi, false)
}

/// Rasterise a page for display: no page background (transparent pixels
/// outside the drawn content) and straight, not premultiplied, alpha — the
/// page is drawn over the drawing background like its vector content.
pub fn rasterize_page_display(path: &str, page: &str) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, RASTER_DPI, true)
}

fn rasterize_cached(path: &str, page: &str, dpi: f32, transparent: bool) -> Option<Arc<PdfPage>> {
    let key = (path.to_string(), page.to_string(), dpi.to_bits(), transparent);
    if let Some(hit) = page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }

    let built = rasterize_uncached(path, page, dpi, transparent);
    page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, built.clone());
    built
}

/// Rasterise without touching the memo: for one-off consumers such as a
/// print job, whose 300 DPI page would otherwise stay cached for the whole
/// session (tens of megabytes per sheet).
pub fn rasterize_page_at_dpi_uncached(path: &str, page: &str, dpi: f32) -> Option<Arc<PdfPage>> {
    rasterize_uncached(path, page, dpi, false)
}

fn rasterize_uncached(path: &str, page: &str, dpi: f32, transparent: bool) -> Option<Arc<PdfPage>> {
    let bytes = source_bytes(path)?;
    let pdf = Pdf::new(bytes).ok()?;
    let page_no = page.trim().parse::<usize>().unwrap_or(1).max(1);
    let page = pdf.pages().get(page_no - 1)?;
    let scale = dpi / 72.0;
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: if transparent { TRANSPARENT } else { WHITE },
            ..Default::default()
        },
    );
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    let mut pixels = pixmap.data_as_u8_slice().to_vec();
    if transparent {
        unpremultiply(&mut pixels);
    }
    Some(Arc::new(PdfPage {
        pixels: Arc::new(pixels),
        width,
        height,
        dpi,
    }))
}

fn unpremultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// Number of pages in a PDF source.
pub fn page_count(path: &str) -> Option<usize> {
    let bytes = source_bytes(path)?;
    let pdf = Pdf::new(bytes).ok()?;
    Some(pdf.pages().len())
}

/// Size of a 1-based page in inches (its rendered width and height at 72
/// points per inch). An underlay is one drawing unit per page inch.
pub fn page_size_inches(path: &str, page: &str) -> Option<(f64, f64)> {
    let bytes = source_bytes(path)?;
    let pdf = Pdf::new(bytes).ok()?;
    let page_no = page.trim().parse::<usize>().ok()?;
    let page = pdf.pages().get(page_no.checked_sub(1)?)?;
    let (w, h) = page.render_dimensions();
    Some((w as f64 / 72.0, h as f64 / 72.0))
}

/// Display adjustments of an underlay page, applied to its display raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageAdjust {
    /// 0..=100; 100 keeps the colours, lower values pull them to mid grey.
    pub contrast: u8,
    pub monochrome: bool,
    /// Lighten dark, unsaturated content on a dark background (black lines
    /// and text become white), keeping coloured content.
    pub adjust_for_background: bool,
    pub dark_background: bool,
}

type AdjustKey = (String, String, PageAdjust);

fn adjusted_cache() -> &'static Mutex<HashMap<AdjustKey, Arc<Vec<u8>>>> {
    static CACHE: OnceLock<Mutex<HashMap<AdjustKey, Arc<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The display raster with the underlay's contrast / monochrome / background
/// adjustments applied, memoised per source, page and adjustment.
pub fn adjusted_pixels(path: &str, page: &str, raster: &PdfPage, adjust: PageAdjust) -> Arc<Vec<u8>> {
    let identity = adjust.contrast >= 100 && !adjust.monochrome && !adjust.adjust_for_background;
    if identity {
        return raster.pixels.clone();
    }
    let key = (path.to_string(), page.to_string(), adjust);
    if let Some(hit) = adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }
    let mut pixels = raster.pixels.as_ref().clone();
    for px in pixels.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let mut rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        if adjust.monochrome {
            let l = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
            rgb = [l; 3];
        }
        if adjust.adjust_for_background {
            let max = rgb[0].max(rgb[1]).max(rgb[2]);
            let min = rgb[0].min(rgb[1]).min(rgb[2]);
            let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
            let l = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
            // ponytail: greyscale inversion of low-saturation content only;
            // hue-preserving remap if coloured content must adapt too.
            let clash = if adjust.dark_background { l < 0.5 } else { l > 0.5 };
            if saturation < 0.25 && clash {
                rgb = rgb.map(|c| 1.0 - c);
            }
        }
        if adjust.contrast < 100 {
            let k = adjust.contrast as f32 / 100.0;
            rgb = rgb.map(|c| 0.5 + (c - 0.5) * k);
        }
        for (dst, c) in px[..3].iter_mut().zip(rgb) {
            *dst = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    let pixels = Arc::new(pixels);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, pixels.clone());
    pixels
}

pub(crate) fn source_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    if let Some(bytes) = source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        .cloned()
    {
        return Some(bytes);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(path).ok().map(Arc::new)
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use printpdf::{Mm, PdfDocument, PdfPage as OutputPage, PdfSaveOptions};

    use super::*;

    #[test]
    fn registered_pdf_bytes_render_without_a_filesystem_path() {
        let mut document = PdfDocument::new("PDF underlay test");
        document
            .pages
            .push(OutputPage::new(Mm(25.4), Mm(25.4), Vec::new()));
        let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let path = "memory://pdf-underlay-test.pdf";

        register_source(path, Arc::new(bytes));
        let page = rasterize_page(path, "1").expect("registered PDF should render");

        assert_eq!((page.width, page.height), (150, 150));
        assert_eq!(page.pixels.len(), 150 * 150 * 4);
    }
}
