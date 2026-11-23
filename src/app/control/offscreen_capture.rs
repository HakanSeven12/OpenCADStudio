//! Headless off-screen `capture` rendering across dual backends (`wgpu` WYSIWYG
//! canvas and CPU SVG/`resvg` vector rasterizer) for named layouts and `aspect_fit`
//! bounding boxes without freezing the UI thread or mutating live state.

use super::{failure, sheets, string, vision, OpenCADStudio};
use crate::app::Message;
use crate::io::pdf_export::{PdfPageInput, PdfPlotOptions, PlotContent, PlotQuality, PlotWire};
use crate::scene::model::hatch_model::HatchPattern;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const MM_PER_PX: f64 = 25.4 / 96.0;
const HIGHLIGHT_COLOR: [f32; 4] = [0.0, 0.72, 1.0, 1.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestedRenderer {
    Auto,
    Gpu,
    Cpu,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectiveBackend {
    Gpu,
    Cpu,
}

/// Returns `true` when a `capture` request asks for off-screen layout,
/// `aspect_fit`, or explicit `renderer` capture instead of a live window screenshot.
pub(super) fn is_offscreen_capture(req: &Value) -> bool {
    req.get("layout").is_some_and(|v| !v.is_null())
        || req
            .get("aspect_fit")
            .is_some_and(|v| !v.is_null() && v.as_bool() != Some(false))
        || req
            .get("renderer")
            .is_some_and(|v| !v.is_null() && v.as_str() != Some("auto"))
}

fn parse_renderer(req: &Value) -> Result<RequestedRenderer, Value> {
    let Some(raw) = req.get("renderer").filter(|v| !v.is_null()) else {
        return Ok(RequestedRenderer::Auto);
    };
    let s = raw.as_str().ok_or_else(|| {
        failure("invalid_request", "renderer must be 'auto', 'gpu', or 'cpu'")
    })?;
    match s {
        "auto" => Ok(RequestedRenderer::Auto),
        "gpu" => Ok(RequestedRenderer::Gpu),
        "cpu" => Ok(RequestedRenderer::Cpu),
        _ => Err(failure(
            "invalid_request",
            format!("Unsupported renderer '{s}'; expected 'auto', 'gpu', or 'cpu'"),
        )),
    }
}

fn parse_view(req: &Value) -> Result<&'static str, Value> {
    let Some(raw) = req.get("view").filter(|v| !v.is_null()) else {
        return Ok("current");
    };
    let s = raw.as_str().ok_or_else(|| {
        failure(
            "invalid_request",
            "view must be 'current', 'extents', 'selection', or 'region'",
        )
    })?;
    match s {
        "current" => Ok("current"),
        "extents" => Ok("extents"),
        "selection" => Ok("selection"),
        "region" => Ok("region"),
        _ => Err(failure(
            "invalid_request",
            format!("Unsupported view '{s}'; expected 'current', 'extents', 'selection', or 'region'"),
        )),
    }
}

fn parse_aspect_fit(req: &Value) -> Result<bool, Value> {
    match req.get("aspect_fit") {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(failure("invalid_request", "aspect_fit must be a boolean")),
    }
}

/// Strictly parses an optional JSON array of hexadecimal handle strings (`0x`/`0X` optional).
fn parse_hex_handles(req: &Value, key: &str) -> Result<Vec<document::Handle>, Value> {
    let Some(raw) = req.get(key).filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let arr = raw
        .as_array()
        .ok_or_else(|| failure("invalid_request", format!("{key} must be an array of hex handle strings")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item.as_str().ok_or_else(|| {
            failure(
                "invalid_request",
                format!("{key} contains non-string handle {item}"),
            )
        })?;
        let trimmed = s.trim();
        let hex = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        if hex.is_empty() {
            return Err(failure(
                "invalid_request",
                format!("{key} contains empty handle string"),
            ));
        }
        let val = u64::from_str_radix(hex, 16).map_err(|_| {
            failure(
                "invalid_request",
                format!("{key} contains invalid hexadecimal handle '{s}'"),
            )
        })?;
        out.push(document::Handle::new(val));
    }
    Ok(out)
}

/// Normalizes and validates an optional `[min_x, min_y, max_x, max_y]` array.
fn parse_explicit_bounds(req: &Value) -> Result<Option<[f64; 4]>, Value> {
    let Some(raw) = req.get("bounds").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let invalid = || {
        failure(
            "invalid_request",
            "bounds must be [min_x, min_y, max_x, max_y] with positive width and height",
        )
    };
    let arr = raw.as_array().ok_or_else(invalid)?;
    if arr.len() != 4 {
        return Err(invalid());
    }
    let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
        arr[0].as_f64(),
        arr[1].as_f64(),
        arr[2].as_f64(),
        arr[3].as_f64(),
    ) else {
        return Err(invalid());
    };
    if !x0.is_finite()
        || !y0.is_finite()
        || !x1.is_finite()
        || !y1.is_finite()
        || (x0 - x1).abs() <= 1e-9
        || (y0 - y1).abs() <= 1e-9
    {
        return Err(invalid());
    }
    Ok(Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]))
}

/// Normalizes a bounding box, expands any zero-width axis symmetrically for
/// single-axis/point geometry, and pads each axis symmetrically by `margin_frac`.
fn pad_bounds(min_x: f64, min_y: f64, max_x: f64, max_y: f64, margin_frac: f64) -> [f64; 4] {
    let mut x0 = min_x.min(max_x);
    let mut x1 = min_x.max(max_x);
    let mut y0 = min_y.min(max_y);
    let mut y1 = min_y.max(max_y);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let fallback = dx.max(dy).max(1.0) * 0.1;
    if dx <= 1e-9 {
        let cx = (x0 + x1) * 0.5;
        x0 = cx - fallback * 0.5;
        x1 = cx + fallback * 0.5;
    }
    if dy <= 1e-9 {
        let cy = (y0 + y1) * 0.5;
        y0 = cy - fallback * 0.5;
        y1 = cy + fallback * 0.5;
    }
    let pad_x = (x1 - x0) * margin_frac;
    let pad_y = (y1 - y0) * margin_frac;
    [x0 - pad_x, y0 - pad_y, x1 + pad_x, y1 + pad_y]
}

/// Computes non-strict extents for a [`PlotContent`] including single-axis lines
/// and point entities (unlike `plot_content_extents`, which rejects zero-area boxes).
fn plot_content_extents_inclusive(content: &PlotContent) -> Option<(f64, f64, f64, f64)> {
    let mut bounds = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut any = false;
    let mut include = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
            any = true;
        }
    };
    for wire in content
        .wires
        .iter()
        .filter(|wire| wire.name != "paper_printable_area")
    {
        if wire.aabb == crate::scene::WireModel::UNBOUNDED_AABB {
            continue;
        }
        let [x0, y0, x1, y1] = wire.aabb;
        include(f64::from(x0), f64::from(y0));
        include(f64::from(x1), f64::from(y1));
    }
    for hatch in content.wipeouts.iter().chain(content.hatches.iter()) {
        for &[x, y] in hatch.boundary.iter() {
            include(
                hatch.world_origin[0] + f64::from(x),
                hatch.world_origin[1] + f64::from(y),
            );
        }
    }
    for plot in &content.images {
        for corner in plot.image.corners {
            include(f64::from(corner[0]), f64::from(corner[1]));
        }
    }
    (any
        && bounds.0.is_finite()
        && bounds.1.is_finite()
        && bounds.2.is_finite()
        && bounds.3.is_finite()
        && bounds.2 >= bounds.0
        && bounds.3 >= bounds.1)
        .then_some(bounds)
}

/// Computes the 2D world bounding box union across all `handles` belonging to
/// `scene`'s current layout.
fn union_entity_bounds(scene: &crate::scene::Scene, handles: &[document::Handle]) -> Option<[f64; 4]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;

    for &handle in handles {
        if !scene.entity_belongs_to_current_layout(handle) {
            continue;
        }
        let Some(entity) = scene.document.get_entity(handle) else {
            continue;
        };
        let (b_min, b_max) = crate::scene::convert::tess::entity_bounds_in(&scene.document, entity);
        let min_dvec = glam::DVec3::from(b_min);
        let max_dvec = glam::DVec3::from(b_max);
        if !min_dvec.is_finite() || !max_dvec.is_finite() {
            continue;
        }
        min_x = min_x.min(b_min[0].min(b_max[0]));
        min_y = min_y.min(b_min[1].min(b_max[1]));
        max_x = max_x.max(b_min[0].max(b_max[0]));
        max_y = max_y.max(b_min[1].max(b_max[1]));
        any = true;
    }

    any.then_some([min_x, min_y, max_x, max_y])
}

/// Unprojects the live viewport corners onto the camera's target plane when valid.
fn current_view_bounds(
    camera: &crate::scene::view::camera::Camera,
    vp_size: (f32, f32),
) -> Option<[f64; 4]> {
    if vp_size.0 < 1.0 || vp_size.1 < 1.0 {
        return None;
    }
    let rect = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: vp_size.0,
        height: vp_size.1,
    };
    let corners = [
        iced::Point::new(0.0, 0.0),
        iced::Point::new(vp_size.0, 0.0),
        iced::Point::new(vp_size.0, vp_size.1),
        iced::Point::new(0.0, vp_size.1),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for corner in corners {
        let world = camera.pick_on_target_plane(corner, rect);
        if !world.is_finite() {
            return None;
        }
        min_x = min_x.min(world.x);
        min_y = min_y.min(world.y);
        max_x = max_x.max(world.x);
        max_y = max_y.max(world.y);
    }
    ((max_x - min_x) > 1e-9 && (max_y - min_y) > 1e-9)
        .then_some([min_x, min_y, max_x, max_y])
}

fn resolve_extents_bounds(
    scene: &crate::scene::Scene,
    target_layout: &str,
    content_ext: Option<(f64, f64, f64, f64)>,
) -> [f64; 4] {
    if target_layout == "Model" {
        if let Some((min, max)) = scene
            .visible_block_bounds(scene.model_space_block_handle())
            .or_else(|| {
                scene
                    .model_space_extents()
                    .map(|(a, b)| (a.as_dvec3(), b.as_dvec3()))
            })
        {
            if min.is_finite() && max.is_finite() {
                return pad_bounds(min.x, min.y, max.x, max.y, 0.03);
            }
        }
        if let Some((x0, y0, x1, y1)) = content_ext {
            return pad_bounds(x0, y0, x1, y1, 0.03);
        }
        return [-10.0, -10.0, 10.0, 10.0];
    }

    // Paper space: derive block extents directly if `content_ext` wasn't passed (GPU path).
    let paper_content_ext = content_ext.or_else(|| {
        let block = scene.current_layout_block_handle();
        scene
            .visible_block_bounds(block)
            .map(|(min, max)| (min.x, min.y, max.x, max.y))
    });
    if let Some(((px0, py0), (px1, py1))) = scene.paper_limits() {
        if (px1 - px0) > 1e-9 && (py1 - py0) > 1e-9 {
            let (u0, v0, u1, v1) = match paper_content_ext {
                Some((cx0, cy0, cx1, cy1)) => {
                    (px0.min(cx0), py0.min(cy0), px1.max(cx1), py1.max(cy1))
                }
                None => (px0, py0, px1, py1),
            };
            return pad_bounds(u0, v0, u1, v1, 0.02);
        }
    }
    if let Some((x0, y0, x1, y1)) = paper_content_ext {
        pad_bounds(x0, y0, x1, y1, 0.03)
    } else {
        [-10.0, -10.0, 10.0, 10.0]
    }
}

struct TargetBoundsContext<'a> {
    scene: &'a crate::scene::Scene,
    target_layout: &'a str,
    original_layout: &'a str,
    original_block_edit: Option<document::Handle>,
    original_camera: &'a crate::scene::view::camera::Camera,
    live_vp_size: (f32, f32),
    view: &'a str,
    focus_handles: &'a [document::Handle],
    explicit_bounds: Option<[f64; 4]>,
    highlight_handles: &'a [document::Handle],
    content_ext: Option<(f64, f64, f64, f64)>,
}

fn resolve_target_bounds(ctx: &TargetBoundsContext<'_>) -> Result<[f64; 4], Value> {
    if !ctx.focus_handles.is_empty() {
        let [x0, y0, x1, y1] = union_entity_bounds(ctx.scene, ctx.focus_handles).ok_or_else(|| {
            failure(
                "invalid_request",
                "focus_handles did not match any bounded entities in the target layout",
            )
        })?;
        return Ok(pad_bounds(x0, y0, x1, y1, 0.05));
    }
    if let Some(bounds) = ctx.explicit_bounds {
        return Ok(bounds);
    }
    if ctx.view == "selection" {
        let selected;
        let handles = if !ctx.highlight_handles.is_empty() {
            ctx.highlight_handles
        } else {
            selected = ctx.scene.selected_handles_in_order();
            &selected
        };
        if let Some([x0, y0, x1, y1]) = union_entity_bounds(ctx.scene, handles) {
            return Ok(pad_bounds(x0, y0, x1, y1, 0.05));
        }
    }
    if ctx.view == "current"
        && ctx.target_layout == ctx.original_layout
        && ctx.original_block_edit.is_none()
    {
        if let Some(bounds) = current_view_bounds(ctx.original_camera, ctx.live_vp_size) {
            return Ok(bounds);
        }
    }
    Ok(resolve_extents_bounds(
        ctx.scene,
        ctx.target_layout,
        ctx.content_ext,
    ))
}

fn compute_output_dimensions(
    world_w: f64,
    world_h: f64,
    max_dimension: u32,
    aspect_fit: bool,
    live_vp_size: (f32, f32),
) -> (u32, u32) {
    if aspect_fit {
        return vision::compute_aspect_fit_dimensions(world_w, world_h, max_dimension);
    }
    let aspect = if live_vp_size.0 >= 1.0 && live_vp_size.1 >= 1.0 {
        f64::from(live_vp_size.0) / f64::from(live_vp_size.1)
    } else {
        4.0 / 3.0
    };
    vision::compute_aspect_fit_dimensions(aspect.max(1e-3), 1.0, max_dimension)
}

fn centered_visible_bounds(
    target_bounds: [f64; 4],
    img_w: u32,
    img_h: u32,
    clamp_f32_scale: bool,
) -> ([f64; 4], f64) {
    let world_w = (target_bounds[2] - target_bounds[0]).max(1e-9);
    let world_h = (target_bounds[3] - target_bounds[1]).max(1e-9);
    let raw_px_per_world = (f64::from(img_w) / world_w).min(f64::from(img_h) / world_h);
    let eff_px_per_world = if clamp_f32_scale {
        let scale_f32 = ((raw_px_per_world * MM_PER_PX) as f32).max(1e-12);
        f64::from(scale_f32) / MM_PER_PX
    } else {
        raw_px_per_world.max(1e-12)
    };
    let cx = (target_bounds[0] + target_bounds[2]) * 0.5;
    let cy = (target_bounds[1] + target_bounds[3]) * 0.5;
    let vis_w = f64::from(img_w) / eff_px_per_world;
    let vis_h = f64::from(img_h) / eff_px_per_world;
    let vis_bounds = [
        cx - vis_w * 0.5,
        cy - vis_h * 0.5,
        cx + vis_w * 0.5,
        cy + vis_h * 0.5,
    ];
    (vis_bounds, eff_px_per_world * MM_PER_PX)
}

fn build_offscreen_page(
    content: PlotContent,
    target_bounds: [f64; 4],
    img_w: u32,
    img_h: u32,
) -> (PdfPageInput, [f64; 4]) {
    let (vis_bounds, scale) = centered_visible_bounds(target_bounds, img_w, img_h, true);
    let page = PdfPageInput {
        content,
        paper_w: f64::from(img_w) * MM_PER_PX,
        paper_h: f64::from(img_h) * MM_PER_PX,
        offset_x: -vis_bounds[0],
        offset_y: -vis_bounds[1],
        rotation_deg: 0,
        content_rotation: 0.0,
        scale: (scale as f32).max(1e-12),
        clip: None,
        plot_style: None,
        geopdf: None,
        options: PdfPlotOptions {
            object_lineweights: true,
            scale_lineweights: false,
            transparency: true,
            stamp: false,
            merge_lines: false,
            quality: PlotQuality::Normal,
            raster_dpi: None,
            vector_dpi: None,
            text_as_outlines: true,
            non_scaling_stroke: false,
            layer_attributes: false,
        },
    };
    (page, vis_bounds)
}

fn highlight_plot_content(content: &mut PlotContent, hl: &rustc_hash::FxHashSet<document::Handle>) {
    for pw in std::sync::Arc::make_mut(&mut content.wires).iter_mut() {
        if crate::scene::Scene::handle_from_wire_name(&pw.wire.name).is_some_and(|h| hl.contains(&h))
        {
            pw.wire.color = HIGHLIGHT_COLOR;
            pw.wire.aci = 0;
            pw.wire.line_weight_px = pw.wire.line_weight_px.max(2.5);
            for vert in &mut pw.wire.text_verts {
                if vert.color[3] >= 0.01 {
                    vert.color[0] = HIGHLIGHT_COLOR[0];
                    vert.color[1] = HIGHLIGHT_COLOR[1];
                    vert.color[2] = HIGHLIGHT_COLOR[2];
                }
            }
            for run in &mut pw.wire.searchable_text {
                if run.color[3] >= 0.01 {
                    run.color[0] = HIGHLIGHT_COLOR[0];
                    run.color[1] = HIGHLIGHT_COLOR[1];
                    run.color[2] = HIGHLIGHT_COLOR[2];
                }
            }
        }
    }

    for hatch in &mut content.hatches {
        if hatch.entity.is_some_and(|h| hl.contains(&h)) {
            hatch.color = HIGHLIGHT_COLOR;
            hatch.aci = 0;
            hatch.line_weight_px = hatch.line_weight_px.max(2.5);
            if let HatchPattern::Gradient { color2, .. } = &mut hatch.pattern {
                *color2 = HIGHLIGHT_COLOR;
            }
        }
    }

    let mut wipeout_outlines = Vec::new();
    for wipeout in &content.wipeouts {
        if wipeout.entity.is_some_and(|h| hl.contains(&h)) && wipeout.boundary.len() >= 2 {
            let ox = wipeout.world_origin[0];
            let oy = wipeout.world_origin[1];
            let mut points: Vec<[f64; 3]> = wipeout
                .boundary
                .iter()
                .map(|&[bx, by]| [ox + f64::from(bx), oy + f64::from(by), 0.0])
                .collect();
            if points.first() != points.last() {
                if let Some(&first) = points.first() {
                    points.push(first);
                }
            }
            let mut wire = crate::scene::WireModel::solid_f64(
                format!(
                    "{}",
                    wipeout.entity.unwrap_or(document::Handle::NULL).value()
                ),
                points,
                HIGHLIGHT_COLOR,
                false,
            );
            wire.aci = 0;
            wire.line_weight_px = 2.5;
            wire.plot_visible = true;
            wipeout_outlines.push(PlotWire {
                wire,
                draw_depth: (wipeout.draw_depth + 0.01).clamp(-1.0, 1.0),
            });
        }
    }
    if !wipeout_outlines.is_empty() {
        let len = wipeout_outlines.len();
        std::sync::Arc::make_mut(&mut content.wires).extend(wipeout_outlines);
        if content.group_splits.wires + len == content.wires.len() {
            content.group_splits.wires = content.wires.len();
        }
    }
}

/// Overlays Set-of-Marks visual badges for all grounded entities onto `image`.
pub(super) fn draw_som_annotations(image: &mut image::RgbaImage, visible_entities: &[Value]) {
    for ent_val in visible_entities {
        if let (Some(tag), Some(px_arr), Some(selected)) = (
            ent_val["tag"].as_u64(),
            ent_val["screen_pixel"].as_array(),
            ent_val["selected"].as_bool(),
        ) {
            if px_arr.len() == 2 {
                if let (Some(x), Some(y)) = (px_arr[0].as_i64(), px_arr[1].as_i64()) {
                    vision::draw_som_badge(image, x as i32, y as i32, tag as usize, selected);
                }
            }
        }
    }
}

/// Encodes and saves a capture PNG natively, or hands a browser download on wasm32.
pub(super) fn save_capture_png(image: &image::RgbaImage, path: &str) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let download = path
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("capture.png")
            .to_string();
        crate::sys::download_bytes(&download, &png);
        Ok(())
    }
}

/// Formats the shared `_spatial` metadata payload for capture results.
pub(super) fn spatial_metadata_json(
    doc_unit: &str,
    image: &image::RgbaImage,
    grounding: &vision::VisionGrounding,
) -> Value {
    json!({
        "crs": "CAD_WCS",
        "unit": doc_unit,
        "pixel_resolution": [image.width(), image.height()],
        "world_bounds": grounding.viewport_world_bounds,
        "pixel_to_world_matrix": grounding.pixel_to_world_matrix,
        "world_to_pixel_matrix": grounding.world_to_pixel_matrix,
        "camera": grounding.camera,
        "target_plane": grounding.target_plane,
        "annotations": grounding.visible_entities.clone(),
    })
}

enum OffscreenRenderJob {
    #[cfg(not(target_arch = "wasm32"))]
    Gpu {
        primitive: crate::scene::view::render::Primitive,
        clear_color: [f32; 4],
    },
    Cpu {
        page: PdfPageInput,
    },
}

fn resolve_backend(requested: RequestedRenderer) -> Result<(EffectiveBackend, bool), Value> {
    #[cfg(target_arch = "wasm32")]
    {
        match requested {
            RequestedRenderer::Gpu => Err(failure(
                "renderer_unavailable",
                "wgpu off-screen capture is not available on web; use renderer 'cpu' or 'auto'",
            )),
            RequestedRenderer::Cpu => Ok((EffectiveBackend::Cpu, false)),
            RequestedRenderer::Auto => Ok((EffectiveBackend::Cpu, true)),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match requested {
            RequestedRenderer::Cpu => Ok((EffectiveBackend::Cpu, false)),
            RequestedRenderer::Gpu => {
                if super::offscreen_gpu::gpu_available() {
                    Ok((EffectiveBackend::Gpu, false))
                } else {
                    Err(failure(
                        "renderer_unavailable",
                        super::offscreen_gpu::gpu_unavailable_reason(),
                    ))
                }
            }
            RequestedRenderer::Auto => {
                if super::offscreen_gpu::gpu_available() {
                    Ok((EffectiveBackend::Gpu, false))
                } else {
                    Ok((EffectiveBackend::Cpu, true))
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn make_top_down_camera(
    cx: f64,
    cy: f64,
    vis_bounds: [f64; 4],
    base: &crate::scene::view::camera::Camera,
) -> crate::scene::view::camera::Camera {
    let mut cam = base.clone();
    let vis_h = (vis_bounds[3] - vis_bounds[1]).max(1e-6);
    cam.yaw = 0.0;
    cam.pitch = std::f32::consts::FRAC_PI_2;
    cam.rotation = crate::scene::view::camera::yaw_pitch_to_quat(0.0, cam.pitch, 0.0);
    cam.projection = crate::scene::view::camera::Projection::Orthographic;
    cam.target = glam::DVec3::new(cx, cy, 0.0);
    let fov_y = if cam.fov_y.is_finite() && cam.fov_y > 1e-3 {
        cam.fov_y
    } else {
        45.0_f32.to_radians()
    };
    cam.fov_y = fov_y;
    let tan_half = f64::from((fov_y * 0.5).tan()).max(1e-6);
    cam.distance = ((vis_h * 0.5) / tan_half) as f32;
    cam
}

impl OpenCADStudio {
    /// Settles an async off-screen capture worker result on the active pending operation.
    pub(in crate::app) fn control_offscreen_capture_done(&mut self, result: Result<Value, Value>) {
        self.control.capture_cancel = None;
        match result {
            Ok(v) => self.set_control_result(v),
            Err(e) => self.set_control_failure(e),
        }
    }

    /// Prepares an off-screen render payload inside `with_transient_layout` on
    /// the UI thread, then dispatches rasterization + PNG write to a worker
    /// thread via [`crate::app::update::background_task`].
    pub(super) fn control_capture_offscreen(&mut self, req: &Value) -> Result<iced::Task<Message>, Value> {
        if req.get("scope").and_then(Value::as_str) == Some("window") {
            return Err(failure(
                "invalid_request",
                "scope 'window' requires a live GUI window; use scope 'viewport' for off-screen layout/aspect_fit capture",
            ));
        }
        let path = string(req, "path")?.to_string();
        let requested_renderer = parse_renderer(req)?;
        let view = parse_view(req)?;
        let aspect_fit = parse_aspect_fit(req)?;
        let target_layout = if let Some(raw) = req.get("layout").filter(|v| !v.is_null()) {
            let raw_str = raw
                .as_str()
                .ok_or_else(|| failure("invalid_request", "layout must be a string"))?;
            sheets::canonical_layout_name(&self.tabs[self.active_tab].scene, raw_str)?
        } else {
            self.tabs[self.active_tab].scene.current_layout.clone()
        };

        let max_dimension = req["max_dimension"].as_u64().unwrap_or(1600).clamp(256, 4096) as u32;
        let annotate = req["annotate"].as_bool().unwrap_or(false);
        let focus_handles = parse_hex_handles(req, "focus_handles")?;
        let highlight_handles_vec = parse_hex_handles(req, "highlight_handles")?;
        let highlight_set: Option<rustc_hash::FxHashSet<document::Handle>> =
            (!highlight_handles_vec.is_empty())
                .then(|| highlight_handles_vec.iter().copied().collect());
        let explicit_bounds = parse_explicit_bounds(req)?;

        let (backend, gpu_fallback) = resolve_backend(requested_renderer)?;

        let tab = &self.tabs[self.active_tab];
        let tab_id = tab.id;
        let edit_revision = tab.edit_revision;
        let original_camera_gen = tab.scene.camera_generation;
        let original_layout = tab.scene.current_layout.clone();
        let original_block_edit = tab.scene.block_edit_block;
        let original_camera = tab.scene.camera.borrow().clone();
        let live_vp_size = tab.scene.selection.borrow().view.vp_size;
        let doc_unit = crate::app::properties::insunits_name(
            tab.scene.document.header.insertion_units,
        );
        #[cfg(not(target_arch = "wasm32"))]
        let model_render_mode = tab.scene.active_model_tile_render_mode();

        let (job, img_w, img_h, grounding) = self.tabs[self.active_tab]
            .scene
            .with_transient_layout(&target_layout, |scene| -> Result<_, Value> {
                let paper_space_last = target_layout != "Model"
                    && scene
                        .plot_settings_for(&target_layout)
                        .map(|ps| ps.flags.draw_viewports_first)
                        .unwrap_or(true);
                let layout_block = scene.current_layout_block_handle();

                match backend {
                    EffectiveBackend::Cpu => {
                        let mut content = crate::app::update::file::plot::plot_scene_content(
                            scene,
                            paper_space_last,
                            None,
                        );
                        if target_layout != "Model" {
                            content.group_splits.wires = content.wires
                                [..content.group_splits.wires.min(content.wires.len())]
                                .iter()
                                .filter(|wire| wire.name != "paper_printable_area")
                                .count();
                            std::sync::Arc::make_mut(&mut content.wires)
                                .retain(|wire| wire.name != "paper_printable_area");
                        }
                        if let Some(hl) = &highlight_set {
                            highlight_plot_content(&mut content, hl);
                        }
                        let content_ext = plot_content_extents_inclusive(&content);
                        let target_bounds = resolve_target_bounds(&TargetBoundsContext {
                            scene,
                            target_layout: &target_layout,
                            original_layout: &original_layout,
                            original_block_edit,
                            original_camera: &original_camera,
                            live_vp_size,
                            view,
                            focus_handles: &focus_handles,
                            explicit_bounds,
                            highlight_handles: &highlight_handles_vec,
                            content_ext,
                        })?;
                        let world_w = (target_bounds[2] - target_bounds[0]).max(1e-9);
                        let world_h = (target_bounds[3] - target_bounds[1]).max(1e-9);
                        let (img_w, img_h) = compute_output_dimensions(
                            world_w,
                            world_h,
                            max_dimension,
                            aspect_fit,
                            live_vp_size,
                        );
                        let (page, vis_bounds) =
                            build_offscreen_page(content, target_bounds, img_w, img_h);
                        let grounding = vision::compute_offscreen_grounding(
                            scene,
                            layout_block,
                            vis_bounds,
                            img_w,
                            img_h,
                            highlight_set.as_ref(),
                            true,
                            None,
                        );
                        Ok((OffscreenRenderJob::Cpu { page }, img_w, img_h, grounding))
                    }
                    EffectiveBackend::Gpu => {
                        #[cfg(target_arch = "wasm32")]
                        {
                            unreachable!("GPU backend rejected in resolve_backend on wasm32")
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let top_down_rot = crate::scene::view::camera::yaw_pitch_to_quat(
                                0.0,
                                std::f32::consts::FRAC_PI_2,
                                0.0,
                            );
                            let preserve_3d_camera = target_layout == "Model"
                                && target_layout == original_layout
                                && original_block_edit.is_none()
                                && view == "current"
                                && focus_handles.is_empty()
                                && explicit_bounds.is_none()
                                && (original_camera.projection
                                    != crate::scene::view::camera::Projection::Orthographic
                                    || original_camera.rotation.dot(top_down_rot).abs()
                                        < 1.0 - 1e-4);

                            let (img_w, img_h, vis_bounds, camera_override) = if preserve_3d_camera {
                                let (w, h) = compute_output_dimensions(
                                    1.0,
                                    1.0,
                                    max_dimension,
                                    false,
                                    live_vp_size,
                                );
                                (w, h, [0.0; 4], Some(original_camera.clone()))
                            } else {
                                let target_bounds = resolve_target_bounds(&TargetBoundsContext {
                                    scene,
                                    target_layout: &target_layout,
                                    original_layout: &original_layout,
                                    original_block_edit,
                                    original_camera: &original_camera,
                                    live_vp_size,
                                    view,
                                    focus_handles: &focus_handles,
                                    explicit_bounds,
                                    highlight_handles: &highlight_handles_vec,
                                    content_ext: None,
                                })?;
                                let world_w = (target_bounds[2] - target_bounds[0]).max(1e-9);
                                let world_h = (target_bounds[3] - target_bounds[1]).max(1e-9);
                                let (w, h) = compute_output_dimensions(
                                    world_w,
                                    world_h,
                                    max_dimension,
                                    aspect_fit,
                                    live_vp_size,
                                );
                                let (vis_bounds, _) =
                                    centered_visible_bounds(target_bounds, w, h, false);
                                let cx = (vis_bounds[0] + vis_bounds[2]) * 0.5;
                                let cy = (vis_bounds[1] + vis_bounds[3]) * 0.5;
                                let cam = make_top_down_camera(cx, cy, vis_bounds, &original_camera);
                                *scene.camera.borrow_mut() = cam;
                                (w, h, vis_bounds, None)
                            };

                            let bounds_rect = iced::Rectangle {
                                x: 0.0,
                                y: 0.0,
                                width: img_w as f32,
                                height: img_h as f32,
                            };
                            let primitive = scene.build_offscreen_primitive(
                                bounds_rect,
                                model_render_mode,
                                highlight_set.as_ref(),
                                paper_space_last,
                            );
                            let clear_color = if target_layout == "Model" {
                                scene.bg_color
                            } else {
                                scene.desk_bg_color
                            };
                            let grounding = vision::compute_offscreen_grounding(
                                scene,
                                layout_block,
                                vis_bounds,
                                img_w,
                                img_h,
                                highlight_set.as_ref(),
                                false,
                                camera_override.as_ref(),
                            );
                            Ok((
                                OffscreenRenderJob::Gpu {
                                    primitive,
                                    clear_color,
                                },
                                img_w,
                                img_h,
                                grounding,
                            ))
                        }
                    }
                }
            })?;

        let cancel = Arc::new(AtomicBool::new(false));
        self.control.capture_cancel = Some(Arc::clone(&cancel));
        let worker_cancel = Arc::clone(&cancel);

        let task = crate::app::update::background_task(
            "offscreen_capture",
            move || -> Result<Value, Value> {
                if worker_cancel.load(Ordering::SeqCst) {
                    return Err(failure("cancelled", "Capture cancelled"));
                }

                let (mut image, renderer_name, gpu_adapter, gpu_software): (
                    _,
                    _,
                    Option<String>,
                    Option<bool>,
                ) = match job {
                    OffscreenRenderJob::Cpu { page } => {
                        let rgba = crate::app::update::file::raster_page_rgba(&page, img_w, img_h)
                            .map_err(|e| failure("capture_failed", e))?;
                        (rgba, "cpu", None, None)
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    OffscreenRenderJob::Gpu {
                        primitive,
                        clear_color,
                    } => {
                        let gpu_out = super::offscreen_gpu::render_primitive_rgba(
                            primitive,
                            img_w,
                            img_h,
                            clear_color,
                        )
                        .map_err(|e| failure("capture_failed", e))?;
                        (
                            gpu_out.image,
                            "gpu",
                            Some(gpu_out.adapter_name),
                            Some(gpu_out.software),
                        )
                    }
                };

                if worker_cancel.load(Ordering::SeqCst) {
                    return Err(failure("cancelled", "Capture cancelled"));
                }
                if annotate {
                    draw_som_annotations(&mut image, &grounding.visible_entities);
                }
                if worker_cancel.load(Ordering::SeqCst) {
                    return Err(failure("cancelled", "Capture cancelled"));
                }
                save_capture_png(&image, &path).map_err(|e| failure("capture_failed", e))?;

                let spatial_obj = spatial_metadata_json(doc_unit, &image, &grounding);
                let mut payload = json!({
                    "path": path,
                    "scope": "viewport",
                    "layout": target_layout,
                    "offscreen": true,
                    "renderer": renderer_name,
                    "gpu_fallback": gpu_fallback,
                    "aspect_fit": aspect_fit,
                    "width": image.width(),
                    "height": image.height(),
                    "scale_factor": 1.0,
                    "waited_for_frame": false,
                    "document_id": tab_id,
                    "revision": edit_revision,
                    "camera_revision": original_camera_gen,
                    "_spatial": spatial_obj,
                    "pixel_to_world_matrix": grounding.pixel_to_world_matrix,
                    "world_to_pixel_matrix": grounding.world_to_pixel_matrix,
                    "viewport_world_bounds": grounding.viewport_world_bounds,
                    "camera": grounding.camera,
                    "annotated": annotate,
                    "annotations": grounding.visible_entities.clone(),
                    "visible_entities": grounding.visible_entities,
                });
                if let Some(adapter) = gpu_adapter {
                    payload["gpu_adapter"] = json!(adapter);
                }
                if let Some(sw) = gpu_software {
                    payload["gpu_software"] = json!(sw);
                }
                Ok(payload)
            },
            Message::ControlOffscreenCaptureDone,
        );

        Ok(task)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::session_id;
    use super::super::tests::request;
    use super::*;
    use rstest::rstest;
    use std::path::PathBuf;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(label: &str, ext: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(
                std::env::temp_dir()
                    .join(format!("ocs-offscreen-{}-{seq}-{label}.{ext}", session_id())),
            )
        }

        fn path_str(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn decode_png(path: &PathBuf) -> image::RgbaImage {
        assert!(path.exists(), "PNG file should exist at {}", path.display());
        image::open(path)
            .expect("captured PNG should decode")
            .to_rgba8()
    }

    fn any_non_bg_pixel(img: &image::RgbaImage, bg: [u8; 3]) -> bool {
        img.pixels().any(|p| {
            p[0].abs_diff(bg[0]) > 15 || p[1].abs_diff(bg[1]) > 15 || p[2].abs_diff(bg[2]) > 15
        })
    }

    fn apply_affine(mat: &[Value], x: f64, y: f64) -> (f64, f64) {
        let m: Vec<f64> = mat.iter().map(|v| v.as_f64().unwrap()).collect();
        (m[0] * x + m[1] * y + m[4], m[2] * x + m[3] * y + m[5])
    }

    fn pixel_diff_in_box(a: &image::RgbaImage, b: &image::RgbaImage, bounds: &[Value]) -> usize {
        let (x0, y0, x1, y1) = (
            bounds[0].as_i64().unwrap().saturating_sub(4).max(0) as u32,
            bounds[1].as_i64().unwrap().saturating_sub(4).max(0) as u32,
            (bounds[2].as_i64().unwrap() + 4).min(a.width() as i64 - 1) as u32,
            (bounds[3].as_i64().unwrap() + 4).min(a.height() as i64 - 1) as u32,
        );
        let mut diffs = 0usize;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let pa = a.get_pixel(x, y);
                let pb = b.get_pixel(x, y);
                if pa[0].abs_diff(pb[0]) > 20
                    || pa[1].abs_diff(pb[1]) > 20
                    || pa[2].abs_diff(pb[2]) > 20
                {
                    diffs += 1;
                }
            }
        }
        diffs
    }

    #[rstest]
    #[case("cpu")]
    #[case("gpu")]
    fn dual_backend_rich_scene_fidelity_isolation_and_grounding(#[case] renderer: &str) {
        if renderer == "gpu" && !super::super::offscreen_gpu::gpu_available() {
            return;
        }
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);

        // Write a tiny 8x8 solid bright-red PNG and embed it as an Ole2Frame in Model.
        let red_png = TempFile::new("red-img", "png");
        let red_img = image::RgbaImage::from_pixel(8, 8, image::Rgba([240, 20, 20, 255]));
        red_img.save(&red_png.0).unwrap();
        let emb = request(
            &mut app,
            json!({"op": "embed_image", "path": red_png.path_str(), "at": [10, 50, 0], "width": 30}),
        );
        assert_eq!(emb["ok"], true, "{emb}");

        // Create Line, Hatch (solid), Text, Wipeout, plus off/frozen/invisible/Defpoints entities.
        let created = request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [
                    {"type": "Line", "start": [0, 0], "end": [80, 40]},
                    {"type": "Hatch", "solid": true, "boundary": [[10, 10], [40, 10], [40, 35], [10, 35]]},
                    {"type": "Text", "value": "OCS_TEST", "position": [45, 15], "height": 8.0}
                ]
            }),
        );
        assert_eq!(created["ok"], true, "{created}");
        let handles = created["result"]["handles"].as_array().unwrap();
        let line_hex = handles[0].as_str().unwrap().to_string();
        let hatch_hex = handles[1].as_str().unwrap().to_string();
        let text_hex = handles[2].as_str().unwrap().to_string();
        let line_handle = document::Handle::new(u64::from_str_radix(&line_hex, 16).unwrap());

        let (hidden_hex, frozen_hex, defpoints_hex, model_vp_center) = {
            let scene = &mut app.tabs[app.active_tab].scene;
            let wipeout_h = scene.add_entity(document::EntityType::Wipeout(
                document::entities::Wipeout::polygonal(
                    &[
                        document::types::Vector2::new(60.0, 50.0),
                        document::types::Vector2::new(80.0, 50.0),
                        document::types::Vector2::new(80.0, 70.0),
                        document::types::Vector2::new(60.0, 70.0),
                    ],
                    0.0,
                ),
            ));
            let _ = wipeout_h;

            let mut off_layer = document::tables::Layer::new("OFF_L");
            off_layer.flags.off = true;
            let _ = scene.document.layers.add(off_layer);

            let mut frozen_layer = document::tables::Layer::new("FROZEN_L");
            frozen_layer.flags.frozen = true;
            let _ = scene.document.layers.add(frozen_layer);

            let mut dp_layer = document::tables::Layer::new("Defpoints");
            dp_layer.is_plottable = false;
            let _ = scene.document.layers.add(dp_layer);

            let mut off_line = document::entities::Line::from_points(
                document::types::Vector3::new(5.0, 5.0, 0.0),
                document::types::Vector3::new(25.0, 5.0, 0.0),
            );
            off_line.common.layer = "OFF_L".to_string();
            let off_h = scene.add_entity(document::EntityType::Line(off_line));
            let _ = off_h;

            let mut fr_line = document::entities::Line::from_points(
                document::types::Vector3::new(5.0, 8.0, 0.0),
                document::types::Vector3::new(25.0, 8.0, 0.0),
            );
            fr_line.common.layer = "FROZEN_L".to_string();
            let fr_h = scene.add_entity(document::EntityType::Line(fr_line));

            let mut inv_line = document::entities::Line::from_points(
                document::types::Vector3::new(5.0, 12.0, 0.0),
                document::types::Vector3::new(25.0, 12.0, 0.0),
            );
            inv_line.common.invisible = true;
            let inv_h = scene.add_entity(document::EntityType::Line(inv_line));

            let mut dp_line = document::entities::Line::from_points(
                document::types::Vector3::new(5.0, 15.0, 0.0),
                document::types::Vector3::new(25.0, 15.0, 0.0),
            );
            dp_line.common.layer = "Defpoints".to_string();
            let dp_h = scene.add_entity(document::EntityType::Line(dp_line));

            (
                format!("{:X}", inv_h.value()),
                format!("{:X}", fr_h.value()),
                format!("{:X}", dp_h.value()),
                glam::DVec2::new(40.0, 40.0),
            )
        };

        // Populate Layout1 with a triangular-clipped Model Viewport + overlapping Paper Space line.
        assert_eq!(
            request(&mut app, json!({"op": "layout_activate", "name": "Layout1"}))["ok"],
            true
        );
        let paper_line = request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [{"type": "Line", "start": [30, 30], "end": [250, 180]}]
            }),
        );
        let paper_hex = paper_line["result"]["handles"][0]
            .as_str()
            .unwrap()
            .to_string();
        {
            let scene = &mut app.tabs[app.active_tab].scene;
            let mut clip_poly = document::entities::LwPolyline::from_points(vec![
                document::types::Vector2::new(30.0, 30.0),
                document::types::Vector2::new(260.0, 30.0),
                document::types::Vector2::new(145.0, 190.0),
            ]);
            clip_poly.is_closed = true;
            let clip_h = scene.add_entity(document::EntityType::LwPolyline(clip_poly));

            let mut vp = document::entities::Viewport::new();
            vp.id = 2;
            vp.center = document::types::Vector3::new(145.0, 110.0, 0.0);
            vp.width = 230.0;
            vp.height = 160.0;
            vp.view_center =
                document::types::Vector3::new(model_vp_center.x, model_vp_center.y, 0.0);
            vp.view_height = 100.0;
            vp.clip_boundary_handle = clip_h;
            vp.status.non_rectangular_clipping = true;
            vp.status.is_off = false;
            let _ = scene.add_entity(document::EntityType::Viewport(Box::new(vp)));
        }

        // Switch back to Model, select a line, set custom camera, verify zero disturbance.
        assert_eq!(
            request(&mut app, json!({"op": "layout_activate", "name": "Model"}))["ok"],
            true
        );
        let idx = app.active_tab;
        {
            let scene = &mut app.tabs[idx].scene;
            scene.deselect_all();
            scene.select_entity(line_handle, false);
            {
                let mut cam = scene.camera.borrow_mut();
                cam.target = glam::DVec3::new(17.5, -9.25, 0.0);
                cam.distance = 123.0;
            }
            scene.camera_generation += 7;
        }
        let orig_cam_target = app.tabs[idx].scene.camera.borrow().target;
        let orig_cam_distance = app.tabs[idx].scene.camera.borrow().distance;
        let orig_cam_gen = app.tabs[idx].scene.camera_generation;
        let orig_selection = app.tabs[idx].scene.selected_handles_in_order();

        let layout_file = TempFile::new(&format!("layout1-{renderer}"), "png");
        let cap_layout = request(
            &mut app,
            json!({
                "op": "capture",
                "path": layout_file.path_str(),
                "layout": "Layout1",
                "view": "extents",
                "renderer": renderer,
                "max_dimension": 800,
                "annotate": true
            }),
        );
        assert_eq!(cap_layout["ok"], true, "{cap_layout}");
        assert_eq!(cap_layout["result"]["renderer"], renderer);
        assert_eq!(cap_layout["result"]["layout"], "Layout1");
        assert_eq!(cap_layout["result"]["offscreen"], true);
        assert_eq!(cap_layout["result"]["annotated"], true);

        // Verify zero live-state mutation.
        {
            let scene = &app.tabs[idx].scene;
            assert_eq!(scene.current_layout, "Model");
            assert_eq!(scene.camera.borrow().target, orig_cam_target);
            assert_eq!(scene.camera.borrow().distance, orig_cam_distance);
            assert_eq!(scene.camera_generation, orig_cam_gen);
            assert_eq!(scene.selected_handles_in_order(), orig_selection);
        }

        let layout_img = decode_png(&layout_file.0);
        // Embedded red image inside Model viewport must appear with bright-red pixels on BOTH backends!
        let has_red = layout_img
            .pixels()
            .any(|p| p[0] > 180 && p[1] < 80 && p[2] < 80);
        assert!(has_red, "Expected embedded red image inside Layout1 viewport on {renderer}");
        // Viewport-projected Model entities are also grounded cleanly alongside Paper entities!
        let layout_anns = cap_layout["result"]["_spatial"]["annotations"]
            .as_array()
            .unwrap();
        assert!(
            layout_anns.iter().any(|e| e["handle"].as_str() == Some(paper_hex.as_str()) && e.get("viewport").is_none()),
            "Layout1 annotations should include paper line: {layout_anns:?}"
        );
        assert!(
            layout_anns.iter().any(|e| e["handle"].as_str() == Some(line_hex.as_str()) && e.get("viewport").is_some()),
            "Layout1 annotations should include projected Model entity with viewport field: {layout_anns:?}"
        );

        // Reverse direction: activate Layout1, warm wire_cache, capture Model unhighlighted vs highlighted!
        assert_eq!(
            request(&mut app, json!({"op": "layout_activate", "name": "Layout1"}))["ok"],
            true
        );
        app.tabs[app.active_tab].scene.deselect_all();
        let _ = app.tabs[app.active_tab].scene.entity_wires();

        let base_file = TempFile::new(&format!("model-base-{renderer}"), "png");
        let cap_base = request(
            &mut app,
            json!({
                "op": "capture",
                "path": base_file.path_str(),
                "layout": "Model",
                "view": "extents",
                "renderer": renderer,
                "max_dimension": 800
            }),
        );
        assert_eq!(cap_base["ok"], true, "{cap_base}");
        assert_eq!(app.tabs[app.active_tab].scene.current_layout, "Layout1");

        let hl_file = TempFile::new(&format!("model-hl-{renderer}"), "png");
        let cap_hl = request(
            &mut app,
            json!({
                "op": "capture",
                "path": hl_file.path_str(),
                "layout": "Model",
                "view": "extents",
                "renderer": renderer,
                "max_dimension": 800,
                "highlight_handles": [line_hex, hatch_hex, text_hex]
            }),
        );
        assert_eq!(cap_hl["ok"], true, "{cap_hl}");
        assert_eq!(cap_hl["result"]["annotated"], false);
        assert!(
            app.tabs[app.active_tab]
                .scene
                .selected_handles_in_order()
                .is_empty(),
            "highlight_handles must not mutate live selection"
        );

        let base_img = decode_png(&base_file.0);
        let hl_img = decode_png(&hl_file.0);
        let anns = cap_base["result"]["_spatial"]["annotations"]
            .as_array()
            .unwrap();
        let ann_handles: Vec<&str> = anns.iter().filter_map(|e| e["handle"].as_str()).collect();
        assert!(ann_handles.contains(&line_hex.as_str()));
        assert!(ann_handles.contains(&hatch_hex.as_str()));
        assert!(ann_handles.contains(&text_hex.as_str()));
        assert!(!ann_handles.contains(&paper_hex.as_str()));
        assert!(!ann_handles.contains(&hidden_hex.as_str()));
        assert!(!ann_handles.contains(&frozen_hex.as_str()));
        if renderer == "cpu" {
            assert!(!ann_handles.contains(&defpoints_hex.as_str()));
        } else {
            assert!(ann_handles.contains(&defpoints_hex.as_str()));
        }

        // Verify highlight_handles visibly changed pixels around BOTH Line and Hatch (and Text/SoM)!
        for h in [&line_hex, &hatch_hex, &text_hex] {
            let ent = anns
                .iter()
                .find(|e| e["handle"].as_str() == Some(h.as_str()))
                .unwrap();
            let sb = ent["screen_bounds"].as_array().unwrap();
            let diffs = pixel_diff_in_box(&base_img, &hl_img, sb);
            assert!(diffs > 0, "highlight_handles should change pixels for {h} on {renderer}");
        }
    }

    #[test]
    fn offscreen_framing_geometry_and_validation_matrix() {
        let mut app = OpenCADStudio::new_for_test();
        let err_start = TempFile::new("start", "png");
        let start_res = app.control_request(json!({
            "protocol": 1,
            "op": "capture",
            "request_id": "start-cap",
            "document_id": app.tabs[app.active_tab].id,
            "path": err_start.path_str(),
            "layout": "Model"
        })).0;
        assert_eq!(start_res["ok"], false, "{start_res}");
        assert_eq!(start_res["code"], "no_document");

        // 1. Tall/wide bounds, matrix roundtrip, and explicit `aspect_fit: false`.
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        let created = request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [
                    {"type": "Line", "start": [0, 0], "end": [10, 100]},
                    {"type": "Line", "start": [20, 20], "end": [60, 40]}
                ]
            }),
        );
        let h1 = created["result"]["handles"][0].as_str().unwrap().to_string();

        let tall = TempFile::new("tall", "png");
        let r_tall = request(
            &mut app,
            json!({
                "op": "capture",
                "path": tall.path_str(),
                "aspect_fit": true,
                "bounds": [0, 0, 10, 100],
                "max_dimension": 800,
                "renderer": "cpu"
            }),
        );
        assert_eq!(r_tall["ok"], true, "{r_tall}");
        assert_eq!(r_tall["result"]["width"], 80);
        assert_eq!(r_tall["result"]["height"], 800);
        let img_tall = decode_png(&tall.0);
        assert!(any_non_bg_pixel(&img_tall, [255, 255, 255]));
        let (wx, wy) = apply_affine(
            r_tall["result"]["pixel_to_world_matrix"].as_array().unwrap(),
            40.0,
            400.0,
        );
        assert!((wx - 5.0).abs() < 1e-6 && (wy - 50.0).abs() < 1e-6);

        let fixed_aspect = TempFile::new("fixed-aspect", "png");
        let r_fixed = request(
            &mut app,
            json!({
                "op": "capture",
                "path": fixed_aspect.path_str(),
                "layout": "Model",
                "aspect_fit": false,
                "bounds": [0, 0, 10, 100],
                "max_dimension": 800,
                "renderer": "cpu"
            }),
        );
        assert_eq!(r_fixed["ok"], true, "{r_fixed}");
        assert_eq!(r_fixed["result"]["aspect_fit"], false);
        assert_eq!(r_fixed["result"]["width"], 800);
        assert_eq!(r_fixed["result"]["height"], 600);

        // focus_handles framing + view: "selection" + view: "current" (with live vp_size)
        let focus = TempFile::new("focus", "png");
        let r_focus = request(
            &mut app,
            json!({
                "op": "capture",
                "path": focus.path_str(),
                "layout": "Model",
                "focus_handles": [h1],
                "renderer": "cpu"
            }),
        );
        assert_eq!(r_focus["ok"], true, "{r_focus}");
        assert!(any_non_bg_pixel(&decode_png(&focus.0), [255, 255, 255]));

        // 2. Single-axis horizontal line, single point, empty layout, UTM coordinates.
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [{"type": "Line", "start": [100, 50], "end": [300, 50]}]
            }),
        );
        let horiz = TempFile::new("horiz", "png");
        let r_horiz = request(
            &mut app,
            json!({
                "op": "capture",
                "path": horiz.path_str(),
                "layout": "Model",
                "view": "extents",
                "renderer": "cpu",
                "max_dimension": 600
            }),
        );
        assert_eq!(r_horiz["ok"], true, "{r_horiz}");
        assert!(
            any_non_bg_pixel(&decode_png(&horiz.0), [255, 255, 255]),
            "horizontal-only Model drawing must not render blank"
        );

        // UTM scale (> 100 km span where old 1e-6 clamp would crop)
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [{"type": "Line", "start": [100_000_000.0, 200_000_000.0], "end": [200_000_000.0, 250_000_000.0]}]
            }),
        );
        let utm = TempFile::new("utm", "png");
        let r_utm = request(
            &mut app,
            json!({
                "op": "capture",
                "path": utm.path_str(),
                "layout": "Model",
                "view": "extents",
                "renderer": "cpu",
                "max_dimension": 400
            }),
        );
        assert_eq!(r_utm["ok"], true, "{r_utm}");
        assert!(any_non_bg_pixel(&decode_png(&utm.0), [255, 255, 255]));

        // Empty layout
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        let empty_f = TempFile::new("empty", "png");
        let r_empty = request(
            &mut app,
            json!({
                "op": "capture",
                "path": empty_f.path_str(),
                "layout": "Model",
                "view": "extents",
                "renderer": "cpu"
            }),
        );
        assert_eq!(r_empty["ok"], true, "{r_empty}");

        // 3. Strict validation error matrix
        for (bad_req, expected_code) in [
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"Missing"}), "layout_missing"),
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"   "}), "name_required"),
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"Model","scope":"window"}), "invalid_request"),
            (json!({"op":"capture","path":empty_f.path_str(),"aspect_fit":true,"bounds":[5,5,5,10]}), "invalid_request"),
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"Model","focus_handles":["not-hex"]}), "invalid_request"),
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"Model","view":"bogus"}), "invalid_request"),
            (json!({"op":"capture","path":empty_f.path_str(),"layout":"Model","renderer":"vulkan_magic"}), "invalid_request"),
        ] {
            let resp = request(&mut app, bad_req);
            assert_eq!(resp["ok"], false, "{resp}");
            assert_eq!(resp["code"], expected_code, "{resp}");
        }
    }

    #[test]
    fn offscreen_async_cancel_command_busy_and_inactive_tab() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        let tab1_id = app.tabs[app.active_tab].id;
        let created = request(
            &mut app,
            json!({
                "op": "entities_create",
                "entities": [{"type": "Line", "start": [0, 0], "end": [40, 30]}]
            }),
        );
        let tab1_hex = created["result"]["handles"][0].as_str().unwrap().to_string();

        // 1. Cancel a stale pending live capture, then ensure an offscreen capture completes cleanly.
        app.control.capture_frame_pending = true;
        let _ = request(&mut app, json!({"op": "cancel"}));
        assert!(
            !app.control.capture_frame_pending,
            "cancel must clear stale capture_frame_pending"
        );

        // 2. Start an interactive command owned by "test", then capture from "other-client":
        // offscreen capture must NOT hit command_busy, must settle as "completed" (not
        // "waiting_input"), and must preserve "test" as the active command owner.
        let _ = request(&mut app, json!({"op": "start", "cmd": "LINE"}));
        assert_eq!(app.control.owner, Some((tab1_id, "test".to_string())));
        let busy_cap = TempFile::new("during-cmd", "png");
        let (acc_busy, busy_task) = app.control_request(json!({
            "protocol": 1,
            "op": "capture",
            "request_id": "busy-cap-1",
            "document_id": tab1_id,
            "client_id": "other-client",
            "path": busy_cap.path_str(),
            "layout": "Model",
            "view": "extents",
            "renderer": "cpu"
        }));
        assert_eq!(acc_busy["ok"], true, "{acc_busy}");
        app.drive_headless_task(busy_task).unwrap();
        let settled_busy = app
            .control_request(json!({"op": "operation", "request_id": "busy-cap-1"}))
            .0;
        assert_eq!(settled_busy["ok"], true, "{settled_busy}");
        assert_eq!(settled_busy["status"], "completed", "{settled_busy}");
        assert_eq!(app.control.owner, Some((tab1_id, "test".to_string())));
        let _ = request(&mut app, json!({"op": "cancel", "force": true}));

        // 3. Inactive document tab async background_task lifecycle (`accepted` -> `drive_headless_task` -> `completed`).
        assert_eq!(request(&mut app, json!({"op": "new"}))["ok"], true);
        let visible_idx = app.active_tab;
        let visible_id = app.tabs[visible_idx].id;
        assert_ne!(visible_id, tab1_id);
        let tab1_rev = app
            .tabs
            .iter()
            .find(|t| t.id == tab1_id)
            .unwrap()
            .edit_revision;

        let bg_file = TempFile::new("bg-tab", "png");
        let (acc, task) = app.control_request(json!({
            "protocol": 1,
            "op": "capture",
            "request_id": "async-bg-1",
            "document_id": tab1_id,
            "revision": tab1_rev,
            "client_id": "test",
            "path": bg_file.path_str(),
            "layout": "Model",
            "view": "extents",
            "renderer": "cpu"
        }));
        assert_eq!(acc["ok"], true, "{acc}");
        assert_eq!(acc["status"], "accepted", "{acc}");

        app.drive_headless_task(task).unwrap();
        let settled = app
            .control_request(json!({"op": "operation", "request_id": "async-bg-1"}))
            .0;
        assert_eq!(settled["ok"], true, "{settled}");
        assert_eq!(settled["status"], "completed", "{settled}");
        assert_eq!(settled["result"]["document_id"], tab1_id);
        assert_eq!(settled["result"]["renderer"], "cpu");
        assert_eq!(app.active_tab, visible_idx);
        assert_eq!(app.tabs[app.active_tab].id, visible_id);

        let anns = settled["result"]["_spatial"]["annotations"]
            .as_array()
            .unwrap();
        assert!(
            anns.iter()
                .any(|e| e["handle"].as_str() == Some(tab1_hex.as_str())),
            "inactive tab async capture must ground its own entity: {anns:?}"
        );
        assert!(any_non_bg_pixel(&decode_png(&bg_file.0), [255, 255, 255]));
    }
}
