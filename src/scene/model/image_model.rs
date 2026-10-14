// ImageModel — CPU-side data for a raster image quad.
//
// Holds decoded RGBA pixel data and the world-space quad geometry derived
// from the RasterImage entity's insertion point, u/v vectors, and pixel size.

use std::collections::HashMap;
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
/// clip polygon, or the image with the polygon cut out when the boundary
/// hides its inside.
fn clip_triangles_px(img: &codec::entities::RasterImage) -> Vec<[f64; 2]> {
    use codec::entities::ClipMode;
    let w = img.size.x;
    let h = img.size.y;
    let quad = || vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, 0.0], [w, h], [0.0, h]];
    let Some(poly) = crate::entities::raster_image::image_clip_polygon(img) else {
        return quad();
    };
    // Inside mode shows the image with the boundary cut out.
    let (points, triangles) = if img.clip_boundary.clip_mode == ClipMode::Inside {
        kernel::geom2d::triangulate(&[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], &[poly])
    } else {
        kernel::geom2d::triangulate(&poly, &[])
    };
    let tris: Vec<[f64; 2]> = triangles
        .into_iter()
        .flat_map(|t| t.map(|i| points[i]))
        .collect();
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
    /// Magnified pixels stay square (raster images); otherwise they blend
    /// (PDF pages, OLE pictures).
    pub pixelated: bool,
    /// Pixel alpha applies. A raster image whose transparency is off draws
    /// every pixel opaque in its stored colour.
    pub use_alpha: bool,
}

/// The colour adjustments an underlay's content is drawn with over
/// the background: contrast, monochrome, the background adjustment and (DGN)
/// the fade.
pub(crate) fn underlay_adjust(
    u: &codec::entities::Underlay,
    def: &codec::entities::UnderlayDefinition,
    background: [f32; 4],
) -> super::pdf_raster::PageAdjust {
    use super::pdf_raster::PageAdjust;
    use codec::entities::{UnderlayDisplayFlags, UnderlayType};
    let page = crate::entities::underlay::page_of(def);
    // Dark means an HSL lightness under one half: pure blue counts as
    // light, (0, 128, 0) as dark.
    let bg_max = background[0].max(background[1]).max(background[2]);
    let bg_min = background[0].min(background[1]).min(background[2]);
    let bg_lum = (bg_max + bg_min) / 2.0;
    let dgn = def.underlay_type == UnderlayType::Dgn;
    // A DGN model is faded into the background before its contrast is
    // applied (the reference's order), so it is drawn opaque.
    let bg_rgb = [0, 1, 2].map(|i| (background[i].clamp(0.0, 1.0) * 255.0).round() as u8);
    let fade_before_contrast = (dgn && u.fade > 0).then_some((bg_rgb, u.fade.min(100)));
    PageAdjust {
        contrast: u.contrast.min(100),
        // Measured: PDF 0.5; DWF 0.243, or 0.73 once its colours are
        // turned over for a dark background; a DGN model's from its
        // own colours, over any background.
        contrast_pivot: match def.underlay_type {
            UnderlayType::Pdf => 500,
            UnderlayType::Dwf
                if u.flags
                    .contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND)
                    && bg_lum < 0.5 =>
            {
                730
            }
            UnderlayType::Dwf => 243,
            UnderlayType::Dgn => super::underlay_vector::dgn_contrast_pivot(
                &def.file_path,
                page,
                &super::pdf_layers::hidden_layers(u),
                super::underlay_vector::Backdrop::of([background[0], background[1], background[2]]),
                fade_before_contrast,
            )
            .unwrap_or(404),
        },
        monochrome: u.flags.contains(UnderlayDisplayFlags::MONOCHROME),
        // A DGN model's colours follow the background through its
        // colour table, whatever this setting.
        adjust_for_background: u
            .flags
            .contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND)
            && !dgn,
        dark_background: bg_lum < 0.5,
        fade_before_contrast,
    }
}

impl ImageModel {
    /// Build an ImageModel from a DXF RasterImage entity.
    /// Returns `None` if the image file cannot be opened or decoded.
    pub fn from_raster_image(img: &codec::entities::RasterImage) -> Option<Self> {
        let w = img.size.x;
        let h = img.size.y;
        // A degenerate or non-finite size/vectors (hostile entity data)
        // produce NaN placement math that aborts whole PDF exports; treat
        // it as an unresolvable reference instead.
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return None;
        }
        let vector_ok = |v: &codec::Vector3| {
            let (x, y, z) = (v.x, v.y, v.z);
            x.is_finite() && y.is_finite() && z.is_finite() && (x != 0.0 || y != 0.0 || z != 0.0)
        };
        if !vector_ok(&img.u_vector) || !vector_ok(&img.v_vector) {
            return None;
        }
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
            pixels: adjusted_image_pixels(
                &img.file_path,
                &decoded.pixels,
                img.brightness,
                img.contrast,
            ),
            width: decoded.width,
            height: decoded.height,
            opacity,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
            pixelated: true,
            use_alpha: img
                .flags
                .contains(codec::entities::ImageDisplayFlags::TRANSPARENCY_ON),
        })
    }
}

impl ImageModel {
    /// Build an ImageModel from a PDF UNDERLAY + its definition object.
    ///
    /// The page rasterises through `pdf_raster` with no page background, so
    /// only the drawn content covers the drawing; contrast, monochrome and
    /// the background adjustment recolour that raster and fade lowers its
    /// opacity. The world quad is the page size in inches (1 drawing unit per
    /// page inch) times the entity scale, placed at the insertion with the
    /// entity rotation. A clip boundary (on, 2+ vertices, in underlay units)
    /// limits the drawn area to its inside, or to the page outside it when
    /// inverted. `None` when the underlay is off, non-PDF, or the page can't
    /// be rendered (caller keeps the outline placeholder).
    /// `world_per_pixel` is the view's scale: the page is rasterised near
    /// the size it shows on screen (full size when `None` or when larger),
    /// so thin lines stay one crisp pixel wide when zoomed out.
    pub fn from_underlay(
        u: &codec::entities::Underlay,
        def: &codec::entities::UnderlayDefinition,
        background: [f32; 4],
        world_per_pixel: Option<f64>,
    ) -> Option<Self> {
        use super::pdf_raster;
        use codec::entities::{UnderlayDisplayFlags, UnderlayType};
        if !u.flags.contains(UnderlayDisplayFlags::ON) {
            return None;
        }
        if def.unloaded {
            return None;
        }
        let page = crate::entities::underlay::page_of(def);
        let rect = crate::entities::underlay::definition_rect(def)?;
        // The page's longest side on screen, pixels (the view scale is taken
        // a step coarser, so the raster is never reduced on screen and its
        // one-pixel lines stay unbroken).
        let screen_side = world_per_pixel.filter(|wpp| *wpp > 0.0).map(|wpp| {
            let w = (rect[2] - rect[0]) * u.x_scale.abs();
            let h = (rect[3] - rect[1]) * u.y_scale.abs();
            w.max(h) / wpp
        });
        let (source, raster) = match def.underlay_type {
            UnderlayType::Pdf => {
                // Hidden PDF layers come from the underlay's layer overrides.
                let source = super::pdf_layers::underlay_source(u, &def.file_path);
                let inches = (rect[2] - rect[0]).max(rect[3] - rect[1]);
                let raster = match screen_side {
                    Some(side)
                        if inches > 0.0 && side / inches < pdf_raster::DISPLAY_DPI as f64 =>
                    {
                        pdf_raster::rasterize_page_display_at(
                            &source,
                            page,
                            (side / inches).max(8.0) as f32,
                        )?
                    }
                    _ => pdf_raster::rasterize_page_display(&source, page)?,
                };
                (source, raster)
            }
            // DWF sheets and DGN models are drawn as vector wires
            // (`underlay_vector::display_wires`).
            _ => return None,
        };
        let pixels = pdf_raster::adjusted_pixels(
            &source,
            page,
            &raster,
            underlay_adjust(u, def, background),
        );

        // The page rectangle in drawing units (1 unit per PDF inch, or per
        // DWF/DGN sheet unit), entity scale applied, from its lower-left
        // corner.
        let (page_w, page_h) = (rect[2] - rect[0], rect[3] - rect[1]);
        let w_du = page_w * u.x_scale;
        let h_du = page_h * u.y_scale;
        if w_du.abs() <= 0.0 || h_du.abs() <= 0.0 {
            return None;
        }
        let (c, s) = (u.rotation.cos(), u.rotation.sin());
        let (uxv, uyv) = (c * w_du, s * w_du);
        let (vxv, vyv) = (-s * h_du, c * h_du);

        let [oxv, oyv, ozv] = crate::entities::underlay::local_to_world(u, [rect[0], rect[1]]);
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

        let tris_uv = underlay_visible_uv(u, rect);
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
            pixels,
            width: raster.width,
            height: raster.height,
            opacity: 1.0 - u.fade.min(100) as f32 / 100.0,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
            pixelated: false,
            use_alpha: true,
        })
    }
}

/// Visible region of an underlay page as triangles in page UV (0..1, y up):
/// the whole page, the clip polygon, or — for an inverted clip — the page
/// with the polygon cut out.
fn underlay_visible_uv(u: &codec::entities::Underlay, rect: [f64; 4]) -> Vec<[f64; 2]> {
    let (page_w, page_h) = (rect[2] - rect[0], rect[3] - rect[1]);
    use codec::entities::UnderlayDisplayFlags;
    let page = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let whole = || {
        vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ]
    };
    let clip = crate::entities::underlay::clip_polygon_local(u);
    if !u.flags.contains(UnderlayDisplayFlags::CLIPPING) || clip.len() < 3 {
        return whole();
    }
    let ring: Vec<[f64; 2]> = clip
        .iter()
        .map(|p| [(p[0] - rect[0]) / page_w, (p[1] - rect[1]) / page_h])
        .collect();
    let (points, triangles) = if u.clip_inverted {
        kernel::geom2d::triangulate(&page, &[ring])
    } else {
        kernel::geom2d::triangulate(&ring, &[])
    };
    triangles
        .into_iter()
        .flat_map(|t| t.map(|i| points[i]))
        .collect()
}

impl ImageModel {
    /// Build an ImageModel from an OLE2FRAME's embedded presentation.
    /// The blob's compound file is parsed for the native raster or the cached
    /// metafile picture (rasterized by the `gdi` player); returns `None` when
    /// the frame is degenerate or nothing in the blob decodes, so the caller
    /// falls back to the frame placeholder.
    pub fn from_ole2frame(ole: &codec::entities::Ole2Frame) -> Option<Self> {
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
            pixelated: false,
            use_alpha: true,
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
    /// The SOURCE FILE's pixel dimensions (from the pre-decode header probe),
    /// which remain the entity's sizing truth even when `width`/`height`/
    /// `pixels` were display-capped: the `ImageModel` world quad comes from
    /// the entity's own `size` (never from these fields), and a downsampled
    /// buffer keeps the aspect ratio, so the on-paper footprint never
    /// shrinks — while the PDF exporter derives effective DPI from placement
    /// size × stored pixels and self-corrects.
    pub source_width: u32,
    pub source_height: u32,
}

type ImageCacheEntry = Arc<OnceLock<Option<DecodedImage>>>;

fn image_cache() -> &'static Mutex<HashMap<String, ImageCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ImageCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Insertion order of the source cache's keys, so by-name resolution has a
/// deterministic winner (the MOST RECENT registration) instead of HashMap's
/// unspecified iteration order.
fn source_order() -> &'static Mutex<Vec<String>> {
    static ORDER: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    ORDER.get_or_init(|| Mutex::new(Vec::new()))
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
    {
        let mut sources = source_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !sources.contains_key(path) {
            source_order()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(path.to_string());
        }
        sources.insert(path.to_string(), bytes);
    }
    if let Ok(mut cache) = image_cache().lock() {
        cache.retain(|cached, _| !same_file_name(cached, path));
    }
}

/// Is a source registered under exactly this reference? The web paste path
/// uses it to keep its generated `pasted-…` names unique: the registry is
/// last-wins, so a reused name would silently re-point every earlier paste's
/// entity at the newest pixels. Exact key match — generated names embed their
/// own extension and are all lower-case digits.
pub fn has_source(path: &str) -> bool {
    source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains_key(path)
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
    // Deterministic winner: walk the insertion order newest-first so the
    // most recent drop/pick wins, instead of HashMap iteration order.
    let order = source_order()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    order
        .iter()
        .rev()
        .filter(|key| cache.contains_key(*key))
        .find(|key| same_file_name(key, path))
        .and_then(|key| cache.get(key).cloned())
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

/// Brightness and contrast (0–100, 50 = unchanged) as one lookup table per
/// colour channel. Measured against the reference's display of a gradient:
/// brightness is a power curve x^g with g = ((100 − B) / B)^0.72 (0 is black,
/// 100 white); contrast bends lightness around one half the same way,
/// 0.5 ± 0.5·|2x − 1|^h with h = ((100 − C) / C)^0.72 (0 is flat grey, 100 a
/// hard step). Brightness is applied first. Fade is the model's opacity.
pub(crate) fn brightness_contrast_lut(brightness: u8, contrast: u8) -> [u8; 256] {
    // ponytail: the 0.72 exponent is a least-squares fit of seven measured
    // levels (within about 10/255); refine with more reference samples.
    let curve = |v: u8| -> f64 {
        let v = v.min(100);
        (f64::from(100 - v) / f64::from(v.max(1))).powf(0.72)
    };
    let mut lut = [0u8; 256];
    for (i, out) in lut.iter_mut().enumerate() {
        let mut x = i as f64 / 255.0;
        x = match brightness.min(100) {
            0 => 0.0,
            100 => {
                if i == 0 {
                    0.0
                } else {
                    1.0
                }
            }
            b => x.powf(curve(b)),
        };
        x = match contrast.min(100) {
            0 => 0.5,
            100 => {
                if x >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            c => {
                let d = 2.0 * x - 1.0;
                0.5 + 0.5 * d.signum() * d.abs().powf(curve(c))
            }
        };
        *out = (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    lut
}

/// The decoded raster with the entity's brightness and contrast applied,
/// memoised per source and setting (the GPU keeps textures per pixel buffer).
/// An entry holds its source buffer, so the address in its key cannot be
/// handed to another decode (a reloaded file) while the entry lives.
fn adjusted_image_pixels(
    path: &str,
    pixels: &Arc<Vec<u8>>,
    brightness: u8,
    contrast: u8,
) -> Arc<Vec<u8>> {
    if brightness == 50 && contrast == 50 {
        return Arc::clone(pixels);
    }
    type Key = (String, usize, u8, u8);
    type Entry = (Arc<Vec<u8>>, Arc<Vec<u8>>);
    static CACHE: OnceLock<Mutex<HashMap<Key, Entry>>> = OnceLock::new();
    let key = (
        path.to_string(),
        Arc::as_ptr(pixels) as usize,
        brightness,
        contrast,
    );
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((_, hit)) = cache.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
        return Arc::clone(hit);
    }
    let lut = brightness_contrast_lut(brightness, contrast);
    let mut out = pixels.as_ref().clone();
    for px in out.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = lut[*c as usize];
        }
    }
    let out = Arc::new(out);
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    // ponytail: cleared wholesale past 64 entries; an LRU if many images are adjusted.
    if cache.len() > 64 {
        cache.clear();
    }
    cache.insert(key, (Arc::clone(pixels), Arc::clone(&out)));
    out
}

/// Resolve an image reference to decoded pixels, memoised per path. Handles a
/// local file and an `http`/`https` URL (fetched synchronously natively; on
/// the web the first resolve kicks a background fetch and returns `None` —
/// the placeholder shows until the fetched bytes land and the poll
/// re-resolves). Returns `None` for anything that can't be shown (missing
/// file, offline, decode error, or a fetch still in flight on the web build).
/// The result — including a `None` — is cached so the raster loader and the
/// unresolved-reference placeholder agree on one answer without fetching
/// twice.
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
/// `http`/`https` references are a network fetch — synchronous on native, a
/// one-shot background fetch on the web whose bytes register under the URL
/// (so the exact URL and any same-name path both resolve afterwards).
fn decode_reference(path: &str) -> Option<DecodedImage> {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        // The web fetch registers its bytes under the URL itself; a
        // re-resolve (after the poll re-populates) must read them instead of
        // spawning another fetch.
        #[cfg(target_arch = "wasm32")]
        if let Some(bytes) = registered_bytes(path) {
            return decode_or_note(decode_from_memory(&bytes), path);
        }
        let bytes = fetch_remote(path)?;
        return decode_or_note(decode_from_memory(&bytes), path);
    }
    if let Some(bytes) = registered_bytes(path) {
        return decode_or_note(decode_from_memory(&bytes), path);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(decoded) = decode_or_note(decode_from_file(path), path) {
        return Some(decoded);
    }
    registered_bytes_by_name(path)
        .as_deref()
        .and_then(|bytes| decode_or_note(decode_from_memory(bytes), path))
}

// ── Decode budgets (display-LOD bounding) ───────────────────────────────────
//
// A 500 MB aerial/GIS TIFF can decode to multiple GB of RGBA — enough to kill
// a browser tab, and a 20000×20000 raster alone is a >1 GB GPU texture (the
// image pipeline tiles whatever it receives past `max_texture_dimension_2d`,
// so the CPU-side cost is the real problem). Every decode therefore passes
// two per-target budgets, applied in `decode_with_budgets` below:

/// Ceiling on a full RGBA decode (`width × height × 4`). Decodes that would
/// exceed it run with `image::Limits` pinned to the budget, so the decoder
/// errors instead of ballooning the heap; this module turns that error into a
/// localized refusal (see `refusal_message`) — the pixels are simply not
/// available at full size. Desktops have more headroom than a tab, but the
/// whole decode transient lands at once, so it stays well below typical RAM;
/// wasm lives inside a browser tab whose heap the decode must not pop.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DECODE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// wasm: 512 MB of decoded RGBA — comfortably inside a tab's heap.
#[cfg(target_arch = "wasm32")]
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// Ceiling on the STORED pixel count — what feeds the GPU upload, the PDF
/// embed, and (on wasm) the session heap. Images over it still decode, up to
/// `MAX_DECODE_BYTES`, but are Lanczos3-downsampled to the cap before
/// entering an `ImageModel`; the original BYTES stay registered for a future
/// re-decode, and `DecodedImage::source_width/height` keep the file's true
/// dimensions as the entity's sizing truth. Native plots may want the extra
/// headroom (8192² = 67.1 Mpx); the browser keeps a stricter cap
/// (4096² = 16.8 Mpx) to bound both heap and texture upload.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DISPLAY_PIXELS: u64 = 8192 * 8192;
#[cfg(target_arch = "wasm32")]
const MAX_DISPLAY_PIXELS: u64 = 4096 * 4096;

/// How a probed image size may be decoded under the budgets above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeTier {
    /// Store the decoded pixels untouched.
    Full,
    /// Decode fully, then downsample to the display cap before storing.
    Downsample,
    /// The decode's peak memory would exceed `MAX_DECODE_BYTES`. The probe
    /// bills the peak of the decoder's NATIVE bytes per pixel and the final
    /// RGBA buffer (4/px): both transiently coexist mid-decode on 16-bit
    /// formats, and the decoder's own limits reserve at native bpp — so an
    /// image is refused up front when either exceeds the budget, without
    /// decoding.
    OverBudget,
}

/// The decoder's own memory accounting for a format: how many bytes per
/// pixel the DECODE transiently needs (native layout, plus the RGBA output
/// where they coexist) and any codec-specific scratch multiplier on top.
struct NativeFootprint {
    /// Peak bytes per pixel during decode (native bpp + RGBA where both
    /// live simultaneously; plain 8-bit formats overlap them in place).
    peak_bpp: u64,
    /// Extra codec scratch allowance the decoder stacks on the reserve
    /// (image's TIFF codec needs ~3x in the worst strip/tile layout).
    codec_factor: u64,
}

fn native_footprint(
    format: Option<image::ImageFormat>,
    color: Option<image::ColorType>,
) -> NativeFootprint {
    let native_bpp = color.map_or(4u64, |color| match color {
        image::ColorType::L8 => 1,
        image::ColorType::La8 => 2,
        image::ColorType::Rgb8 => 3,
        image::ColorType::Rgba8 => 4,
        image::ColorType::L16 => 2,
        image::ColorType::La16 => 4,
        image::ColorType::Rgb16 => 6,
        image::ColorType::Rgba16 => 8,
        image::ColorType::Rgb32F => 12,
        image::ColorType::Rgba32F => 16,
        _ => 4,
    });
    // `DynamicImage::into_rgba8` only MOVES for Rgba8; every other layout
    // — including the 4 B/px La16 — allocates the RGBA destination while
    // the native buffer is still alive, so both coexist (image 0.25
    // buffer.rs cast_in_color_space).
    let peak_bpp = if matches!(color, Some(image::ColorType::Rgba8)) {
        4
    } else {
        native_bpp + 4
    };
    // GIF decodes the frame into its own buffer while the screen-sized
    // output buffer is live — two RGBA-sized allocations coexist.
    let peak_bpp = match format {
        Some(image::ImageFormat::Gif) => peak_bpp.max(8),
        _ => peak_bpp,
    };
    // tiff 0.11 splits the given budget between the frame buffer and the
    // chunk scratch (a TOTAL cap), so no multiplier is needed: the honest
    // peak is the coexisting native+RGBA pair, already in peak_bpp.
    let codec_factor = 1;
    NativeFootprint {
        peak_bpp,
        codec_factor,
    }
}

/// Budget decision for probed pixel dimensions. Pure and cfg-independent
/// when the budgets are passed explicitly, so tests can probe arbitrary
/// budgets; the decode path always passes this target's constants.
#[cfg_attr(not(test), allow(dead_code))]
fn tier_for(width: u32, height: u32, max_decode_bytes: u64, max_display_pixels: u64) -> DecodeTier {
    tier_for_native(width, height, 4, 1, max_decode_bytes, max_display_pixels)
}

/// [`tier_for`] with the probed format's native footprint: the caller
/// supplies the probe's color type and format so 16-bit images are billed
/// at what their decode actually allocates.
fn tier_for_native(
    width: u32,
    height: u32,
    peak_bpp: u64,
    codec_factor: u64,
    max_decode_bytes: u64,
    max_display_pixels: u64,
) -> DecodeTier {
    let pixels = width as u64 * height as u64;
    if pixels.saturating_mul(peak_bpp).saturating_mul(codec_factor) > max_decode_bytes {
        return DecodeTier::OverBudget;
    }
    if pixels <= max_display_pixels {
        return DecodeTier::Full;
    }
    // The downsample holds the decoded RGBA, the resize's vertical-pass
    // intermediate (width × new height in RGBA f32, 16 B/px) and the output
    // at once, so that sum must fit the budget too.
    let (target_w, target_h) = downsample_to_cap(width, height, max_display_pixels);
    let resize_peak = pixels
        .saturating_mul(4)
        .saturating_add(
            (width as u64)
                .saturating_mul(target_h as u64)
                .saturating_mul(16),
        )
        .saturating_add(target_w as u64 * target_h as u64 * 4);
    if resize_peak > max_decode_bytes {
        DecodeTier::OverBudget
    } else {
        DecodeTier::Downsample
    }
}

/// Largest dimensions that keep `width × height` within `cap` while
/// preserving the aspect ratio: each side floors (so the product never
/// exceeds the cap while both sides stay above 1 px) and clamps to 1 px — a
/// degenerate strip may then stay slightly over, since it cannot shrink
/// further. Images already within the cap pass through unchanged.
fn downsample_to_cap(width: u32, height: u32, cap: u64) -> (u32, u32) {
    let pixels = width as u64 * height as u64;
    if width == 0 || height == 0 || pixels <= cap {
        return (width, height);
    }
    let scale = (cap as f64 / pixels as f64).sqrt();
    let shrink = |dim: u32| {
        let scaled = (dim as f64 * scale).floor() as u64;
        scaled.clamp(1, dim as u64) as u32
    };
    (shrink(width), shrink(height))
}

/// Outcome of one tiered decode attempt.
enum DecodeAttempt {
    /// Pixels decoded — and display-capped when the tier called for it —
    /// ready to store.
    Decoded(DecodedImage),
    /// The decoder's limits rejected the image: full RGBA would exceed
    /// `MAX_DECODE_BYTES`. Carries the probed dimensions for the refusal
    /// note; the caller decides how to surface it.
    Refused { width: u32, height: u32 },
    /// Unreadable, unsupported, or corrupt.
    Failed,
}

/// Decode an image through the budget tiers. The header dimensions are
/// probed BEFORE any pixel decode, so the tiers act on size alone; `open` is
/// called twice — once for the probe, once for the decode — because both
/// passes consume the reader.
fn decode_with_budgets<R: std::io::BufRead + std::io::Seek>(
    open: impl Fn() -> std::io::Result<image::ImageReader<R>>,
    max_decode_bytes: u64,
    max_display_pixels: u64,
) -> DecodeAttempt {
    // Probe pass: header dimensions AND color type, no pixel decode. The
    // probe reader carries the full budget too so constructing a decoder on
    // a huge-but-in-budget header cannot itself fail on image's default
    // 512 MiB probe limits.
    let probed = open()
        .and_then(|reader| reader.with_guessed_format())
        .ok()
        .and_then(|mut reader| {
            let format = reader.format();
            let mut probe_limits = image::Limits::default();
            probe_limits.max_alloc = Some(max_decode_bytes);
            reader.limits(probe_limits);
            use image::ImageDecoder as _;
            let decoder = reader.into_decoder().ok()?;
            let (width, height) = decoder.dimensions();
            let color = decoder.color_type();
            Some((width, height, format, color))
        });
    let tier = probed.map_or(DecodeTier::Full, |(width, height, format, color)| {
        let footprint = native_footprint(format, Some(color));
        tier_for_native(
            width,
            height,
            footprint.peak_bpp,
            footprint.codec_factor,
            max_decode_bytes,
            max_display_pixels,
        )
    });

    // Trust the probe, not the decoder limits: the tier above bills the
    // decode's real peak (native bpp, codec scratch), so an OverBudget
    // image provably cannot fit; refuse without decoding. This covers the
    // grayscale/RGB over-RGBA-budget balloon AND the 16-bit formats whose
    // native layout is double the RGBA size.
    if tier == DecodeTier::OverBudget {
        return DecodeAttempt::Refused {
            width: probed.map_or(0, |(width, _, _, _)| width),
            height: probed.map_or(0, |(_, height, _, _)| height),
        };
    }

    let mut reader = match open().and_then(|reader| reader.with_guessed_format()) {
        Ok(reader) => reader,
        Err(_) => return DecodeAttempt::Failed,
    };
    // For formats that FIT the budget, the limits still guard runaway
    // allocations mid-decode. GIF is special: the probe reads the LOGICAL
    // SCREEN, but a crafted frame may declare itself arbitrarily larger
    // and the gif crate does not enforce frame-vs-screen consistency — so
    // the frame buffer is capped to the probed size plus slack, refusing
    // frame bombs into the existing Refused path. TIFF stacks its own
    // intermediate scratch on top of the reserve (~3x worst case in image
    // 0.25); the tier already folded that factor in, and the allowance
    // matches it so an in-tier TIFF never codec-refuses.
    let mut limits = image::Limits::default();
    // tiff 0.11 SPLITS this cap between the frame buffer and chunk scratch
    // (a total ceiling), so the plain budget is the right allowance; the
    // ×3 multiplier that image's older tiff codec needed would instead
    // admit hostile chunk transients up to 3× the budget.
    limits.max_alloc = Some(match reader.format() {
        Some(image::ImageFormat::Gif) => probed
            .map(|(width, height, _, _)| {
                (width as u64)
                    .saturating_mul(height as u64)
                    .saturating_mul(4)
                    + 64 * 1024 * 1024
            })
            .unwrap_or(max_decode_bytes)
            .min(max_decode_bytes),
        _ => max_decode_bytes,
    });
    reader.limits(limits);
    let img = match reader.decode() {
        Ok(img) => img,
        Err(image::ImageError::Limits(_)) => {
            return DecodeAttempt::Refused {
                width: probed.map_or(0, |(width, _, _, _)| width),
                height: probed.map_or(0, |(_, height, _, _)| height),
            };
        }
        Err(_) => return DecodeAttempt::Failed,
    };

    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    // Display cap: an OverBudget decode that succeeded (native headroom) is
    // necessarily over the cap too — the budgets' invariant guarantees it —
    // so both capping tiers downsample before storing.
    let (target_w, target_h) = match tier {
        DecodeTier::Full => (width, height),
        DecodeTier::Downsample | DecodeTier::OverBudget => {
            downsample_to_cap(width, height, max_display_pixels)
        }
    };
    let rgba = if target_w != width || target_h != height {
        image::imageops::resize(
            &rgba,
            target_w,
            target_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        rgba
    };
    let (stored_w, stored_h) = rgba.dimensions();
    // Sizing truth: the file's probed dimensions, not the (possibly reduced)
    // buffer's, so placement and aspect come from the original raster.
    let (source_width, source_height) = probed.map_or((stored_w, stored_h), |(w, h, _, _)| (w, h));
    DecodeAttempt::Decoded(DecodedImage {
        pixels: Arc::new(rgba.into_raw()),
        width: stored_w,
        height: stored_h,
        source_width,
        source_height,
    })
}

/// Decode in-memory image bytes (picked file, drop, or network body) to RGBA,
/// bounded by the decode/display budgets.
fn decode_from_memory(bytes: &[u8]) -> DecodeAttempt {
    decode_with_budgets(
        || Ok(image::ImageReader::new(std::io::Cursor::new(bytes))),
        MAX_DECODE_BYTES,
        MAX_DISPLAY_PIXELS,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_from_file(path: &str) -> DecodeAttempt {
    decode_with_budgets(
        || image::ImageReader::open(path),
        MAX_DECODE_BYTES,
        MAX_DISPLAY_PIXELS,
    )
}

/// Localized over-budget refusal for `path`: names the image, its probed
/// dimensions, the decoded size the budget rejected, and the limit itself.
/// TIFFs rejected here have no fallback path — the `tiff` crate is only a
/// transitive dependency (via `image`'s tiff feature) and is deliberately
/// not promoted to a direct one, so the refusal IS the handling.
fn refusal_message(path: &str, width: u32, height: u32) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    if width == 0 || height == 0 {
        // The probe could not determine a size (exotic header) — do not
        // print a self-contradictory "0x0 px" figure.
        return crate::tf!(
            "\"{name}\" could not be sized from its header and decoding it exceeded the {limit} MB image limit of this build — it cannot be attached.",
            name = name,
            limit = MAX_DECODE_BYTES / (1024 * 1024)
        )
        .into_owned();
    }
    let decoded_mb = (width as u64)
        .saturating_mul(height as u64)
        .saturating_mul(4)
        / (1024 * 1024);
    let limit_mb = MAX_DECODE_BYTES / (1024 * 1024);
    crate::tf!(
        "\"{name}\" is {width}×{height} px and would decode to {decoded} MB, over the {limit} MB image limit of this build — it cannot be attached. Images within the limit are attached at a reduced display resolution when needed.",
        name = name,
        width = width,
        height = height,
        decoded = decoded_mb,
        limit = limit_mb
    )
    .into_owned()
}

/// Over-budget refusals recorded by `decode_or_note`, drained by the UI
/// (open/drop settle, image pick) to reach the command line.
fn refusal_notes() -> &'static Mutex<Vec<String>> {
    static NOTES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    NOTES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Take and clear every recorded decode refusal (most recent last).
/// Peek without draining — callers that only want THEIR note (the pick
/// path) must not consume refusals queued for other references.
pub fn decode_refusals() -> Vec<String> {
    refusal_notes()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Take and clear — for the drains that own every queued note (the poll and
/// the open-settle paths).
pub fn take_decode_refusals() -> Vec<String> {
    let mut notes = refusal_notes()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    std::mem::take(&mut *notes)
}

/// Store one decode outcome: pixels pass through, a refusal is noted for the
/// UI, and anything else is a plain `None` (broken-reference placeholder).
fn decode_or_note(attempt: DecodeAttempt, path: &str) -> Option<DecodedImage> {
    match attempt {
        DecodeAttempt::Decoded(decoded) => Some(decoded),
        DecodeAttempt::Refused { width, height } => {
            // Several references can name the same over-budget file; keep
            // one note per distinct message so the command line is not
            // spammed by a drawing that inserts it repeatedly.
            let message = refusal_message(path, width, height);
            let mut notes = refusal_notes()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if notes.last() != Some(&message) {
                notes.push(message);
            }
            None
        }
        DecodeAttempt::Failed => None,
    }
}

/// Fetch a remote image reference. `http`/`https` only (the caller checks the
/// scheme, so `file://` and other schemes are never followed); bounded by a
/// request timeout and a response-size cap so a slow or hostile URL can't hang
/// the load or exhaust memory. Native builds fetch synchronously through
/// `ureq`; the web build defers to [`fetch_remote`] in the wasm block below.
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

// ── Web remote raster fetch ────────────────────────────────────────────────
//
// A browser has no synchronous fetch, so the wasm `fetch_remote` can only
// START the request: the first resolve of an `http`/`https` reference spawns
// a background task, memoises the miss, and leaves the placeholder showing.
// When the task settles it registers its bytes under the URL (the same
// source registry a pick or drop uses) and queues an outcome for the 50 ms
// control poll, which re-resolves the drawing's image models — the exact
// flow a dropped raster file already takes.
#[cfg(target_arch = "wasm32")]
mod web_fetch {
    use super::register_source;
    use std::collections::{HashSet, VecDeque};
    use std::sync::{Arc, Mutex, OnceLock};

    /// Bodies above this are refused (with a surfaced note) before and after
    /// the read, so a hostile URL can't exhaust wasm memory. The web cap is
    /// roomier than the native 32 MB because there is no streaming reader
    /// behind a `fetch()` body here.
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    /// Connect+body deadline, mirroring the native agent timeout.
    const TIMEOUT_MS: i32 = 8000;
    /// Outcomes the poll hasn't drained yet; a stuck app can't grow this
    /// without bound. Same cap convention as the drop queues.
    const OUTCOME_CAP: usize = 32;

    /// Why a remote fetch failed. Kept structured so the surface (command
    /// line) localizes the sentence; the URL travels alongside.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RemoteFetchError {
        /// The request never produced a response: offline, DNS, refused, or
        /// (most commonly on the web) a CORS block — the surfaced note says
        /// so, because a CORS failure looks like an opaque network error to
        /// the page.
        Network,
        /// The server answered with a non-OK status (404, 500, …).
        Status(u16),
        /// The declared or received body exceeded [`MAX_BYTES`].
        TooLarge,
        /// The abort deadline fired first.
        Timeout,
    }

    /// A settled background fetch, drained by the app's control poll.
    #[derive(Debug)]
    pub enum RemoteFetchOutcome {
        /// Bytes landed in the source registry under this URL/name — the
        /// poll re-resolves image models referencing it (exact URL or a
        /// same-file-name path both match).
        Loaded(String),
        /// The fetch failed; the caller localizes [`RemoteFetchError`].
        Failed(String, RemoteFetchError),
    }

    fn attempted() -> &'static Mutex<HashSet<String>> {
        static ATTEMPTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        ATTEMPTED.get_or_init(|| Mutex::new(HashSet::new()))
    }

    fn outcomes() -> &'static Mutex<VecDeque<RemoteFetchOutcome>> {
        static OUTCOMES: OnceLock<Mutex<VecDeque<RemoteFetchOutcome>>> = OnceLock::new();
        OUTCOMES.get_or_init(|| Mutex::new(VecDeque::new()))
    }

    /// Queue a settled outcome for the poll. Extras beyond the cap are
    /// dropped with a console warning, mirroring the drop queues.
    fn push_outcome(outcome: RemoteFetchOutcome) {
        let mut queue = outcomes()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.len() >= OUTCOME_CAP {
            drop(queue);
            web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(
                "ocs: remote-image outcome queue full; dropping an outcome",
            ));
            // The dropped outcome may be a Loaded whose rebuild trigger is
            // lost; remember that so the next poll does one unconditional
            // image re-resolution instead of waiting for another event.
            *overflowed()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            return;
        }
        queue.push_back(outcome);
    }

    /// Whether an outcome was dropped at the cap and a catch-up rebuild is
    /// owed. [`take_overflowed`] consumes it.
    fn overflowed() -> &'static Mutex<bool> {
        static FLAG: OnceLock<Mutex<bool>> = OnceLock::new();
        FLAG.get_or_init(|| Mutex::new(false))
    }

    /// Consume the dropped-outcome flag (see [`overflowed`]).
    pub fn take_overflowed() -> bool {
        let mut flag = overflowed()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *flag, false)
    }

    /// Pop the oldest settled remote fetch the poll has not handled yet.
    pub fn take_outcome() -> Option<RemoteFetchOutcome> {
        outcomes()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
    }

    /// Why did the `fetch()` promise reject? An abort arrives as a
    /// `DOMException` named `AbortError`; everything else (a `TypeError`,
    /// in practice) is a network-level failure the page cannot distinguish
    /// from a CORS block.
    fn rejection_kind(error: &wasm_bindgen::JsValue) -> RemoteFetchError {
        use wasm_bindgen::JsCast;
        match error.dyn_ref::<web_sys::DomException>() {
            Some(exception) if exception.name() == "AbortError" => RemoteFetchError::Timeout,
            _ => RemoteFetchError::Network,
        }
    }

    /// Read a response body incrementally, refusing past [`MAX_BYTES`] the
    /// moment the cap is crossed (not after buffering the whole body).
    async fn stream_body_capped(
        window: &web_sys::Window,
        response: &web_sys::Response,
    ) -> Result<Vec<u8>, RemoteFetchError> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;

        let Some(stream) = response.body() else {
            // No stream API (or already consumed): fall back to buffering,
            // with the received-length check still applying after.
            let buffer = JsFuture::from(
                response
                    .array_buffer()
                    .map_err(|_| RemoteFetchError::Network)?,
            )
            .await
            .map_err(|error| rejection_kind(&error))?;
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            if bytes.len() as u64 > MAX_BYTES {
                return Err(RemoteFetchError::TooLarge);
            }
            return Ok(bytes);
        };
        let reader = stream
            .get_reader()
            .dyn_into::<web_sys::ReadableStreamDefaultReader>()
            .map_err(|_| RemoteFetchError::Network)?;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let chunk = JsFuture::from(reader.read())
                .await
                .map_err(|error| rejection_kind(&error))?;
            let done = js_sys::Reflect::get(&chunk, &wasm_bindgen::JsValue::from_str("done"))
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if done {
                break;
            }
            let value = js_sys::Reflect::get(&chunk, &wasm_bindgen::JsValue::from_str("value"))
                .map_err(|_| RemoteFetchError::Network)?;
            let view = js_sys::Uint8Array::new(&value);
            if bytes.len() as u64 + view.length() as u64 > MAX_BYTES {
                // Release the reader lock so the underlying connection can
                // be torn down, then refuse.
                let _ = JsFuture::from(reader.cancel()).await;
                return Err(RemoteFetchError::TooLarge);
            }
            bytes.extend_from_slice(&view.to_vec());
        }
        let _ = window; // reader holds the stream; window kept for symmetry
        Ok(bytes)
    }

    /// The actual request. `GET url` with an abort signal armed at
    /// [`TIMEOUT_MS`]; the body is refused past [`MAX_BYTES`] using the
    /// declared `Content-Length` first and the received length second.
    /// `pub(crate)` so the `?file=` deep link can fetch its drawing through
    /// this exact reader (see `io::web_link`).
    pub(crate) async fn fetch_once(url: &str) -> Result<Vec<u8>, RemoteFetchError> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;

        let window = web_sys::window().ok_or(RemoteFetchError::Network)?;
        let controller = web_sys::AbortController::new().map_err(|_| RemoteFetchError::Network)?;
        let init = web_sys::RequestInit::new();
        init.set_method("GET");
        init.set_signal(Some(&controller.signal()));
        let request = web_sys::Request::new_with_str_and_init(url, &init)
            .map_err(|_| RemoteFetchError::Network)?;
        // Arm the deadline. The one-shot closure outlives the await on
        // purpose: aborting a settled request is a no-op, and clearing the
        // timer would need a handle threaded through the error paths for no
        // behavioural gain.
        let abort = wasm_bindgen::closure::Closure::once_into_js(move || controller.abort());
        let abort = js_sys::Function::from(abort);
        window
            .set_timeout_with_callback_and_timeout_and_arguments(
                abort.as_ref(),
                TIMEOUT_MS,
                &js_sys::Array::new(),
            )
            .map_err(|_| RemoteFetchError::Network)?;

        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|error| rejection_kind(&error))?
            .dyn_into::<web_sys::Response>()
            .map_err(|_| RemoteFetchError::Network)?;
        if !response.ok() {
            return Err(RemoteFetchError::Status(response.status()));
        }
        // Pre-check the declared size so an oversized body is refused before
        // it is buffered; then STREAM the body, aborting the moment the
        // received bytes pass MAX_BYTES — array_buffer() would materialize
        // a hostile chunked reply in full before any check could fire.
        let declared = response
            .headers()
            .get("Content-Length")
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<u64>().ok());
        if declared.is_some_and(|declared| declared > MAX_BYTES) {
            return Err(RemoteFetchError::TooLarge);
        }
        stream_body_capped(&window, &response).await
    }

    /// Start the background fetch for `url` if one hasn't been attempted this
    /// session. Each URL is fetched at most once: successes register under
    /// the URL (and survive `clear_image_cache` — they live in the source
    /// registry), and failures stay unretried like the font store's `Failed`
    /// state, so a hostile or dead URL cannot be re-hammered by every scene
    /// rebuild.
    pub fn spawn(url: &str) {
        let url = url.trim().to_string();
        {
            let mut attempted = attempted()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !attempted.insert(url.clone()) {
                return;
            }
        }
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_once(&url).await {
                Ok(bytes) => {
                    register_source(&url, Arc::new(bytes));
                    push_outcome(RemoteFetchOutcome::Loaded(url));
                }
                Err(error) => push_outcome(RemoteFetchOutcome::Failed(url, error)),
            }
        });
    }
}

/// Web: kick the one-shot background fetch and miss now — the placeholder
/// shows until the poll sees the registered bytes (see `web_fetch` above).
#[cfg(target_arch = "wasm32")]
fn fetch_remote(url: &str) -> Option<Vec<u8>> {
    web_fetch::spawn(url);
    None
}

#[cfg(target_arch = "wasm32")]
pub use web_fetch::{
    take_outcome as take_remote_fetch_outcome, take_overflowed, RemoteFetchError,
    RemoteFetchOutcome,
};
// The capped streaming reader itself, shared with the `?file=` deep-link
// fetch (`io::web_link`) so both doors enforce the same 64 MB / 8 s bounds.
#[cfg(target_arch = "wasm32")]
pub(crate) use web_fetch::fetch_once as fetch_url_bytes;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The decode peak per layout: into_rgba8 is a MOVE for Rgba8 only —
    /// every other layout pays native + RGBA coexisting. GIF additionally
    /// carries the screen-sized output beside a (clamped) frame buffer.
    #[test]
    fn native_footprint_peaks_match_the_coexistence_math() {
        use image::ColorType;
        let png = image::ImageFormat::Png;
        let gif = image::ImageFormat::Gif;
        let cases = [
            (ColorType::L8, 5u64),
            (ColorType::La8, 6),
            (ColorType::Rgb8, 7),
            (ColorType::Rgba8, 4),
            (ColorType::L16, 6),
            // 4 B/px but NOT a move — the La16 hole this test pins.
            (ColorType::La16, 8),
            (ColorType::Rgb16, 10),
            (ColorType::Rgba16, 12),
            (ColorType::Rgb32F, 16),
            (ColorType::Rgba32F, 20),
        ];
        for (color, expected) in cases {
            assert_eq!(
                native_footprint(Some(png), Some(color)).peak_bpp,
                expected,
                "{color:?}"
            );
        }
        assert_eq!(
            native_footprint(Some(gif), Some(ColorType::Rgba8)).peak_bpp,
            8
        );
        assert_eq!(native_footprint(None, None).peak_bpp, 8);
    }

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

    // ── Display-LOD budgets ──────────────────────────────────────────────

    /// The tier decision is exact at the budget boundaries and monotone in
    /// size, and u32 extremes must not overflow into a wrong tier.
    #[test]
    fn decode_tiers_split_at_the_budget_boundaries() {
        // Well within both budgets.
        assert_eq!(tier_for(64, 64, 1_000_000, 10_000), DecodeTier::Full);
        // Pixels over the display cap while RGBA stays within the decode
        // budget: decode, then downsample before storing.
        assert_eq!(
            tier_for(400, 400, 2_000_000, 10_000),
            DecodeTier::Downsample
        );
        // …unless the resize transient (decoded + f32 intermediate + output)
        // is what overflows the budget.
        assert_eq!(
            tier_for(400, 400, 1_000_000, 10_000),
            DecodeTier::OverBudget
        );
        // RGBA over the decode budget.
        assert_eq!(
            tier_for(2_000, 2_000, 1_000_000, 10_000),
            DecodeTier::OverBudget
        );
        // Exact boundary counts stay in the lower tier (limits are ">").
        assert_eq!(tier_for(50, 200, 1_000_000, 10_000), DecodeTier::Full);
        assert_eq!(tier_for(500, 500, 1_000_000, 1_000_000), DecodeTier::Full);
        // Extreme dimensions: the u64 math saturates rather than wrapping.
        assert_eq!(
            tier_for(u32::MAX, u32::MAX, u64::MAX, u64::MAX),
            DecodeTier::Full
        );
        assert_eq!(
            tier_for(u32::MAX, u32::MAX, MAX_DECODE_BYTES, MAX_DISPLAY_PIXELS),
            DecodeTier::OverBudget
        );
    }

    /// The per-target constants keep their invariant: the decode budget can
    /// always hold a display-capped image's RGBA, so an OverBudget image
    /// that nevertheless decodes (native headroom) always downsamples too.
    #[test]
    fn budget_constants_are_consistent() {
        assert!(MAX_DECODE_BYTES >= MAX_DISPLAY_PIXELS * 4);
        assert!(MAX_DISPLAY_PIXELS > 0);
    }

    /// Downsample targets keep the aspect ratio, stay within the cap, and
    /// never collapse a dimension below 1 px.
    #[test]
    fn downsample_targets_preserve_aspect_and_cap() {
        // Within the cap (or degenerate): untouched.
        assert_eq!(downsample_to_cap(100, 50, 10_000), (100, 50));
        assert_eq!(downsample_to_cap(0, 50, 10_000), (0, 50));
        // Over the cap: the product fits while both sides exceed 1 px, and
        // the aspect survives to within rounding (unless a side is too
        // small to express it — a 3 px side clamps to 1 px by design).
        for (width, height) in [
            (4097u32, 4096u32),
            (20000, 15000),
            (12345, 6700),
            (3, 20000000),
        ] {
            let cap = 4096u64 * 4096;
            let (w, h) = downsample_to_cap(width, height, cap);
            assert!(
                (w as u64) * h as u64 <= cap,
                "{width}×{height} -> {w}×{h} over cap"
            );
            assert!(w >= 1 && h >= 1);
            if w >= 8 && h >= 8 {
                let aspect = width as f64 / height as f64;
                assert!(
                    (w as f64 / h as f64 - aspect).abs() / aspect < 0.05,
                    "{width}×{height} -> {w}×{h} lost the aspect"
                );
            }
        }
        // A degenerate strip: the 1 px side cannot shrink further, so the
        // other side alone absorbs the reduction.
        let (w, h) = downsample_to_cap(100_000_000, 1, 4096 * 4096);
        assert_eq!(h, 1);
        assert!(w < 100_000_000);
        // Extreme reduction bottoms out at 1×1, never 0.
        assert_eq!(downsample_to_cap(50_000, 50_000, 1), (1, 1));
    }

    /// A real decode keeps the metadata-vs-buffer split: the stored buffer
    /// obeys the display cap while `source_*` keep the file's true pixel
    /// dimensions, and a decode over the decode budget is refused with its
    /// probed dimensions instead of allocating.
    #[test]
    fn decode_budgets_split_metadata_from_pixels() {
        let bytes = png_bytes(120, 80, 0x5a);
        let open = || Ok(image::ImageReader::new(std::io::Cursor::new(&bytes)));
        // Full tier: buffer and metadata agree.
        match decode_with_budgets(open, 1_000_000, 1_000_000) {
            DecodeAttempt::Decoded(image) => {
                assert_eq!((image.width, image.height), (120, 80));
                assert_eq!((image.source_width, image.source_height), (120, 80));
                assert_eq!(image.pixels.len(), 120 * 80 * 4);
            }
            _ => panic!("a small png must decode"),
        }
        // Display cap only: the buffer shrinks (aspect kept, ≤ cap), the
        // metadata does not — the sizing truth survives the reduction.
        match decode_with_budgets(open, 1_000_000, 3_000) {
            DecodeAttempt::Decoded(image) => {
                assert_eq!((image.source_width, image.source_height), (120, 80));
                assert!((image.width as u64) * image.height as u64 <= 3_000);
                assert!(image.width >= 1 && image.height >= 1);
                assert_eq!(
                    image.pixels.len(),
                    image.width as usize * image.height as usize * 4
                );
                assert!((image.width as f64 / image.height as f64 - 1.5).abs() < 0.1);
            }
            _ => panic!("a display-capped png must still decode"),
        }
        // Decode budget below the full RGBA size: refused, carrying the
        // probed dimensions — no allocation was needed to discover them.
        match decode_with_budgets(open, 1_000, 1_000_000) {
            DecodeAttempt::Refused { width, height } => assert_eq!((width, height), (120, 80)),
            _ => panic!("an over-budget png must be refused"),
        }
    }

    /// Refusals flow through `decode_or_note` into the note queue once per
    /// distinct message, carrying the dimensions and the file name.
    #[test]
    fn refusals_are_noted_once_per_message() {
        let _ = take_decode_refusals();
        let refuse = || DecodeAttempt::Refused {
            width: 30000,
            height: 30000,
        };
        assert!(decode_or_note(refuse(), "C:/gis/aerial.tif").is_none());
        assert!(decode_or_note(refuse(), "C:/gis/aerial.tif").is_none());
        let notes = take_decode_refusals();
        assert_eq!(notes.len(), 1, "duplicate messages collapse: {notes:?}");
        assert!(
            notes[0].contains("30000×30000"),
            "dimensions in: {}",
            notes[0]
        );
        assert!(
            notes[0].contains("aerial.tif"),
            "file name in: {}",
            notes[0]
        );
        assert!(
            take_decode_refusals().is_empty(),
            "the drain cleared the queue"
        );
    }
}
