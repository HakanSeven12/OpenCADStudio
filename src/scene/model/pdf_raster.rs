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
}
