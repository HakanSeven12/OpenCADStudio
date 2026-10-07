// PDF export — converts the paper-space wire model to a PDF file using printpdf.
//
// Each WireModel becomes a sequence of DrawLine operations.  NaN values in the
// points array act as segment separators (pen-up).
//
// Coordinate system: CAD uses mm units with origin at bottom-left and Y up.
// printpdf's Point::new(Mm, Mm) also has origin at bottom-left, so no Y-flip
// is needed — we shift the coordinates by (offset_x, offset_y) to place the
// drawing origin at the paper origin.

use crate::io::plot_style::PlotStyleTable;
use crate::scene::model::hatch_model::HatchModel;
#[cfg(not(target_arch = "wasm32"))]
use crate::scene::model::hatch_model::HatchPattern;
use crate::scene::WireModel;
use crate::scene::model::image_model::ImageModel;
#[cfg(not(target_arch = "wasm32"))]
use crate::scene::model::wire_model::SearchableTextRun;
#[cfg(not(target_arch = "wasm32"))]
use printpdf::{
    BlendMode, BuiltinFont, Codepoint, Color, ExtendedGraphicsState, ExtendedGraphicsStateId,
    FontId, Line, LineCapStyle, LineDashPattern, LineJoinStyle, LinePoint, Mm, Op, PaintMode,
    ParsedFont, PdfDocument, PdfFont, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Polygon,
    PolygonRing, Pt, Rgb, TextItem, TextMatrix, TextRenderingMode, WindingOrder,
};
use std::path::Path;

#[derive(Clone, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct PlotWire {
    pub wire: WireModel,
    pub draw_depth: f32,
}

impl std::ops::Deref for PlotWire {
    type Target = WireModel;

    fn deref(&self) -> &Self::Target {
        &self.wire
    }
}

/// Decoded image geometry plus inherited block/viewport clip boundaries.
#[derive(Clone, Debug)]
pub struct PlotImage {
    pub image: ImageModel,
    pub clips: Vec<Vec<[f64; 2]>>,
}

// The web build has no `printpdf` (it pulls a wasm-incompatible `memchr` via
// lopdf → nom_locate) and no filesystem, so PDF export is native-only; the web
// build gets these stubs so the call sites still compile.
#[cfg(target_arch = "wasm32")]
pub fn export_pdf(_page: &PdfPageInput, _path: &Path) -> Result<(), String> {
    Err("PDF export is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub fn export_pdf_pages(
    _pages: &[PdfPageInput],
    _path: &Path,
    _plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    Err("PDF export is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub async fn pick_pdf_path_owned(_stem: String) -> Option<std::path::PathBuf> {
    None
}

/// mm to PDF points (1 mm = 2.834645 pt).
#[cfg(not(target_arch = "wasm32"))]
const MM_TO_PT: f32 = 2.834645;
/// `wire.line_weight_px` is the on-screen pixel weight. Convert the 96-dpi
/// pixels to points while retaining the viewport's lineweight visibility
/// boost, so "As displayed" output has the same visual hierarchy as the
/// canvas instead of making 0.35 mm ByLayer outlines look half as thick.
#[cfg(not(target_arch = "wasm32"))]
const LW_PX_TO_PT: f32 = MM_TO_PT / (96.0 / 25.4);

#[cfg(not(target_arch = "wasm32"))]
const SCREEN_DOT_MM: f32 = 25.4 / 96.0;

/// Output controls shared by preview, PDF export, and printer rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfPlotOptions {
    pub object_lineweights: bool,
    pub scale_lineweights: bool,
    pub transparency: bool,
    pub stamp: bool,
    pub merge_lines: bool,
}

/// End indexes of the first paper/model render group in each flat input list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlotGroupSplits {
    pub wires: usize,
    pub hatches: usize,
    pub wipeouts: usize,
    pub images: usize,
}

#[derive(Default)]
pub struct PlotContent {
    pub wires: std::sync::Arc<Vec<PlotWire>>,
    pub hatches: Vec<HatchModel>,
    pub wipeouts: Vec<HatchModel>,
    pub images: Vec<PlotImage>,
    pub group_splits: PlotGroupSplits,
}

/// Owned geometry and settings for one PDF page.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct PdfPageInput {
    pub content: PlotContent,
    /// Page dimensions in mm, after any 90/270-degree rotation.
    pub paper_w: f64,
    pub paper_h: f64,
    /// Absolute-world offsets stay f64 to preserve local detail at UTM coordinates.
    pub offset_x: f64,
    pub offset_y: f64,
    pub rotation_deg: i32,
    pub scale: f32,
    pub clip: Option<(f32, f32, f32, f32)>,
    pub options: PdfPlotOptions,
    pub plot_style: Option<PlotStyleTable>,
}

impl Default for PdfPlotOptions {
    fn default() -> Self {
        Self {
            object_lineweights: true,
            scale_lineweights: false,
            transparency: false,
            stamp: false,
            merge_lines: false,
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────

/// Export one page to a PDF file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_pdf(page: &PdfPageInput, path: &Path) -> Result<(), String> {
    export_pdf_pages(std::slice::from_ref(page), path, None)
}

/// Export several independently sized pages into one PDF file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_pdf_pages(
    pages: &[PdfPageInput],
    path: &Path,
    plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("No pages were selected.".into());
    }
    let bytes = build_pdf_pages(pages, plot_style)?;
    write_pdf_atomically(path, &bytes)
}

/// Write a complete PDF beside the destination, then replace it atomically.
#[cfg(not(target_arch = "wasm32"))]
fn write_pdf_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp_path = super::save_temp_path(path);
    if let Err(error) = std::fs::write(&temp_path, bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write PDF data: {error}"));
    }
    if let Err(error) = super::replace_save_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to replace PDF file: {error}"));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn image_clip(ops: &mut Vec<Op>, rings: Vec<Vec<[f32; 2]>>) {
    ops.push(Op::DrawPolygon {
        polygon: Polygon {
            rings: rings
                .into_iter()
                .map(|ring| PolygonRing {
                    points: ring
                        .into_iter()
                        .map(|[x, y]| LinePoint {
                            p: Point { x: Pt(x), y: Pt(y) },
                            bezier: false,
                        })
                        .collect(),
                })
                .collect(),
            mode: PaintMode::Clip,
            winding_order: WindingOrder::EvenOdd,
        },
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_image(
    doc: &mut PdfDocument,
    resources: &mut std::collections::HashMap<(usize, u32, u32), printpdf::XObjectId>,
    ops: &mut Vec<Op>,
    plot: &PlotImage,
    ox: f64,
    oy: f64,
    options: PdfPlotOptions,
) -> Result<(), String> {
    let image = &plot.image;
    let expected_byte_count = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|n| n.checked_mul(4));
    if image.width == 0 || image.height == 0 || expected_byte_count != Some(image.pixels.len()) {
        return Err("Cannot plot bitmap: dimensions do not match its RGBA pixels.".into());
    }
    if image.verts.len() < 3 || image.verts.len() % 3 != 0 {
        return Err("Cannot plot bitmap: incomplete triangle geometry.".into());
    }
    if options.transparency && image.opacity <= 0.0 {
        return Ok(());
    }
    let to_page = |high: [f32; 3], low: [f32; 3]| {
        [
            ((high[0] as f64 + low[0] as f64 + ox) * MM_TO_PT as f64) as f32,
            ((high[1] as f64 + low[1] as f64 + oy) * MM_TO_PT as f64) as f32,
        ]
    };
    let corners: [[f32; 2]; 4] =
        std::array::from_fn(|i| to_page(image.corners[i], image.corners_low[i]));
    if !corners.iter().flatten().all(|v| v.is_finite())
        || !image.verts.iter().all(|v| {
            to_page(v.pos, v.pos_low)
                .iter()
                .chain(&v.uv)
                .all(|c| c.is_finite())
        })
        || !plot
            .clips
            .iter()
            .all(|ring| ring.len() >= 3 && ring.iter().flatten().all(|v| v.is_finite()))
        || !image.opacity.is_finite()
    {
        return Err("Cannot plot bitmap: invalid coordinates, clip boundary, or opacity.".into());
    }
    // The low bit (free: the pixel buffer is aligned) keeps the opaque copy
    // of a picture apart from its transparent one.
    let key = (
        std::sync::Arc::as_ptr(&image.pixels) as usize | usize::from(!image.use_alpha),
        image.width,
        image.height,
    );
    let id = resources
        .entry(key)
        .or_insert_with(|| {
            let id = printpdf::XObjectId::new();
            // add_image clones its argument; the public map accepts owned pixels.
            doc.resources.xobjects.map.insert(
                id.clone(),
                printpdf::XObject::Image(printpdf::RawImage {
                    pixels: printpdf::RawImageData::U8(if image.use_alpha {
                        image.pixels.as_ref().clone()
                    } else {
                        // Transparency off: every pixel opaque in its colour.
                        let mut opaque = image.pixels.as_ref().clone();
                        opaque.chunks_exact_mut(4).for_each(|px| px[3] = 255);
                        opaque
                    }),
                    width: image.width as usize,
                    height: image.height as usize,
                    data_format: printpdf::RawImageFormat::RGBA8,
                    tag: Vec::new(),
                }),
            );
            id
        })
        .clone();
    ops.push(Op::SaveGraphicsState);
    if options.transparency && image.opacity < 1.0 {
        // printpdf 0.9.1 swaps the serialized CA/ca keys. Set both inside
        // this image's saved state so the nonstroking alpha is correct.
        let alpha = image.opacity.clamp(0.0, 1.0);
        let gs = doc.add_graphics_state(
            ExtendedGraphicsState::default()
                .with_current_fill_alpha(alpha)
                .with_current_stroke_alpha(alpha),
        );
        ops.push(Op::LoadGraphicsState { gs });
    }
    for clip in &plot.clips {
        image_clip(
            ops,
            vec![clip
                .iter()
                .map(|p| {
                    [
                        ((p[0] + ox) * MM_TO_PT as f64) as f32,
                        ((p[1] + oy) * MM_TO_PT as f64) as f32,
                    ]
                })
                .collect()],
        );
    }
    let draw = |ops: &mut Vec<Op>, matrix: [f32; 6]| {
        ops.push(Op::SetTransformationMatrix {
            matrix: printpdf::CurTransMat::Raw(matrix),
        });
        ops.push(Op::UseXobject {
            id: id.clone(),
            // Cancel printpdf's pixel-size transform: matrix maps the PDF
            // image's unit square directly onto the CAD quad in page points.
            transform: printpdf::XObjectTransform {
                dpi: Some(72.0),
                scale_x: Some(1.0 / image.width as f32),
                scale_y: Some(1.0 / image.height as f32),
                ..Default::default()
            },
        });
    };
    let u = [corners[1][0] - corners[0][0], corners[1][1] - corners[0][1]];
    let v = [corners[3][0] - corners[0][0], corners[3][1] - corners[0][1]];
    let affine = (0..2).all(|axis| {
        (corners[2][axis] - corners[0][axis] - u[axis] - v[axis]).abs()
            <= 1e-5 * (u[axis].abs() + v[axis].abs()).max(1.0)
    });
    if affine {
        // A single draw preserves intrinsic IMAGE clipping without overlapping image draws.
        image_clip(
            ops,
            image
                .verts
                .chunks_exact(3)
                .map(|tri| {
                    tri.iter()
                        .map(|vertex| to_page(vertex.pos, vertex.pos_low))
                        .collect()
                })
                .collect(),
        );
        draw(ops, [u[0], u[1], v[0], v[1], corners[0][0], corners[0][1]]);
    } else {
        // PDF transforms are affine; perspective sampling is approximate.
        for triangle in image.verts.chunks_exact(3) {
            let page = std::array::from_fn(|i| to_page(triangle[i].pos, triangle[i].pos_low));
            let uv = std::array::from_fn(|i| [triangle[i].uv[0], 1.0 - triangle[i].uv[1]]);
            let matrix = image_triangle_matrix(page, uv)
                .filter(|matrix| matrix.iter().all(|v| v.is_finite()))
                .ok_or("Cannot plot bitmap: degenerate texture mapping.")?;
            ops.push(Op::SaveGraphicsState);
            image_clip(ops, vec![page.to_vec()]);
            draw(ops, matrix);
            ops.push(Op::RestoreGraphicsState);
        }
    }
    ops.push(Op::RestoreGraphicsState);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn image_triangle_matrix(p: [[f32; 2]; 3], uv: [[f32; 2]; 3]) -> Option<[f32; 6]> {
    let u = [uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]];
    let v = [uv[2][0] - uv[0][0], uv[2][1] - uv[0][1]];
    let determinant = u[0] * v[1] - v[0] * u[1];
    if determinant.abs() < 1e-12 {
        return None;
    }
    let axes: [[f32; 2]; 2] = std::array::from_fn(|i| {
        [
            ((p[1][i] - p[0][i]) * v[1] - (p[2][i] - p[0][i]) * u[1]) / determinant,
            (u[0] * (p[2][i] - p[0][i]) - v[0] * (p[1][i] - p[0][i])) / determinant,
        ]
    });
    Some([
        axes[0][0],
        axes[1][0],
        axes[0][1],
        axes[1][1],
        p[0][0] - axes[0][0] * uv[0][0] - axes[0][1] * uv[0][1],
        p[0][1] - axes[1][0] * uv[0][0] - axes[1][1] * uv[0][1],
    ])
}

/// Show a parented PDF save-file dialog and return the chosen path.
///
/// The parent comes from `iced::window::run`, keeping the portal request tied
/// to the visible app window on Wayland instead of silently resolving to
/// `None` on desktops that reject a parentless save dialog (#537).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
pub fn pick_pdf_path_owned(
    stem: String,
    parent: &dyn iced::window::Window,
) -> Option<std::path::PathBuf> {
    let path = crate::sys::blocking_file_dialog()
        .set_parent(parent)
        .set_title(crate::t!("Export as PDF").as_ref())
        .set_file_name(&format!("{stem}.pdf"))
        .add_filter(crate::t!("PDF Files").as_ref(), &["pdf"])
        .add_filter(crate::t!("All Files").as_ref(), &["*"])
        .save_file()
        ?;
    crate::config::remember_dialog_dir(&path);
    Some(path)
}

/// Windows: pick the PDF destination with the async dialog on a worker
/// thread. The parented blocking dialog ran `IFileDialog::Show` on the UI
/// thread inside the window callback, and when the target name already
/// existed the overwrite-confirmation popup is a second nested modal that
/// never gets pumped there — the app froze instead of asking. The async
/// backend runs the dialog off-thread; it is the same pattern the DWG
/// Save As flow uses, whose confirm popup works.
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub async fn pick_pdf_path_async(stem: String) -> Option<std::path::PathBuf> {
    let handle = crate::sys::file_dialog()
        .set_title(crate::t!("Export as PDF").as_ref())
        .set_file_name(format!("{stem}.pdf"))
        .add_filter(crate::t!("PDF Files").as_ref(), &["pdf"])
        .add_filter(crate::t!("All Files").as_ref(), &["*"])
        .save_file()
        .await?;
    Some(crate::sys::handle_path(&handle))
}

// ── PDF builder ───────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn build_pdf_pages(pages: &[PdfPageInput], plot_style: Option<&PlotStyleTable>) -> Result<Vec<u8>, String> {
    let mut doc = PdfDocument::new("Open CAD Studio Export");
    // Subset-embed one TrueType font per used system family before any page
    // references them (fonts live on the document resource dictionary).
    let fonts = SearchableFontSet::build(&mut doc, pages);
    // Borrowing all pages keeps their pixel Arcs alive until this cache is dropped.
    // Allocation addresses cannot be reused by another source during this export.
    let mut image_resources = std::collections::HashMap::new();
    let mut text_fonts = TextFonts::default();
    for (index, page) in pages.iter().enumerate() {
        append_pdf_page(
            &mut doc,
            &mut image_resources,
            &mut text_fonts,
            page,
            plot_style,
            &fonts,
        )
        .map_err(|error| format!("Page {}: {error}", index + 1))?;
    }
    // Subset and embed the TrueType faces the text was drawn with, now that
    // every page has recorded which glyphs it uses.
    text_fonts.install(&mut doc);
    let mut warnings = Vec::new();
    // printpdf 0.9's `optimize` is a no-op (its `doc.compress()` is commented
    // out), so every page's content stream — megabytes of vector operators
    // for a CAD sheet — went out uncompressed: a 16-sheet set weighed 280 MB.
    // Serialise through lopdf ourselves and Flate-compress the streams.
    let mut lo = printpdf::to_lopdf_doc(&doc, &PdfSaveOptions::default(), &mut warnings);
    lo.compress();
    let mut bytes = Vec::new();
    lo.save_to(&mut bytes)
        .map_err(|error| format!("PDF serialisation failed: {error}"))?;
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn append_pdf_page(
    doc: &mut PdfDocument,
    image_resources: &mut std::collections::HashMap<(usize, u32, u32), printpdf::XObjectId>,
    text_fonts: &mut TextFonts,
    page: &PdfPageInput,
    fallback_plot_style: Option<&PlotStyleTable>,
    fonts: &SearchableFontSet,
) -> Result<(), String> {
    let PlotContent { wires, hatches, wipeouts, images, group_splits } = &page.content;
    let (paper_w, paper_h) = (page.paper_w as f32, page.paper_h as f32);
    let (ox, oy) = (page.offset_x, page.offset_y);
    let (rotation_deg, scale, clip) = (page.rotation_deg, page.scale, page.clip);
    let plot_style = page.plot_style.as_ref().or(fallback_plot_style);
    let options = page.options;
    let mut ops: Vec<Op> = Vec::new();

    // White page background.
    ops.push(Op::SetFillColor {
        col: Color::Rgb(Rgb {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            icc_profile: None,
        }),
    });
    ops.push(Op::DrawRectangle {
        rectangle: printpdf::Rect::from_wh(Mm(paper_w).into(), Mm(paper_h).into()),
    });

    let normal_blend = if options.merge_lines {
        let merge = doc.add_graphics_state(
            ExtendedGraphicsState::default().with_blend_mode(BlendMode::multiply()),
        );
        let normal = doc.add_graphics_state(
            ExtendedGraphicsState::default().with_blend_mode(BlendMode::normal()),
        );
        ops.push(Op::SaveGraphicsState);
        ops.push(Op::LoadGraphicsState { gs: merge });
        Some(normal)
    } else {
        None
    };

    // Round line caps/joins for CAD aesthetics.
    ops.push(Op::SetLineCapStyle {
        cap: LineCapStyle::Round,
    });
    ops.push(Op::SetLineJoinStyle {
        join: LineJoinStyle::Round,
    });

    // Apply rotation/scale/clip transform if needed.
    // PDF uses mm-based coordinate system with origin at bottom-left.
    // We save state, apply a CTM (+ optional clip path), then restore after drawing.
    let needs_state = rotation_deg != 0 || (scale - 1.0).abs() > 1e-6 || clip.is_some();
    if needs_state {
        let (cos_a, sin_a, tx, ty) = match rotation_deg {
            // `paper_w`/`paper_h` are already the effective, rotation-swapped
            // page dimensions. A 90° turn maps x' = page_w - y, y' = x;
            // 270° maps x' = y, y' = page_h - x.
            90 => (0.0_f64, 1.0_f64, paper_w as f64, 0.0),
            180 => (-1.0_f64, 0.0_f64, paper_w as f64, paper_h as f64),
            270 => (0.0_f64, -1.0_f64, 0.0, paper_h as f64),
            _ => (1.0_f64, 0.0_f64, 0.0, 0.0),
        };
        let s = scale as f64;
        // PDF CTM: [a b c d e f] = [cos*s sin*s -sin*s cos*s tx ty]
        ops.push(Op::SaveGraphicsState);
        // Convert mm translation to points (1 mm = 2.834645 pt).
        let tx_pt = (tx * 2.834645) as f32;
        let ty_pt = (ty * 2.834645) as f32;
        ops.push(Op::SetTransformationMatrix {
            matrix: printpdf::CurTransMat::Raw([
                (cos_a * s) as f32,
                (sin_a * s) as f32,
                (-(sin_a) * s) as f32,
                (cos_a * s) as f32,
                tx_pt,
                ty_pt,
            ]),
        });
        // Clip rectangle (mm), applied in the pre-scale coordinate space so it
        // matches the wires drawn under the same CTM.
        if let Some((cx, cy, cw, ch)) = clip {
            ops.push(Op::DrawPolygon {
                polygon: Polygon {
                    rings: vec![PolygonRing {
                        points: vec![
                            LinePoint {
                                p: Point { x: Pt(cx * MM_TO_PT), y: Pt(cy * MM_TO_PT) },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point { x: Pt((cx + cw) * MM_TO_PT), y: Pt(cy * MM_TO_PT) },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point {
                                    x: Pt((cx + cw) * MM_TO_PT),
                                    y: Pt((cy + ch) * MM_TO_PT),
                                },
                                bezier: false,
                            },
                            LinePoint {
                                p: Point { x: Pt(cx * MM_TO_PT), y: Pt((cy + ch) * MM_TO_PT) },
                                bezier: false,
                            },
                        ],
                    }],
                    mode: PaintMode::Clip,
                    winding_order: WindingOrder::NonZero,
                },
            });
        }
    }


    let (first_wires, second_wires) =
        wires.split_at(group_splits.wires.min(wires.len()));
    let (first_hatches, second_hatches) =
        hatches.split_at(group_splits.hatches.min(hatches.len()));
    let (first_wipeouts, second_wipeouts) =
        wipeouts.split_at(group_splits.wipeouts.min(wipeouts.len()));
    let (first_images, second_images) =
        images.split_at(group_splits.images.min(images.len()));
    for (wires, hatches, wipeouts, images) in [
        (first_wires, first_hatches, first_wipeouts, first_images),
        (second_wires, second_hatches, second_wipeouts, second_images),
    ] {
    enum DrawItem<'a> {
        WireFill(&'a PlotWire),
        Hatch(&'a HatchModel),
        Image(&'a PlotImage),
        Wire(&'a PlotWire),
        Text(&'a PlotWire),
    }

    let mut draw_items = Vec::with_capacity(wires.len() * 2 + hatches.len() + wipeouts.len() + images.len());
    let mut sequence = 0usize;
    for wire in wires {
        if !wire.fill_tris.is_empty() {
            draw_items.push((wire.draw_depth, 0u8, sequence, DrawItem::WireFill(wire)));
            sequence += 1;
        }
        draw_items.push((wire.draw_depth, 2u8, sequence, DrawItem::Wire(wire)));
        sequence += 1;
        if !wire.text_verts.is_empty() || !wire.searchable_text.is_empty() {
            draw_items.push((wire.draw_depth, 3u8, sequence, DrawItem::Text(wire)));
            sequence += 1;
        }
    }
    for hatch in wipeouts.iter().chain(hatches.iter()) {
        draw_items.push((hatch.draw_depth, 1u8, sequence, DrawItem::Hatch(hatch)));
        sequence += 1;
    }
    for image in images {
        draw_items.push((image.image.draw_depth, 1u8, sequence, DrawItem::Image(image)));
        sequence += 1;
    }
    // Everything outside the export window would only be clipped away, yet
    // still written: a window over a corner of a large drawing came out as a
    // blank page of several megabytes. Drop items whose ink misses the window.
    if let Some((cx, cy, cw, ch)) = clip {
        let marker_h = paper_h as f64 / scale.max(1e-6) as f64;
        let window = [cx as f64, cy as f64, (cx + cw) as f64, (cy + ch) as f64];
        draw_items.retain(|(_, _, _, item)| {
            let bounds = match item {
                DrawItem::WireFill(wire) | DrawItem::Wire(wire) | DrawItem::Text(wire) => {
                    wire_sheet_bounds(&wire.wire, ox, oy, marker_h)
                }
                DrawItem::Hatch(hatch) => hatch_sheet_bounds(hatch, ox, oy),
                DrawItem::Image(_) => None,
            };
            bounds.is_none_or(|b| sheet_overlaps(b, window))
        });
    }
    draw_items.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut last_color: Option<[f32; 3]> = None;
    let mut last_lw: Option<f32> = None;
    let mut last_cap = Some(LineCapStyle::Round);
    let mut last_join = Some(LineJoinStyle::Round);
    // Current PDF dash array (empty = solid). Tracked so the dash op is only
    // re-emitted when it actually changes between wires.
    let mut last_dash: Option<Vec<i64>> = None;

    for (_, _, _, item) in draw_items {
        let wire = match item {
            DrawItem::WireFill(wire) => {
                emit_wire_fills(
                    &mut ops,
                    std::slice::from_ref(&wire.wire),
                    wire.draw_depth,
                    ox,
                    oy,
                    plot_style,
                    scale,
                    options,
                    normal_blend.as_ref(),
                );
                last_color = None;
                last_lw = None;
                last_dash = None;
                continue;
            }
            DrawItem::Hatch(hatch) => {
                emit_hatch(
                    &mut ops,
                    hatch,
                    ox,
                    oy,
                    plot_style,
                    scale,
                    options,
                    normal_blend.as_ref(),
                );
                last_color = None;
                last_lw = None;
                last_dash = None;
                continue;
            }
            DrawItem::Text(wire) => {
                // Outlined runs (stroke fonts, shaped/mixed scripts) keep
                // their vector outlines and add an *invisible* (`Tr 3`) text
                // layer on top for searching, copying and screen readers.
                // A run that went out as embedded-font text is already
                // searchable; only outlined runs need the invisible layer.
                let embedded = emit_text(
                    &mut ops,
                    text_fonts,
                    std::slice::from_ref(&wire.wire),
                    ox,
                    oy,
                    scale,
                    plot_style,
                    options,
                );
                if !embedded {
                    emit_searchable_text(
                        &mut ops,
                        std::slice::from_ref(&wire.wire),
                        ox,
                        oy,
                        clip,
                        plot_style,
                        options,
                        fonts,
                    );
                }
                last_color = None;
                last_lw = None;
                last_dash = None;
                continue;
            }
            DrawItem::Image(image) => {
                emit_image(doc, image_resources, &mut ops, image, ox, oy, options)?;
                continue;
            }
            DrawItem::Wire(wire) => wire,
        };
        let [mut r, mut g, mut b, a] = wire.color;
        if a < 0.01 {
            continue;
        }
        // Skip screen-only paper helpers. The PDF page supplies its own white
        // boundary, and the printable-area rectangle is a UI guide, not ink.
        if matches!(wire.name.as_str(), "__paper_boundary__" | "paper_printable_area") {
            continue;
        }
        // Apply CTB plot style table overrides (color + lineweight).
        let mut lw_override: Option<f32> = None;
        let mut screening = 1.0;
        let mut color_overridden = false;
        let mut cap = None;
        let mut join = None;
        if let Some(ctb) = plot_style {
            if wire.aci > 0 {
                if let Some([cr, cg, cb]) = ctb.resolve_color(wire.aci) {
                    r = cr;
                    g = cg;
                    b = cb;
                    color_overridden = true;
                }
                lw_override = ctb
                    .resolve_lineweight(wire.aci)
                    .map(|mm| (mm * MM_TO_PT).max(0.1));
                screening = ctb.resolve_screening(wire.aci);
                if let Some(entry) = ctb.aci_entries.get(wire.aci as usize) {
                    cap = match entry.end_style {
                        0 => Some(LineCapStyle::Butt),
                        1 | 3 => Some(LineCapStyle::ProjectingSquare),
                        2 => Some(LineCapStyle::Round),
                        _ => None,
                    };
                    join = match entry.join_style {
                        0 => Some(LineJoinStyle::Miter),
                        1 | 3 => Some(LineJoinStyle::Bevel),
                        2 => Some(LineJoinStyle::Round),
                        _ => None,
                    };
                }
            }
        }
        // Near-white and near-yellow (viewport active border) → dark grey for print
        // (only when no CTB override was applied).
        // `aci == 0` is an explicit true colour and plots as drawn, like the
        // hatch pass (#1621); only a pure white there is an unresolved colour 7.
        if !color_overridden && (wire.aci != 0 || [r, g, b] == [1.0; 3]) {
            // An authored white (not colour 7) plots as drawn, like on screen.
            let is_light = r > 0.80
                && g > 0.80
                && b > 0.80
                && !crate::scene::convert::tess_util::is_authored_white([r, g, b]);
            let is_yellow = r > 0.80 && g > 0.70 && b < 0.30;
            let is_cyan = r < 0.30 && g > 0.70 && b > 0.70;
            if is_light || is_yellow {
                r = 0.0;
                g = 0.0;
                b = 0.0;
            } else if is_cyan {
                // Viewport border: print as dark blue.
                r = 0.0;
                g = 0.15;
                b = 0.50;
            }
        }
        [r, g, b] = plotted_color([r, g, b], a, screening, options);

        let cap = cap.unwrap_or(LineCapStyle::Round);
        if last_cap != Some(cap) {
            ops.push(Op::SetLineCapStyle { cap });
            last_cap = Some(cap);
        }
        let join = join.unwrap_or(LineJoinStyle::Round);
        if last_join != Some(join) {
            ops.push(Op::SetLineJoinStyle { join });
            last_join = Some(join);
        }

        if last_color
            .map(|c| (c[0] - r).abs() > 0.01 || (c[1] - g).abs() > 0.01 || (c[2] - b).abs() > 0.01)
            .unwrap_or(true)
        {
            let color = Color::Rgb(Rgb {
                r,
                g,
                b,
                icc_profile: None,
            });
            ops.push(Op::SetOutlineColor {
                col: color.clone(),
            });
            ops.push(Op::SetFillColor { col: color });
            last_color = Some([r, g, b]);
        }

        // Line weight: style override or object weight. Normal output divides
        // by the page transform so physical pen widths stay constant; the
        // scale-lineweights option deliberately keeps the transformed width.
        //
        // A wide polyline is the exception: its band is a geometric width in
        // drawing units, so it must SCALE with the plot (no `/ scale`). Stroke
        // the centre-line at `world_width`, converted mm → pt exactly like the
        // geometry coordinates so the CTM scale renders the band at its true
        // size; the linetype dash pattern below then strokes it dashed. This
        // replaces the model-space hatch band that the shader-band change
        // dropped, and overrides any CTB pen weight (the width is geometry, not
        // a lineweight).
        let pen_divisor = if options.scale_lineweights {
            1.0
        } else {
            scale.max(1e-6)
        };
        let lw_pt = if wire.world_width > 0.0 {
            wire.world_width * MM_TO_PT
        } else {
            let physical = if options.object_lineweights {
                lw_override.unwrap_or_else(|| (wire.line_weight_px * LW_PX_TO_PT).max(0.1))
            } else {
                0.1
            };
            physical / pen_divisor
        };
        if last_lw.map(|l| (l - lw_pt).abs() > 0.01).unwrap_or(true) {
            ops.push(Op::SetOutlineThickness { pt: Pt(lw_pt) });
            last_lw = Some(lw_pt);
        }

        // Linetype dash pattern. Without this every wire exported as a solid
        // line regardless of its linetype (dashed / centre / dash-dot). (#155)
        let dash_arr = dash_array_from_pattern(wire.pattern_length, &wire.pattern, MM_TO_PT);
        let stationed =
            !dash_arr.is_empty() && wire.pattern_stations.len() > wire.points.len();
        if stationed {
            if last_dash.as_ref().is_none_or(|dash| !dash.is_empty()) {
                ops.push(Op::SetLineDashPattern {
                    dash: LineDashPattern::default(),
                });
                last_dash = Some(Vec::new());
            }
            for index in 0..wire.points.len().saturating_sub(1) {
                if !wire.points[index][0].is_finite()
                    || !wire.points[index + 1][0].is_finite()
                {
                    continue;
                }
                let start = wire.point_world(index, paper_h as f64 / scale.max(1e-6) as f64);
                let end = wire.point_world(index + 1, paper_h as f64 / scale.max(1e-6) as f64);
                for [from, to] in visible_station_ranges(
                    wire.pattern_stations[index],
                    wire.pattern_stations[index + 1],
                    wire.pattern_length,
                    &wire.pattern,
                ) {
                    let point = |t: f32| {
                        LinePoint {
                            p: Point::new(
                                Mm((start.x + (end.x - start.x) * t as f64 + ox) as f32),
                                Mm((start.y + (end.y - start.y) * t as f64 + oy) as f32),
                            ),
                            bezier: false,
                        }
                    };
                    flush_line(&mut ops, &[point(from), point(to)], None);
                }
            }
            continue;
        }
        if last_dash.as_deref() != Some(dash_arr.as_slice()) {
            let dash = if dash_arr.is_empty() {
                LineDashPattern::default()
            } else {
                LineDashPattern::from_array(&dash_arr, 0)
            };
            ops.push(Op::SetLineDashPattern { dash });
            last_dash = Some(dash_arr.clone());
        }

        // Emit segments (NaN = pen-up). Points are the "high" half of a
        // double-single pair; fold in the `points_low` residual and cancel the
        // offset in f64 before narrowing. Dropping the residual (or narrowing
        // first) snaps a UTM drawing onto the f32 grid — ~3 cm across, ~50 cm
        // along northing — which is exactly the distortion the plot showed while
        // low-coordinate drawings came out clean. The result is a sheet-mm value
        // in single digits, so f32 is lossless from here.
        let mut segment: Vec<LinePoint> = Vec::new();
        let dot_radius = (wire.name == "viewport_hatch_pattern")
            .then_some(Pt(SCREEN_DOT_MM * MM_TO_PT / (2.0 * scale.max(1e-6))));
        for (pi, &[x, y, _z]) in wire.points.iter().enumerate() {
            if x.is_nan() || y.is_nan() {
                flush_line(&mut ops, &segment, dot_radius);
                segment.clear();
            } else {
                let point = wire.point_world(pi, paper_h as f64 / scale.max(1e-6) as f64);
                let wx = (point.x + ox) as f32;
                let wy = (point.y + oy) as f32;
                segment.push(LinePoint {
                    p: Point::new(Mm(wx), Mm(wy)),
                    bezier: false,
                });
            }
        }
        flush_line(&mut ops, &segment, dot_radius);
    }
    }

    if needs_state {
        ops.push(Op::RestoreGraphicsState);
    }
    if options.merge_lines {
        ops.push(Op::RestoreGraphicsState);
    }
    if options.stamp {
        emit_plot_stamp(&mut ops);
    }

    let page = PdfPage::new(Mm(paper_w), Mm(paper_h), ops);
    doc.pages.push(page);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
/// Slack around the export window, in sheet mm, so a stroke whose centreline
/// sits just outside still contributes its width.
const CULL_MARGIN_MM: f64 = 10.0;

#[cfg(not(target_arch = "wasm32"))]
fn sheet_overlaps(b: [f64; 4], window: [f64; 4]) -> bool {
    b[0] <= window[2] + CULL_MARGIN_MM
        && b[2] >= window[0] - CULL_MARGIN_MM
        && b[1] <= window[3] + CULL_MARGIN_MM
        && b[3] >= window[1] - CULL_MARGIN_MM
}

#[cfg(not(target_arch = "wasm32"))]
fn grow(bounds: &mut Option<[f64; 4]>, x: f64, y: f64) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    let b = bounds.get_or_insert([x, y, x, y]);
    b[0] = b[0].min(x);
    b[1] = b[1].min(y);
    b[2] = b[2].max(x);
    b[3] = b[3].max(y);
}

#[cfg(not(target_arch = "wasm32"))]
/// Sheet-mm bounds of everything a wire inks (strokes, glyphs, fills).
fn wire_sheet_bounds(wire: &WireModel, ox: f64, oy: f64, marker_h: f64) -> Option<[f64; 4]> {
    let mut bounds = None;
    for (index, point) in wire.points.iter().enumerate() {
        if point[0].is_finite() && point[1].is_finite() {
            let world = wire.point_world(index, marker_h);
            grow(&mut bounds, world.x + ox, world.y + oy);
        }
    }
    for vertex in &wire.text_verts {
        let [x, y] = glyph_world_xy(vertex);
        grow(&mut bounds, x + ox, y + oy);
    }
    for (index, point) in wire.fill_tris.iter().enumerate() {
        let low = wire.fill_tris_low.get(index).copied().unwrap_or([0.0; 3]);
        grow(
            &mut bounds,
            point[0] as f64 + low[0] as f64 + ox,
            point[1] as f64 + low[1] as f64 + oy,
        );
    }
    bounds
}

#[cfg(not(target_arch = "wasm32"))]
/// Sheet-mm bounds of a hatch, resolved the way `emit_hatch` places it.
fn hatch_sheet_bounds(hatch: &HatchModel, ox: f64, oy: f64) -> Option<[f64; 4]> {
    let mut bounds = None;
    for &[x, y] in hatch.boundary.iter() {
        grow(
            &mut bounds,
            x as f64 + hatch.world_origin[0] + ox,
            y as f64 + hatch.world_origin[1] + oy,
        );
    }
    bounds
}

/// Build a PDF dash array (in points) from a WireModel linetype pattern.
///
/// `pattern` holds the linetype run lengths in paper-mm: positive = dash,
/// negative = gap, exactly 0 = a dot, and trailing zeros are padding — so the
/// real length is the index of the last non-zero element + 1 (same convention
/// the wire shader uses). Returns an empty vec for a solid line. printpdf's
/// `LineDashPattern` holds at most six entries, so longer patterns are
/// truncated to three dash/gap pairs.
#[cfg(not(target_arch = "wasm32"))]
fn dash_array_from_pattern(pattern_length: f32, pattern: &[f32; 8], mm_to_pt: f32) -> Vec<i64> {
    if pattern_length <= 1e-6 {
        return Vec::new();
    }
    let count = match pattern.iter().rposition(|&v| v != 0.0) {
        Some(i) => (i + 1).min(6),
        None => return Vec::new(),
    };
    pattern[..count]
        .iter()
        // Round to whole points (printpdf dash entries are integers) and keep a
        // 1 pt floor so a zero-length dot still prints as a short mark.
        .map(|&v| (((v.abs() * mm_to_pt).round()) as i64).max(1))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn visible_station_ranges(
    start: f32,
    end: f32,
    pattern_length: f32,
    pattern: &[f32; 8],
) -> Vec<[f32; 2]> {
    let count = pattern.iter().rposition(|value| *value != 0.0).map_or(0, |i| i + 1);
    if count == 0 || pattern_length <= 1e-6 {
        return vec![[0.0, 1.0]];
    }
    let dot = 1.0 / MM_TO_PT;
    let mut elements: Vec<(f32, bool)> = pattern[..count]
        .iter()
        .map(|value| (if *value == 0.0 { dot } else { value.abs() }, *value >= 0.0))
        .collect();
    let total: f32 = elements.iter().map(|(length, _)| *length).sum();
    if total <= 1e-6 {
        return vec![[0.0, 1.0]];
    }
    let factor = pattern_length / total;
    for (length, _) in &mut elements {
        *length *= factor;
    }
    let delta = end - start;
    let state = |station: f32, forward: bool| {
        let mut phase = station.rem_euclid(pattern_length);
        if !forward && phase <= 1e-6 {
            phase = pattern_length;
        }
        let mut offset = 0.0;
        if forward {
            for &(length, drawn) in &elements {
                let end = offset + length;
                if phase < end - 1e-6 {
                    return (drawn, end - phase);
                }
                offset = end;
            }
            (elements[0].1, elements[0].0)
        } else {
            offset = pattern_length;
            for &(length, drawn) in elements.iter().rev() {
                offset -= length;
                if phase > offset + 1e-6 {
                    return (drawn, phase - offset);
                }
            }
            let last = elements[elements.len() - 1];
            (last.1, last.0)
        }
    };
    if delta.abs() <= 1e-6 {
        return state(start, true).0.then_some([0.0, 1.0]).into_iter().collect();
    }

    let mut ranges = Vec::new();
    let mut t = 0.0;
    while t < 1.0 - 1e-6 {
        let station = start + delta * t;
        let (drawn, remaining) = state(station, delta > 0.0);
        let next = (t + remaining / delta.abs()).clamp(t + 1e-6, 1.0);
        if drawn {
            ranges.push([t, next]);
        }
        t = next;
    }
    ranges
}

#[cfg(not(target_arch = "wasm32"))]
fn flush_line(ops: &mut Vec<Op>, pts: &[LinePoint], dot_radius: Option<Pt>) {
    if pts.len() < 2 {
        return;
    }
    if let Some(radius) = dot_radius {
        let first = pts[0].p;
        let coincident = pts.iter().skip(1).all(|point| {
            (point.p.x.0 - first.x.0).abs() <= 1e-6
                && (point.p.y.0 - first.y.0).abs() <= 1e-6
        });
        if coincident {
            emit_round_dot(ops, first, radius);
            return;
        }
    }
    ops.push(Op::DrawLine {
        line: Line {
            points: pts.to_vec(),
            is_closed: false,
        },
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_round_dot(ops: &mut Vec<Op>, center: Point, radius: Pt) {
    const SIDES: usize = 12;
    let points = (0..SIDES)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / SIDES as f32;
            LinePoint {
                p: Point {
                    x: Pt(center.x.0 + radius.0 * angle.cos()),
                    y: Pt(center.y.0 + radius.0 * angle.sin()),
                },
                bezier: false,
            }
        })
        .collect();
    ops.push(Op::DrawPolygon {
        polygon: Polygon {
            rings: vec![PolygonRing { points }],
            mode: PaintMode::Fill,
            winding_order: WindingOrder::NonZero,
        },
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn plotted_color(
    rgb: [f32; 3],
    alpha: f32,
    screening: f32,
    options: PdfPlotOptions,
) -> [f32; 3] {
    let amount = screening.clamp(0.0, 1.0)
        * if options.transparency {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
    [
        1.0 - (1.0 - rgb[0]) * amount,
        1.0 - (1.0 - rgb[1]) * amount,
        1.0 - (1.0 - rgb[2]) * amount,
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_wire_fills(
    ops: &mut Vec<Op>,
    wires: &[WireModel],
    wire_depth: f32,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    scale: f32,
    options: PdfPlotOptions,
    normal_blend: Option<&ExtendedGraphicsStateId>,
) {
    for wire in wires {
        if wire.fill_tris.is_empty() {
            continue;
        }
        let styled_pattern = plot_style.and_then(|table| {
            (wire.aci > 0)
                .then(|| table.aci_entries.get(wire.aci as usize))
                .flatten()
                .and_then(|entry| {
                    (65..=72)
                        .contains(&entry.fill_style)
                        .then(|| {
                            crate::scene::model::hatch_model::plot_style_fill_pattern(
                                entry.fill_style,
                            )
                        })
                        .flatten()
                })
        });
        if let Some(pattern) = styled_pattern {
            for (triangle_index, triangle) in wire.fill_tris.chunks_exact(3).enumerate() {
                let mut boundary = Vec::with_capacity(4);
                for (point_index, point) in triangle.iter().enumerate() {
                    let index = triangle_index * 3 + point_index;
                    let low = wire.fill_tris_low.get(index).copied().unwrap_or([0.0; 3]);
                    boundary.push([point[0] + low[0], point[1] + low[1]]);
                }
                boundary.push(boundary[0]);
                let hatch = HatchModel {
                    pattern_origin: None,
                    render_instance: wire.render_instance.clone(),
                    world_origin: [0.0, 0.0],
                    boundary: std::sync::Arc::new(boundary),
                    boundary_wcs: None,
                    fill_plane: None,
                    fill_plane_boundary: None,
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: codec::entities::HatchStyleType::Normal,
                    pattern: pattern.clone(),
                    name: "PLOTSTYLE".to_string(),
                    color: wire.color,
                    aci: wire.aci,
                    line_weight_px: wire.line_weight_px,
                    angle_offset: 0.0,
                    scale: 1.0 / scale.max(1.0e-6),
                    // The host wire's composed draw depth (PlotWire carries
                    // wire_draw_depth). depth_override alone is a per-block
                    // child label and would sort the fill outside its block's
                    // band; keep the pattern fill co-sorted with its wire.
                    draw_depth: wire_depth,
                };
                emit_hatch(
                    ops,
                    &hatch,
                    ox,
                    oy,
                    plot_style,
                    scale,
                    options,
                    normal_blend,
                );
            }
            continue;
        }
        let [mut r, mut g, mut b, a] = wire.color;
        if a < 0.01 {
            continue;
        }
        if wire.bg_adapt.as_deref().is_some_and(|adapt| adapt.canvas_color) {
            // A background mask: it covers with the paper itself.
            [r, g, b] = [1.0, 1.0, 1.0];
        } else {
            let mut screening = 1.0;
            let mut color_overridden = false;
            if let Some(table) = plot_style {
                if wire.aci > 0 {
                    if let Some(color) = table.resolve_color(wire.aci) {
                        [r, g, b] = color;
                        color_overridden = true;
                    }
                    screening = table.resolve_screening(wire.aci);
                }
            }
            if !color_overridden {
                [r, g, b] = adapt_text_color([r, g, b]);
            }
            [r, g, b] = plotted_color([r, g, b], a, screening, options);
        }
        ops.push(Op::SetFillColor {
            col: Color::Rgb(Rgb {
                r,
                g,
                b,
                icc_profile: None,
            }),
        });
        for (triangle_index, triangle) in wire.fill_tris.chunks_exact(3).enumerate() {
            let mut points = Vec::with_capacity(3);
            for (point_index, &[x, y, _]) in triangle.iter().enumerate() {
                let index = triangle_index * 3 + point_index;
                let low = wire.fill_tris_low.get(index).copied().unwrap_or([0.0; 3]);
                points.push(LinePoint {
                    p: Point::new(
                        Mm((x as f64 + low[0] as f64 + ox) as f32),
                        Mm((y as f64 + low[1] as f64 + oy) as f32),
                    ),
                    bezier: false,
                });
            }
            ops.push(Op::DrawPolygon {
                polygon: Polygon {
                    rings: vec![PolygonRing { points }],
                    mode: PaintMode::Fill,
                    winding_order: WindingOrder::NonZero,
                },
            });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_plot_stamp(ops: &mut Vec<Op>) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    let label = format!("Open CAD Studio | {user} | {timestamp}");
    ops.extend([
        Op::SaveGraphicsState,
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point::new(Mm(4.0), Mm(3.0)),
        },
        Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            size: Pt(6.0),
        },
        Op::SetFillColor {
            col: Color::Rgb(Rgb {
                r: 0.25,
                g: 0.25,
                b: 0.25,
                icc_profile: None,
            }),
        },
        Op::ShowText {
            items: vec![TextItem::Text(label)],
        },
        Op::EndTextSection,
        Op::RestoreGraphicsState,
    ]);
}

/// Emit a single hatch / wipeout as a filled (or stroked, for pattern fills)
/// polygon. NaN sentinels in `hatch.boundary` split the path into multiple
/// rings so islands and holes render correctly under the even-odd rule.
/// Mirrors `scene::paper_canvas::draw_hatch`: solid → fill, pattern → outline,
/// gradient → solid fill of the averaged colour.
#[cfg(not(target_arch = "wasm32"))]
fn emit_hatch(
    ops: &mut Vec<Op>,
    hatch: &HatchModel,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    scale: f32,
    options: PdfPlotOptions,
    normal_blend: Option<&ExtendedGraphicsStateId>,
) {
    if hatch.boundary.is_empty() {
        return;
    }
    let mut styled_hatch = None;
    if let Some(table) = plot_style {
        if hatch.aci > 0 && matches!(hatch.pattern, HatchPattern::Solid) {
            if let Some(pattern) = table
                .aci_entries
                .get(hatch.aci as usize)
                .and_then(|entry| {
                    crate::scene::model::hatch_model::plot_style_fill_pattern(
                        entry.fill_style,
                    )
                })
            {
                let mut model = hatch.clone();
                model.pattern = pattern;
                model.scale = 1.0 / scale.max(1.0e-6);
                styled_hatch = Some(model);
            }
        }
    }
    let hatch = styled_hatch.as_ref().unwrap_or(hatch);
    let [mut r, mut g, mut b, a] = hatch.color;
    if a < 0.01 {
        return;
    }
    // Adapt hatch fills to the white sheet, mirroring the wire pass: colours
    // arrive adapted to the (dark) screen background, so a white/ACI-7 fill
    // would vanish white-on-white on paper. Force near-white/near-yellow → black
    // and near-cyan → dark blue for a readable white-sheet result.
    // Genuine colours are untouched; WIPEOUTS keep their paper-white mask.
    let is_wipeout = hatch.name == "WIPEOUT_FILL";
    let mut screening = 1.0;
    let mut lw_override = None;
    let mut color_overridden = false;
    if !is_wipeout {
        if let Some(table) = plot_style {
            if hatch.aci > 0 {
                if let Some([cr, cg, cb]) = table.resolve_color(hatch.aci) {
                    r = cr;
                    g = cg;
                    b = cb;
                    color_overridden = true;
                }
                screening = table.resolve_screening(hatch.aci);
                lw_override = table
                    .resolve_lineweight(hatch.aci)
                    .map(|mm| (mm * MM_TO_PT).max(0.1));
            }
        }
    }
    if is_wipeout {
        // Screen wipeouts match the configured canvas colour; printed
        // wipeouts must mask with the white paper colour.
        r = 1.0;
        g = 1.0;
        b = 1.0;
    } else if !color_overridden
        // `aci == 0` is an explicit true colour: it plots as drawn, only
        // indexed colours meant for the dark screen are adapted. (#1417)
        && hatch.aci != 0
        && !(hatch.aci == 7 && matches!(hatch.pattern, HatchPattern::Solid))
    {
        // An authored white (not colour 7) plots as drawn, like on screen.
        let is_light = r > 0.80
            && g > 0.80
            && b > 0.80
            && !crate::scene::convert::tess_util::is_authored_white([r, g, b]);
        let is_yellow = r > 0.80 && g > 0.70 && b < 0.30;
        let is_cyan = r < 0.30 && g > 0.70 && b > 0.70;
        if is_light || is_yellow {
            r = 0.0;
            g = 0.0;
            b = 0.0;
        } else if is_cyan {
            r = 0.0;
            g = 0.15;
            b = 0.50;
        }
    }
    [r, g, b] = plotted_color([r, g, b], a, screening, options);
    // `boundary` holds f32 offsets from the f64 `world_origin`, so resolve the
    // pair in f64 and only narrow once the offset has cancelled — casting
    // `world_origin` to f32 first re-introduces the ~0.5 m UTM quantisation the
    // boundary-relative encoding exists to avoid.
    let (world_ox, world_oy) = (hatch.world_origin[0], hatch.world_origin[1]);

    // Split the boundary into rings on every NaN-NaN separator.
    let mut rings: Vec<PolygonRing> = Vec::new();
    let mut current: Vec<LinePoint> = Vec::new();
    for &[bx, by] in hatch.boundary.iter() {
        if bx.is_nan() || by.is_nan() {
            if current.len() >= 3 {
                rings.push(PolygonRing { points: std::mem::take(&mut current) });
            } else {
                current.clear();
            }
            continue;
        }
        let px = (bx as f64 + world_ox + ox) as f32;
        let py = (by as f64 + world_oy + oy) as f32;
        current.push(LinePoint {
            p: Point::new(Mm(px), Mm(py)),
            bezier: false,
        });
    }
    if current.len() >= 3 {
        rings.push(PolygonRing { points: current });
    }
    if rings.is_empty() {
        return;
    }

    let (paint_mode, fill_color) = match &hatch.pattern {
        HatchPattern::Solid => (PaintMode::Fill, [r, g, b]),
        HatchPattern::Pattern(_) => {
            // Pattern fills are emitted as raster line segments below; the
            // outline polygon path itself is skipped because pattern
            // hatches in real DXF do not draw their boundary as part of
            // the fill.
            (PaintMode::Clip, [r, g, b]) // sentinel — handled below
        }
        HatchPattern::Gradient { color2, .. } => {
            // PDF gradients are stored in resource dictionaries; for the
            // fast path we average the two colours, matching paper_canvas.
            let second = if color_overridden {
                [r, g, b]
            } else {
                plotted_color(
                    adapt_text_color([color2[0], color2[1], color2[2]]),
                    color2[3],
                    screening,
                    options,
                )
            };
            let avg = [
                (r + second[0]) * 0.5,
                (g + second[1]) * 0.5,
                (b + second[2]) * 0.5,
            ];
            (PaintMode::Fill, avg)
        }
    };

    // Pattern hatches: rasterise the family lines clipped to the boundary
    // and emit each as a stroked line. Skips the polygon outline entirely.
    if matches!(hatch.pattern, HatchPattern::Pattern(_)) {
        let physical = if options.object_lineweights {
            lw_override.unwrap_or_else(|| (hatch.line_weight_px * LW_PX_TO_PT).max(0.1))
        } else {
            0.1
        };
        let divisor = if options.scale_lineweights {
            1.0
        } else {
            scale.max(1e-6)
        };
        let segments = hatch.pattern_segments_for_plot();
        if segments.is_empty() {
            return;
        }
        let color = Color::Rgb(Rgb {
            r,
            g,
            b,
            icc_profile: None,
        });
        ops.push(Op::SetOutlineColor {
            col: color.clone(),
        });
        ops.push(Op::SetFillColor { col: color });
        ops.push(Op::SetOutlineThickness {
            pt: Pt(physical / divisor),
        });
        // Pattern dashes are already materialized by `pattern_segments`.
        // Clear any linetype left by the preceding paper/model render group.
        ops.push(Op::SetLineDashPattern {
            dash: LineDashPattern::default(),
        });
        for [a, b_pt] in segments {
            // `pattern_segments` returns absolute world f64; cancel the offset
            // before narrowing, as everywhere else in this file.
            let (ax, ay) = ((a[0] + ox) as f32, (a[1] + oy) as f32);
            let (bx, by) = ((b_pt[0] + ox) as f32, (b_pt[1] + oy) as f32);
            let points = vec![
                LinePoint {
                    p: Point::new(Mm(ax), Mm(ay)),
                    bezier: false,
                },
                LinePoint {
                    p: Point::new(Mm(bx), Mm(by)),
                    bezier: false,
                },
            ];
            let dot_radius = Pt(SCREEN_DOT_MM * MM_TO_PT / (2.0 * scale.max(1e-6)));
            flush_line(ops, &points, Some(dot_radius));
        }
        return;
    }

    // Solid / gradient: filled polygon path.
    if matches!(paint_mode, PaintMode::Fill | PaintMode::FillStroke) {
        ops.push(Op::SetFillColor {
            col: Color::Rgb(Rgb {
                r: fill_color[0],
                g: fill_color[1],
                b: fill_color[2],
                icc_profile: None,
            }),
        });
    }
    if is_wipeout {
        if let Some(gs) = normal_blend {
            ops.push(Op::SaveGraphicsState);
            ops.push(Op::LoadGraphicsState { gs: gs.clone() });
        }
    }
    ops.push(Op::DrawPolygon {
        polygon: Polygon {
            rings,
            mode: paint_mode,
            winding_order: WindingOrder::EvenOdd,
        },
    });
    if is_wipeout && normal_blend.is_some() {
        ops.push(Op::RestoreGraphicsState);
    }
}

// ── Text (SDF glyph quads → vector strokes / fills) ────────────────────────

/// Absolute world XY of a glyph vertex (double-single high + low parts folded).
///
/// The fold must happen in f64: the pair exists because the absolute coordinate
/// does not fit an f32, so `pos + pos_low` evaluated in f32 rounds straight back
/// to `pos` and throws away the residual it was carrying.
#[cfg(not(target_arch = "wasm32"))]
fn glyph_world_xy(v: &crate::scene::pipeline::text_gpu::TextVertex) -> [f64; 2] {
    [
        v.pos[0] as f64 + v.pos_low[0] as f64,
        v.pos[1] as f64 + v.pos_low[1] as f64,
    ]
}

/// Adapt a text colour to the white sheet, mirroring the wire/hatch passes:
/// near-white / near-yellow (colour-7-on-white) → black, near-cyan → dark blue.
#[cfg(not(target_arch = "wasm32"))]
fn adapt_text_color([r, g, b]: [f32; 3]) -> [f32; 3] {
    // An authored white (not colour 7) plots as drawn, like on screen.
    let is_light = r > 0.80
        && g > 0.80
        && b > 0.80
        && !crate::scene::convert::tess_util::is_authored_white([r, g, b]);
    let is_yellow = r > 0.80 && g > 0.70 && b < 0.30;
    let is_cyan = r < 0.30 && g > 0.70 && b > 0.70;
    if is_light || is_yellow {
        [0.0, 0.0, 0.0]
    } else if is_cyan {
        [0.0, 0.15, 0.50]
    } else {
        [r, g, b]
    }
}

/// Invisible searchable text layer for laid-out runs (`Tr 3`).
///
/// Each `SearchableTextRun` carries the visible string plus its final
/// world-space baseline origin, cap height, advance width and rotation — all
/// baked by the layout stage (MTEXT `\P` newlines arrive as separate per-line
/// runs, `attachment_point`/width wrapping and block placement already
/// applied), so the exporter only places a `BT … Tj ET` with an
/// arbitrary-rotation text matrix (`Tm`), honouring the MTEXT traps by
/// construction.
///
/// Design (review V2): the vector outlines are **always** drawn — the plot
/// looks pixel-identical with or without this layer — and the runs add an
/// invisible (`Tr 3`) text layer for searching, copying and screen readers.
/// A gap here can only affect extractability, never the rendered sheet.
/// Decorations (underline/overline/strike), stroke fonts, shaped and mixed
/// scripts keep working because their outlines are untouched.
///
/// Font choice per run (review V3 — "why Helvetica, not the DWG font?"):
/// - A run whose style resolves to an installed TrueType family is set in a
///   **subset CIDFontType2 of that exact family** (+ `/ToUnicode`), cut down
///   by `allsorts` to the glyphs the plot uses. Bold runs resolve the bold
///   face via fontdb weight matching.
/// - SHX/LFF stroke fonts have no embeddable font program (there is no TrueType
///   behind `txt`/`romans` — only pen strokes), unresolvable names and CFF/
///   collection edge cases fall back to Base-14 Helvetica/Helvetica-Bold.
///   A Type3 font synthesized from the SDF atlas would cover those exactly and
///   is the natural follow-up; it needs no new layout data.
/// - `Tf` takes an *em* size but `run.height` is a *cap* height: the em size
///   is `height / cap_ratio` (per-font OS/2 sCapHeight, 0.72 for Helvetica) so
///   the invisible run aligns with the drawn cap height.
/// - Width-matching (`Tz`, baked into the `Tm` x-basis — equivalent, and the
///   only horizontal scaling printpdf exposes): `k = laid_advance /
///   natural_advance` from the run's stored pen advance and the font's hmtx
///   advances, so centered/right-aligned runs highlight where they draw.
/// - Scaling: the font size is converted world-mm → points via `MM_TO_PT`;
///   the page CTM (plot scale/rotation/clip) applies on top inside the same
///   graphics state as the wire geometry, so text and outlines scale together.
/// - Runs fully outside the export-window clip are skipped (rect overlap on
///   the run rect — same space as the clip polygon).
///
/// Returns the number of runs emitted.
#[cfg(not(target_arch = "wasm32"))]
fn emit_searchable_text(
    ops: &mut Vec<Op>,
    wires: &[WireModel],
    ox: f64,
    oy: f64,
    clip: Option<(f32, f32, f32, f32)>,
    plot_style: Option<&PlotStyleTable>,
    options: PdfPlotOptions,
    fonts: &SearchableFontSet,
) -> usize {
    use crate::scene::model::wire_model::run_rect_overlap;
    /// Helvetica cap height ≈ 0.72 em.
    const HELVETICA_CAP_RATIO: f32 = 0.72;
    let mut emitted = 0;
    for wire in wires {
        if wire.searchable_text.is_empty() {
            continue;
        }
        let mut ctb_color: Option<[f32; 3]> = None;
        let mut screening = 1.0;
        if let Some(ctb) = plot_style {
            if wire.aci > 0 {
                ctb_color = ctb.resolve_color(wire.aci);
                screening = ctb.resolve_screening(wire.aci);
            }
        }
        for run in &wire.searchable_text {
            if run.text.is_empty() || run.height <= 0.0 || !run.height.is_finite() {
                continue;
            }
            // Export-window clip culls by run rect (origin + advance × cap
            // height box); the survivors inherit the clip path anyway.
            if let Some((cx, cy, cw, ch)) = clip {
                let px = run.origin[0] + ox;
                let py = run.origin[1] + oy;
                if !run_rect_overlap(
                    [px, py],
                    run.rotation,
                    run.adv_width as f64,
                    run.height as f64,
                    [cx as f64, cy as f64, (cx + cw) as f64, (cy + ch) as f64],
                ) {
                    continue;
                }
            }
            // Per-run font choice: the DWG style's own family when a subset
            // is embedded, else the WinAnsi-gated Helvetica fallback.
            enum RunFont<'a> {
                Subset(&'a EmbeddedSubset),
                Helvetica,
            }
            let choice = match crate::scene::text::font_face::Face::resolve(&run.font) {
                crate::scene::text::font_face::Face::Ttf { family, .. } => fonts
                    .find(&family, run.bold)
                    .map(RunFont::Subset)
                    .unwrap_or(RunFont::Helvetica),
                _ => RunFont::Helvetica,
            };
            if matches!(choice, RunFont::Helvetica) && !is_winansi_encodable(&run.text) {
                continue;
            }
            let rgb = ctb_color.unwrap_or_else(|| adapt_text_color([run.color[0], run.color[1], run.color[2]]));
            let [r, g, b] = plotted_color(rgb, run.color[3], screening, options);
            // World mm → page points; the page CTM (plot scale/rotation/clip)
            // applies on top, exactly as for the wire geometry.
            let x_pt = ((run.origin[0] + ox) * MM_TO_PT as f64) as f32;
            let y_pt = ((run.origin[1] + oy) * MM_TO_PT as f64) as f32;
            if !x_pt.is_finite() || !y_pt.is_finite() {
                continue;
            }
            // Em size from cap height, and width-match factor baked into the
            // Tm x-basis (= Tz, the only horizontal scaling printpdf exposes).
            let (font_handle, size_pt, width_k) = match choice {
                RunFont::Subset(sub) => {
                    let size = (run.height * MM_TO_PT / sub.cap_ratio).max(0.1);
                    (PdfFontHandle::External(sub.font_id.clone()), size, sub.width_factor(run))
                }
                RunFont::Helvetica => {
                    let size = (run.height * MM_TO_PT / HELVETICA_CAP_RATIO).max(0.1);
                    let handle = PdfFontHandle::Builtin(if run.bold {
                        BuiltinFont::HelveticaBold
                    } else {
                        BuiltinFont::Helvetica
                    });
                    (handle, size, 1.0)
                }
            };
            if !size_pt.is_finite() {
                continue;
            }
            let (sin_r, cos_r) = run.rotation.sin_cos();
            ops.push(Op::StartTextSection);
            ops.push(Op::SetFont { font: font_handle, size: Pt(size_pt) });
            ops.push(Op::SetTextRenderingMode {
                mode: TextRenderingMode::Invisible,
            });
            ops.push(Op::SetTextMatrix {
                matrix: printpdf::TextMatrix::Raw([
                    cos_r * width_k,
                    sin_r * width_k,
                    -sin_r,
                    cos_r,
                    x_pt,
                    y_pt,
                ]),
            });
            ops.push(Op::SetFillColor {
                col: Color::Rgb(Rgb { r, g, b, icc_profile: None }),
            });
            ops.push(Op::ShowText {
                items: vec![TextItem::Text(run.text.clone())],
            });
            ops.push(Op::EndTextSection);
            emitted += 1;
        }
    }
    emitted
}

/// Whether every char in `text` survives Base-14 WinAnsi encoding.
///
/// ASCII + Latin-1 incl. French accents and the `°±Ø` DXF specials — but NOT
/// the C1 control range U+0080–U+009F (unassigned in WinAnsi; emitting them
/// would mojibake). Anything else is rejected per-run so the exporter never
/// emits a wrong `Tj` — those runs keep their outlines and stay unsearchable.
/// (Runs set in an embedded subset bypass this gate: Identity-H covers any
/// glyph the subset carries.)
#[cfg(not(target_arch = "wasm32"))]
fn is_winansi_encodable(text: &str) -> bool {
    text.chars().all(|c| {
        let cp = c as u32;
        cp <= 0xFF && !(0x80..=0x9F).contains(&cp)
    })
}

// ── Embedded subset fonts (one system family × weight, cut to used glyphs) ──

/// One embedded subset TrueType font for the search layer: a single system
/// family at one weight, cut by `allsorts` to the glyphs the plot uses.
#[cfg(not(target_arch = "wasm32"))]
struct EmbeddedSubset {
    family: String,
    bold: bool,
    font_id: printpdf::FontId,
    /// Subset-space GID per char, re-parsed from the subset bytes (never
    /// assumed from allsorts' assignment order).
    gid_of: std::collections::BTreeMap<char, u16>,
    /// Raw advance per subset GID, in font units (hmtx, for Tz).
    width_of: std::collections::BTreeMap<u16, u16>,
    upem: u16,
    /// Cap-height fraction of em (OS/2 sCapHeight, else 0.7).
    cap_ratio: f32,
}

#[cfg(not(target_arch = "wasm32"))]
impl EmbeddedSubset {
    /// Width-match factor for a run: laid advance ÷ natural advance at the
    /// run's cap height. 1.0 when the widths are unknown (never explodes a
    /// selection highlight: clamped to a sane band).
    fn width_factor(&self, run: &SearchableTextRun) -> f32 {
        let natural: u32 = run
            .text
            .chars()
            .filter_map(|c| self.gid_of.get(&c))
            .filter_map(|g| self.width_of.get(g))
            .map(|w| *w as u32)
            .sum();
        if natural == 0 || run.adv_width <= 0.0 || run.height <= 0.0 {
            return 1.0;
        }
        // laid (drawing mm) vs natural at cap height, both in em fractions:
        // k = adv / (height * Σw / (upem * cap)).
        let k = run.adv_width * self.upem as f32 * self.cap_ratio
            / (run.height * natural as f32);
        if !k.is_finite() {
            1.0
        } else {
            k.clamp(0.25, 4.0)
        }
    }
}

/// Embedded subsets for one export (Helvetica fallback needs no entry).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SearchableFontSet {
    subsets: Vec<EmbeddedSubset>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SearchableFontSet {
    /// Collect every TTF-resolvable run's chars per (family, bold), subset
    /// each bucket with `allsorts`, and register the subsets on the document.
    /// Anything unresolvable (SHX/LFF names, big-font pairs, CFF outlines,
    /// missing files) simply gets no entry — those runs take Helvetica.
    fn build(doc: &mut PdfDocument, pages: &[PdfPageInput]) -> Self {
        use std::collections::{BTreeMap, BTreeSet};
        let mut buckets: BTreeMap<(String, bool), BTreeSet<char>> = BTreeMap::new();
        for page in pages {
            for wire in page.content.wires.iter() {
                for run in &wire.searchable_text {
                    if run.text.is_empty() {
                        continue;
                    }
                    if let crate::scene::text::font_face::Face::Ttf { family, .. } =
                        crate::scene::text::font_face::Face::resolve(&run.font)
                    {
                        buckets
                            .entry((family, run.bold))
                            .or_default()
                            .extend(run.text.chars());
                    }
                }
            }
        }
        let mut out = Self::default();
        for ((family, bold), chars) in &buckets {
            if let Some(subset) = subset_family(doc, family, *bold, chars) {
                out.subsets.push(subset);
            }
        }
        out
    }

    fn find(&self, family: &str, bold: bool) -> Option<&EmbeddedSubset> {
        self.subsets
            .iter()
            .find(|s| s.family == family && s.bold == bold)
    }
}

/// Subset one system family to `chars`, embed the subset, and register it.
/// Returns `None` whenever embedding is impossible or pointless (no file, CFF
/// outlines the stub can't describe, subset failure, nothing mappable) — the
/// caller falls back to Helvetica, never to an invalid font dict.
#[cfg(not(target_arch = "wasm32"))]
fn subset_family(
    doc: &mut PdfDocument,
    family: &str,
    bold: bool,
    chars: &std::collections::BTreeSet<char>,
) -> Option<EmbeddedSubset> {
    use allsorts::{binary::read::ReadScope, font_data::FontData, tables::FontTableProvider};
    let weight = if bold {
        fontdb::Weight::BOLD
    } else {
        fontdb::Weight::NORMAL
    };
    let (bytes, index) = crate::scene::text::sysfont::with_face_data_weighted(
        family,
        weight,
        |data, index| (data.to_vec(), index),
    )?;
    // Original GIDs for the used chars (+ .notdef, mandatory for subsetting).
    let orig = ttf_parser::Face::parse(&bytes, index).ok()?;
    let mut ids: Vec<u16> = vec![0];
    for c in chars {
        if let Some(gid) = orig.glyph_index(*c) {
            if !ids.contains(&gid.0) {
                ids.push(gid.0);
            }
        }
    }
    if ids.len() <= 1 {
        return None;
    }
    let scope = ReadScope::new(&bytes);
    let font_file = scope.read::<FontData<'_>>().ok()?;
    let provider = font_file.table_provider(index as usize).ok()?;
    // The no-text_layout ParsedFont stub can only describe TrueType
    // (CIDFontType2 + FontFile2) — never mislabel CFF outlines as TrueType.
    if provider.has_table(allsorts::tag::CFF) || provider.has_table(allsorts::tag::CFF2) {
        return None;
    }
    let subset_bytes = allsorts::subset::subset(
        &provider,
        &ids,
        &allsorts::subset::SubsetProfile::Pdf,
        allsorts::subset::CmapTarget::Unicode,
    )
    .ok()?;
    // Re-parse the SUBSET: GID assignment is allsorts' business, and the maps
    // below must be in subset space. Anything unmappable stays out.
    let sub = ttf_parser::Face::parse(&subset_bytes, 0).ok()?;
    let upem = sub.units_per_em().max(1);
    let cap_ratio = sub
        .capital_height()
        .filter(|c| *c > 0)
        .map(|c| c as f32 / upem as f32)
        .filter(|r| (0.2..=1.0).contains(r))
        .unwrap_or(0.7);
    let mut gid_chars: std::collections::BTreeMap<char, u16> = std::collections::BTreeMap::new();
    let mut width_of: std::collections::BTreeMap<u16, u16> = std::collections::BTreeMap::new();
    for c in chars {
        if let Some(gid) = sub.glyph_index(*c) {
            gid_chars.insert(*c, gid.0);
            width_of.insert(gid.0, sub.glyph_hor_advance(gid).unwrap_or(0));
        }
    }
    if gid_chars.is_empty() {
        return None;
    }
    // printpdf (no text_layout) embeds `original_bytes` verbatim as the
    // CIDFontType2 stream and derives ToUnicode/widths from these maps — so
    // handing it the SUBSET bytes + subset-space maps yields a true subset
    // embed with no lopdf surgery. (Metrics out before the bytes move.)
    let (ascent, descent) = (sub.ascender(), sub.descender());
    let cp_map: std::collections::BTreeMap<u32, u16> =
        gid_chars.iter().map(|(c, g)| (*c as u32, *g)).collect();
    let parsed = printpdf::ParsedFont::with_glyph_data(
        subset_bytes,
        0,
        Some(family.to_string()),
        cp_map,
        width_of.clone(),
        upem,
        printpdf::FontMetrics { ascent, descent },
    );
    let font_id = doc.add_font(&parsed);
    Some(EmbeddedSubset {
        family: family.to_string(),
        bold,
        font_id,
        gid_of: gid_chars,
        width_of,
        upem,
        cap_ratio,
    })
}

/// Re-emit every wire's SDF text as vector geometry.
///
/// Each visible glyph rides on `wire.text_verts` as one 6-vertex quad (two
/// triangles) whose corners are the glyph's atlas `plane` rect run through the
/// text transform. We recover the glyph's outline / fill from the atlas by the
/// quad's `uv_min` and map it into that quad by affine interpolation of the
/// plane rect — so a stroke (LFF) font emits polylines and a filled TrueType
/// glyph emits filled triangles, exactly where the SDF quad sits.
///
/// Always drawn: the visible glyph rendering (strokes for LFF/SHX pen fonts,
/// fills for TrueType outlines, solid bars for decorations). The invisible
/// searchable layer above adds extractability; it never replaces this.
#[cfg(not(target_arch = "wasm32"))]
fn emit_text(
    ops: &mut Vec<Op>,
    fonts: &mut TextFonts,
    wires: &[WireModel],
    ox: f64,
    oy: f64,
    scale: f32,
    plot_style: Option<&PlotStyleTable>,
    options: PdfPlotOptions,
) -> bool {
    use crate::scene::text::sdf_atlas;

    if wires.iter().all(|w| w.text_verts.is_empty()) {
        return false;
    }
    // The atlas' baked-glyph geometry, snapshotted once per export.
    let Some(snapshot) = fonts.glyph_table() else {
        return false;
    };
    let (table, solid_key) = (&snapshot.0, snapshot.1);

    // `Op::SetLineDashPattern` is persistent graphics state and the wire pass
    // above only re-emits it on change, so whatever the last wire needed is
    // still active here — without this reset a drawing whose last wire carries a
    // HIDDEN/CENTER linetype prints its glyph outlines dashed.
    ops.push(Op::SetLineDashPattern {
        dash: LineDashPattern::default(),
    });

    let mut embedded = true;
    for wire in wires {
        let verts = &wire.text_verts;
        if verts.is_empty() {
            embedded = false;
            continue;
        }
        // A run drawn wholly in faces the PDF can embed goes out as real text
        // and is its own searchable layer. Any other run keeps its outlines and
        // the invisible layer carries its text: one copy of the text either way.
        let embed = fonts.embeds_all(verts, table, solid_key);
        embedded &= embed;
        // Mirror the wire pass: indexed style color, screening, and pen width.
        let mut ctb_color: Option<[f32; 3]> = None;
        let mut lw_override: Option<f32> = None;
        let mut screening = 1.0;
        if let Some(ctb) = plot_style {
            if wire.aci > 0 {
                ctb_color = ctb.resolve_color(wire.aci);
                lw_override = options.object_lineweights.then(|| {
                    ctb.resolve_lineweight(wire.aci).map(|mm| {
                        let divisor = if options.scale_lineweights {
                            1.0
                        } else {
                            scale.max(1e-6)
                        };
                        (mm * MM_TO_PT).max(0.1) / divisor
                    })
                }).flatten();
                screening = ctb.resolve_screening(wire.aci);
            }
        }
        let mut gi = 0;
        while gi + 6 <= verts.len() {
            let quad = &verts[gi..gi + 6];
            gi += 6;

            let a = quad[0].color[3];
            if a < 0.01 {
                continue;
            }
            // A CTB colour override wins over the white-sheet adaptation, exactly
            // as in the wire pass — else a monochrome.ctb plot plots the lines
            // black and leaves the text on its screen colour.
            let rgb = ctb_color.unwrap_or_else(|| {
                adapt_text_color([quad[0].color[0], quad[0].color[1], quad[0].color[2]])
            });
            let [r, g, b] = plotted_color(rgb, a, screening, options);

            // Quad corners in world XY: verts run [bl, br, tr, bl, tr, tl].
            let bl = glyph_world_xy(&quad[0]);
            let br = glyph_world_xy(&quad[1]);
            let tr = glyph_world_xy(&quad[2]);
            let tl = glyph_world_xy(&quad[5]);
            // `tl` carries uv = (uv_min.x, uv_min.y) — the atlas tile key.
            let key = sdf_atlas::uv_key([quad[5].uv[0], quad[5].uv[1]]);

            // Cancel the offset in f64, then narrow: the sheet-mm result is a
            // small number even when the world coordinate is UTM-scale.
            let point = |wx: f64, wy: f64| Point::new(Mm((wx + ox) as f32), Mm((wy + oy) as f32));

            if let Some(ge) = table.get(&key) {
                // Affine basis of the quad: plane_min → bl, +x → br, +y → tl.
                // The glyph-space maths is small and stays f32; only the lift into
                // world coordinates needs f64.
                let (pmin, pmax) = (ge.plane_min, ge.plane_max);
                let (sx, sy) = (pmax[0] - pmin[0], pmax[1] - pmin[1]);
                if sx.abs() < 1e-9 || sy.abs() < 1e-9 {
                    continue;
                }
                let map = |p: [f32; 2]| -> Point {
                    let u = ((p[0] - pmin[0]) / sx) as f64;
                    let v = ((p[1] - pmin[1]) / sy) as f64;
                    let wx = bl[0] + u * (br[0] - bl[0]) + v * (tl[0] - bl[0]);
                    let wy = bl[1] + u * (br[1] - bl[1]) + v * (tl[1] - bl[1]);
                    point(wx, wy)
                };

                if !ge.fill_tris.is_empty() {
                    ops.push(Op::SetFillColor {
                        col: Color::Rgb(Rgb { r, g, b, icc_profile: None }),
                    });
                    // Filled TrueType glyph from a face we can embed: draw it as
                    // text in that font. Its outline tessellates into hundreds
                    // of triangles for a CJK face, which made text-heavy sheets
                    // export at hundreds of MB and crawl in viewers.
                    if let Some(source) = ge.source.as_ref().filter(|_| embed) {
                        if let Some(font) = fonts.font_for(source, ge.ch) {
                            let matrix =
                                glyph_text_matrix(source, pmin, [sx, sy], bl, br, tl, ox, oy);
                            push_glyph_text(ops, font, source.gid, ge.ch, matrix);
                            continue;
                        }
                    }
                    // Otherwise one filled triangle per triple.
                    for tri in ge.fill_tris.chunks_exact(3) {
                        ops.push(Op::DrawPolygon {
                            polygon: Polygon {
                                rings: vec![PolygonRing {
                                    points: tri
                                        .iter()
                                        .map(|&p| LinePoint { p: map(p), bezier: false })
                                        .collect(),
                                }],
                                mode: PaintMode::Fill,
                                winding_order: WindingOrder::NonZero,
                            },
                        });
                    }
                } else {
                    // Stroke (LFF/SHX pen) font or hollow glyph: polylines.
                    // Match the SDF atlas' nominal glyph-space pen instead of
                    // borrowing the entity lineweight: Roman Duplex and similar
                    // multi-stroke faces rely on that band to close the narrow
                    // gaps between parallel centrelines. An explicit CTB
                    // lineweight still wins and stays absolute under the plot CTM.
                    ops.push(Op::SetOutlineColor {
                        col: Color::Rgb(Rgb {
                            r,
                            g,
                            b,
                            icc_profile: None,
                        }),
                    });
                    let pen = if let Some(ctb_pen) = lw_override {
                        if ge.bold {
                            ctb_pen * 1.7
                        } else {
                            ctb_pen
                        }
                    } else {
                        let glyph_unit_mm = (((tl[0] - bl[0]).powi(2) + (tl[1] - bl[1]).powi(2))
                            .sqrt()
                            / sy.abs() as f64) as f32;
                        (2.0 * sdf_atlas::stroke_pen_half_units(ge.bold) * glyph_unit_mm * MM_TO_PT)
                            .max(0.1)
                    };
                    ops.push(Op::SetOutlineThickness { pt: Pt(pen) });
                    for stroke in &ge.strokes {
                        if stroke.len() < 2 {
                            continue;
                        }
                        ops.push(Op::DrawLine {
                            line: Line {
                                points: stroke
                                    .iter()
                                    .map(|&p| LinePoint { p: map(p), bezier: false })
                                    .collect(),
                                is_closed: false,
                            },
                        });
                    }
                }
            } else if key == solid_key {
                // Decoration bar (underline / overline / strike): the quad is a
                // solid-texel rectangle — fill it directly from its corners.
                ops.push(Op::SetFillColor {
                    col: Color::Rgb(Rgb { r, g, b, icc_profile: None }),
                });
                ops.push(Op::DrawPolygon {
                    polygon: Polygon {
                        rings: vec![PolygonRing {
                            points: [bl, br, tr, tl]
                                .iter()
                                .map(|&c| LinePoint { p: point(c[0], c[1]), bezier: false })
                                .collect(),
                        }],
                        mode: PaintMode::Fill,
                        winding_order: WindingOrder::NonZero,
                    },
                });
            }
        }
    }
    embedded
}

// ── Embedded TrueType text ─────────────────────────────────────────────────

/// The atlas export table plus its solid-tile key.
#[cfg(not(target_arch = "wasm32"))]
type GlyphTable = std::sync::Arc<(
    std::collections::HashMap<u64, crate::scene::text::sdf_atlas::GlyphExport>,
    u64,
)>;

/// The TrueType faces a PDF export draws text with, and the glyphs each uses.
///
/// Pages register glyphs as they are drawn (each under a font id fixed up
/// front); once every page is done, [`TextFonts::install`] subsets each face to
/// the glyphs actually used, renumbers them in the page streams, and adds the
/// subset fonts to the document. printpdf 0.9 would otherwise embed whole
/// fonts (its own subsetting is disabled), and a CJK face is megabytes.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct TextFonts {
    /// The atlas export table, snapshotted once: every page's text quads are
    /// laid out before the export starts, so one snapshot serves the whole
    /// document. Re-walking the atlas per text wire made a 28-sheet set with
    /// ~4 000 text entities take over nine minutes.
    glyphs: Option<GlyphTable>,
    fonts: Vec<EmbeddedFont>,
    /// Font file identity → index into `fonts`; `None` = printpdf can't parse
    /// the face, so its glyphs keep the outline fill.
    by_blob: std::collections::HashMap<(usize, u32, u32), Option<usize>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct EmbeddedFont {
    blob: std::sync::Arc<crate::scene::text::ttf_glyph::FontBlob>,
    id: FontId,
    /// Original glyph id → the character it draws (for ToUnicode).
    used: std::collections::BTreeMap<u16, char>,
}

#[cfg(not(target_arch = "wasm32"))]
impl TextFonts {
    /// The atlas' glyph export table (and solid-tile key), built on first use.
    fn glyph_table(&mut self) -> Option<GlyphTable> {
        use crate::scene::text::sdf_atlas;
        if self.glyphs.is_none() {
            let atlas = sdf_atlas::text_atlas().lock().ok()?;
            self.glyphs = Some(std::sync::Arc::new((
                atlas.export_table(),
                sdf_atlas::uv_key(atlas.solid_uv()),
            )));
        }
        self.glyphs.clone()
    }

    /// The font id to draw `source`'s glyph with, recording the glyph as used.
    /// `None` when the face can't be embedded.
    fn font_for(
        &mut self,
        source: &crate::scene::text::ttf_glyph::GlyphSource,
        ch: char,
    ) -> Option<FontId> {
        let slot = self.slot(source)?;
        let font = &mut self.fonts[slot];
        font.used.entry(source.gid).or_insert(ch);
        Some(font.id.clone())
    }

    /// The `fonts` slot of `source`'s face; `None` when printpdf can't parse it.
    fn slot(&mut self, source: &crate::scene::text::ttf_glyph::GlyphSource) -> Option<usize> {
        match self.by_blob.get(&source.font.id) {
            Some(slot) => *slot,
            None => {
                let parses =
                    ParsedFont::from_bytes(&source.font.data, source.font.index, &mut Vec::new())
                        .is_some();
                let slot = parses.then(|| {
                    self.fonts.push(EmbeddedFont {
                        blob: source.font.clone(),
                        id: FontId::new(),
                        used: std::collections::BTreeMap::new(),
                    });
                    self.fonts.len() - 1
                });
                self.by_blob.insert(source.font.id, slot);
                slot
            }
        }
    }

    /// Whether every glyph of `verts` is a filled glyph of a face the PDF can
    /// embed, so the whole run can go out as real text.
    fn embeds_all(
        &mut self,
        verts: &[crate::scene::pipeline::text_gpu::TextVertex],
        table: &std::collections::HashMap<u64, crate::scene::text::sdf_atlas::GlyphExport>,
        solid_key: u64,
    ) -> bool {
        use crate::scene::text::sdf_atlas;
        let mut any = false;
        for quad in verts.chunks_exact(6) {
            if quad[0].color[3] < 0.01 {
                continue;
            }
            let key = sdf_atlas::uv_key([quad[5].uv[0], quad[5].uv[1]]);
            if key == solid_key {
                continue;
            }
            let Some(ge) = table.get(&key) else {
                continue;
            };
            let Some(source) = ge.source.as_ref().filter(|_| !ge.fill_tris.is_empty()) else {
                return false;
            };
            if self.slot(source).is_none() {
                return false;
            }
            any = true;
        }
        any
    }

    /// Subset each face to its used glyphs, renumber those glyphs in the page
    /// streams, and register the fonts under the ids the pages already use.
    /// A face that fails to subset is embedded whole, ids unchanged.
    fn install(self, doc: &mut PdfDocument) {
        // A face only probed by `embeds_all` for a run that kept its outlines
        // draws nothing.
        for font in self.fonts.into_iter().filter(|font| !font.used.is_empty()) {
            // `.notdef` must stay glyph 0; the subset numbers glyphs in list order.
            let gids: Vec<u16> = std::iter::once(0)
                .chain(font.used.keys().copied().filter(|&gid| gid != 0))
                .collect();
            let subset = subset_font(&font.blob, &gids)
                .and_then(|bytes| ParsedFont::from_bytes(&bytes, 0, &mut Vec::new()));
            let parsed = match subset {
                Some(parsed) => {
                    let remap: std::collections::HashMap<u16, u16> = gids
                        .iter()
                        .enumerate()
                        .map(|(new, &old)| (old, new as u16))
                        .collect();
                    renumber_glyphs(&mut doc.pages, &font.id, &remap);
                    parsed
                }
                None => {
                    log::warn!("PDF export: font subsetting failed; embedding the whole face");
                    let whole =
                        ParsedFont::from_bytes(&font.blob.data, font.blob.index, &mut Vec::new());
                    match whole {
                        Some(parsed) => parsed,
                        None => continue, // `font_for` already checked it parses
                    }
                }
            };
            doc.resources
                .fonts
                .map
                .insert(font.id, PdfFont::new(parsed));
        }
    }
}

/// `blob`'s face reduced to `gids` (glyph 0 first), keeping the hinting tables
/// a tricky CJK face (DFKai-SB, MingLiU) needs to draw its strokes in place.
#[cfg(not(target_arch = "wasm32"))]
fn subset_font(blob: &crate::scene::text::ttf_glyph::FontBlob, gids: &[u16]) -> Option<Vec<u8>> {
    use allsorts::binary::read::ReadScope;
    use allsorts::font_data::FontData;
    use allsorts::subset::{subset, CmapTarget, SubsetProfile};

    let font = ReadScope::new(&blob.data).read::<FontData<'_>>().ok()?;
    let provider = font.table_provider(blob.index as usize).ok()?;
    subset(
        &provider,
        gids,
        &SubsetProfile::Pdf,
        CmapTarget::Unrestricted,
    )
    .ok()
}

/// Rewrite the glyph ids shown in font `id` through `remap` (original → subset).
#[cfg(not(target_arch = "wasm32"))]
fn renumber_glyphs(
    pages: &mut [PdfPage],
    id: &FontId,
    remap: &std::collections::HashMap<u16, u16>,
) {
    for page in pages {
        let mut in_font = false;
        for op in &mut page.ops {
            match op {
                Op::SetFont { font, .. } => {
                    in_font = matches!(font, PdfFontHandle::External(fid) if fid == id);
                }
                Op::ShowText { items } if in_font => {
                    for item in items {
                        if let TextItem::GlyphIds(codepoints) = item {
                            for codepoint in codepoints {
                                if let Some(&gid) = remap.get(&codepoint.gid) {
                                    codepoint.gid = gid;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Text matrix that lays the embedded glyph exactly over its SDF quad.
///
/// The quad maps the glyph's 9-unit tile rect `pmin .. pmin + size` onto the
/// world corners `bl`/`br`/`tl`; the outline was normalised font-unit × `k`,
/// so one em (`units_per_em` font units) spans `k · units_per_em` 9-units.
/// With `Tf` size 1 a text-space unit is one em, which this matrix carries to
/// sheet points under the page's CTM — the same space the wire pass draws in.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn glyph_text_matrix(
    source: &crate::scene::text::ttf_glyph::GlyphSource,
    pmin: [f32; 2],
    size: [f32; 2],
    bl: [f64; 2],
    br: [f64; 2],
    tl: [f64; 2],
    ox: f64,
    oy: f64,
) -> [f32; 6] {
    let em = source.k as f64 * source.units_per_em.max(1) as f64;
    let (sx, sy) = (size[0] as f64, size[1] as f64);
    // World displacement per 9-unit along the glyph's x and y axes.
    let ex = [(br[0] - bl[0]) / sx, (br[1] - bl[1]) / sx];
    let ey = [(tl[0] - bl[0]) / sy, (tl[1] - bl[1]) / sy];
    // Glyph origin (9-unit 0,0 = pen position on the baseline) in world space.
    let (px, py) = (pmin[0] as f64, pmin[1] as f64);
    let origin = [
        bl[0] - px * ex[0] - py * ey[0],
        bl[1] - px * ex[1] - py * ey[1],
    ];
    let pt = MM_TO_PT as f64;
    [
        (ex[0] * em * pt) as f32,
        (ex[1] * em * pt) as f32,
        (ey[0] * em * pt) as f32,
        (ey[1] * em * pt) as f32,
        ((origin[0] + ox) * pt) as f32,
        ((origin[1] + oy) * pt) as f32,
    ]
}

/// One glyph as a text object: font `font` at size 1, positioned by `matrix`.
/// The character rides along as the glyph's Unicode so the text is searchable.
#[cfg(not(target_arch = "wasm32"))]
fn push_glyph_text(ops: &mut Vec<Op>, font: FontId, gid: u16, ch: char, matrix: [f32; 6]) {
    ops.push(Op::StartTextSection);
    ops.push(Op::SetFont {
        font: PdfFontHandle::External(font),
        size: Pt(1.0),
    });
    ops.push(Op::SetTextMatrix {
        matrix: TextMatrix::Raw(matrix),
    });
    ops.push(Op::ShowText {
        items: vec![TextItem::GlyphIds(vec![Codepoint {
            gid,
            offset: 0.0,
            cid: Some(ch.to_string()),
        }])],
    });
    ops.push(Op::EndTextSection);
}

/// Everything searchable in an exported file: the raw bytes (printpdf
/// leaves small content streams uncompressed) plus any zlib streams that
/// decode to mostly-printable text, so the assertions hold either way.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn pdf_stream_text(bytes: &[u8]) -> String {
    use std::io::Read as _;
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for start in 0..bytes.len().saturating_sub(2) {
        // zlib streams start with a 2-byte header: deflate method, valid check.
        let (cmf, flg) = (bytes[start], bytes[start + 1]);
        if cmf & 0x0f != 8 || ((cmf as u16) << 8 | flg as u16) % 31 != 0 {
            continue;
        }
        let mut decoded = Vec::new();
        if flate2::read::ZlibDecoder::new(&bytes[start..])
            .read_to_end(&mut decoded)
            .is_ok()
            && decoded.len() > 32
        {
            let printable = decoded
                .iter()
                .filter(|&&b| matches!(b, b'\n' | b'\r' | b'\t' | 32..=126))
                .count();
            if printable * 10 >= decoded.len() * 9 {
                text.push_str(&String::from_utf8_lossy(&decoded));
            }
        }
    }
    text
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn test_page(wires: Vec<PlotWire>) -> PdfPageInput {
        PdfPageInput {
            content: PlotContent { wires: std::sync::Arc::new(wires), ..Default::default() },
            paper_w: 210.0,
            paper_h: 297.0,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            scale: 1.0,
            clip: None,
            options: PdfPlotOptions::default(),
            plot_style: None,
        }
    }

    #[test]
    fn atomic_write_replaces_an_existing_pdf() {
        let path = std::env::temp_dir().join(format!(
            "ocs-pdf-replace-{}-{}.pdf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();

        write_pdf_atomically(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn clip_and_scale_emit_pdf_bytes() {
        let w = PlotWire {
            wire: WireModel::solid(
                "test".into(),
                vec![[0.0, 0.0, 0.0], [50.0, 50.0, 0.0]],
                WireModel::WHITE,
                false,
            ),
            draw_depth: 0.0,
        };
        let mut page = test_page(vec![w]);
        page.scale = 2.0;
        page.clip = Some((10.0, 10.0, 100.0, 100.0));
        let bytes = build_pdf_pages(&[page], None).unwrap();
        // A valid PDF is produced (starts with the PDF header) and is non-trivial.
        assert!(bytes.starts_with(b"%PDF"), "not a PDF");
        assert!(bytes.len() > 200, "suspiciously small: {}", bytes.len());
    }


    #[test]
    fn export_fallback_plot_style_recolors_wires() {
        // ACI 1 carrying pure device red: without a style it must export as an
        // RGB operator; the monochrome fallback must recolor it. Guards the
        // automation plot path, where the per-page style can be dropped.
        let make_page = || {
            let mut wire = PlotWire {
                wire: WireModel::solid(
                    "test".into(),
                    vec![[0.0, 0.0, 0.0], [50.0, 50.0, 0.0]],
                    [1.0, 0.0, 0.0, 1.0],
                    false,
                ),
                draw_depth: 0.0,
            };
            wire.wire.aci = 1;
            test_page(vec![wire])
        };

        let unstyled = pdf_stream_text(&build_pdf_pages(&[make_page()], None).unwrap());
        let monochrome =
            PlotStyleTable::builtin("monochrome.ctb").expect("shipped monochrome.ctb parses");
        let styled =
            pdf_stream_text(&build_pdf_pages(&[make_page()], Some(&monochrome)).unwrap());

        assert!(
            unstyled.contains("1 0 0 rg") || unstyled.contains("1 0 0 RG"),
            "unstyled export should keep the wire's RGB color"
        );
        assert!(
            !styled.contains("1 0 0 rg") && !styled.contains("1 0 0 RG"),
            "monochrome fallback must recolor the wire"
        );
    }

    /// Parse every stroke-width operator (`<pt> w`) out of the content text.
    fn stroke_widths(text: &str) -> Vec<f32> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        tokens
            .windows(2)
            .filter_map(|pair| {
                if pair[1] == "w" {
                    pair[0].parse::<f32>().ok()
                } else {
                    None
                }
            })
            .collect()
    }

    fn distinct_widths(mut widths: Vec<f32>) -> Vec<f32> {
        widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
        widths.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        widths
    }

    #[test]
    fn plot_honors_per_object_lineweight_with_and_without_style() {
        // Two objects whose resolved lineweights differ (as two layers set to
        // 0.13 mm and 0.50 mm produce): the PDF must carry two distinct pen
        // widths, and a plot style that leaves weights at "use object" must
        // keep that hierarchy.
        let make_page = |px: f32| {
            let mut wire = PlotWire {
                wire: WireModel::solid(
                    "test".into(),
                    vec![[0.0, 0.0, 0.0], [50.0, 50.0, 0.0]],
                    [1.0, 0.0, 0.0, 1.0],
                    false,
                ),
                draw_depth: 0.0,
            };
            wire.wire.aci = 7;
            wire.wire.line_weight_px = px;
            test_page(vec![wire])
        };

        let monochrome =
            PlotStyleTable::builtin("monochrome.ctb").expect("shipped monochrome.ctb parses");
        eprintln!(
            "monochrome resolve_lineweight(7) = {:?}",
            monochrome.resolve_lineweight(7)
        );

        let unstyled = stroke_widths(&pdf_stream_text(
            &build_pdf_pages(&[make_page(1.0), make_page(3.0)], None).unwrap(),
        ));
        let unstyled = distinct_widths(unstyled);
        assert_eq!(unstyled.len(), 2, "two object weights, two pens: {unstyled:?}");
        assert!(
            (unstyled[1] / unstyled[0] - 3.0).abs() < 0.1,
            "pen widths follow the object weights 1px:3px: {unstyled:?}"
        );

        let styled = stroke_widths(&pdf_stream_text(
            &build_pdf_pages(&[make_page(1.0), make_page(3.0)], Some(&monochrome)).unwrap(),
        ));
        let styled = distinct_widths(styled);
        assert_eq!(
            styled.len(),
            2,
            "monochrome.ctb leaves weights at 'use object', so the two weights survive: {styled:?}"
        );
        assert!(
            (styled[1] / styled[0] - 3.0).abs() < 0.1,
            "weight hierarchy matches the unstyled plot: {styled:?}"
        );
    }

    // Build a WireModel carrying the SDF glyph quads for `text` in the embedded
    // "txt" stroke font, laid out into the process-wide atlas emit_text reads.
    fn text_wire(text: &str, origin: [f64; 3]) -> PlotWire {
        use crate::scene::pipeline::text_gpu::push_glyph_vertices;
        use crate::scene::text::{glyph_quads::layout_glyph_quads, sdf_atlas};
        let (quads, _) = {
            let mut atlas = sdf_atlas::text_atlas().lock().unwrap();
            layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, text)
        };
        assert!(!quads.is_empty(), "stroke glyphs laid out for {text:?}");
        let mut verts = Vec::new();
        push_glyph_vertices(&mut verts, &quads, origin, 1.0, [1.0, 0.0, 0.0, 1.0], 0.0);
        PlotWire {
            wire: WireModel {
                text_verts: verts,
                ..WireModel::solid("t".into(), Vec::new(), WireModel::WHITE, false)
            },
            draw_depth: 0.0,
        }
    }

    // End-to-end: a page whose only content is SDF text produces a larger PDF
    // than the same page with the text stripped — proving text reaches the file.
    #[test]
    fn text_grows_the_pdf_vs_no_text() {
        let wire = text_wire("HELLO", [20.0, 20.0, 0.0]);
        let mut blank = wire.clone();
        blank.wire.text_verts.clear();

        let with_text = build_pdf_pages(&[test_page(vec![wire])], None).unwrap();
        let no_text = build_pdf_pages(&[test_page(vec![blank])], None).unwrap();
        assert!(with_text.starts_with(b"%PDF"));
        assert!(
            with_text.len() > no_text.len(),
            "text did not add content: {} !> {}",
            with_text.len(),
            no_text.len()
        );
    }

    fn searchable_wire(text: &str, origin: [f64; 3]) -> PlotWire {
        use crate::scene::model::wire_model::SearchableTextRun;
        PlotWire {
            wire: WireModel {
                searchable_text: vec![SearchableTextRun {
                    text: text.to_string(),
                    origin,
                    height: 10.0,
                    rotation: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                    bold: false,
                    font: "txt".into(),
                    adv_width: 10.0 * text.chars().count().max(1) as f32,
                }],
                ..WireModel::solid("t".into(), Vec::new(), WireModel::WHITE, false)
            },
            draw_depth: 0.0,
        }
    }

    // Outline-only export exposes zero BT operators and no extractable text;
    // a preserved run must emit a real invisible (`Tr 3`) `BT … Tj ET` with
    // the string, on top of the unchanged outlines.
    #[test]
    fn searchable_text_emits_bt_tj_with_string() {
        let wire = searchable_wire("CMa-03", [20.0, 30.0, 0.0]);
        let bytes = build_pdf_pages(&[test_page(vec![wire])], None).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "not a PDF");
        let stream = pdf_stream_text(&bytes);
        assert!(stream.contains("BT"), "no text object (BT) in searchable export");
        assert!(
            stream.contains("CMa-03"),
            "searchable string missing from content stream"
        );
        assert!(
            stream.contains("/Helvetica"),
            "searchable export should reference the text font resource"
        );
        assert!(
            stream.contains("Tj") || stream.contains("TJ"),
            "no text-showing operator (Tj/TJ)"
        );
        assert!(
            stream.contains("Tr"),
            "searchable layer must be invisible (Tr rendering mode)"
        );
    }

    // Invisible layer coexists with outlines: the searchable file carries the
    // string while the outlines-only file does not, and the outlines are
    // identical in both (zero visual change — the layer only adds bytes).
    #[test]
    fn searchable_layer_coexists_with_outlines() {
        use crate::scene::text::sdf_atlas;
        // Other tests share the glyph atlas; a re-bake between laying out the
        // glyphs and exporting them moves the outline lookups, so build and
        // export under one atlas generation.
        let (outline_bytes, searchable_bytes) = loop {
            let generation = sdf_atlas::generation();
            let mut outline_only = text_wire("HELLO", [20.0, 20.0, 0.0]);
            outline_only.wire.searchable_text.clear();
            let mut searchable = text_wire("HELLO", [20.0, 20.0, 0.0]);
            searchable.wire.searchable_text =
                searchable_wire("HELLO", [20.0, 20.0, 0.0]).wire.searchable_text;

            // Outlines are never suppressed: both wires keep their glyph quads.
            assert!(
                !searchable.wire.text_verts.is_empty(),
                "searchable wire must keep its outline quads"
            );

            let outline_bytes = build_pdf_pages(&[test_page(vec![outline_only])], None).unwrap();
            let searchable_bytes = build_pdf_pages(&[test_page(vec![searchable])], None).unwrap();
            if sdf_atlas::generation() == generation {
                break (outline_bytes, searchable_bytes);
            }
        };
        let outline_stream = pdf_stream_text(&outline_bytes);
        let searchable_stream = pdf_stream_text(&searchable_bytes);

        assert!(
            !outline_stream.contains("HELLO"),
            "outline path must not leak extractable text"
        );
        assert!(
            searchable_stream.contains("HELLO") && searchable_stream.contains("BT"),
            "searchable path must carry extractable BT/Tj text"
        );
        assert!(
            searchable_stream.contains("Tr"),
            "searchable text must render invisibly (Tr)"
        );
        assert!(
            searchable_bytes.len() > outline_bytes.len(),
            "invisible layer adds bytes ({} > {}); visuals come from the kept outlines",
            searchable_bytes.len(),
            outline_bytes.len()
        );
    }

    // Per-run fallback: a non-WinAnsi run (Greek/Cyrillic/CJK/symbols) emits
    // no `Tj` — its outlines still draw, so nothing renders as missing glyphs.
    #[test]
    fn searchable_text_skips_non_winansi_runs() {
        let wire = searchable_wire("ΩΩ", [20.0, 20.0, 0.0]);
        let stream = pdf_stream_text(&build_pdf_pages(&[test_page(vec![wire])], None).unwrap());
        assert!(
            !stream.contains("BT"),
            "non-encodable run must not emit a text object"
        );
    }

    // Rotation must ride a text matrix (Tm), not just an angle — a 90° run
    // keeps its string and emits a rotated Tm.
    #[test]
    fn searchable_text_rotation_uses_text_matrix() {
        let mut wire = searchable_wire("AB", [20.0, 20.0, 0.0]);
        wire.wire.searchable_text[0].rotation = std::f32::consts::FRAC_PI_2;
        let stream = pdf_stream_text(&build_pdf_pages(&[test_page(vec![wire])], None).unwrap());
        assert!(stream.contains("AB"), "rotated run lost its string");
        assert!(stream.contains("Tm"), "rotated run needs an explicit text matrix (Tm)");
    }

    #[test]
    fn winansi_filter_covers_accents_and_rejects_cjk() {
        assert!(is_winansi_encodable("CMa-03"));
        assert!(is_winansi_encodable("crème °±Ø"));
        assert!(!is_winansi_encodable("Ω"));
        assert!(!is_winansi_encodable("中"));
        // C1 controls are unassigned in WinAnsi: emitting them would mojibake
        // (explicit \u escapes — a literal control char would not survive
        // editors, and NBSP U+00A0 next door *is* encodable).
        assert!(is_winansi_encodable("a\u{a0}b")); // NBSP is WinAnsi 0xA0
        assert!(!is_winansi_encodable("a\u{80}b"));
        assert!(!is_winansi_encodable("a\u{9f}b"));
    }

    #[test]
    fn width_factor_matches_laid_to_natural_advance() {
        use std::collections::BTreeMap;
        let sub = EmbeddedSubset {
            family: "Test".into(),
            bold: false,
            font_id: printpdf::FontId::new(),
            gid_of: [('A', 1), ('B', 2)].into_iter().collect::<BTreeMap<_, _>>(),
            width_of: [(1, 600), (2, 620)].into_iter().collect::<BTreeMap<_, _>>(),
            upem: 1000,
            cap_ratio: 0.7,
        };
        let run = SearchableTextRun {
            text: "AB".into(),
            origin: [0.0, 0.0, 0.0],
            height: 10.0,
            rotation: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
            bold: false,
            font: "Test".into(),
            // Laid advance exactly equals natural at cap height, so k = 1:
            // adv = height * Σw / (upem * cap) = 10*1220/(1000*0.7).
            adv_width: 10.0 * 1220.0 / (1000.0 * 0.7),
        };
        assert!((sub.width_factor(&run) - 1.0).abs() < 1e-4);
        let mut wide = run.clone();
        wide.adv_width *= 2.0;
        assert!((sub.width_factor(&wide) - 2.0).abs() < 1e-4);
        let mut empty = run;
        empty.adv_width = 0.0;
        assert_eq!(sub.width_factor(&empty), 1.0);
    }

    // A TTF-backed run embeds a subset of its own family with ToUnicode —
    // the DWG font, not Helvetica. Skips where no test font is installed.
    #[test]
    fn subset_embeds_the_runs_own_family_with_tounicode() {
        let family = ["DejaVu Sans", "Liberation Sans", "Arial"]
            .into_iter()
            .find(|f| {
                crate::scene::text::sysfont::with_face_data_weighted(
                    f,
                    fontdb::Weight::NORMAL,
                    |_, _| true,
                )
                .unwrap_or(false)
            });
        let Some(family) = family else {
            eprintln!("no test TTF installed; skipping subset test");
            return;
        };
        let full_len =
            crate::scene::text::sysfont::with_face_data_weighted(family, fontdb::Weight::NORMAL, |data, _| {
                data.len()
            })
            .unwrap();
        let mut wire = searchable_wire("CMa-03 pincé", [20.0, 30.0, 0.0]);
        wire.wire.searchable_text[0].font = family.to_string();
        // Unit level: the subset is built for the run's family and maps the
        // used chars (extractability source of truth — the content stream
        // carries hex GIDs, resolved via ToUnicode, not literal text).
        let mut probe = printpdf::PdfDocument::new("subset-probe");
        let fonts =
            SearchableFontSet::build(&mut probe, &[test_page(vec![wire.clone()])]);
        let sub = fonts
            .find(family, false)
            .expect("subset embedded for the run's own family");
        assert!(sub.gid_of.contains_key(&'C'), "used chars must be mapped");
        assert!(sub.gid_of.contains_key(&'é'), "accents must survive subsetting");
        // PDF level: ToUnicode + embedded bytes, and no Helvetica fallback.
        let bytes = build_pdf_pages(&[test_page(vec![wire])], None).unwrap();
        let stream = pdf_stream_text(&bytes);
        assert!(
            stream.contains("ToUnicode"),
            "subset font must carry a ToUnicode CMap"
        );
        assert!(
            stream.contains("FontFile2"),
            "subset font must embed its (TrueType) bytes"
        );
        assert!(
            !stream.contains("/Helvetica"),
            "own-family run must not fall back to Helvetica"
        );
        assert!(
            bytes.len() < full_len,
            "subset PDF ({} bytes) must be lighter than the full font ({} bytes)",
            bytes.len(),
            full_len
        );
    }

    // Runs fully outside the export window never reach the content stream.
    #[test]
    fn clip_culls_runs_outside_the_export_window() {
        let inside = searchable_wire("IN", [20.0, 20.0, 0.0]);
        let outside = searchable_wire("OUT", [500.0, 500.0, 0.0]);
        // Same page, windowed clip around the origin run only.
        let mut page = test_page(vec![inside, outside]);
        page.clip = Some((0.0, 0.0, 100.0, 100.0));
        let stream = pdf_stream_text(&build_pdf_pages(&[page], None).unwrap());
        assert!(stream.contains("IN"), "inside run must survive the clip");
        assert!(!stream.contains("OUT"), "outside run must be culled by the clip");
    }
    // A filled TrueType glyph exports as text in an embedded subset of its
    // face — not as its tessellated outline, which for a CJK face is hundreds
    // of triangles per glyph (a 28-sheet DFKai-SB set came out at 320 MB).
    #[test]
    fn truetype_text_embeds_a_subset_font() {
        use crate::scene::pipeline::text_gpu::push_glyph_vertices;
        use crate::scene::text::font_face::Face;
        use crate::scene::text::{glyph_quads::layout_glyph_quads, sdf_atlas, sysfont};

        // Any installed TrueType family that draws 'H'; hosts without fonts skip.
        let Some((family, source)) = sysfont::families().iter().find_map(|family| {
            let face = Face::resolve(family);
            matches!(face, Face::Ttf { .. })
                .then(|| face.glyph_source('H'))
                .flatten()
                .map(|source| (family.clone(), source))
        }) else {
            eprintln!("no TrueType font installed; skipped");
            return;
        };
        // The atlas and TEXTFILL are process-wide and other tests reset, grow
        // or unfill them concurrently; a change between laying the quads out
        // and exporting them invalidates their tile keys, so retry until one
        // export runs against a stable, filled atlas.
        let (bytes, text) = (0..20)
            .find_map(|_| {
                let generation = sdf_atlas::generation();
                if !sdf_atlas::textfill() {
                    std::thread::yield_now();
                    return None;
                }
                let (quads, _) = {
                    let mut atlas = sdf_atlas::text_atlas().lock().unwrap();
                    layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, &family, false, "HHH")
                };
                assert_eq!(quads.len(), 3, "three glyphs laid out in {family}");
                let mut verts = Vec::new();
                push_glyph_vertices(
                    &mut verts,
                    &quads,
                    [20.0, 20.0, 0.0],
                    1.0,
                    [0.0, 0.0, 0.0, 1.0],
                    0.0,
                );
                let wire = PlotWire {
                    wire: WireModel {
                        text_verts: verts,
                        ..WireModel::solid("t".into(), Vec::new(), WireModel::WHITE, false)
                    },
                    draw_depth: 0.0,
                };
                let bytes = build_pdf_pages(&[test_page(vec![wire])], None).unwrap();
                let stable = sdf_atlas::generation() == generation && sdf_atlas::textfill();
                stable.then(|| {
                    let text = pdf_stream_text(&bytes);
                    (bytes, text)
                })
            })
            .expect("the shared glyph atlas never held still for one export");
        assert!(text.contains(" Tf"), "glyphs drawn as text in a font");
        assert_eq!(
            text.matches(" Tm").count(),
            3,
            "one positioned text object per glyph"
        );
        assert!(
            String::from_utf8_lossy(&bytes).contains("/FontFile2"),
            "the TrueType program is embedded"
        );
        // A subset of a handful of glyphs, not the whole face.
        assert!(
            bytes.len() < source.font.data.len().max(200_000) / 2,
            "PDF {} bytes vs font file {} bytes",
            bytes.len(),
            source.font.data.len()
        );
    }
}
