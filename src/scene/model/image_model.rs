// ImageModel — CPU-side data for a raster image quad.
//
// Holds decoded RGBA pixel data and the world-space quad geometry derived
// from the RasterImage entity's insertion point, u/v vectors, and pixel size.

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// One textured triangle vertex of an image's visible region: an RTE-split
/// world position (`pos` high half / `pos_low` low half) plus its texture UV.
#[derive(Clone, Copy, Debug)]
pub struct ImageQuadVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub pos_low: [f32; 3],
}

/// Build the two-triangle quad (6 verts) for an unclipped image from its
/// corners. Texel (0,0) maps to the top-left corner.
fn quad_verts(corners: &[[f32; 3]; 4], corners_low: &[[f32; 3]; 4]) -> Vec<ImageQuadVertex> {
    let [p0, p1, p2, p3] = *corners;
    let [l0, l1, l2, l3] = *corners_low;
    vec![
        ImageQuadVertex {
            pos: p0,
            uv: [0.0, 1.0],
            pos_low: l0,
        },
        ImageQuadVertex {
            pos: p1,
            uv: [1.0, 1.0],
            pos_low: l1,
        },
        ImageQuadVertex {
            pos: p2,
            uv: [1.0, 0.0],
            pos_low: l2,
        },
        ImageQuadVertex {
            pos: p0,
            uv: [0.0, 1.0],
            pos_low: l0,
        },
        ImageQuadVertex {
            pos: p2,
            uv: [1.0, 0.0],
            pos_low: l2,
        },
        ImageQuadVertex {
            pos: p3,
            uv: [0.0, 0.0],
            pos_low: l3,
        },
    ]
}

/// Visible-region triangles in image PIXEL space (flat, groups of 3): the whole
/// image rectangle when unclipped, else the clip rectangle or the triangulated
/// clip polygon. An inverted (show-outside) boundary isn't a simple filled
/// region, so it falls back to the whole image rather than mis-clip.
fn clip_triangles_px(img: &acadrust::entities::RasterImage) -> Vec<[f64; 2]> {
    use acadrust::entities::{ClipMode, ClipType};
    let w = img.size.x;
    let h = img.size.y;
    let quad = || vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, 0.0], [w, h], [0.0, h]];
    if !img.clipping_enabled {
        return quad();
    }
    let cb = &img.clip_boundary;
    if cb.clip_mode == ClipMode::Inside {
        return quad();
    }
    // Clip-boundary Y is in image raster space (row 0 = top, Y increasing
    // downward), whereas this pixel space matches the image's v-vector (Y up
    // from the insertion corner). Flip each vertex's Y (`h - y`) so the clip
    // lands where AutoCAD draws it and samples the matching texels.
    let tris: Vec<[f64; 2]> = match cb.clip_type {
        ClipType::Rectangular if cb.vertices.len() >= 2 => {
            let (v0, v1) = (cb.vertices[0], cb.vertices[1]);
            let (xa, xb) = (v0.x.min(v1.x), v0.x.max(v1.x));
            let (y0, y1) = (h - v0.y, h - v1.y);
            let (ya, yb) = (y0.min(y1), y0.max(y1));
            vec![[xa, ya], [xb, ya], [xb, yb], [xa, ya], [xb, yb], [xa, yb]]
        }
        ClipType::Polygonal if cb.vertices.len() >= 3 => {
            let poly: Vec<[f64; 3]> = cb.vertices.iter().map(|v| [v.x, h - v.y, 0.0]).collect();
            crate::entities::mesh::triangulate_planar(&poly)
                .into_iter()
                .map(|p| [p[0], p[1]])
                .collect()
        }
        _ => quad(),
    };
    if tris.is_empty() {
        quad()
    } else {
        tris
    }
}

#[derive(Clone, Debug)]
pub struct ImageModel {
    pub render_instance: Option<super::instance_model::RenderInstance>,
    /// Original file path (used for reload / display in properties).
    pub file_path: String,
    /// RGBA8 pixel data in row-major order. Arc-wrapped so cloning ImageModel
    /// is O(1) — the pixel bytes are shared, not copied.
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Opacity: 1.0 = opaque, 0.0 = transparent.
    pub opacity: f32,
    /// World-space quad corners (CCW), same order as image_corners() helper:
    ///   [0] origin (bottom-left)
    ///   [1] origin + U*W (bottom-right)
    ///   [2] origin + U*W + V*H (top-right)
    ///   [3] origin + V*H (top-left)
    pub corners: [[f32; 3]; 4],
    /// Low residual paired with `corners` (double-single) so the GPU keeps
    /// sub-unit precision at UTM-scale insertion points.
    pub corners_low: [[f32; 3]; 4],
    /// Normalized draw-order depth in (0,1); higher draws on top. Fed to the
    /// image pipeline as a small clip-z bias so the raster orders correctly
    /// against other entity types.
    pub draw_depth: f32,
    /// Textured triangles of the image's VISIBLE region — the full quad, or
    /// (when the entity carries a clip boundary) the triangulated clip polygon
    /// — so the raster is drawn only inside its clip boundary.
    pub verts: Vec<ImageQuadVertex>,
}

impl ImageModel {
    /// Build an ImageModel from a DXF RasterImage entity.
    /// Returns `None` if the image file cannot be opened or decoded.
    pub fn from_raster_image(img: &acadrust::entities::RasterImage) -> Option<Self> {
        let w = img.size.x;
        let h = img.size.y;
        // Model-space geometry is drawn in (WCS - world_offset) so large UTM-
        // scale coordinates stay within f32 precision; offset the image too.
        // Corners come from a large insertion point plus small u/v spans.
        // Split each into double-single (high, low) f32 so the GPU keeps
        // sub-unit precision at UTM scale and after a cross-drawing paste.
        let oxv = img.insertion_point.x;
        let oyv = img.insertion_point.y;
        let ozv = img.insertion_point.z;
        let ux = (img.u_vector.x * w) as f32;
        let uy = (img.u_vector.y * w) as f32;
        let uz = (img.u_vector.z * w) as f32;
        let vx = (img.v_vector.x * h) as f32;
        let vy = (img.v_vector.y * h) as f32;
        let vz = (img.v_vector.z * h) as f32;
        // High/low split of the anchor; the u/v spans are small and added to
        // the high half (their own residual is below f32 noise at this scale).
        let ox = oxv as f32;
        let oy = oyv as f32;
        let oz = ozv as f32;
        let oxl = (oxv - ox as f64) as f32;
        let oyl = (oyv - oy as f64) as f32;
        let ozl = (ozv - oz as f64) as f32;
        let corners = [
            [ox, oy, oz],
            [ox + ux, oy + uy, oz + uz],
            [ox + ux + vx, oy + uy + vy, oz + uz + vz],
            [ox + vx, oy + vy, oz + vz],
        ];
        let corners_low = [[oxl, oyl, ozl]; 4];
        let opacity = 1.0 - img.fade as f32 / 100.0;

        // Visible region as textured triangles. When the entity carries a clip
        // boundary this is the triangulated clip polygon (each pixel-space
        // vertex mapped to world via u/v and to a texel UV), so the raster is
        // painted only inside its boundary; otherwise it's the full quad.
        let verts: Vec<ImageQuadVertex> = clip_triangles_px(img)
            .iter()
            .map(|&[px, py]| {
                let fu = (px / w) as f32;
                let fv = (py / h) as f32;
                ImageQuadVertex {
                    pos: [
                        ox + ux * fu + vx * fv,
                        oy + uy * fu + vy * fv,
                        oz + uz * fu + vz * fv,
                    ],
                    uv: [fu, 1.0 - fv],
                    pos_low: [oxl, oyl, ozl],
                }
            })
            .collect();

        let decoded = resolve_image(&img.file_path)?;
        Some(Self {
            render_instance: None,
            file_path: img.file_path.clone(),
            pixels: decoded.pixels,
            width: decoded.width,
            height: decoded.height,
            opacity,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
        })
    }
}

impl ImageModel {
    /// Build an ImageModel from a PDF UNDERLAY + its definition object.
    ///
    /// The page rasterises through `pdf_raster` (system pdftoppm, cached);
    /// the world quad sizes from the page's physical size — 1 underlay unit =
    /// 1 PDF inch — times the entity's scale, placed at the insertion with
    /// the entity's rotation. `None` when the underlay is off, non-PDF, or
    /// the page can't be rendered (caller keeps the outline placeholder).
    pub fn from_underlay(
        u: &acadrust::entities::Underlay,
        def: &acadrust::entities::UnderlayDefinition,
    ) -> Option<Self> {
        use acadrust::entities::{UnderlayDisplayFlags, UnderlayType};
        if !u.flags.contains(UnderlayDisplayFlags::ON) {
            return None;
        }
        if !matches!(def.underlay_type, UnderlayType::Pdf) {
            return None;
        }
        let page = if def.page_name.trim().is_empty() {
            "1"
        } else {
            def.page_name.trim()
        };
        let raster = super::pdf_raster::rasterize_page(&def.file_path, page)?;

        // Page size in drawing units (1 unit per PDF inch), entity scale applied.
        let w_du = raster.width as f64 / raster.dpi as f64 * u.x_scale;
        let h_du = raster.height as f64 / raster.dpi as f64 * u.y_scale;
        if w_du <= 0.0 || h_du <= 0.0 {
            return None;
        }
        let (c, s) = (u.rotation.cos(), u.rotation.sin());
        let (uxv, uyv) = (c * w_du, s * w_du);
        let (vxv, vyv) = (-s * h_du, c * h_du);

        let oxv = u.insertion_point.x;
        let oyv = u.insertion_point.y;
        let ozv = u.insertion_point.z;
        let ox = oxv as f32;
        let oy = oyv as f32;
        let oz = ozv as f32;
        let oxl = (oxv - ox as f64) as f32;
        let oyl = (oyv - oy as f64) as f32;
        let ozl = (ozv - oz as f64) as f32;
        let (ux, uy) = (uxv as f32, uyv as f32);
        let (vx, vy) = (vxv as f32, vyv as f32);
        let corners = [
            [ox, oy, oz],
            [ox + ux, oy + uy, oz],
            [ox + ux + vx, oy + uy + vy, oz],
            [ox + vx, oy + vy, oz],
        ];
        let corners_low = [[oxl, oyl, ozl]; 4];

        // Visible region: the clip polygon (vertices in underlay units,
        // scaled like the quad) fan-triangulated, or the full page. Inverted
        // clips fall back to the full page (rare; better whole than hidden).
        let clipping = u.flags.contains(UnderlayDisplayFlags::CLIPPING)
            && !u.clip_inverted
            && u.clip_boundary_vertices.len() >= 3;
        let tris_uv: Vec<[f64; 2]> = if clipping {
            let pts: Vec<[f64; 2]> = u
                .clip_boundary_vertices
                .iter()
                .map(|p| [p.x * u.x_scale / w_du, p.y * u.y_scale / h_du])
                .collect();
            let mut tris = Vec::with_capacity((pts.len().saturating_sub(2)) * 3);
            for i in 1..pts.len() - 1 {
                tris.push(pts[0]);
                tris.push(pts[i]);
                tris.push(pts[i + 1]);
            }
            tris
        } else {
            vec![
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ]
        };
        let verts: Vec<ImageQuadVertex> = tris_uv
            .iter()
            .map(|&[fu, fv]| {
                let (fu, fv) = (fu as f32, fv as f32);
                ImageQuadVertex {
                    pos: [ox + ux * fu + vx * fv, oy + uy * fu + vy * fv, oz],
                    uv: [fu, 1.0 - fv],
                    pos_low: [oxl, oyl, ozl],
                }
            })
            .collect();

        Some(Self {
            render_instance: None,
            file_path: def.file_path.clone(),
            pixels: raster.pixels.clone(),
            width: raster.width,
            height: raster.height,
            opacity: 1.0 - u.fade as f32 / 100.0,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
        })
    }
}

impl ImageModel {
    /// Build an ImageModel from an OLE2FRAME's embedded presentation.
    /// The blob's compound file is parsed for the native raster or the cached
    /// metafile picture (rasterized by the `gdi` player); returns `None` when
    /// the frame is degenerate or nothing in the blob decodes, so the caller
    /// falls back to the frame placeholder.
    pub fn from_ole2frame(ole: &acadrust::entities::Ole2Frame) -> Option<Self> {
        let payload = ole.encoded_payload();
        let (pixels, width, height) = super::ole_pres::decode(&payload)?;

        // Frame rectangle in WCS. `upper_left`/`lower_right` name the diagonal;
        // normalise to left/right/top/bottom so the bitmap sits upright.
        let left = ole.upper_left_corner.x.min(ole.lower_right_corner.x);
        let right = ole.upper_left_corner.x.max(ole.lower_right_corner.x);
        let bottom = ole.upper_left_corner.y.min(ole.lower_right_corner.y);
        let top = ole.upper_left_corner.y.max(ole.lower_right_corner.y);
        let z = ole.upper_left_corner.z;
        if (right - left).abs() < 1e-9 || (top - bottom).abs() < 1e-9 {
            return None;
        }

        // Double-single split per corner so the quad stays precise at UTM scale.
        let split = |x: f64, y: f64| -> ([f32; 3], [f32; 3]) {
            let (hx, hy, hz) = (x as f32, y as f32, z as f32);
            (
                [hx, hy, hz],
                [
                    (x - hx as f64) as f32,
                    (y - hy as f64) as f32,
                    (z - hz as f64) as f32,
                ],
            )
        };
        // corners: [BL, BR, TR, TL] — the image pipeline maps texel (0,0) to TL.
        let (c0, l0) = split(left, bottom);
        let (c1, l1) = split(right, bottom);
        let (c2, l2) = split(right, top);
        let (c3, l3) = split(left, top);

        let corners = [c0, c1, c2, c3];
        let corners_low = [l0, l1, l2, l3];
        let verts = quad_verts(&corners, &corners_low);
        Some(Self {
            render_instance: None,
            file_path: "OLE2FRAME".to_string(),
            pixels: Arc::new(pixels),
            width,
            height,
            opacity: 1.0,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
        })
    }
}

/// Decoded RGBA image shared between the raster pipeline and the
/// unresolved-reference probe. Cheap to clone — the pixels are `Arc`-shared.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

type ImageCacheEntry = Arc<OnceLock<Option<DecodedImage>>>;

fn image_cache() -> &'static Mutex<HashMap<String, ImageCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ImageCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn source_cache() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lower-cased trailing component of a path-style reference. Drawings store
/// image paths as the author wrote them (often an absolute foreign path), so
/// name — not full path — is the only component a dropped or picked file on
/// another machine can be expected to match.
pub(crate) fn file_name_key(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Do two image references name the same file? Compares trailing components
/// case-insensitively (the web has no path beyond the name at all, and Windows
/// paths are case-insensitive where the drawing was authored).
pub(crate) fn same_file_name(a: &str, b: &str) -> bool {
    let (a, b) = (file_name_key(a), file_name_key(b));
    !a.is_empty() && a == b
}

/// Keep bytes selected through a file dialog or dropped onto the page, keyed
/// by the reference the drawing will carry. The browser exposes no reusable
/// filesystem path, so `decode_reference` reads these instead of a disk file;
/// native builds also skip a second disk read. Registering invalidates every
/// memoised answer that names the same file, so a re-pick (or a drop that
/// finally supplies a missing reference) re-decodes instead of replaying a
/// stale success or a cached failure.
pub fn register_source(path: &str, bytes: Arc<Vec<u8>>) {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(path.to_string(), bytes);
    if let Ok(mut cache) = image_cache().lock() {
        cache.retain(|cached, _| !same_file_name(cached, path));
    }
}

/// Every reference with bytes currently registered (picked or dropped).
/// Used after a web open settles to catch images registered while the load —
/// which builds the raster models — was already running.
pub fn registered_image_names() -> Vec<String> {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .keys()
        .cloned()
        .collect()
}

/// Bytes registered for exactly this reference.
fn registered_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        .cloned()
}

/// Bytes registered under a different path that names the same file — the
/// web fallback for images inside opened drawings, whose stored path is the
/// original author's (a foreign absolute path no browser can open) while the
/// user dropped or picked only the bare file.
fn registered_bytes_by_name(path: &str) -> Option<Arc<Vec<u8>>> {
    let cache = source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cache
        .iter()
        .find(|(key, _)| same_file_name(key, path))
        .map(|(_, bytes)| Arc::clone(bytes))
}

/// Original image file bytes registered for a reference, resolved the way
/// `decode_reference` resolves pixels: exact reference first, then any
/// registration that names the same file. The web ARCHIVE/ETRANSMIT
/// packages these bytes into the transmittal; `None` means nothing was
/// picked or dropped for the reference this session.
pub fn registered_source_bytes(path: &str) -> Option<Arc<Vec<u8>>> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    registered_bytes(path).or_else(|| registered_bytes_by_name(path))
}

/// Drop every memoised image so the next resolve re-reads / re-fetches. Called
/// when a new document is opened — a fresh drawing shouldn't inherit a prior
/// one's stale successes or offline failures.
pub fn clear_image_cache() {
    if let Ok(mut cache) = image_cache().lock() {
        cache.clear();
    }
}

/// Resolve an image reference to decoded pixels, memoised per path. Handles a
/// local file and — on native builds — an `http`/`https` URL. Returns `None`
/// for anything that can't be shown (missing file, offline, decode error, or a
/// URL on the web build). The result — including a `None` — is cached so the
/// raster loader and the unresolved-reference placeholder agree on one answer
/// without fetching twice.
pub fn resolve_image(path: &str) -> Option<DecodedImage> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let entry = {
        let mut cache = image_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        Arc::clone(
            cache
                .entry(path.to_string())
                .or_insert_with(|| Arc::new(OnceLock::new())),
        )
    };
    entry.get_or_init(|| decode_reference(path)).clone()
}

/// Decode a reference to RGBA pixels. Priority: registered bytes (picked or
/// dropped — the only source on the web, where the reference is a name-only
/// path), then the filesystem (native), then any registered file that merely
/// shares the name (a drawing's foreign absolute path vs a dropped file).
/// `http`/`https` references stay a native-only network fetch.
fn decode_reference(path: &str) -> Option<DecodedImage> {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let bytes = fetch_remote(path)?;
        return decode_from_memory(&bytes);
    }
    if let Some(bytes) = registered_bytes(path) {
        return decode_from_memory(&bytes);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(decoded) = decode_from_file(path) {
        return Some(decoded);
    }
    registered_bytes_by_name(path)
        .as_deref()
        .and_then(|bytes| decode_from_memory(bytes))
}

/// Decode in-memory image bytes (picked file, drop, or network body) to RGBA.
fn decode_from_memory(bytes: &[u8]) -> Option<DecodedImage> {
    let img = image::load_from_memory(bytes).ok()?;
    rgba_image(img)
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_from_file(path: &str) -> Option<DecodedImage> {
    let img = image::open(Path::new(path)).ok()?;
    rgba_image(img)
}

fn rgba_image(img: image::DynamicImage) -> Option<DecodedImage> {
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some(DecodedImage {
        pixels: Arc::new(rgba.into_raw()),
        width,
        height,
    })
}

/// Fetch a remote image reference. `http`/`https` only (the caller checks the
/// scheme, so `file://` and other schemes are never followed); bounded by a
/// request timeout and a response-size cap so a slow or hostile URL can't hang
/// the load or exhaust memory. Never runs on the web build — a browser has no
/// synchronous fetch and would hit CORS on a cross-origin image anyway.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_remote(url: &str) -> Option<Vec<u8>> {
    let agent = crate::network::agent(std::time::Duration::from_secs(8));
    let mut resp = agent
        .get(url)
        .header(
            "User-Agent",
            concat!("OpenCADStudio/", env!("OCS_APP_VERSION")),
        )
        .call()
        .ok()?;
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    resp.body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .ok()
}

#[cfg(target_arch = "wasm32")]
fn fetch_remote(_url: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Encode a small deterministic PNG entirely in memory — the same codec
    /// `decode_from_memory` reads, so no fixture file is needed.
    fn png_bytes(w: u32, h: u32, tint: u8) -> Vec<u8> {
        let img =
            image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, tint, 0xff]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode test png");
        out
    }

    /// Registered bytes decode without any filesystem path — the web attach
    /// flow, where the reference is the picked file's name and nothing else.
    #[test]
    fn registered_bytes_decode_without_a_filesystem_path() {
        let path = "memory://image-registry-exact.png";
        register_source(path, Arc::new(png_bytes(7, 5, 0x42)));
        let decoded = resolve_image(path).expect("registered bytes should decode");

        assert_eq!((decoded.width, decoded.height), (7, 5));
        assert_eq!(decoded.pixels.len(), 7 * 5 * 4);
        // First texel is (x=0, y=0) — row 0 counted from the top in the PNG.
        assert_eq!(&decoded.pixels[0..3], &[0, 0, 0x42]);
    }

    /// A re-registration replaces the bytes AND the memoised decode: picking
    /// the same name again must show the new pixels, not the cached old ones.
    #[test]
    fn reregistering_replaces_the_memoised_decode() {
        let path = "memory://image-registry-repick.png";
        register_source(path, Arc::new(png_bytes(3, 3, 0x11)));
        assert_eq!(resolve_image(path).expect("first decode").width, 3);

        register_source(path, Arc::new(png_bytes(9, 4, 0x22)));
        let decoded = resolve_image(path).expect("second decode");
        assert_eq!((decoded.width, decoded.height), (9, 4));
        assert_eq!(&decoded.pixels[0..3], &[0, 0, 0x22]);
    }

    /// The opened-drawing flow: the entity stores the author's absolute path,
    /// which exists on no machine but this one's — a registered file that
    /// merely shares the (case-insensitive) name resolves it.
    #[test]
    fn a_name_only_match_resolves_a_foreign_path() {
        register_source("Registry-Name.png", Arc::new(png_bytes(4, 2, 0x33)));
        let foreign = if cfg!(windows) {
            "C:\\far away\\registry-name.PNG"
        } else {
            "/mnt/far away/registry-name.PNG"
        };

        let decoded = resolve_image(foreign).expect("name match should decode");
        assert_eq!((decoded.width, decoded.height), (4, 2));
        assert_eq!(decoded.pixels.len(), 4 * 2 * 4);
    }

    /// A cached failure must not outlive the registration that fixes it: the
    /// drop flow registers bytes after the open already memoised `None`.
    #[test]
    fn registering_after_a_failed_resolve_retries_the_decode() {
        let path = "memory://image-registry-late.png";
        let foreign = format!("/definitely/not/here/{path}");
        assert!(resolve_image(&foreign).is_none(), "nothing registered yet");

        register_source(path, Arc::new(png_bytes(2, 2, 0x44)));
        let decoded = resolve_image(&foreign).expect("late registration should resolve");
        assert_eq!((decoded.width, decoded.height), (2, 2));
    }

    /// A reference nothing can satisfy stays `None` (and stays memoised as
    /// such) — the placeholder keeps its broken-reference treatment.
    #[test]
    fn an_unmatchable_reference_stays_none() {
        assert!(resolve_image("memory://image-registry-missing.png").is_none());
        assert!(resolve_image("").is_none());
        assert!(resolve_image("   ").is_none());
    }

    /// The name comparison keys on the trailing component only, ignoring
    /// case and separators, and rejects empty names.
    #[test]
    fn file_name_comparison_rules() {
        assert!(same_file_name("C:\\A\\B\\site.png", "site.png"));
        assert!(same_file_name("/a/b/SITE.PNG", "site.PNG"));
        assert!(!same_file_name("site.png", "site.jpg"));
        assert!(!same_file_name("", ""));
        assert!(!same_file_name("C:\\dir\\", "dir"));
    }
}
