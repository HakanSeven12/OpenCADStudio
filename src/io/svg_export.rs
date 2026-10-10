// SVG export — vector plot from the shared paper-space `PlotPage` model.
//
// Each page becomes one standalone `.svg` file in millimetre user units
// (`viewBox="0 0 paper_w paper_h"`), Y-flipped because SVG's origin is
// top-left while the plot geometry (like PDF) is bottom-left. Rotation and
// plot scale are baked into the coordinates with the same transform the PDF
// backend puts in its page CTM, so the two outputs agree geometrically.
//
// Scope notes (prototype):
// - Text goes out as searchable `<text>` runs (same `SearchableTextRun` source
//   as the PDF search layer). SDF-quad vector outlines are not emitted, and
//   there is no `Tz`-style advance width-matching: centered/right-aligned runs
//   may highlight slightly off their glyphs in text-heavy viewers.
// - Stationed dash phases (`pattern_stations`, PLINEGEN) are approximated with
//   a plain `stroke-dasharray`; non-affine (perspective) image quads are drawn
//   with their affine corner fit.
// - Like PDF export this is native-only for file writing; `build_svg` itself
//   is pure and works on wasm too.

use std::path::Path;

use crate::io::plot::{PlotImage, PlotOptions, PlotPage, PlotWire};
use crate::io::plot_style::PlotStyleTable;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::WireModel;

/// Screen pixels (96 dpi) to millimetres.
const PX_TO_MM: f64 = 25.4 / 96.0;
/// PDF points to millimetres (for CTB lineweights, which resolve in mm already
/// here — kept for the hairline floor only).
const PT_TO_MM: f64 = 25.4 / 72.0;
/// Helvetica cap-height fraction of em, same value the PDF backend uses to
/// size its fallback font from a run's cap height.
const HELVETICA_CAP_RATIO: f64 = 0.72;

// ── Public entry points ────────────────────────────────────────────────────

/// Export one page to an SVG file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_svg(page: &PlotPage, path: &Path) -> Result<(), String> {
    export_svg_pages(std::slice::from_ref(page), path, None)
}

/// Export several pages. One page writes `path` itself; several write
/// `<stem>-1.svg`, `<stem>-2.svg`, … beside `path` (SVG has no multi-page
/// container, unlike PDF).
#[cfg(not(target_arch = "wasm32"))]
pub fn export_svg_pages(
    pages: &[PlotPage],
    path: &Path,
    plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("No pages were selected.".into());
    }
    if pages.len() == 1 {
        let svg = build_svg(&pages[0], plot_style)?;
        return write_svg_atomically(path, svg.as_bytes());
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".to_string());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for (index, page) in pages.iter().enumerate() {
        let svg = build_svg(page, plot_style)
            .map_err(|e| format!("Page {}: {e}", index + 1))?;
        let numbered = parent.join(format!("{stem}-{}.svg", index + 1));
        write_svg_atomically(&numbered, svg.as_bytes())
            .map_err(|e| format!("Page {}: {e}", index + 1))?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn export_svg(_page: &PlotPage, _path: &Path) -> Result<(), String> {
    Err("SVG export is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub fn export_svg_pages(
    _pages: &[PlotPage],
    _path: &Path,
    _plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    Err("SVG export is not available in the web version.".into())
}

/// Write beside the destination, then rename atomically (same pattern as the
/// PDF and DWG savers: a failed write never truncates the previous file).
#[cfg(not(target_arch = "wasm32"))]
fn write_svg_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp_path = super::save_temp_path(path);
    if let Err(error) = std::fs::write(&temp_path, bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write SVG data: {error}"));
    }
    if let Err(error) = super::replace_save_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to replace SVG file: {error}"));
    }
    Ok(())
}

/// Build a standalone SVG document for one plot page.
pub fn build_svg(
    page: &PlotPage,
    fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<String, String> {
    let paper_w = page.paper_w.max(1.0);
    let paper_h = page.paper_h.max(1.0);
    let plot_style = page.plot_style.as_ref().or(fallback_plot_style);
    let map = PageMap {
        paper_w,
        paper_h,
        scale: page.scale.max(1e-6) as f64,
        rotation_deg: page.rotation_deg,
    };
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    out.push_str(&format!(
        " width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\"",
        fmt(paper_w),
        fmt(paper_h),
        fmt(paper_w),
        fmt(paper_h)
    ));
    out.push_str(">\n<title>Open CAD Studio plot</title>\n");
    out.push_str("<rect x=\"0\" y=\"0\"");
    out.push_str(&format!(" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n", fmt(paper_w), fmt(paper_h)));

    let mut clip_id: Option<String> = None;
    if let Some((cx, cy, cw, ch)) = page.clip {
        // Same pre-transform space as the PDF clip path: map the corners and
        // take the bounds (rotation keeps axis-alignment for 0/90/180/270).
        let corners = [
            map.apply(cx as f64, cy as f64),
            map.apply((cx + cw) as f64, cy as f64),
            map.apply((cx + cw) as f64, (cy + ch) as f64),
            map.apply(cx as f64, (cy + ch) as f64),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x1 > x0 && y1 > y0 && x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite() {
            out.push_str(&format!(
                "<clipPath id=\"plot-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n",
                fmt(x0), fmt(y0), fmt(x1 - x0), fmt(y1 - y0)
            ));
            clip_id = Some("plot-clip".to_string());
        }
    }

    let blend = if page.options.merge_lines {
        " style=\"mix-blend-mode:multiply\""
    } else {
        ""
    };
    out.push_str(&format!("<g{blend}"));
    if let Some(id) = &clip_id {
        out.push_str(&format!(" clip-path=\"url(#{id})\""));
    }
    out.push_str(" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n");

    emit_page_content(&mut out, page, plot_style, &map)?;

    out.push_str("</g>\n");
    if page.options.stamp {
        out.push_str(&stamp_element(paper_w, paper_h));
    }
    out.push_str("</svg>\n");
    Ok(out)
}

// ── Page transform ─────────────────────────────────────────────────────────

/// Bakes the PDF page CTM (plot scale + 0/90/180/270° rotation) plus the SVG
/// Y-flip into one mapping from pre-transform paper millimetres to SVG units.
struct PageMap {
    paper_w: f64,
    paper_h: f64,
    scale: f64,
    rotation_deg: i32,
}

impl PageMap {
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let s = self.scale;
        let (xt, yt) = match self.rotation_deg {
            90 => (self.paper_w - s * y, s * x),
            180 => (self.paper_w - s * x, self.paper_h - s * y),
            270 => (s * y, self.paper_h - s * x),
            _ => (s * x, s * y),
        };
        (xt, self.paper_h - yt)
    }
}

// ── Draw-ordered emission (mirrors the PDF backend) ────────────────────────

enum DrawItem<'a> {
    WireFill(&'a PlotWire),
    Hatch(&'a HatchModel),
    Image(&'a PlotImage),
    Wire(&'a PlotWire),
    Text(&'a PlotWire),
}

fn emit_page_content(
    out: &mut String,
    page: &PlotPage,
    plot_style: Option<&PlotStyleTable>,
    map: &PageMap,
) -> Result<(), String> {
    let content = &page.content;
    let (ox, oy) = (page.offset_x, page.offset_y);
    let options = page.options;

    let (first_wires, second_wires) = content
        .wires
        .split_at(content.group_splits.wires.min(content.wires.len()));
    let (first_hatches, second_hatches) = content
        .hatches
        .split_at(content.group_splits.hatches.min(content.hatches.len()));
    let (first_wipeouts, second_wipeouts) = content
        .wipeouts
        .split_at(content.group_splits.wipeouts.min(content.wipeouts.len()));
    let (first_images, second_images) = content
        .images
        .split_at(content.group_splits.images.min(content.images.len()));

    for (wires, hatches, wipeouts, images) in [
        (first_wires, first_hatches, first_wipeouts, first_images),
        (second_wires, second_hatches, second_wipeouts, second_images),
    ] {
        let mut items = Vec::with_capacity(wires.len() * 2 + hatches.len() + wipeouts.len() + images.len());
        let mut sequence = 0usize;
        for wire in wires {
            if !wire.fill_tris.is_empty() {
                items.push((wire.draw_depth, 0u8, sequence, DrawItem::WireFill(wire)));
                sequence += 1;
            }
            items.push((wire.draw_depth, 2u8, sequence, DrawItem::Wire(wire)));
            sequence += 1;
            if !wire.searchable_text.is_empty() {
                items.push((wire.draw_depth, 3u8, sequence, DrawItem::Text(wire)));
                sequence += 1;
            }
        }
        for hatch in wipeouts.iter().chain(hatches.iter()) {
            items.push((hatch.draw_depth, 1u8, sequence, DrawItem::Hatch(hatch)));
            sequence += 1;
        }
        for image in images {
            items.push((image.image.draw_depth, 1u8, sequence, DrawItem::Image(image)));
            sequence += 1;
        }
        items.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        for (_, _, _, item) in items {
            match item {
                DrawItem::WireFill(wire) => emit_wire_fills(out, &wire.wire, ox, oy, plot_style, options, map),
                DrawItem::Hatch(hatch) => emit_hatch(out, hatch, ox, oy, plot_style, options, map),
                DrawItem::Text(wire) => emit_searchable_text(out, &wire.wire, ox, oy, plot_style, options, map),
                DrawItem::Wire(wire) => emit_wire(out, &wire.wire, ox, oy, plot_style, options, map),
                DrawItem::Image(image) => emit_image(out, image, ox, oy, options, map)?,
            }
        }
    }
    Ok(())
}

// ── Wires ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn emit_wire(
    out: &mut String,
    wire: &WireModel,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    options: PlotOptions,
    map: &PageMap,
) {
    let [mut r, mut g, mut b, a] = wire.color;
    if a < 0.01 {
        return;
    }
    if matches!(wire.name.as_str(), "__paper_boundary__" | "paper_printable_area") {
        return;
    }
    let mut lw_override_mm: Option<f64> = None;
    let mut screening = 1.0;
    let mut color_overridden = false;
    let mut cap = "round";
    let mut join = "round";
    if let Some(table) = plot_style {
        if wire.aci > 0 {
            if let Some([cr, cg, cb]) = table.resolve_color(wire.aci) {
                [r, g, b] = [cr, cg, cb];
                color_overridden = true;
            }
            lw_override_mm = table.resolve_lineweight(wire.aci).map(|mm| mm as f64);
            screening = table.resolve_screening(wire.aci);
            if let Some(entry) = table.aci_entries.get(wire.aci as usize) {
                cap = match entry.end_style {
                    0 => "butt",
                    1 | 3 => "square",
                    _ => "round",
                };
                join = match entry.join_style {
                    0 => "miter",
                    1 | 3 => "bevel",
                    _ => "round",
                };
            }
        }
    }
    if !color_overridden && (wire.aci != 0 || [r, g, b] == [1.0; 3]) {
        [r, g, b] = adapt_text_color([r, g, b]);
    }
    [r, g, b] = plotted_color([r, g, b], a, screening, options);

    let s = map.scale;
    let width_mm = if wire.world_width > 0.0 {
        wire.world_width as f64 * s
    } else {
        let physical = if options.object_lineweights {
            lw_override_mm.unwrap_or_else(|| wire.line_weight_px as f64 * PX_TO_MM)
        } else {
            0.1 * PT_TO_MM
        }
        .max(0.1 * PT_TO_MM);
        if options.scale_lineweights {
            physical * s
        } else {
            physical
        }
    };

    let dash = svg_dasharray(wire.pattern_length, &wire.pattern, s);
    let dash_attr = dash
        .map(|d| format!(" stroke-dasharray=\"{d}\""))
        .unwrap_or_default();
    let opacity_attr = svg_opacity(a, options.transparency);

    // NaN separates sub-paths (pen-up), like the PDF `flush_line` pass.
    let view_h = map.paper_h / s;
    let mut d = String::new();
    let mut pen_down = false;
    let mut any = false;
    for (pi, &[x, y, _]) in wire.points.iter().enumerate() {
        if x.is_nan() || y.is_nan() {
            pen_down = false;
            continue;
        }
        let p = wire.point_world(pi, view_h);
        let (sx, sy) = map.apply(p.x + ox, p.y + oy);
        if !sx.is_finite() || !sy.is_finite() {
            pen_down = false;
            continue;
        }
        d.push_str(&format!("{} {} {} ", if pen_down { 'L' } else { 'M' }, fmt(sx), fmt(sy)));
        pen_down = true;
        any = true;
    }
    if !any {
        return;
    }
    out.push_str(&format!(
        "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"{dash_attr}{opacity_attr}/>\n",
        d.trim_end(),
        hex_color([r, g, b]),
        fmt(width_mm.max(0.01)),
    ));
}

/// Solid triangle fills (SOLID fills, arrowheads, 2-D overlays).
fn emit_wire_fills(
    out: &mut String,
    wire: &WireModel,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    options: PlotOptions,
    map: &PageMap,
) {
    if wire.fill_tris.is_empty() {
        return;
    }
    let [mut r, mut g, mut b, a] = wire.color;
    if a < 0.01 {
        return;
    }
    if wire.bg_adapt.as_deref().is_some_and(|adapt| adapt.canvas_color) {
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
    let mut d = String::new();
    for (ti, tri) in wire.fill_tris.chunks_exact(3).enumerate() {
        let mut pts = Vec::with_capacity(3);
        let mut ok = true;
        for (pi, &[x, y, _]) in tri.iter().enumerate() {
            let low = wire.fill_tris_low.get(ti * 3 + pi).copied().unwrap_or([0.0; 3]);
            let (sx, sy) = map.apply(x as f64 + low[0] as f64 + ox, y as f64 + low[1] as f64 + oy);
            if !sx.is_finite() || !sy.is_finite() {
                ok = false;
                break;
            }
            pts.push((sx, sy));
        }
        if !ok {
            continue;
        }
        d.push_str(&format!(
            "M {} {} L {} {} L {} {} Z ",
            fmt(pts[0].0), fmt(pts[0].1),
            fmt(pts[1].0), fmt(pts[1].1),
            fmt(pts[2].0), fmt(pts[2].1),
        ));
    }
    if d.is_empty() {
        return;
    }
    out.push_str(&format!(
        "<path d=\"{}\" fill=\"{}\" stroke=\"none\"{}/>\n",
        d.trim_end(),
        hex_color([r, g, b]),
        svg_opacity(a, options.transparency),
    ));
}

// ── Hatches / wipeouts ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn emit_hatch(
    out: &mut String,
    hatch: &HatchModel,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    options: PlotOptions,
    map: &PageMap,
) {
    if hatch.boundary.is_empty() {
        return;
    }
    let is_wipeout = hatch.name == "WIPEOUT_FILL";
    let [mut r, mut g, mut b, a] = hatch.color;
    if a < 0.01 {
        return;
    }
    let mut screening = 1.0;
    let mut lw_override_mm: Option<f64> = None;
    let mut color_overridden = false;
    if !is_wipeout {
        if let Some(table) = plot_style {
            if hatch.aci > 0 {
                if let Some([cr, cg, cb]) = table.resolve_color(hatch.aci) {
                    [r, g, b] = [cr, cg, cb];
                    color_overridden = true;
                }
                screening = table.resolve_screening(hatch.aci);
                lw_override_mm = table.resolve_lineweight(hatch.aci).map(|mm| mm as f64);
            }
        }
    }
    if is_wipeout {
        [r, g, b] = [1.0, 1.0, 1.0];
    } else if !color_overridden && hatch.aci != 0 && !(hatch.aci == 7 && matches!(hatch.pattern, HatchPattern::Solid)) {
        let is_light = r > 0.80
            && g > 0.80
            && b > 0.80
            && !crate::scene::convert::tess_util::is_authored_white([r, g, b]);
        let is_yellow = r > 0.80 && g > 0.70 && b < 0.30;
        let is_cyan = r < 0.30 && g > 0.70 && b > 0.70;
        if is_light || is_yellow {
            [r, g, b] = [0.0, 0.0, 0.0];
        } else if is_cyan {
            [r, g, b] = [0.0, 0.15, 0.50];
        }
    }
    [r, g, b] = plotted_color([r, g, b], a, screening, options);
    let (wox, woy) = (hatch.world_origin[0], hatch.world_origin[1]);

    if matches!(hatch.pattern, HatchPattern::Pattern(_)) {
        let s = map.scale;
        let physical = if options.object_lineweights {
            lw_override_mm.unwrap_or_else(|| hatch.line_weight_px as f64 * PX_TO_MM)
        } else {
            0.1 * PT_TO_MM
        }
        .max(0.1 * PT_TO_MM);
        let width_mm = if options.scale_lineweights { physical * s } else { physical };
        let segments = hatch.pattern_segments_for_plot();
        if segments.is_empty() {
            return;
        }
        let mut d = String::new();
        for [p, q] in segments {
            let (ax, ay) = map.apply(p[0] + ox, p[1] + oy);
            let (bx, by) = map.apply(q[0] + ox, q[1] + oy);
            if !(ax.is_finite() && ay.is_finite() && bx.is_finite() && by.is_finite()) {
                continue;
            }
            if (ax - bx).abs() < 1e-9 && (ay - by).abs() < 1e-9 {
                let rad = fmt((width_mm * 0.5).max(0.02));
                let diam = fmt(width_mm.max(0.04));
                d.push_str(&format!("M {x} {y} m -{rad} 0 a {rad} {rad} 0 1 0 {diam} 0 a {rad} {rad} 0 1 0 -{diam} 0 ", x = fmt(ax), y = fmt(ay)));
            } else {
                d.push_str(&format!("M {} {} L {} {} ", fmt(ax), fmt(ay), fmt(bx), fmt(by)));
            }
        }
        if d.is_empty() {
            return;
        }
        out.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n",
            d.trim_end(),
            hex_color([r, g, b]),
            fmt(width_mm.max(0.01)),
            svg_opacity(a, options.transparency),
        ));
        return;
    }

    if let HatchPattern::Gradient { color2, .. } = &hatch.pattern {
        let second = plotted_color(
            adapt_text_color([color2[0], color2[1], color2[2]]),
            color2[3],
            screening,
            options,
        );
        [r, g, b] = [(r + second[0]) * 0.5, (g + second[1]) * 0.5, (b + second[2]) * 0.5];
    }

    let mut d = String::new();
    let mut current_empty = true;
    let mut any = false;
    for &[bx, by] in hatch.boundary.iter() {
        if bx.is_nan() || by.is_nan() {
            if !current_empty {
                d.push_str("Z ");
            }
            current_empty = true;
            continue;
        }
        let (sx, sy) = map.apply(bx as f64 + wox + ox, by as f64 + woy + oy);
        if !sx.is_finite() || !sy.is_finite() {
            continue;
        }
        d.push_str(&format!("{} {} {} ", if current_empty { 'M' } else { 'L' }, fmt(sx), fmt(sy)));
        current_empty = false;
        any = true;
    }
    if !current_empty {
        d.push_str("Z ");
    }
    if !any {
        return;
    }
    out.push_str(&format!(
        "<path d=\"{}\" fill=\"{}\" fill-rule=\"evenodd\" stroke=\"none\"{}/>\n",
        d.trim_end(),
        hex_color([r, g, b]),
        svg_opacity(a, options.transparency),
    ));
}

// ── Searchable text as visible <text> ──────────────────────────────────────

fn emit_searchable_text(
    out: &mut String,
    wire: &WireModel,
    ox: f64,
    oy: f64,
    plot_style: Option<&PlotStyleTable>,
    options: PlotOptions,
    map: &PageMap,
) {
    let mut ctb_color: Option<[f32; 3]> = None;
    let mut screening = 1.0;
    if let Some(table) = plot_style {
        if wire.aci > 0 {
            ctb_color = table.resolve_color(wire.aci);
            screening = table.resolve_screening(wire.aci);
        }
    }
    for run in &wire.searchable_text {
        if run.text.is_empty() || run.height <= 0.0 || !run.height.is_finite() {
            continue;
        }
        let rgb = ctb_color
            .unwrap_or_else(|| adapt_text_color([run.color[0], run.color[1], run.color[2]]));
        let [r, g, b] = plotted_color(rgb, run.color[3], screening, options);
        let (sx, sy) = map.apply(run.origin[0] + ox, run.origin[1] + oy);
        if !sx.is_finite() || !sy.is_finite() {
            continue;
        }
        // Cap height → em size, then the page scale baked in like the PDF CTM.
        let size_mm = (run.height as f64 * map.scale / HELVETICA_CAP_RATIO).max(0.1);
        let deg = -(run.rotation as f64).to_degrees();
        let rotate = if deg.abs() > 1e-6 {
            format!(" transform=\"rotate({} {} {})\"", fmt(deg), fmt(sx), fmt(sy))
        } else {
            String::new()
        };
        let weight = if run.bold { " font-weight=\"bold\"" } else { "" };
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{rotate}{weight} fill=\"{}\" font-family=\"{}\"{opacity}>{text}</text>\n",
            fmt(sx),
            fmt(sy),
            fmt(size_mm),
            hex_color([r, g, b]),
            xml_escape(&svg_font_family(&run.font)),
            opacity = svg_opacity(run.color[3], options.transparency),
            text = xml_escape(&run.text),
        ));
    }
}

fn svg_font_family(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "sans-serif".to_string();
    }
    // SHX names (`txt`, `romans`) are stroke fonts with no SVG equivalent.
    if !trimmed.contains([' ', '-', '_']) && trimmed.chars().all(|c| c.is_ascii_lowercase()) {
        return "sans-serif".to_string();
    }
    format!("{trimmed}, sans-serif")
}

// ── Images ─────────────────────────────────────────────────────────────────

fn emit_image(
    out: &mut String,
    plot: &PlotImage,
    ox: f64,
    oy: f64,
    options: PlotOptions,
    map: &PageMap,
) -> Result<(), String> {
    let image = &plot.image;
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|n| n.checked_mul(4));
    if image.width == 0 || image.height == 0 || expected != Some(image.pixels.len()) {
        return Err("Cannot plot bitmap: dimensions do not match its RGBA pixels.".into());
    }
    if image.verts.len() < 3 || image.verts.len() % 3 != 0 {
        return Err("Cannot plot bitmap: incomplete triangle geometry.".into());
    }
    if options.transparency && image.opacity <= 0.0 {
        return Ok(());
    }
    let to_svg = |high: [f32; 3], low: [f32; 3]| {
        map.apply(high[0] as f64 + low[0] as f64 + ox, high[1] as f64 + low[1] as f64 + oy)
    };
    let corners: [[f64; 2]; 4] =
        std::array::from_fn(|i| { let (x, y) = to_svg(image.corners[i], image.corners_low[i]); [x, y] });
    if !corners.iter().flatten().all(|v| v.is_finite())
        || !image.verts.iter().all(|v| {
            let (x, y) = to_svg(v.pos, v.pos_low);
            x.is_finite() && y.is_finite() && v.uv.iter().all(|c| c.is_finite())
        })
        || !plot
            .clips
            .iter()
            .all(|ring| ring.len() >= 3 && ring.iter().flatten().all(|v| v.is_finite()))
        || !image.opacity.is_finite()
    {
        return Err("Cannot plot bitmap: invalid coordinates, clip boundary, or opacity.".into());
    }

    let mut rgba: Vec<u8> = image.pixels.as_ref().clone();
    if !image.use_alpha {
        rgba.chunks_exact_mut(4).for_each(|px| px[3] = 255);
    }
    let png = {
        let img = image::RgbaImage::from_raw(image.width, image.height, rgba)
            .ok_or("Cannot plot bitmap: pixel buffer mismatch.")?;
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("Cannot plot bitmap: PNG encode failed: {e}"))?;
        buf.into_inner()
    };
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);

    // Pixel space (u right, v down from the top row) onto the quad. Affine fit
    // through top-left / top-right / bottom-left; perspective quads keep the
    // corner fit (same approximation the PDF backend documents).
    let w = image.width as f64;
    let h = image.height as f64;
    let (p0, p2, p3) = (corners[0], corners[2], corners[3]);
    let a = (p2[0] - p3[0]) / w;
    let b = (p2[1] - p3[1]) / w;
    let c = (p0[0] - p3[0]) / h;
    let d = (p0[1] - p3[1]) / h;
    if ![a, b, c, d].iter().all(|v| v.is_finite()) {
        return Err("Cannot plot bitmap: degenerate placement.".into());
    }
    let opacity_attr = if options.transparency && image.opacity < 1.0 {
        format!(" opacity=\"{}\"", fmt(image.opacity.clamp(0.0, 1.0) as f64))
    } else {
        String::new()
    };
    // Inherited clip boundaries become one evenodd clip path per image.
    let mut clip_open = String::new();
    let mut clip_close = "";
    if !plot.clips.is_empty() {
        static CLIP_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = CLIP_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut rings = String::new();
        for ring in &plot.clips {
            let mut d = String::new();
            for (i, p) in ring.iter().enumerate() {
                let (x, y) = map.apply(p[0] + ox, p[1] + oy);
                d.push_str(&format!("{} {} {} ", if i == 0 { 'M' } else { 'L' }, fmt(x), fmt(y)));
            }
            d.push_str("Z ");
            rings.push_str(&format!("<path d=\"{}\"/>\n", d.trim_end()));
        }
        out.push_str(&format!("<clipPath id=\"img-clip-{id}\">{rings}</clipPath>\n"));
        clip_open = format!("<g clip-path=\"url(#img-clip-{id})\">\n");
        clip_close = "</g>\n";
    }
    out.push_str(&clip_open);
    out.push_str(&format!(
        "<image x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{b64}\" transform=\"matrix({} {} {} {} {} {})\"{opacity_attr}/>\n",
        fmt(a), fmt(b), fmt(c), fmt(d), fmt(p3[0]), fmt(p3[1]),
    ));
    out.push_str(clip_close);
    Ok(())
}

// ── Colour / dash / formatting helpers ─────────────────────────────────────

/// White-sheet adaptation shared with the PDF backend: colours authored for
/// the dark screen remap so they stay readable on white paper.
fn adapt_text_color([r, g, b]: [f32; 3]) -> [f32; 3] {
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

/// Screening/alpha blend towards white, mirroring `plotted_color` in PDF export.
fn plotted_color(rgb: [f32; 3], alpha: f32, screening: f32, options: PlotOptions) -> [f32; 3] {
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

fn hex_color([r, g, b]: [f32; 3]) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
}

/// `stroke-dasharray` in final millimetres from a linetype pattern
/// (positive = dash, negative = gap, 0 = dot). `None` = solid.
fn svg_dasharray(pattern_length: f32, pattern: &[f32; 8], scale: f64) -> Option<String> {
    if pattern_length <= 1e-6 {
        return None;
    }
    let count = pattern.iter().rposition(|&v| v != 0.0).map(|i| i + 1)?;
    if count == 0 {
        return None;
    }
    let parts: Vec<String> = pattern[..count]
        .iter()
        .map(|&v| {
            let mm = if v == 0.0 {
                // Dot: same hairline floor the PDF backend uses (1 pt).
                1.0 * PT_TO_MM * scale
            } else {
                v.abs() as f64 * scale
            };
            fmt(mm.max(0.05))
        })
        .collect();
    Some(parts.join(","))
}

fn svg_opacity(alpha: f32, transparency: bool) -> String {
    if transparency && alpha < 1.0 && alpha.is_finite() {
        format!(" opacity=\"{}\"", fmt(alpha.clamp(0.0, 1.0) as f64))
    } else {
        String::new()
    }
}

fn stamp_element(_paper_w: f64, paper_h: f64) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    format!(
        "<text x=\"4\" y=\"{}\" font-size=\"2.12\" fill=\"#404040\" font-family=\"sans-serif\">Open CAD Studio | {} | {}</text>\n",
        fmt(paper_h - 3.0),
        xml_escape(&user),
        timestamp
    )
}

/// Compact number formatting: up to 3 decimals, no trailing zeros.
fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v.abs() < 0.0005 {
        return "0".to_string();
    }
    let rounded = (v * 1000.0).round() / 1000.0;
    let s = format!("{rounded:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_with(content: crate::io::plot::PlotContent, w: f64, h: f64) -> PlotPage {
        PlotPage {
            content,
            paper_w: w,
            paper_h: h,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            scale: 1.0,
            clip: None,
            options: PlotOptions::default(),
            plot_style: None,
        }
    }

    #[test]
    fn empty_page_builds_a_valid_svg_shell() {
        let page = page_with(Default::default(), 210.0, 297.0);
        let svg = build_svg(&page, None).expect("builds");
        assert!(svg.starts_with("<?xml"), "{svg}");
        assert!(svg.contains("viewBox=\"0 0 210 297\""), "{svg}");
        assert!(svg.contains("width=\"210mm\""), "{svg}");
        assert!(svg.contains("<rect"), "{svg}");
        assert!(svg.trim_end().ends_with("</svg>"), "{svg}");
    }

    #[test]
    fn y_flip_puts_bottom_left_geometry_at_the_svg_bottom() {
        // A wire point at paper y=0 (sheet bottom) must land at y=paper_h in SVG.
        let map = PageMap { paper_w: 210.0, paper_h: 297.0, scale: 1.0, rotation_deg: 0 };
        assert_eq!(map.apply(10.0, 0.0), (10.0, 297.0));
        assert_eq!(map.apply(0.0, 297.0), (0.0, 0.0));
    }

    #[test]
    fn rotation_matches_the_pdf_ctm_convention() {
        // PDF 90°: x' = paper_w - s*y, y' = s*x, then SVG flips y.
        let map = PageMap { paper_w: 297.0, paper_h: 210.0, scale: 1.0, rotation_deg: 90 };
        let (x, y) = map.apply(0.0, 0.0);
        assert_eq!((x, y), (297.0, 210.0));
        let (x, y) = map.apply(5.0, 7.0);
        assert_eq!((x, y), (297.0 - 7.0, 210.0 - 5.0));
        let map = PageMap { paper_w: 210.0, paper_h: 297.0, scale: 1.0, rotation_deg: 180 };
        let (x, y) = map.apply(0.0, 0.0);
        assert_eq!((x, y), (210.0, 0.0));
    }

    #[test]
    fn wire_points_become_a_stroked_path() {
        let wire = WireModel::solid(
            "L1".to_string(),
            vec![[10.0, 20.0, 0.0], [30.0, 40.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
            false,
        );
        let content = crate::io::plot::PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire,
                draw_depth: 0.5,
            }]),
            ..Default::default()
        };
        let svg = build_svg(&page_with(content, 210.0, 297.0), None).expect("builds");
        // y=20 → 277, y=40 → 257 after the flip.
        assert!(svg.contains("M 10 277 L 30 257"), "{svg}");
        assert!(svg.contains("stroke=\"#ff0000\""), "{svg}");
    }

    #[test]
    fn nan_splits_subpaths_and_helpers_stay_paper_coloured() {
        let wire = WireModel::solid(
            "L1".to_string(),
            vec![[0.0, 0.0, 0.0], [f32::NAN, f32::NAN, 0.0], [5.0, 5.0, 0.0], [6.0, 5.0, 0.0]],
            [0.0, 0.0, 1.0, 1.0],
            false,
        );
        let content = crate::io::plot::PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire,
                draw_depth: 0.5,
            }]),
            ..Default::default()
        };
        let svg = build_svg(&page_with(content, 100.0, 100.0), None).expect("builds");
        assert!(svg.contains("M 0 100"), "{svg}");
        assert!(svg.contains("M 5 95 L 6 95"), "{svg}");
        // Screen helpers never plot.
        let helper = WireModel::solid(
            "__paper_boundary__".to_string(),
            vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
            false,
        );
        let content = crate::io::plot::PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire: helper,
                draw_depth: 0.5,
            }]),
            ..Default::default()
        };
        let svg = build_svg(&page_with(content, 100.0, 100.0), None).expect("builds");
        assert!(!svg.contains("<path d=\"M"), "{svg}");
    }

    #[test]
    fn dash_and_escape_helpers() {
        assert!(svg_dasharray(0.0, &[0.0; 8], 1.0).is_none());
        assert!(svg_dasharray(5.0, &[0.0; 8], 1.0).is_none());
        let dash = svg_dasharray(5.0, &[3.0, -1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 1.0).unwrap();
        assert_eq!(dash, "3,1.5");
        assert_eq!(xml_escape("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        assert_eq!(hex_color([1.0, 0.0, 0.0]), "#ff0000");
        assert_eq!(fmt(10.0), "10");
        assert_eq!(fmt(2.5004), "2.5");
        assert_eq!(fmt(0.0001), "0");
    }

    #[test]
    fn clip_rect_becomes_a_clip_path() {
        let mut page = page_with(Default::default(), 210.0, 297.0);
        page.clip = Some((10.0, 20.0, 100.0, 50.0));
        let svg = build_svg(&page, None).expect("builds");
        assert!(svg.contains("<clipPath id=\"plot-clip\">"), "{svg}");
        // y-flipped: rect y = 297 - (20 + 50) = 227.
        assert!(svg.contains("x=\"10\" y=\"227\" width=\"100\" height=\"50\""), "{svg}");
    }
}
