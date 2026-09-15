//! Cross-platform PDF underlay rasterisation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};

/// One rasterised PDF page. `dpi` ties its pixel size to its physical size.
pub struct PdfPage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
}

const RASTER_DPI: f32 = 150.0;

type PageKey = (String, String);

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
        .retain(|(cached_path, _), _| cached_path != path);
}

/// Rasterise a 1-based PDF page, memoised by source path and page name.
pub fn rasterize_page(path: &str, page: &str) -> Option<Arc<PdfPage>> {
    let key = (path.to_string(), page.to_string());
    if let Some(hit) = page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }

    let built = rasterize_uncached(path, page);
    page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, built.clone());
    built
}

fn rasterize_uncached(path: &str, page: &str) -> Option<Arc<PdfPage>> {
    let bytes = source_bytes(path)?;
    let pdf = Pdf::new(bytes).ok()?;
    let page_no = page.trim().parse::<usize>().unwrap_or(1).max(1);
    let pixmap = raster_page(&pdf, page_no - 1, RASTER_DPI)?;
    let RasterizedPage {
        pixels,
        width,
        height,
    } = pixmap;
    Some(Arc::new(PdfPage {
        pixels,
        width,
        height,
        dpi: RASTER_DPI,
    }))
}

struct RasterizedPage {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

/// Rasterise one 0-based page index at `dpi` onto a white background.
fn raster_page(pdf: &Pdf, index: usize, dpi: f32) -> Option<RasterizedPage> {
    let page = pdf.pages().get(index)?;
    let scale = dpi / 72.0;
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        },
    );
    Some(RasterizedPage {
        pixels: Arc::new(pixmap.data_as_u8_slice().to_vec()),
        width: u32::from(pixmap.width()),
        height: u32::from(pixmap.height()),
    })
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
        .filter_map(|index| {
            let page = raster_page(&pdf, index, dpi)?;
            Some(Arc::new(PdfPage {
                pixels: page.pixels,
                width: page.width,
                height: page.height,
                dpi,
            }))
        })
        .collect()
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
            let page = raster_page(&pdf, index, page_edge_clamped_dpi(size, dpi))?;
            Some(Arc::new(PdfPage {
                pixels: page.pixels,
                width: page.width,
                height: page.height,
                dpi,
            }))
        })
        .collect();
    (pages, total)
}

fn source_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
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
        assert!(pages[0].width >= 4000 && pages[0].height >= 4000);
        let pixels = pages[0].width as f64 * pages[0].height as f64;
        assert!(pixels <= 4096.0 * 4096.0, "clamped, not {pixels} px");
    }
}
