//! The bytes of the PDF files a drawing references, shared by every reader,
//! and the rendered pages kept within a memory budget.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use lru::LruCache;

/// One rasterised PDF page. `dpi` ties its pixel size to its physical size.
pub struct PdfPage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
}

/// Source path, page, DPI bits and whether the page background is transparent.
pub(crate) type PageKey = (String, String, u32, bool);

/// LRU memo with a byte budget, counted from each entry's reported cost.
pub(crate) struct BudgetedRasterCache<K: std::hash::Hash + Eq, V> {
    pub(crate) entries: LruCache<K, (V, usize)>,
    pub(crate) bytes: usize,
}

pub(crate) fn page_cache() -> &'static Mutex<BudgetedRasterCache<PageKey, Option<Arc<PdfPage>>>> {
    static CACHE: OnceLock<Mutex<BudgetedRasterCache<PageKey, Option<Arc<PdfPage>>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BudgetedRasterCache::new()))
}

pub(crate) fn source_cache() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
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
        .retain(|(cached_path, _, _, _)| cached_path != path);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|key| key.0 != path);
}

/// Size of a 1-based page in inches (its rendered width and height at 72
/// points per inch). An underlay is one drawing unit per page inch.
pub fn page_size_inches(path: &str, page: &str) -> Option<(f64, f64)> {
    let bytes = source_bytes(path)?;
    let pdf = crate::media::pdf_password::open(path, bytes).ok()?;
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

pub(crate) type AdjustKey = (String, String, PageAdjust);

pub(crate) fn adjusted_cache() -> &'static Mutex<BudgetedRasterCache<AdjustKey, Arc<Vec<u8>>>> {
    static CACHE: OnceLock<Mutex<BudgetedRasterCache<AdjustKey, Arc<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BudgetedRasterCache::new()))
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
