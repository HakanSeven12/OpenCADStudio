//! Cross-platform PDF underlay rasterisation.

use std::collections::HashMap;
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::{RenderCache, RenderSettings};
use lru::LruCache;

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
type PageKey = (String, String, u32, bool);

/// Per-cache byte budget for the derived rasters (page and adjusted caches
/// each get one). Every zoom band a page is shown at adds a full-page RGBA
/// that would otherwise stay resident for the whole process lifetime; past
/// the budget the least recently used rasters are dropped and re-rasterised
/// on demand. Tunable with `OCS_PDF_RASTER_CACHE_MB`.
fn raster_cache_budget_bytes() -> usize {
    const DEFAULT_MIB: usize = 64;
    std::env::var("OCS_PDF_RASTER_CACHE_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MIB)
        .saturating_mul(1024 * 1024)
}

/// LRU memo with a byte budget, counted from each entry's reported cost.
pub(crate) struct BudgetedRasterCache<K: std::hash::Hash + Eq, V> {
    entries: LruCache<K, (V, usize)>,
    bytes: usize,
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> BudgetedRasterCache<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(4096).expect("nonzero cache capacity")),
            bytes: 0,
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        self.entries.get(key).map(|(value, _)| value.clone())
    }

    pub(crate) fn pop(&mut self, key: &K) -> Option<V> {
        let (value, old_bytes) = self.entries.pop(key)?;
        self.bytes -= old_bytes;
        Some(value)
    }

    fn insert(&mut self, key: K, value: V, bytes: usize) {
        self.insert_with_budget(key, value, bytes, raster_cache_budget_bytes());
    }

    pub(crate) fn insert_with_budget(&mut self, key: K, value: V, bytes: usize, budget: usize) {
        if bytes > budget {
            return;
        }
        if let Some((_, old_bytes)) = self.entries.pop(&key) {
            self.bytes -= old_bytes;
        }
        while self.bytes > budget - bytes {
            let Some((_, (_, old_bytes))) = self.entries.pop_lru() else {
                break;
            };
            self.bytes -= old_bytes;
        }
        if let Some((_, (_, old_bytes))) = self.entries.push(key, (value, bytes)) {
            self.bytes -= old_bytes;
        }
        self.bytes += bytes;
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        let stale: Vec<K> = self
            .entries
            .iter()
            .filter(|(key, _)| !keep(key))
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some((_, old_bytes)) = self.entries.pop(&key) {
                self.bytes -= old_bytes;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

fn page_cache() -> &'static Mutex<BudgetedRasterCache<PageKey, Option<Arc<PdfPage>>>> {
    static CACHE: OnceLock<Mutex<BudgetedRasterCache<PageKey, Option<Arc<PdfPage>>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BudgetedRasterCache::new()))
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
        .retain(|(cached_path, _, _, _)| cached_path != path);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|key| key.0 != path);
}

/// Register bytes under a derived source name (a layer override of a
/// file), leaving the file's own caches alone.
pub(crate) fn register_derived_source(key: &str, bytes: Arc<Vec<u8>>) {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key.to_string(), bytes);
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

/// The display raster at a lower DPI for a page shown small on screen
/// (never above the on-screen 150 DPI).
pub fn rasterize_page_display_at(path: &str, page: &str, dpi: f32) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, dpi.clamp(1.0, RASTER_DPI), true)
}

/// The DPI of the full-size display raster.
pub const DISPLAY_DPI: f32 = RASTER_DPI;

/// The DPI an underlay page is drawn at for a plot.
pub const PRINT_DPI: f32 = 300.0;

/// The underlay raster for a plot: the display raster's transparent page at
/// [`PRINT_DPI`]. `None` for a page too large to draw at that DPI.
pub fn rasterize_page_print(path: &str, page: &str) -> Option<Arc<PdfPage>> {
    rasterize_cached(path, page, PRINT_DPI, true)
}

/// Longest side of a zoomed-in region raster, pixels.
pub const REGION_SIDE: f64 = 4096.0;

/// Part of a page for display at `dpi`: `region` is min x, min y, max x,
/// max y in page inches from the lower-left corner of the shown page. The
/// last region is kept, so other entities' reseeds do not draw it again.
/// `None` for rotated or encrypted pages (they show the whole-page raster).
pub fn rasterize_page_region(
    path: &str,
    page: &str,
    region: [f64; 4],
    dpi: f32,
) -> Option<Arc<PdfPage>> {
    type RegionKey = (String, String, [u64; 4], u32);
    static LAST: OnceLock<Mutex<Option<(RegionKey, Option<Arc<PdfPage>>)>>> = OnceLock::new();
    let key = (
        path.to_string(),
        page.to_string(),
        region.map(f64::to_bits),
        dpi.to_bits(),
    );
    let last = LAST.get_or_init(|| Mutex::new(None));
    if let Some((k, hit)) = last.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key {
            return hit.clone();
        }
    }
    let built = region_uncached(path, page, region, dpi);
    *last.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, built.clone()));
    built
}

fn region_uncached(path: &str, page: &str, region: [f64; 4], dpi: f32) -> Option<Arc<PdfPage>> {
    use hayro::hayro_syntax::page::Rotation;
    let bytes = source_bytes(path)?;
    if super::pdf_password::is_encrypted(&bytes) {
        return None;
    }
    let index = page.trim().parse::<usize>().unwrap_or(1).max(1) - 1;
    let cropped = {
        let pdf = Pdf::new(bytes.clone()).ok()?;
        let page = pdf.pages().get(index)?;
        if !matches!(page.rotation(), Rotation::None) {
            return None;
        }
        let id = page.raw().obj_id()?;
        let dict = page.raw().data();
        let close = dict.iter().rposition(|&c| c == b'>')?;
        if close == 0 || dict[close - 1] != b'>' {
            return None;
        }
        let crop = page.intersected_crop_box();
        let r = [
            crop.x0 + region[0] * 72.0,
            crop.y0 + region[1] * 72.0,
            crop.x0 + region[2] * 72.0,
            crop.y0 + region[3] * 72.0,
        ];
        if !r.iter().all(|v| v.is_finite()) || r[2] <= r[0] || r[3] <= r[1] {
            return None;
        }
        let prev = last_startxref(&bytes)?;
        let root = pdf.xref().root_id();
        // An incremental update giving the page a box around the region; the
        // last definition of a dictionary key wins, so the old boxes give way.
        let mut out = bytes.as_ref().clone();
        out.push(b'\n');
        let offset = out.len();
        out.extend_from_slice(format!("{} {} obj\n", id.obj_number, id.gen_number).as_bytes());
        out.extend_from_slice(&dict[..close - 1]);
        let boxed = format!("[{} {} {} {}]", r[0], r[1], r[2], r[3]);
        out.extend_from_slice(format!(" /MediaBox {boxed} /CropBox {boxed} >>\nendobj\n").as_bytes());
        let xref = out.len();
        out.extend_from_slice(
            format!(
                "xref\n{} 1\n{offset:010} {:05} n\r\ntrailer\n<< /Size {} /Root {} {} R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n",
                id.obj_number,
                id.gen_number,
                (id.obj_number as usize + 1).max(pdf.len() + 1),
                root.obj_number,
                root.gen_number,
            )
            .as_bytes(),
        );
        out
    };
    let pdf = Pdf::new(Arc::new(cropped)).ok()?;
    raster_pdf_page(&pdf, index, dpi, true)
}

/// Offset after the file's last `startxref`.
fn last_startxref(bytes: &[u8]) -> Option<usize> {
    let tail = &bytes[bytes.len().saturating_sub(2048)..];
    let at = tail.windows(9).rposition(|w| w == b"startxref")?;
    let digits: String = tail[at + 9..]
        .iter()
        .skip_while(|c| c.is_ascii_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .map(|&c| c as char)
        .collect();
    digits.parse().ok()
}

fn rasterize_cached(path: &str, page: &str, dpi: f32, transparent: bool) -> Option<Arc<PdfPage>> {
    let key = (
        path.to_string(),
        page.to_string(),
        dpi.to_bits(),
        transparent,
    );
    if let Some(hit) = page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
    {
        return hit;
    }

    let built = rasterize_uncached(path, page, dpi, transparent);
    let bytes = size_of::<PageKey>()
        + built
            .as_ref()
            .map_or(0, |raster| size_of::<PdfPage>() + raster.pixels.capacity());
    page_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, built.clone(), bytes);
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
    let pdf = super::pdf_password::open(path, bytes).ok()?;
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
    let pdf = super::pdf_password::open(path, bytes).ok()?;
    Some(pdf.pages().len())
}

/// Size of a 1-based page in inches (its rendered width and height at 72
/// points per inch). An underlay is one drawing unit per page inch.
pub fn page_size_inches(path: &str, page: &str) -> Option<(f64, f64)> {
    let bytes = source_bytes(path)?;
    let pdf = super::pdf_password::open(path, bytes).ok()?;
    let page_no = page.trim().parse::<usize>().ok()?;
    let page = pdf.pages().get(page_no.checked_sub(1)?)?;
    let (w, h) = page.render_dimensions();
    Some((w as f64 / 72.0, h as f64 / 72.0))
}

/// Drawing units per page inch a page imports at: the X ratio of the
/// first viewport's measure dictionary (`/VP [<< /Measure << /X [<< /C
/// units-per-point >>] >> >>]`, written by plotters with the plot scale)
/// times 72, or one unit per inch without one; times the page's
/// `/UserUnit` (points per user-space unit) when it has one.
pub fn page_units_per_inch(path: &str, page: &str) -> Option<f64> {
    use hayro::hayro_syntax::object::{Array, Dict, Number};
    let bytes = source_bytes(path)?;
    let pdf = super::pdf_password::open(path, bytes).ok()?;
    let page_no = page.trim().parse::<usize>().ok()?;
    let page = pdf.pages().get(page_no.checked_sub(1)?)?;
    let positive = |v: f64| (v.is_finite() && v > 0.0).then_some(v);
    let measured = (|| {
        let viewport = page
            .raw()
            .get::<Array<'_>>(b"VP")?
            .iter::<Dict<'_>>()
            .next()?;
        let ratio = viewport
            .get::<Dict<'_>>(b"Measure")?
            .get::<Array<'_>>(b"X")?
            .iter::<Dict<'_>>()
            .next()?
            .get::<Number>(b"C")?
            .as_f64();
        positive(ratio).map(|r| r * 72.0)
    })();
    let user_unit = page
        .raw()
        .get::<Number>(b"UserUnit")
        .and_then(|n| positive(n.as_f64()))
        .unwrap_or(1.0);
    Some(measured.unwrap_or(1.0) * user_unit)
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

fn adjusted_cache() -> &'static Mutex<BudgetedRasterCache<AdjustKey, Arc<Vec<u8>>>> {
    static CACHE: OnceLock<Mutex<BudgetedRasterCache<AdjustKey, Arc<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BudgetedRasterCache::new()))
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
    {
        return hit;
    }
    let mut pixels = raster.pixels.as_ref().clone();
    adjust_in_place(&mut pixels, &adjust);
    let bytes = size_of::<AdjustKey>() + pixels.capacity();
    let pixels = Arc::new(pixels);
    adjusted_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, pixels.clone(), bytes);
    pixels
}

/// [`adjusted_pixels`] for a one-off raster (a zoomed-in region), not memoised.
pub fn adjusted_pixels_uncached(raster: &PdfPage, adjust: PageAdjust) -> Arc<Vec<u8>> {
    if adjust.contrast >= 100
        && !adjust.monochrome
        && !adjust.adjust_for_background
        && adjust.fade_before_contrast.is_none()
    {
        return raster.pixels.clone();
    }
    let mut pixels = raster.pixels.as_ref().clone();
    adjust_in_place(&mut pixels, &adjust);
    Arc::new(pixels)
}

fn adjust_in_place(pixels: &mut [u8], adjust: &PageAdjust) {
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
            adjust,
        );
        for (dst, c) in px[..3].iter_mut().zip(rgb) {
            *dst = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
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

    /// Serialises the tests that shrink the process-wide raster budget: the
    /// env override is shared by every test thread in this process.
    static BUDGET_LOCK: Mutex<()> = Mutex::new(());

    struct BudgetEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl BudgetEnvGuard {
        fn set(mib: &str) -> Self {
            let previous = std::env::var_os("OCS_PDF_RASTER_CACHE_MB");
            std::env::set_var("OCS_PDF_RASTER_CACHE_MB", mib);
            Self { previous }
        }
    }

    impl Drop for BudgetEnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("OCS_PDF_RASTER_CACHE_MB", value),
                None => std::env::remove_var("OCS_PDF_RASTER_CACHE_MB"),
            }
        }
    }

    /// One square page with a filled rectangle: at 150 DPI a 100 mm page
    /// rasterises to ~1.3 MiB, so a handful of zoom bands passes a 2 MiB
    /// budget.
    fn budget_test_pdf(side_mm: f32) -> Vec<u8> {
        use printpdf::{
            Color, LinePoint, Op, PaintMode, Point, Polygon, PolygonRing, Rgb, WindingOrder,
        };
        let mut document = PdfDocument::new("PDF underlay budget test");
        let lo = side_mm * 0.1;
        let hi = side_mm * 0.9;
        let ops = vec![
            Op::SetFillColor {
                col: Color::Rgb(Rgb {
                    r: 0.1,
                    g: 0.4,
                    b: 0.8,
                    icc_profile: None,
                }),
            },
            Op::DrawPolygon {
                polygon: Polygon {
                    rings: vec![PolygonRing {
                        points: vec![
                            LinePoint {
                                p: Point {
                                    x: Mm(lo).into(),
                                    y: Mm(lo).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(hi).into(),
                                    y: Mm(lo).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(hi).into(),
                                    y: Mm(hi).into(),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Mm(lo).into(),
                                    y: Mm(hi).into(),
                                },
                                bezier: false,
                            },
                        ],
                    }],
                    mode: PaintMode::Fill,
                    winding_order: WindingOrder::NonZero,
                },
            },
        ];
        document
            .pages
            .push(OutputPage::new(Mm(side_mm), Mm(side_mm), ops));
        document.save(&PdfSaveOptions::default(), &mut Vec::new())
    }

    const SWEEP_BANDS: [f32; 6] = [106.066, 75.0, 53.033, 37.5, 26.517, 18.75];

    #[test]
    fn zoom_sweep_past_budget_evicts_the_oldest_page_raster() {
        let _lock = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _budget = BudgetEnvGuard::set("2");
        let path = "memory://pdf-budget-evict-page.pdf";
        register_source(path, Arc::new(budget_test_pdf(100.0)));

        let first = rasterize_page_display_at(path, "1", 150.0).expect("150 DPI raster");
        for dpi in SWEEP_BANDS {
            drop(rasterize_page_display_at(path, "1", dpi).expect("band raster"));
        }

        let refetched = rasterize_page_display_at(path, "1", 150.0).expect("refetched raster");
        assert!(
            !Arc::ptr_eq(&first, &refetched),
            "the oldest raster must be evicted once the sweep passes the budget"
        );
    }

    #[test]
    fn zoom_sweep_past_budget_evicts_the_oldest_adjusted_copy() {
        let _lock = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _budget = BudgetEnvGuard::set("2");
        let path = "memory://pdf-budget-evict-adjust.pdf";
        register_source(path, Arc::new(budget_test_pdf(100.0)));
        let adjust = PageAdjust {
            contrast: 50,
            contrast_pivot: 500,
            monochrome: false,
            adjust_for_background: false,
            dark_background: false,
            fade_before_contrast: None,
        };

        let raster = rasterize_page_display_at(path, "1", 150.0).expect("150 DPI raster");
        let first = adjusted_pixels(path, "1", &raster, adjust);
        for dpi in SWEEP_BANDS {
            let band = rasterize_page_display_at(path, "1", dpi).expect("band raster");
            drop(adjusted_pixels(path, "1", &band, adjust));
        }

        let again = adjusted_pixels(path, "1", &raster, adjust);
        assert!(
            !Arc::ptr_eq(&first, &again),
            "the oldest adjusted copy must be evicted once the sweep passes the budget"
        );
    }

    /// A cache hit must promote the entry: a re-touched older raster survives
    /// a sweep that evicts an untouched newer one. A 90 mm page keeps the
    /// first two rasters (~1.6 MiB) safely inside the 2 MiB budget before the
    /// sweep pushes past it.
    #[test]
    fn re_touching_an_older_raster_keeps_it_cached_past_the_budget() {
        let _lock = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _budget = BudgetEnvGuard::set("2");
        let path = "memory://pdf-budget-recency.pdf";
        register_source(path, Arc::new(budget_test_pdf(90.0)));

        let oldest = rasterize_page_display_at(path, "1", 150.0).expect("150 DPI raster");
        let middle = rasterize_page_display_at(path, "1", 106.066).expect("106 DPI raster");
        let touched = rasterize_page_display_at(path, "1", 150.0).expect("re-touch hit");
        assert!(
            Arc::ptr_eq(&oldest, &touched),
            "re-touch reads the cached raster"
        );
        drop(touched);
        for dpi in [75.0, 53.033] {
            drop(rasterize_page_display_at(path, "1", dpi).expect("band raster"));
        }

        let refetched = rasterize_page_display_at(path, "1", 150.0).expect("refetched raster");
        assert!(
            Arc::ptr_eq(&oldest, &refetched),
            "the re-touched raster survives eviction as the most recently used"
        );
        let middle_again = rasterize_page_display_at(path, "1", 106.066).expect("middle refetch");
        assert!(
            !Arc::ptr_eq(&middle, &middle_again),
            "the untouched middle raster is the least recently used victim"
        );
    }

    /// Regression guard (passes with and without eviction): a sweep that
    /// stays inside the budget keeps its most recent raster cached.
    #[test]
    fn recent_raster_survives_a_sweep_within_budget() {
        let _lock = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _budget = BudgetEnvGuard::set("128");
        let path = "memory://pdf-budget-recent.pdf";
        register_source(path, Arc::new(budget_test_pdf(100.0)));

        let mut recent = None;
        for dpi in [150.0].into_iter().chain(SWEEP_BANDS) {
            recent = Some(rasterize_page_display_at(path, "1", dpi).expect("band raster"));
        }
        let recent = recent.expect("last band raster");

        let refetched = rasterize_page_display_at(path, "1", 18.75).expect("refetched raster");
        assert!(
            Arc::ptr_eq(&recent, &refetched),
            "the most recent raster stays cached while the sweep fits the budget"
        );
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
