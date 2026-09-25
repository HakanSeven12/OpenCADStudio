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
use printpdf::{
    BlendMode, BuiltinFont, Color, ExtendedGraphicsState, ExtendedGraphicsStateId, Line,
    LineCapStyle, LineDashPattern, LineJoinStyle, LinePoint, Mm, Op, PaintMode, PdfDocument,
    PdfFontHandle, PdfPage, PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb, TextItem,
    WindingOrder,
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
    let key = (
        std::sync::Arc::as_ptr(&image.pixels) as usize,
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
                    pixels: printpdf::RawImageData::U8(image.pixels.as_ref().clone()),
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
    // Borrowing all pages keeps their pixel Arcs alive until this cache is dropped.
    // Allocation addresses cannot be reused by another source during this export.
    let mut image_resources = std::collections::HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        append_pdf_page(&mut doc, &mut image_resources, page, plot_style)
            .map_err(|error| format!("Page {}: {error}", index + 1))?;
    }
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
    page: &PdfPageInput,
    fallback_plot_style: Option<&PlotStyleTable>,
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
        if !wire.text_verts.is_empty() {
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
                emit_text(
                    &mut ops,
                    std::slice::from_ref(&wire.wire),
                    ox,
                    oy,
                    scale,
                    plot_style,
                    options,
                );
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
        if !color_overridden {
            let is_light = r > 0.80 && g > 0.80 && b > 0.80;
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
        && !(hatch.aci == 7 && matches!(hatch.pattern, HatchPattern::Solid))
    {
        let is_light = r > 0.80 && g > 0.80 && b > 0.80;
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
    let is_light = r > 0.80 && g > 0.80 && b > 0.80;
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

/// Re-emit every wire's SDF text as vector geometry.
///
/// Each visible glyph rides on `wire.text_verts` as one 6-vertex quad (two
/// triangles) whose corners are the glyph's atlas `plane` rect run through the
/// text transform. We recover the glyph's outline / fill from the atlas by the
/// quad's `uv_min` and map it into that quad by affine interpolation of the
/// plane rect — so a stroke (LFF) font emits polylines and a filled TrueType
/// glyph emits filled triangles, exactly where the SDF quad sits.
#[cfg(not(target_arch = "wasm32"))]
fn emit_text(
    ops: &mut Vec<Op>,
    wires: &[WireModel],
    ox: f64,
    oy: f64,
    scale: f32,
    plot_style: Option<&PlotStyleTable>,
    options: PdfPlotOptions,
) {
    use crate::scene::text::sdf_atlas;

    if wires.iter().all(|w| w.text_verts.is_empty()) {
        return;
    }
    // Snapshot the atlas' baked-glyph geometry once; drop the lock before use.
    let (table, solid_key) = {
        let Ok(atlas) = sdf_atlas::text_atlas().lock() else {
            return;
        };
        (atlas.export_table(), sdf_atlas::uv_key(atlas.solid_uv()))
    };

    // `Op::SetLineDashPattern` is persistent graphics state and the wire pass
    // above only re-emits it on change, so whatever the last wire needed is
    // still active here — without this reset a drawing whose last wire carries a
    // HIDDEN/CENTER linetype prints its glyph outlines dashed.
    ops.push(Op::SetLineDashPattern {
        dash: LineDashPattern::default(),
    });

    for wire in wires {
        let verts = &wire.text_verts;
        if verts.is_empty() {
            continue;
        }
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
                    // Filled TrueType glyph: one filled triangle per triple.
                    ops.push(Op::SetFillColor {
                        col: Color::Rgb(Rgb { r, g, b, icc_profile: None }),
                    });
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
        let quads = {
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
}
