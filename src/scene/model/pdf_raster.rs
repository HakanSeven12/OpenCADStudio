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

/// Ceiling on the print raster's pixel count (RGBA ⇒ ×4 bytes): an ANSI E
/// sheet at 300 DPI is ~135 M px and stays under it, while a crafted
/// MediaBox cannot ask hayro for a multi-gigabyte pixmap — its `u16`
/// dimensions saturate at 65 535, i.e. a 17 GB allocation (abort or a
/// multi-minute freeze).
const MAX_PRINT_RASTER_PIXELS: f64 = 268_435_456.0;

/// Source path, page, DPI bits and whether the page background is transparent.
type PageKey = (String, String, u32, bool, bool);

fn page_cache() -> &'static Mutex<HashMap<PageKey, Option<Arc<PdfPage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PageKey, Option<Arc<PdfPage>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn source_cache() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Bytes registered for exactly this PDF reference (the attach flow keys by
/// the same path the underlay definition stores). The web ARCHIVE/ETRANSMIT
/// packages the underlay into the transmittal; `None` means no PDF was
/// picked for this reference this session.
pub fn registered_source_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        .cloned()
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
        .retain(|(cached_path, _, _, _, _), _| cached_path != path);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|key, _| key.0 != path);
}

/// Register bytes under a derived source name (a layer override of a
/// file), leaving the file's own caches alone.
pub(crate) fn register_derived_source(key: &str, bytes: Arc<Vec<u8>>) {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key.to_string(), bytes);
}

/// A display-only copy of a vector PDF with its stroked paths set to PDF
/// hairlines (zero-width strokes, rendered at device-pixel width). The source
/// bytes are never modified. If the parser cannot preserve a document, the
/// caller keeps its existing rasterisation path.
fn hairline_pdf_source(bytes: &[u8]) -> Option<Arc<Vec<u8>>> {
    let mut warnings = Vec::new();
    let mut doc = printpdf::PdfDocument::parse(
        bytes,
        &printpdf::PdfParseOptions::default(),
        &mut warnings,
    ).ok()?;
    for page in &mut doc.pages {
        // PDF's initial linewidth is 1 user-space unit. Override it even
        // when a page does not contain any explicit "w" operators.
        page.ops.insert(0, printpdf::Op::SetOutlineThickness { pt: printpdf::Pt(0.0) });
        for op in &mut page.ops {
            if let printpdf::Op::SetOutlineThickness { pt } = op {
                *pt = printpdf::Pt(0.0);
            }
        }
    }
    let mut warnings = Vec::new();
    Some(Arc::new(doc.save(&printpdf::PdfSaveOptions::default(), &mut warnings)))
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
    rasterize_cached(path, page, RASTER_DPI, true, false)
}

/// The display raster at a lower DPI for a page shown small on screen
/// (never above the on-screen 150 DPI).
pub fn rasterize_page_display_at(path: &str, page: &str, dpi: f32) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, dpi.clamp(1.0, RASTER_DPI), true, false)
}

/// Render the PDF as device hairlines for display only. Export and printing
/// continue through the normal rendering path.
pub fn rasterize_page_display_mode(path: &str, page: &str, dpi: f32, thin: bool) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, dpi.clamp(1.0, RASTER_DPI), true, thin)
}

/// The DPI of the full-size display raster.
pub const DISPLAY_DPI: f32 = RASTER_DPI;

fn rasterize_cached(path: &str, page: &str, dpi: f32, transparent: bool, thin: bool) -> Option<Arc<PdfPage>> {
    let key = (
        path.to_string(),
        page.to_string(),
        dpi.to_bits(),
        transparent,
        thin,
    );
    if let Some(hit) = page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }

    let built = rasterize_uncached(path, page, dpi, transparent, thin);
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
    rasterize_uncached(path, page, dpi, false, false)
}

fn rasterize_uncached(path: &str, page: &str, dpi: f32, transparent: bool, thin: bool) -> Option<Arc<PdfPage>> {
    let bytes = source_bytes(path)?;
    let render_bytes = if thin { hairline_pdf_source(&bytes).unwrap_or_else(|| bytes.clone()) } else { bytes };
    let pdf = Pdf::new(render_bytes).ok()?;
    let page_no = page.trim().parse::<usize>().unwrap_or(1).max(1);
    raster_pdf_page(&pdf, page_no - 1, dpi, transparent)
}

/// Rasterise one 0-based page index at `dpi`. Screen DPIs clamp the longest
/// side; higher (print) DPIs refuse an oversized page instead of rescaling.
fn raster_pdf_page(pdf: &Pdf, index: usize, dpi: f32, transparent: bool) -> Option<Arc<PdfPage>> {
    let page = pdf.pages().get(index)?;
    let mut scale = dpi / 72.0;
    let mut effective_dpi = dpi;
    // Page size in points, straight from the file's MediaBox/CropBox.
    let (page_w, page_h) = page.render_dimensions();
    let longest = (page_w as f64).max(page_h as f64);
    if longest.is_finite() && longest > 0.0 {
        if dpi <= DISPLAY_DPI {
            // Screen rasters get the same longest-side clamp the DWF/DGN
            // underlays receive (`underlay_vector::RASTER_SIDE`); the page
            // maps onto the same world rect regardless of pixel count.
            if longest * f64::from(dpi) / 72.0 > super::underlay_vector::RASTER_SIDE {
                let clamped = super::underlay_vector::RASTER_SIDE / longest;
                scale = clamped as f32;
                effective_dpi = (clamped * 72.0) as f32;
            }
        } else {
            // Print keeps the exact DPI — the sheet size in millimetres is
            // derived from it (`page.width / dpi`) — so an oversized page
            // is refused instead of rescaled (a rescale would print the
            // wrong physical size).
            let pw = page_w as f64 * f64::from(dpi) / 72.0;
            let ph = page_h as f64 * f64::from(dpi) / 72.0;
            if !(pw.is_finite() && ph.is_finite()) || pw * ph > MAX_PRINT_RASTER_PIXELS {
                return None;
            }
        }
    }
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
        dpi: effective_dpi,
    }))
}

/// One-shot rasterisation of freshly generated PDF bytes (plot preview).
/// Uncached: the bytes differ on every plot, so a cache would only grow.
/// Returns at most `max_pages` pages; larger plots preview their first pages.
pub fn rasterize_bytes_pages(bytes: Arc<Vec<u8>>, dpi: f32, max_pages: usize) -> Vec<Arc<PdfPage>> {
    let Ok(pdf) = Pdf::new(bytes) else {
        return Vec::new();
    };
    let count = pdf.pages().len().min(max_pages);
    (0..count)
        .filter_map(|index| raster_pdf_page(&pdf, index, dpi, false))
        .collect()
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

fn rgb_to_hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= f32::EPSILON {
        return [l; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
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
    /// 0..=100; 100 keeps the colours, lower values pull their lightness
    /// towards `contrast_pivot` (hue and saturation kept).
    pub contrast: u8,
    /// The lightness contrast 0 leaves every colour at, in thousandths:
    /// measured 500 for PDF and 243 for DWF; a DGN model's follows its
    /// colours.
    pub contrast_pivot: u16,
    pub monochrome: bool,
    /// Lighten dark, unsaturated content on a dark background (black lines
    /// and text become white), keeping coloured content.
    pub adjust_for_background: bool,
    pub dark_background: bool,
    /// A fade blended into the colours before the contrast (a DGN
    /// model's): the background and the share, 0..=100, of the way each
    /// colour is taken to it.
    pub fade_before_contrast: Option<([u8; 3], u8)>,
}

type AdjustKey = (String, String, PageAdjust);

fn adjusted_cache() -> &'static Mutex<HashMap<AdjustKey, Arc<Vec<u8>>>> {
    static CACHE: OnceLock<Mutex<HashMap<AdjustKey, Arc<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One colour (0..=1 channels) with the underlay's contrast / monochrome /
/// background adjustments applied.
pub fn adjust_rgb(rgb: [f32; 3], adjust: &PageAdjust) -> [f32; 3] {
    let mut rgb = rgb;
    if adjust.monochrome {
        let l = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
        rgb = [l; 3];
    }
    if adjust.adjust_for_background && adjust.dark_background {
        // On a dark background every colour's lightness is turned over
        // (plus 20/255, capped at full), hue and saturation kept: black
        // becomes white, dark red light pink. On a light background the
        // colours stay as drawn.
        let (h, s, l) = rgb_to_hsl(rgb);
        rgb = hsl_to_rgb(h, s, (1.0 - l + 20.0 / 255.0).min(1.0));
    }
    if let Some((bg, fade)) = adjust.fade_before_contrast {
        let f = fade.min(100) as f32 / 100.0;
        for (c, b) in rgb.iter_mut().zip(bg) {
            *c += (b as f32 / 255.0 - *c) * f;
        }
    }
    if adjust.contrast < 100 {
        // Lightness is pulled towards the pivot, hue and saturation kept:
        // contrast 0 leaves every colour at the pivot's lightness.
        let (h, s, l) = rgb_to_hsl(rgb);
        let k = adjust.contrast as f32 / 100.0;
        let pivot = adjust.contrast_pivot as f32 / 1000.0;
        rgb = hsl_to_rgb(h, s, pivot + (l - pivot) * k);
    }
    rgb
}

/// The display raster with the underlay's contrast / monochrome / background
/// adjustments applied, memoised per source, page and adjustment.
pub fn adjusted_pixels(
    path: &str,
    page: &str,
    raster: &PdfPage,
    adjust: PageAdjust,
) -> Arc<Vec<u8>> {
    let identity = adjust.contrast >= 100
        && !adjust.monochrome
        && !adjust.adjust_for_background
        && adjust.fade_before_contrast.is_none();
    if identity {
        return raster.pixels.clone();
    }
    // One entry per raster size: a page is rasterised again for the zoom.
    let key = (
        path.to_string(),
        format!("{page}@{}x{}", raster.width, raster.height),
        adjust,
    );
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
        let rgb = adjust_rgb(
            [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
            &adjust,
        );
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

/// Upper bound on one previewed page's pixel edge. Rasteriser dimensions
/// saturate far above this, so the clamp must happen here: a page sized by a
/// hostile (or merely mistyped) paper width would otherwise allocate until
/// the tab aborts.
pub const PREVIEW_MAX_PAGE_EDGE_PX: f64 = 4096.0;

/// Total pixel budget for one preview. Bounds both the wasm-heap RGBA pages
/// and the browser's decoded-image memory behind the overlay. Memory scales
/// with DPI², so a multipage preview lowers its DPI instead of its content.
pub const PREVIEW_PIXEL_BUDGET: f64 = 64.0 * 1024.0 * 1024.0;

/// DPI that keeps every page edge within [`PREVIEW_MAX_PAGE_EDGE_PX`].
fn page_edge_clamped_dpi(page_pt: (f32, f32), dpi: f32) -> f32 {
    let (w_pt, h_pt) = (page_pt.0 as f64, page_pt.1 as f64);
    let clamp = |edge_pt: f64| -> f64 {
        if edge_pt <= 0.0 {
            dpi as f64
        } else {
            (PREVIEW_MAX_PAGE_EDGE_PX * 72.0 / edge_pt).min(dpi as f64)
        }
    };
    clamp(w_pt).min(clamp(h_pt)).max(1.0) as f32
}

/// Rasterise the first `max_pages` pages of freshly generated PDF bytes for
/// preview under a total pixel budget: the DPI starts at `max_dpi` and drops
/// uniformly (never below a legibility floor of 24) until the whole set fits,
/// with a per-page edge clamp for absurd sheet sizes. Returns the pages and
/// the PDF's true page count so the caller can say "first N of M".
pub fn rasterize_preview_pages(
    bytes: Arc<Vec<u8>>,
    max_dpi: f32,
    max_pages: usize,
) -> (Vec<Arc<PdfPage>>, usize) {
    let Ok(pdf) = Pdf::new(bytes) else {
        return (Vec::new(), 0);
    };
    let total = pdf.pages().len();
    let count = total.min(max_pages);
    // Point sizes come from the page tree — no allocation before the clamp.
    let sizes: Vec<(f32, f32)> = (0..count)
        .map(|index| {
            pdf.pages()
                .get(index)
                .map(|page| page.base_dimensions())
                .unwrap_or((595.0, 842.0))
        })
        .collect();
    let pixels_at_max: f64 = sizes
        .iter()
        .map(|&(w_pt, h_pt)| {
            let dpi = page_edge_clamped_dpi((w_pt, h_pt), max_dpi) as f64;
            let w = (w_pt as f64 / 72.0 * dpi).max(1.0);
            let h = (h_pt as f64 / 72.0 * dpi).max(1.0);
            w * h
        })
        .sum();
    let dpi = if pixels_at_max <= PREVIEW_PIXEL_BUDGET {
        max_dpi
    } else {
        ((max_dpi as f64) * (PREVIEW_PIXEL_BUDGET / pixels_at_max).sqrt())
            .clamp(24.0, max_dpi as f64) as f32
    };
    let pages = sizes
        .iter()
        .enumerate()
        .filter_map(|(index, &size)| {
            raster_pdf_page(&pdf, index, page_edge_clamped_dpi(size, dpi), false)
        })
        .collect();
    (pages, total)
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
    fn thin_mode_overrides_explicit_pdf_stroke_weights_without_changing_source() {
        use printpdf::{Op, Pt, PdfParseOptions};

        let mut document = PdfDocument::new("hairline");
        document.pages.push(OutputPage::new(Mm(25.4), Mm(25.4), vec![
            Op::SetOutlineThickness { pt: Pt(8.0) },
        ]));
        let original = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let modified = hairline_pdf_source(&original).expect("parseable PDF");
        assert_ne!(modified.as_ref(), &original);
        let mut warnings = Vec::new();
        let parsed = PdfDocument::parse(&modified, &PdfParseOptions::default(), &mut warnings)
            .expect("hairline copy should parse");
        assert_eq!(parsed.pages.len(), 1);
        assert!(parsed.pages[0].ops.iter().any(|op| matches!(
            op, Op::SetOutlineThickness { pt } if pt.0 == 0.0
        )));
        assert!(!parsed.pages[0].ops.iter().any(|op| matches!(
            op, Op::SetOutlineThickness { pt } if pt.0 > 0.0
        )));
        // Original PDF remains untouched, for printing and export.
        let mut warnings = Vec::new();
        let original_doc = PdfDocument::parse(&original, &PdfParseOptions::default(), &mut warnings)
            .expect("source should parse");
        assert!(original_doc.pages[0].ops.iter().any(|op| matches!(
            op, Op::SetOutlineThickness { pt } if pt.0 == 8.0
        )));
    }

    #[test]
    fn thin_and_normal_display_rasters_have_separate_cache_entries() {
        use printpdf::{Op, Pt};
        let mut document = PdfDocument::new("hairline cache");
        document.pages.push(OutputPage::new(Mm(25.4), Mm(25.4), vec![
            Op::SetOutlineThickness { pt: Pt(8.0) },
        ]));
        let path = "memory://thin-line-cache-test.pdf";
        register_source(path, Arc::new(document.save(&PdfSaveOptions::default(), &mut Vec::new())));
        let normal = rasterize_page_display_mode(path, "1", 75.0, false).unwrap();
        let thin = rasterize_page_display_mode(path, "1", 75.0, true).unwrap();
        assert!(!Arc::ptr_eq(&normal, &thin), "different modes must not share raster");
        let thin_again = rasterize_page_display_mode(path, "1", 75.0, true).unwrap();
        assert!(Arc::ptr_eq(&thin, &thin_again), "same mode should reuse cached raster");
    }

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

    /// A 15 m square MediaBox at 150 DPI would be 88 583 px a side: hayro
    /// saturates its `u16` dimensions to 65 535 and `Pixmap::new` asks for
    /// 65 535² × 4 = **17 GB** — the report's allocation-failure abort.
    /// The display raster must clamp to `RASTER_SIDE` instead, and the
    /// print raster (exact DPI, physical size derived from it) must be
    /// refused rather than rescaled.
    #[test]
    fn an_enormous_page_is_clamped_or_refused_never_a_17gb_pixmap() {
        let mut document = PdfDocument::new("PDF underlay huge-page test");
        document
            .pages
            .push(OutputPage::new(Mm(15_000.0), Mm(15_000.0), Vec::new()));
        let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let path = "memory://pdf-underlay-huge-page.pdf";

        register_source(path, Arc::new(bytes));
        let page = rasterize_page_display(path, "1").expect("display raster is clamped");
        assert_eq!((page.width, page.height), (3072, 3072));
        assert!(
            page.dpi < DISPLAY_DPI,
            "clamped raster reports its effective DPI"
        );

        assert!(
            rasterize_page_at_dpi(path, "1", 300.0).is_none(),
            "a print raster over the pixel budget is refused, not rescaled"
        );
    }

    #[test]
    fn bytes_pages_render_every_page_up_to_the_cap() {
        let mut document = PdfDocument::new("multi-page");
        for _ in 0..3 {
            document
                .pages
                .push(OutputPage::new(Mm(50.8), Mm(25.4), Vec::new()));
        }
        let bytes = Arc::new(document.save(&PdfSaveOptions::default(), &mut Vec::new()));

        let all = rasterize_bytes_pages(bytes.clone(), 72.0, 10);
        assert_eq!(all.len(), 3);
        assert_eq!((all[0].width, all[0].height), (144, 72));
        assert_eq!(all[0].dpi, 72.0);

        let capped = rasterize_bytes_pages(bytes, 72.0, 2);
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn preview_pages_keep_full_dpi_for_small_sets() {
        let mut document = PdfDocument::new("small set");
        for _ in 0..3 {
            document
                .pages
                .push(OutputPage::new(Mm(50.8), Mm(25.4), Vec::new()));
        }
        let bytes = Arc::new(document.save(&PdfSaveOptions::default(), &mut Vec::new()));

        let (pages, total) = rasterize_preview_pages(bytes, 72.0, 12);
        assert_eq!((total, pages.len()), (3, 3));
        assert_eq!((pages[0].width, pages[0].height), (144, 72));
        assert_eq!(pages[0].dpi, 72.0);
    }

    #[test]
    fn preview_pages_clamp_absurd_sheet_sizes_before_allocating() {
        // 2 m × 2 m sheet: naive 150 DPI would be ~139 Mpx for one page; the
        // edge clamp bounds each side instead of letting the rasteriser
        // allocate it (which saturates its u16 dims and aborts the tab).
        let mut document = PdfDocument::new("huge sheet");
        document
            .pages
            .push(OutputPage::new(Mm(2000.0), Mm(2000.0), Vec::new()));
        let bytes = Arc::new(document.save(&PdfSaveOptions::default(), &mut Vec::new()));

        let (pages, total) = rasterize_preview_pages(bytes, 150.0, 12);
        assert_eq!((total, pages.len()), (1, 1));
        assert!(pages[0].width <= 4096 && pages[0].height <= 4096);
        assert!(pages[0].width > 0 && pages[0].height > 0);
        let pixels = pages[0].width as f64 * pages[0].height as f64;
        assert!(pixels <= 4096.0 * 4096.0, "clamped, not {pixels} px");
    }
}
