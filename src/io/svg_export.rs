// SVG export — vector plot from the shared paper-space `PlotPage` model.
//
// Each page becomes one standalone `.svg` file in millimetre user units
// (`viewBox="0 0 paper_w paper_h"`), Y-flipped because SVG's origin is
// top-left while the plot geometry (like PDF) is bottom-left. Rotation and
// plot scale are baked into the coordinates with the same transform the PDF
// backend puts in its page CTM, so the two outputs agree geometrically.
//
// Scope notes (prototype):
// - Text defaults to searchable `<text>` runs (the same `SearchableTextRun`
//   source as the PDF search layer). `text_as_outlines` instead re-emits the
//   positioned SDF quads as font-independent vector strokes and fills.
// - Stationed dash phases (`pattern_stations`, PLINEGEN) are approximated with
//   a plain `stroke-dasharray`; non-affine (perspective) image quads are drawn
//   with their affine corner fit.
// - Like PDF export this is native-only for file writing; `build_svg` itself
//   is pure and works on wasm too.

use std::path::Path;

use crate::io::pdf_export::adapt_to_paper;
use crate::io::plot::{PlotImage, PlotOptions, PlotPage, PlotWire};
use crate::io::plot_style::PlotStyleTable;
use crate::scene::WireModel;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};

/// Screen pixels (96 dpi) to millimetres.
const PX_TO_MM: f64 = 25.4 / 96.0;

/// `vector-effect="non-scaling-stroke"` when the opt-in SVG option is on. It
/// affects only stroked paths; the CAD plot scale and PDF output never change.
fn non_scaling_stroke_attr(options: PlotOptions) -> &'static str {
    if options.non_scaling_stroke {
        " vector-effect=\"non-scaling-stroke\""
    } else {
        ""
    }
}

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
        let svg = build_svg(page, plot_style).map_err(|e| format!("Page {}: {e}", index + 1))?;
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
    if let Err(error) = super::replace_save_file(&temp_path, path, false) {
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
    let stamp = page.options.stamp.then(plot_stamp_label);
    // Lay the stamp out before snapshotting: the atlas keeps its UVs across
    // growth (tiles persist), so one snapshot taken after the stamp's layout
    // serves both the page quads and the stamp — no pre-stamp geometry to
    // keep alive.
    let stamp_wire = if page.options.text_as_outlines {
        stamp.as_deref().and_then(stamp_outline_wire)
    } else {
        None
    };
    let glyphs = if page.options.text_as_outlines {
        GlyphSnapshot::snapshot()
    } else {
        None
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
    out.push_str(&format!(
        " width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        fmt(paper_w),
        fmt(paper_h)
    ));

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
        let (mut x0, mut y0, mut x1, mut y1) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x1 > x0
            && y1 > y0
            && x0.is_finite()
            && y0.is_finite()
            && x1.is_finite()
            && y1.is_finite()
        {
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

    emit_page_content(&mut out, page, plot_style, &map, glyphs.as_ref())?;

    out.push_str("</g>\n");
    if let Some(label) = stamp {
        let outlined = match (stamp_wire.as_ref(), glyphs.as_ref()) {
            (Some(wire), Some(glyphs)) => {
                out.push_str("<g stroke-linecap=\"round\" stroke-linejoin=\"round\">\n");
                let outlined = emit_text_outlines(
                    &mut out,
                    wire,
                    OutlineContext {
                        ox: 0.0,
                        oy: 0.0,
                        plot_style: None,
                        options: page.options,
                        map: &PageMap {
                            paper_w,
                            paper_h,
                            scale: 1.0,
                            rotation_deg: 0,
                        },
                        glyphs,
                    },
                );
                out.push_str("</g>\n");
                outlined
            }
            _ => false,
        };
        if !outlined {
            out.push_str(&stamp_element(&label, paper_h));
        }
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
    glyphs: Option<&GlyphSnapshot>,
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
        let mut items =
            Vec::with_capacity(wires.len() * 2 + hatches.len() + wipeouts.len() + images.len());
        let mut sequence = 0usize;
        for wire in wires {
            if !wire.fill_tris.is_empty() {
                items.push((wire.draw_depth, 0u8, sequence, DrawItem::WireFill(wire)));
                sequence += 1;
            }
            items.push((wire.draw_depth, 2u8, sequence, DrawItem::Wire(wire)));
            sequence += 1;
            if !wire.searchable_text.is_empty()
                || (options.text_as_outlines && !wire.text_verts.is_empty())
            {
                items.push((wire.draw_depth, 3u8, sequence, DrawItem::Text(wire)));
                sequence += 1;
            }
        }
        for hatch in wipeouts.iter().chain(hatches.iter()) {
            items.push((hatch.draw_depth, 1u8, sequence, DrawItem::Hatch(hatch)));
            sequence += 1;
        }
        for image in images {
            items.push((
                image.image.draw_depth,
                1u8,
                sequence,
                DrawItem::Image(image),
            ));
            sequence += 1;
        }
        items.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let mut open_layer: Option<&str> = None;
        for (_, _, _, item) in items {
            let layer = if options.layer_attributes {
                match &item {
                    DrawItem::WireFill(w) | DrawItem::Wire(w) | DrawItem::Text(w) => {
                        crate::io::plot::wire_layer(&w.wire, &content.entity_layers)
                    }
                    DrawItem::Hatch(h) => crate::io::plot::hatch_layer(h, &content.entity_layers),
                    DrawItem::Image(_) => None,
                }
            } else {
                None
            };

            // Emit into a scratch string so the item can go into its drawing
            // layer's group; items in a row on one layer share a group.
            let mut rendered = String::new();
            match item {
                DrawItem::WireFill(wire) => {
                    emit_wire_fills(&mut rendered, &wire.wire, ox, oy, plot_style, options, map);
                }
                DrawItem::Hatch(hatch) => {
                    emit_hatch(&mut rendered, hatch, ox, oy, plot_style, options, map)
                }
                DrawItem::Text(wire) => {
                    let outlined = glyphs.is_some_and(|glyphs| {
                        emit_text_outlines(
                            &mut rendered,
                            &wire.wire,
                            OutlineContext {
                                ox,
                                oy,
                                plot_style,
                                options,
                                map,
                                glyphs,
                            },
                        )
                    });
                    if !outlined {
                        emit_searchable_text(
                            &mut rendered,
                            &wire.wire,
                            ox,
                            oy,
                            plot_style,
                            options,
                            map,
                        );
                    }
                }
                DrawItem::Wire(wire) => {
                    emit_wire(&mut rendered, &wire.wire, ox, oy, plot_style, options, map)
                }
                DrawItem::Image(image) => emit_image(&mut rendered, image, ox, oy, options, map)?,
            };

            if layer != open_layer {
                if open_layer.is_some() {
                    out.push_str("</g>\n");
                }
                if let Some(layer) = layer {
                    out.push_str(&format!("<g data-layer=\"{}\">\n", xml_attr(layer)));
                }
                open_layer = layer;
            }
            out.push_str(&rendered);
        }
        if open_layer.is_some() {
            out.push_str("</g>\n");
        }
    }
    Ok(())
}

// ── Wires ────────────────────────────────────────────────────────────────────

/// Escapes a value for use inside an XML attribute (layer names are free-form
/// drawing text).
fn xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

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
    if matches!(
        wire.name.as_str(),
        "__paper_boundary__" | "paper_printable_area"
    ) {
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
        [r, g, b] = adapt_to_paper([r, g, b]);
    }
    [r, g, b] = plotted_color([r, g, b], a, screening, options);

    let s = map.scale;
    let width_mm = if wire.world_width > 0.0 {
        wire.world_width as f64 * s
    } else {
        let physical = if options.object_lineweights {
            lw_override_mm.unwrap_or(wire.line_weight_px as f64 * PX_TO_MM)
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
    // A circle or arc is drawn as its curve, not as chords.
    let spans = crate::io::plot::wire_arc_beziers(wire, view_h).unwrap_or_default();
    for (index, span) in spans.iter().enumerate() {
        let [p0, c1, c2, p3] = span.map(|p| map.apply(p.x + ox, p.y + oy));
        if index == 0 {
            d.push_str(&format!("M {} {} ", fmt(p0.0), fmt(p0.1)));
        }
        d.push_str(&format!(
            "C {} {} {} {} {} {} ",
            fmt(c1.0),
            fmt(c1.1),
            fmt(c2.0),
            fmt(c2.1),
            fmt(p3.0),
            fmt(p3.1)
        ));
        any = true;
    }
    let chords = if spans.is_empty() { wire.points.as_slice() } else { &[] };
    for (pi, &[x, y, _]) in chords.iter().enumerate() {
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
        d.push_str(&format!(
            "{} {} {} ",
            if pen_down { 'L' } else { 'M' },
            fmt(sx),
            fmt(sy)
        ));
        pen_down = true;
        any = true;
    }
    if !any {
        return;
    }
    out.push_str(&format!(
        "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\"{dash_attr}{opacity_attr}{non_scaling}/>\n",
        d.trim_end(),
        hex_color([r, g, b]),
        fmt(width_mm.max(0.01)),
        non_scaling = non_scaling_stroke_attr(options),
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
    if wire
        .bg_adapt
        .as_deref()
        .is_some_and(|adapt| adapt.canvas_color)
    {
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
            [r, g, b] = adapt_to_paper([r, g, b]);
        }
        [r, g, b] = plotted_color([r, g, b], a, screening, options);
    }
    let mut d = String::new();
    for (ti, tri) in wire.fill_tris.chunks_exact(3).enumerate() {
        let mut pts = Vec::with_capacity(3);
        let mut ok = true;
        for (pi, &[x, y, _]) in tri.iter().enumerate() {
            let low = wire
                .fill_tris_low
                .get(ti * 3 + pi)
                .copied()
                .unwrap_or([0.0; 3]);
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
            fmt(pts[0].0),
            fmt(pts[0].1),
            fmt(pts[1].0),
            fmt(pts[1].1),
            fmt(pts[2].0),
            fmt(pts[2].1),
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
    } else if !color_overridden
        && hatch.aci != 0
        && !(hatch.aci == 7 && matches!(hatch.pattern, HatchPattern::Solid))
    {
        [r, g, b] = adapt_to_paper([r, g, b]);
    }
    [r, g, b] = plotted_color([r, g, b], a, screening, options);
    let (wox, woy) = (hatch.world_origin[0], hatch.world_origin[1]);

    if matches!(hatch.pattern, HatchPattern::Pattern(_)) {
        let s = map.scale;
        let physical = if options.object_lineweights {
            lw_override_mm.unwrap_or(hatch.line_weight_px as f64 * PX_TO_MM)
        } else {
            0.1 * PT_TO_MM
        }
        .max(0.1 * PT_TO_MM);
        let width_mm = if options.scale_lineweights {
            physical * s
        } else {
            physical
        };
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
                d.push_str(&format!(
                    "M {} {} L {} {} ",
                    fmt(ax),
                    fmt(ay),
                    fmt(bx),
                    fmt(by)
                ));
            }
        }
        if d.is_empty() {
            return;
        }
        out.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}{non_scaling}/>\n",
            d.trim_end(),
            hex_color([r, g, b]),
            fmt(width_mm.max(0.01)),
            svg_opacity(a, options.transparency),
            non_scaling = non_scaling_stroke_attr(options),
        ));
        return;
    }

    if let HatchPattern::Gradient { color2, .. } = &hatch.pattern {
        let second = plotted_color(
            adapt_to_paper([color2[0], color2[1], color2[2]]),
            color2[3],
            screening,
            options,
        );
        [r, g, b] = [
            (r + second[0]) * 0.5,
            (g + second[1]) * 0.5,
            (b + second[2]) * 0.5,
        ];
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
        d.push_str(&format!(
            "{} {} {} ",
            if current_empty { 'M' } else { 'L' },
            fmt(sx),
            fmt(sy)
        ));
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

// ── Optional text as vector outlines ───────────────────────────────────────

use crate::scene::text::glyph_to_world;

/// The atlas' export snapshot, shared with the PDF text path — the quad
/// maths (uv key, plane mapping, glyph units, nominal pen) lives in
/// [`crate::scene::text::glyph_to_world`].
type GlyphSnapshot = glyph_to_world::GlyphSnapshot;

/// `None` rejects a non-finite alpha; `Some(false)` is intentionally invisible.
/// Both the preflight and emission passes use this exact classification.
fn outline_visibility(alpha: f32) -> Option<bool> {
    alpha.is_finite().then_some(alpha >= 0.01)
}

fn outline_path<'a>(paths: &'a mut Vec<(String, String)>, style: String) -> &'a mut String {
    if paths.last().is_none_or(|(current, _)| current != &style) {
        paths.push((style, String::new()));
    }
    &mut paths.last_mut().expect("just pushed").1
}

struct OutlineContext<'a> {
    ox: f64,
    oy: f64,
    plot_style: Option<&'a PlotStyleTable>,
    options: PlotOptions,
    map: &'a PageMap,
    glyphs: &'a GlyphSnapshot,
}

/// Validate one visible quad and all geometry it would emit. This runs for the
/// whole wire before anything is appended, so one malformed glyph cannot leave
/// a partial label or suppress the searchable fallback.
fn outline_quad_is_valid(
    quad: &[crate::scene::pipeline::text_gpu::TextVertex; 6],
    glyphs: &GlyphSnapshot,
    ox: f64,
    oy: f64,
    map: &PageMap,
) -> bool {
    use crate::scene::text::glyph_to_world::QuadBasis;

    let vertex_xy = |vertex: &crate::scene::pipeline::text_gpu::TextVertex| {
        [
            vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
            vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
        ]
    };
    let corners = [0, 1, 2, 5].map(|index| vertex_xy(&quad[index]));
    if corners
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return false;
    }
    let page_point = |point: [f64; 2]| map.apply(point[0] + ox, point[1] + oy);
    if corners
        .iter()
        .map(|&point| page_point(point))
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return false;
    }

    let key = glyph_to_world::quad_uv_key(quad);
    if key == glyphs.solid_key {
        return true;
    }
    let Some(glyph) = glyphs.glyph(key) else {
        return false;
    };
    if !glyph
        .plane_min
        .iter()
        .chain(glyph.plane_max.iter())
        .all(|v| v.is_finite())
    {
        return false;
    }
    let Some(basis) = QuadBasis::of(quad, glyph) else {
        return false;
    };
    let valid_glyph_point = |point: &[f32; 2]| {
        if !point.iter().all(|coordinate| coordinate.is_finite()) {
            return false;
        }
        let (x, y) = page_point(basis.world(point));
        x.is_finite() && y.is_finite()
    };
    if !glyph.fill_tris.is_empty() {
        glyph.fill_tris.len().is_multiple_of(3) && glyph.fill_tris.iter().all(valid_glyph_point)
    } else {
        // Empty substrokes carry no geometry; a one-point stroke is a visible
        // dot and is emitted as a zero-length round-cap segment below.
        glyph.strokes.iter().any(|stroke| !stroke.is_empty())
            && glyph
                .strokes
                .iter()
                .all(|stroke| stroke.iter().all(valid_glyph_point))
    }
}

/// Re-emit the placed SDF quads as font-independent vector paths. Returns false
/// before emitting anything when any visible glyph or mapped point is unusable,
/// so the caller keeps the complete searchable `<text>` fallback.
fn emit_text_outlines(out: &mut String, wire: &WireModel, context: OutlineContext) -> bool {
    let verts = &wire.text_verts;
    if verts.is_empty() || verts.len() % 6 != 0 {
        return false;
    }
    for quad in glyph_to_world::wire_quads(wire) {
        match outline_visibility(quad[0].color[3]) {
            None => return false,
            Some(false) => continue,
            Some(true) => {}
        }
        if !outline_quad_is_valid(quad, context.glyphs, context.ox, context.oy, context.map) {
            return false;
        }
    }

    let mut ctb_color = None;
    let mut lw_override_mm = None;
    let mut screening = 1.0;
    if let Some(table) = context.plot_style {
        if wire.aci > 0 {
            ctb_color = table.resolve_color(wire.aci);
            screening = table.resolve_screening(wire.aci);
            if context.options.object_lineweights {
                lw_override_mm = table.resolve_lineweight(wire.aci).map(|mm| mm as f64);
            }
        }
    }
    let options = context.options;
    let map_page = context.map;
    // Coalesce only adjacent equal styles. Global grouping would reorder
    // overlapping glyphs when inline colors change within one text run.
    let mut paths = Vec::<(String, String)>::new();

    for quad in glyph_to_world::wire_quads(wire) {
        let a = quad[0].color[3];
        match outline_visibility(a) {
            None => return false,
            Some(false) => continue,
            Some(true) => {}
        }
        let rgb = ctb_color.unwrap_or_else(|| {
            adapt_to_paper([quad[0].color[0], quad[0].color[1], quad[0].color[2]])
        });
        let color = plotted_color(rgb, a, screening, options);
        let opacity = svg_opacity(a, options.transparency);
        let fill_style = || {
            format!(
                "fill=\"{}\" fill-rule=\"nonzero\" stroke=\"none\"{}",
                hex_color(color),
                opacity
            )
        };

        // Quad corners are [bl, br, tr, bl, tr, tl]. Work in f64 until the
        // final page mapping so UTM-scale text keeps its local glyph detail.
        let page_point =
            |point: [f64; 2]| map_page.apply(point[0] + context.ox, point[1] + context.oy);
        let key = glyph_to_world::quad_uv_key(quad);
        if let Some(glyph) = context.glyphs.glyph(key) {
            let Some(basis) = glyph_to_world::QuadBasis::of(quad, glyph) else {
                // Defensive only: preflight rejects this before any output.
                return false;
            };
            let glyph_point = |point: [f32; 2]| basis.world(&point);
            if !glyph.fill_tris.is_empty() {
                let d = outline_path(&mut paths, fill_style());
                for triangle in glyph.fill_tris.chunks_exact(3) {
                    let points = triangle
                        .iter()
                        .map(|&point| page_point(glyph_point(point)))
                        .collect::<Vec<_>>();
                    d.push_str(&format!(
                        "M {} {} L {} {} L {} {} Z ",
                        fmt(points[0].0),
                        fmt(points[0].1),
                        fmt(points[1].0),
                        fmt(points[1].1),
                        fmt(points[2].0),
                        fmt(points[2].1),
                    ));
                }
            } else {
                let width_mm = if let Some(mm) = lw_override_mm {
                    let mm = if glyph.bold { mm * 1.7 } else { mm };
                    if options.scale_lineweights {
                        mm * map_page.scale
                    } else {
                        mm
                    }
                } else {
                    glyph_to_world::nominal_pen_world_mm(glyph.bold, basis.glyph_unit_mm() as f32)
                        * map_page.scale
                }
                .max(0.1 * PT_TO_MM);
                let style = format!(
                    "fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}{}",
                    hex_color(color),
                    fmt(width_mm),
                    opacity,
                    non_scaling_stroke_attr(options)
                );
                let d = outline_path(&mut paths, style);
                for stroke in &glyph.strokes {
                    if stroke.is_empty() {
                        continue;
                    }
                    for (index, &point) in stroke.iter().enumerate() {
                        let (x, y) = page_point(glyph_point(point));
                        d.push_str(&format!(
                            "{} {} {} ",
                            if index == 0 { 'M' } else { 'L' },
                            fmt(x),
                            fmt(y)
                        ));
                    }
                    if stroke.len() == 1 {
                        let (x, y) = page_point(glyph_point(stroke[0]));
                        d.push_str(&format!("L {} {} ", fmt(x), fmt(y)));
                    }
                }
            }
        } else if key == context.glyphs.solid_key {
            // Atlas solid tile: underline, overline, and strike decorations.
            let corner = |index: usize| {
                let v = &quad[index];
                [
                    v.pos[0] as f64 + v.pos_low[0] as f64,
                    v.pos[1] as f64 + v.pos_low[1] as f64,
                ]
            };
            let points = [corner(0), corner(1), corner(2), corner(5)].map(page_point);
            outline_path(&mut paths, fill_style()).push_str(&format!(
                "M {} {} L {} {} L {} {} L {} {} Z ",
                fmt(points[0].0),
                fmt(points[0].1),
                fmt(points[1].0),
                fmt(points[1].1),
                fmt(points[2].0),
                fmt(points[2].1),
                fmt(points[3].0),
                fmt(points[3].1),
            ));
        } else {
            // Defensive only: preflight rejects this before any output.
            return false;
        }
    }

    for (style, d) in paths {
        if !d.is_empty() {
            out.push_str(&format!("<path d=\"{}\" {style}/>\n", d.trim_end()));
        }
    }
    true
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
        let rgb =
            ctb_color.unwrap_or_else(|| adapt_to_paper([run.color[0], run.color[1], run.color[2]]));
        let [r, g, b] = plotted_color(rgb, run.color[3], screening, options);
        let (sx, sy) = map.apply(run.origin[0] + ox, run.origin[1] + oy);
        if !sx.is_finite() || !sy.is_finite() {
            continue;
        }
        // Cap height → em size, then the page scale baked in like the PDF CTM.
        let size_mm = (run.height as f64 * map.scale / HELVETICA_CAP_RATIO).max(0.1);
        let deg = -(run.rotation as f64).to_degrees();
        let rotate = if deg.abs() > 1e-6 {
            format!(
                " transform=\"rotate({} {} {})\"",
                fmt(deg),
                fmt(sx),
                fmt(sy)
            )
        } else {
            String::new()
        };
        let weight = if run.bold {
            " font-weight=\"bold\""
        } else {
            ""
        };
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
    // Resolved SHX identities can carry a legacy code page and a big-font
    // path; neither belongs in an SVG font-family attribute.
    let trimmed = trimmed
        .split([
            crate::scene::text::font_face::BIG_FONT_SEP,
            crate::scene::text::font_face::CODE_PAGE_SEP,
        ])
        .next()
        .unwrap_or(trimmed)
        .trim();
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
    if image.verts.len() < 3 || !image.verts.len().is_multiple_of(3) {
        return Err("Cannot plot bitmap: incomplete triangle geometry.".into());
    }
    if options.transparency && image.opacity <= 0.0 {
        return Ok(());
    }
    let to_svg = |high: [f32; 3], low: [f32; 3]| {
        map.apply(
            high[0] as f64 + low[0] as f64 + ox,
            high[1] as f64 + low[1] as f64 + oy,
        )
    };
    let corners: [[f64; 2]; 4] = std::array::from_fn(|i| {
        let (x, y) = to_svg(image.corners[i], image.corners_low[i]);
        [x, y]
    });
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
                d.push_str(&format!(
                    "{} {} {} ",
                    if i == 0 { 'M' } else { 'L' },
                    fmt(x),
                    fmt(y)
                ));
            }
            d.push_str("Z ");
            rings.push_str(&format!("<path d=\"{}\"/>\n", d.trim_end()));
        }
        out.push_str(&format!(
            "<clipPath id=\"img-clip-{id}\">{rings}</clipPath>\n"
        ));
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

fn plot_stamp_label() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    format!("Open CAD Studio | {user} | {timestamp}")
}

fn stamp_glyph_quads(
    atlas: &mut crate::scene::text::sdf_atlas::GlyphAtlas,
    label: &str,
) -> Vec<crate::scene::text::glyph_quads::GlyphQuad> {
    crate::scene::text::glyph_quads::layout_glyph_quads(
        atlas,
        (2.12 * HELVETICA_CAP_RATIO) as f32,
        0.0,
        1.0,
        0.0,
        1.0,
        "Helvetica",
        false,
        label,
    )
    .0
}

fn stamp_wire_from_atlas(
    atlas: &mut crate::scene::text::sdf_atlas::GlyphAtlas,
    label: &str,
) -> Option<WireModel> {
    // The atlas keeps its UVs when it grows, so one layout pass is enough.
    let quads = stamp_glyph_quads(atlas, label);
    let mut wire = WireModel::default();
    crate::scene::pipeline::text_gpu::push_glyph_vertices(
        &mut wire.text_verts,
        &quads,
        [4.0, 3.0, 0.0],
        1.0,
        [0.25, 0.25, 0.25, 1.0],
        0.0,
    );
    Some(wire)
}

fn stamp_outline_wire(label: &str) -> Option<WireModel> {
    let mut atlas = crate::scene::text::sdf_atlas::text_atlas().lock().ok()?;
    stamp_wire_from_atlas(&mut atlas, label)
}

fn stamp_element(label: &str, paper_h: f64) -> String {
    format!(
        "<text x=\"4\" y=\"{}\" font-size=\"2.12\" fill=\"#404040\" font-family=\"sans-serif\">{}</text>\n",
        fmt(paper_h - 3.0),
        xml_escape(label),
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

/// Escape markup and replace code points that XML 1.0 forbids outright.
/// Character references do not help for those code points: even `&#x2;` is
/// invalid XML, so the replacement character is the only representable marker.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 Char permits tab/LF/CR, U+20–D7FF, E000–FFFD, and
            // U+10000–10FFFF. Rust strings cannot contain surrogate code
            // points, so only these ranges need rejecting here.
            '\u{0}'..='\u{8}'
            | '\u{b}'..='\u{c}'
            | '\u{e}'..='\u{1f}'
            | '\u{fffe}'..='\u{ffff}' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::plot::PlotContent;

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
    fn svg_font_family_omits_resolved_shx_metadata() {
        let font =
            crate::scene::text::font_face::Face::with_code_page("cs_ENGINEERING.shx", "ANSI_1250");
        let paired = crate::scene::text::font_face::Face::with_big_font(&font, "chineset.shx");
        assert_eq!(svg_font_family(&font), "cs_ENGINEERING.shx, sans-serif");
        assert_eq!(svg_font_family(&paired), "cs_ENGINEERING.shx, sans-serif");
    }

    #[test]
    fn y_flip_puts_bottom_left_geometry_at_the_svg_bottom() {
        // A wire point at paper y=0 (sheet bottom) must land at y=paper_h in SVG.
        let map = PageMap {
            paper_w: 210.0,
            paper_h: 297.0,
            scale: 1.0,
            rotation_deg: 0,
        };
        assert_eq!(map.apply(10.0, 0.0), (10.0, 297.0));
        assert_eq!(map.apply(0.0, 297.0), (0.0, 0.0));
    }

    #[test]
    fn wires_land_in_data_layer_groups_when_the_option_is_on() {
        let content = PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire: WireModel::solid(
                    "126".to_string(),
                    vec![[10.0, 20.0, 0.0], [30.0, 40.0, 0.0]],
                    [1.0, 0.0, 0.0, 1.0],
                    false,
                ),
                draw_depth: 0.5,
            }]),
            entity_layers: std::collections::HashMap::from([(
                crate::scene::Scene::handle_from_wire_name("126").expect("handle parses"),
                "Parcels".to_string(),
            )]),
            ..Default::default()
        };

        // Default: no layer groups, output unchanged.
        let mut page = page_with(Default::default(), 210.0, 297.0);
        page.content = content;
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            !svg.contains("data-layer"),
            "default output must carry no layer markup: {svg}"
        );

        // Opt-in: every drawn item sits in its drawing-layer group.
        page.options = PlotOptions {
            layer_attributes: true,
            ..Default::default()
        };
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            svg.contains("<g data-layer=\"Parcels\">"),
            "the wire must sit in its layer group: {svg}"
        );
    }

    #[test]
    fn data_layer_groups_escape_the_layer_name() {
        let content = PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire: WireModel::solid(
                    "127".to_string(),
                    vec![[10.0, 20.0, 0.0], [30.0, 40.0, 0.0]],
                    [1.0, 0.0, 0.0, 1.0],
                    false,
                ),
                draw_depth: 0.5,
            }]),
            entity_layers: std::collections::HashMap::from([(
                crate::scene::Scene::handle_from_wire_name("127").expect("handle parses"),
                "a\"b<&c".to_string(),
            )]),
            ..Default::default()
        };
        let mut page = page_with(Default::default(), 210.0, 297.0);
        page.content = content;
        page.options = PlotOptions {
            layer_attributes: true,
            ..Default::default()
        };
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            svg.contains("<g data-layer=\"a&quot;b&lt;&amp;c\">"),
            "attribute escaping: {svg}"
        );
    }

    #[test]
    fn per_occurrence_layers_override_a_shared_insert_handle() {
        let wire = |layer: &str, depth| {
            let mut wire = WireModel::solid(
                "126".to_string(),
                vec![[10.0, 20.0, 0.0], [30.0, 40.0, 0.0]],
                [1.0, 0.0, 0.0, 1.0],
                false,
            );
            wire.effective_layer = Some(std::sync::Arc::from(layer));
            PlotWire {
                wire,
                draw_depth: depth,
            }
        };
        let content = PlotContent {
            wires: std::sync::Arc::new(vec![wire("FIRST", 0.4), wire("SECOND", 0.5)]),
            entity_layers: std::collections::HashMap::from([(
                crate::scene::Scene::handle_from_wire_name("126").expect("handle parses"),
                "OUTER_INSERT".to_string(),
            )]),
            ..Default::default()
        };
        let mut page = page_with(content, 210.0, 297.0);
        page.options = PlotOptions {
            layer_attributes: true,
            ..Default::default()
        };
        let svg = build_svg(&page, None).expect("builds");
        assert!(svg.contains("<g data-layer=\"FIRST\">"), "{svg}");
        assert!(svg.contains("<g data-layer=\"SECOND\">"), "{svg}");
        assert!(!svg.contains("data-layer=\"OUTER_INSERT\""), "{svg}");
    }

    #[test]
    fn hatches_land_in_layer_groups_through_their_plotted_entity() {
        // Hatch models name their fill pattern in `name` — `SOLID`, `ANSI31`,
        // … — which parses to nothing. The layer attribution must come from
        // the entity the model plots for instead.
        let mut tagged = HatchModel {
            entity: None,
            effective_layer: None,
            pattern_origin: None,
            render_instance: None,
            world_origin: [0.0, 0.0],
            boundary: std::sync::Arc::new(vec![[20.0, 20.0], [40.0, 20.0], [40.0, 40.0]]),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: document::entities::HatchStyleType::Normal,
            pattern: HatchPattern::Solid,
            name: "SOLID".into(),
            color: [1.0, 1.0, 0.0, 1.0],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: 0.3,
        };
        let content_for = |hatch: HatchModel| crate::io::plot::PlotContent {
            hatches: vec![hatch],
            entity_layers: std::collections::HashMap::from([(
                crate::scene::Scene::handle_from_wire_name("128").expect("handle parses"),
                "Parcels".to_string(),
            )]),
            ..Default::default()
        };
        let mut page = page_with(Default::default(), 210.0, 297.0);
        page.options = PlotOptions {
            layer_attributes: true,
            ..Default::default()
        };

        // A hatch whose entity is unknown carries no layer markup.
        page.content = content_for(tagged.clone());
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            !svg.contains("data-layer=\"Parcels\""),
            "no entity set: {svg}"
        );

        // With the plotted entity recorded, the fill sits in its layer group.
        tagged.entity = Some(crate::scene::Scene::handle_from_wire_name("128").expect("handle"));
        page.content = content_for(tagged.clone());
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            svg.contains("<g data-layer=\"Parcels\">"),
            "the hatch fill must sit in its layer group: {svg}"
        );

        tagged.effective_layer = Some(std::sync::Arc::from("INHERITED_PLACEMENT"));
        page.content = content_for(tagged);
        let svg = build_svg(&page, None).expect("builds");
        assert!(
            svg.contains("<g data-layer=\"INHERITED_PLACEMENT\">"),
            "placement metadata must beat the raw entity fallback: {svg}"
        );
        assert!(!svg.contains("data-layer=\"Parcels\""), "{svg}");
    }

    #[test]
    fn non_scaling_stroke_is_opt_in_and_only_marks_stroked_paths() {
        use crate::scene::model::hatch_model::PatFamily;

        let line = WireModel::solid(
            "L1".to_string(),
            vec![[10.0, 20.0, 0.0], [30.0, 40.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
            false,
        );
        let mut fill = WireModel::default();
        fill.color = [0.0, 0.0, 1.0, 1.0];
        fill.fill_tris = vec![[5.0, 5.0, 0.0], [10.0, 5.0, 0.0], [5.0, 10.0, 0.0]];
        let wires = std::sync::Arc::new(vec![
            PlotWire {
                wire: line,
                draw_depth: 0.5,
            },
            PlotWire {
                wire: fill,
                draw_depth: 0.4,
            },
        ]);
        let hatch = |pattern, color| HatchModel {
            entity: None,
            effective_layer: None,
            pattern_origin: None,
            render_instance: None,
            world_origin: [0.0, 0.0],
            boundary: std::sync::Arc::new(vec![
                [20.0, 20.0],
                [40.0, 20.0],
                [40.0, 40.0],
                [20.0, 40.0],
            ]),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: document::entities::HatchStyleType::Normal,
            pattern,
            name: "GENERATED".into(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: 0.3,
        };
        let hatches = vec![
            hatch(
                HatchPattern::Pattern(vec![PatFamily {
                    angle_deg: 0.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 5.0,
                    dashes: Vec::new(),
                }]),
                [0.0, 1.0, 0.0, 1.0],
            ),
            hatch(HatchPattern::Solid, [1.0, 1.0, 0.0, 1.0]),
        ];
        let render = |enabled| {
            let content = crate::io::plot::PlotContent {
                wires: wires.clone(),
                hatches: hatches.clone(),
                ..Default::default()
            };
            let mut page = page_with(content, 210.0, 297.0);
            page.options.non_scaling_stroke = enabled;
            build_svg(&page, None).expect("SVG")
        };
        assert!(!render(false).contains("vector-effect"));
        let svg = render(true);
        let stroked = svg
            .lines()
            .find(|line| line.contains("stroke=\"#ff0000\""))
            .expect("stroked line");
        assert!(stroked.contains("vector-effect=\"non-scaling-stroke\""));
        let filled = svg
            .lines()
            .find(|line| line.contains("fill=\"#0000ff\""))
            .expect("filled triangle");
        assert!(!filled.contains("vector-effect"));
        let pattern = svg
            .lines()
            .find(|line| line.contains("stroke=\"#00ff00\""))
            .expect("pattern-hatch stroke");
        assert!(pattern.contains("vector-effect=\"non-scaling-stroke\""));
        let solid_hatch = svg
            .lines()
            .find(|line| line.contains("fill=\"#ffff00\""))
            .expect("solid hatch fill");
        assert!(!solid_hatch.contains("vector-effect"));
        assert_eq!(svg.matches("vector-effect=").count(), 2);
        roxmltree::Document::parse(&svg).expect("well-formed SVG");

        let pdf = |enabled| {
            let content = crate::io::plot::PlotContent {
                wires: wires.clone(),
                hatches: hatches.clone(),
                ..Default::default()
            };
            let mut page = page_with(content, 210.0, 297.0);
            page.options.non_scaling_stroke = enabled;
            crate::io::pdf_export::render_pdf_bytes(&[page], None).expect("PDF")
        };
        // Raw bytes and the stream-text trailer carry a per-render document ID;
        // the decoded content stream is the deterministic part and must not
        // react to this SVG-only flag.
        let stream_text = |bytes: &[u8]| {
            let text = crate::io::pdf_export::pdf_stream_text(bytes);
            text.rsplit_once("%%EOF")
                .map(|(_, content)| content.to_string())
                .unwrap_or(text)
        };
        assert_eq!(
            stream_text(&pdf(false)),
            stream_text(&pdf(true)),
            "the SVG-only flag changed the PDF content stream"
        );
    }

    #[test]
    fn rotation_matches_the_pdf_ctm_convention() {
        // PDF 90°: x' = paper_w - s*y, y' = s*x, then SVG flips y.
        let map = PageMap {
            paper_w: 297.0,
            paper_h: 210.0,
            scale: 1.0,
            rotation_deg: 90,
        };
        let (x, y) = map.apply(0.0, 0.0);
        assert_eq!((x, y), (297.0, 210.0));
        let (x, y) = map.apply(5.0, 7.0);
        assert_eq!((x, y), (297.0 - 7.0, 210.0 - 5.0));
        let map = PageMap {
            paper_w: 210.0,
            paper_h: 297.0,
            scale: 1.0,
            rotation_deg: 180,
        };
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
            vec![
                [0.0, 0.0, 0.0],
                [f32::NAN, f32::NAN, 0.0],
                [5.0, 5.0, 0.0],
                [6.0, 5.0, 0.0],
            ],
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
    fn mtext_canvas_mask_is_white_on_paper_but_explicit_fill_keeps_its_color() {
        for (flags, expected_fill) in [(0x03, "#ffffff"), (0x01, "#ff0000")] {
            let mut text = document::entities::MText::with_value(
                "MASK",
                document::types::Vector3::new(10.0, 10.0, 0.0),
            );
            text.height = 2.5;
            text.background_fill_flags = flags;
            text.background_color = document::types::Color::Index(1);
            let wires = crate::scene::convert::tessellate::tessellate(
                &document::Document::new(),
                document::Handle::new(123),
                &document::EntityType::MText(Box::new(text)),
                false,
                [1.0; 4],
                0.0,
                [0.0; 8],
                1.0,
                1.0,
                None,
                None,
                [0.0, 0.0, 0.0, 1.0],
                false,
            );
            let fills = wires
                .into_iter()
                .filter(|wire| wire.points.is_empty() && !wire.fill_tris.is_empty())
                .map(|wire| PlotWire {
                    wire,
                    draw_depth: 0.5,
                })
                .collect::<Vec<_>>();
            assert_eq!(fills.len(), 1, "flags={flags:#x}");
            let content = crate::io::plot::PlotContent {
                wires: std::sync::Arc::new(fills),
                ..Default::default()
            };
            let svg = build_svg(&page_with(content, 210.0, 297.0), None).expect("builds");
            assert!(
                svg.lines().any(|line| {
                    line.starts_with("<path") && line.contains(&format!("fill=\"{expected_fill}\""))
                }),
                "flags={flags:#x}: wrong mask color in {svg}"
            );
        }
    }
    #[test]
    fn text_as_outlines_replaces_font_dependent_text_with_vector_paths() {
        // Other tests share and can reset the global atlas. Rebuild the quads
        // whenever that happens, as the existing PDF export regressions do.
        let mut text = String::new();
        let mut outlined = String::new();
        let mut fallback = String::new();
        let mut stable = false;
        for _ in 0..8 {
            let generation = crate::scene::text::sdf_atlas::generation();
            let mtext = document::entities::MText::with_value(
                "AB",
                document::types::Vector3::new(10.0, 10.0, 0.0),
            );
            let wires = crate::scene::convert::tessellate::tessellate(
                &document::Document::new(),
                document::Handle::new(124),
                &document::EntityType::MText(Box::new(mtext)),
                false,
                [1.0; 4],
                0.0,
                [0.0; 8],
                1.0,
                1.0,
                None,
                None,
                [1.0; 4],
                false,
            );
            assert!(wires.iter().any(|wire| !wire.text_verts.is_empty()));
            let wires = wires
                .into_iter()
                .map(|wire| PlotWire {
                    wire,
                    draw_depth: 0.5,
                })
                .collect::<Vec<_>>();
            let render = |outlines| {
                let content = crate::io::plot::PlotContent {
                    wires: std::sync::Arc::new(wires.clone()),
                    ..Default::default()
                };
                let mut page = page_with(content, 210.0, 297.0);
                page.options.text_as_outlines = outlines;
                page.options.stamp = true;
                build_svg(&page, None).expect("builds")
            };
            text = render(false);
            outlined = render(true);

            // A stale/unknown atlas key must not drop the label or emit a
            // partial outline run: retain the original searchable text.
            let mut fallback_wires = wires.clone();
            for vertex in fallback_wires
                .iter_mut()
                .flat_map(|wire| wire.wire.text_verts.iter_mut())
            {
                vertex.uv = [-1.0, -1.0];
            }
            let content = crate::io::plot::PlotContent {
                wires: std::sync::Arc::new(fallback_wires),
                ..Default::default()
            };
            let mut page = page_with(content, 210.0, 297.0);
            page.options.text_as_outlines = true;
            fallback = build_svg(&page, None).expect("fallback builds");
            if crate::scene::text::sdf_atlas::generation() == generation {
                stable = true;
                break;
            }
        }
        assert!(stable, "glyph atlas did not stabilize after eight attempts");

        assert!(text.contains("<text"), "default mode must keep text runs");
        assert!(
            text.contains("Open CAD Studio |"),
            "the default plot stamp remains searchable"
        );
        assert!(
            !outlined.contains("<text") && !outlined.contains("font-family"),
            "outline mode must not need a viewer font: {outlined}"
        );
        assert!(
            outlined.lines().any(|line| line.starts_with("<path")),
            "outline mode emitted no vector glyphs: {outlined}"
        );
        roxmltree::Document::parse(&outlined).expect("outline SVG is well-formed XML");
        assert!(fallback.contains(">AB</text>"), "{fallback}");
    }
    #[test]
    fn text_outlines_preserve_inline_colors_rotation_and_decoration_placement() {
        use crate::scene::text::sdf_atlas::{self, GlyphAtlas};

        // A local atlas keeps this geometry assertion independent of other
        // tests resetting the process-global atlas used by the full builder.
        let mut atlas = GlyphAtlas::new(512, 512);
        let mut wire = WireModel::default();
        for (text, origin, color) in [
            ("R", [40.0, 50.0, 0.0], [1.0, 0.0, 0.0, 1.0]),
            (r"\LG\l", [45.0, 50.0, 0.0], [0.0, 1.0, 0.0, 1.0]),
        ] {
            let (quads, _) = crate::scene::text::glyph_quads::layout_glyph_quads(
                &mut atlas,
                2.5,
                std::f32::consts::FRAC_PI_6,
                1.0,
                0.0,
                1.0,
                "txt",
                false,
                text,
            );
            crate::scene::pipeline::text_gpu::push_glyph_vertices(
                &mut wire.text_verts,
                &quads,
                origin,
                1.0,
                color,
                0.0,
            );
        }
        let glyphs = GlyphSnapshot {
            table: atlas.export_table(),
            solid_key: sdf_atlas::uv_key(atlas.solid_uv()),
        };
        let vertex_xy = |vertex: &crate::scene::pipeline::text_gpu::TextVertex| {
            [
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
            ]
        };
        let decoration = wire
            .text_verts
            .chunks_exact(6)
            .find(|quad| sdf_atlas::uv_key([quad[5].uv[0], quad[5].uv[1]]) == glyphs.solid_key)
            .expect("generated underline quad");
        let corners = [0, 1, 2, 5].map(|index| vertex_xy(&decoration[index]));
        let map = PageMap {
            paper_w: 210.0,
            paper_h: 297.0,
            scale: 1.0,
            rotation_deg: 0,
        };
        let points = corners.map(|point| map.apply(point[0], point[1]));
        let expected_decoration = format!(
            "M {} {} L {} {} L {} {} L {} {} Z",
            fmt(points[0].0),
            fmt(points[0].1),
            fmt(points[1].0),
            fmt(points[1].1),
            fmt(points[2].0),
            fmt(points[2].1),
            fmt(points[3].0),
            fmt(points[3].1),
        );

        let emit = |wire: &WireModel, glyphs: &GlyphSnapshot| {
            let mut out = String::new();
            let emitted = emit_text_outlines(
                &mut out,
                wire,
                OutlineContext {
                    ox: 0.0,
                    oy: 0.0,
                    plot_style: None,
                    options: PlotOptions {
                        text_as_outlines: true,
                        ..Default::default()
                    },
                    map: &map,
                    glyphs,
                },
            );
            (emitted, out)
        };
        let (emitted, paths) = emit(&wire, &glyphs);
        assert!(emitted);
        assert!(!paths.contains("<text") && !paths.contains("font-family"));
        assert!(paths.contains("#ff0000"), "red run missing: {paths}");
        assert!(paths.contains("#00ff00"), "green run missing: {paths}");
        assert!(
            paths.contains(&expected_decoration),
            "rotated underline moved: expected {expected_decoration} in {paths}"
        );
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{paths}</svg>");
        roxmltree::Document::parse(&svg).expect("well-formed outline SVG");

        // The second quad is malformed after a valid first quad. Nothing from
        // the valid prefix may escape before the whole-wire fallback decision.
        let mut malformed = wire.clone();
        malformed.text_verts[6].pos[0] = f32::NAN;
        let (emitted, partial) = emit(&malformed, &glyphs);
        assert!(!emitted && partial.is_empty(), "partial label: {partial}");

        let first_key = sdf_atlas::uv_key([wire.text_verts[5].uv[0], wire.text_verts[5].uv[1]]);
        let mut degenerate = GlyphSnapshot {
            table: glyphs.table.clone(),
            solid_key: glyphs.solid_key,
        };
        let glyph = degenerate.table.get_mut(&first_key).expect("first glyph");
        glyph.plane_max = glyph.plane_min;
        let (emitted, partial) = emit(&wire, &degenerate);
        assert!(!emitted && partial.is_empty(), "partial label: {partial}");

        let mut nonfinite_alpha = wire.clone();
        nonfinite_alpha.text_verts[6].color[3] = f32::NAN;
        nonfinite_alpha.text_verts[11].uv = [-1.0, -1.0];
        let (emitted, partial) = emit(&nonfinite_alpha, &glyphs);
        assert!(!emitted && partial.is_empty(), "partial label: {partial}");

        // Empty substrokes are ignored and a one-point stroke becomes a
        // zero-length segment, painted as a dot by the inherited round cap.
        let mut point_wire = WireModel::default();
        point_wire.text_verts = wire.text_verts[..6].to_vec();
        let mut point_glyphs = GlyphSnapshot {
            table: glyphs.table.clone(),
            solid_key: glyphs.solid_key,
        };
        let glyph = point_glyphs.table.get_mut(&first_key).expect("point glyph");
        let point = glyph.plane_min;
        glyph.fill_tris.clear();
        glyph.strokes = vec![Vec::new(), vec![point]];
        let (emitted, point_path) = emit(&point_wire, &point_glyphs);
        let origin = vertex_xy(&point_wire.text_verts[0]);
        let (x, y) = map.apply(origin[0], origin[1]);
        let expected_dot = format!("M {} {} L {} {}", fmt(x), fmt(y), fmt(x), fmt(y));
        assert!(
            emitted && point_path.contains(&expected_dot),
            "{point_path}"
        );
    }
    /// With the atlas keeping its UVs across growth (tiles persist), one
    /// snapshot taken after the stamp's layout serves the page text and the
    /// stamp alike: the drawing quads laid out before the growth still
    /// resolve, and there is no pre-stamp snapshot to keep.
    #[test]
    fn stamp_atlas_growth_keeps_one_post_stamp_snapshot_valid() {
        use crate::scene::text::glyph_quads::layout_glyph_quads;
        use crate::scene::text::sdf_atlas::{self, GlyphAtlas};

        let mut atlas = GlyphAtlas::new(64, 64);
        let (drawing_quads, _) =
            layout_glyph_quads(&mut atlas, 2.5, 0.0, 1.0, 0.0, 1.0, "txt", false, "A");
        let mut drawing = WireModel::default();
        crate::scene::pipeline::text_gpu::push_glyph_vertices(
            &mut drawing.text_verts,
            &drawing_quads,
            [20.0, 20.0, 0.0],
            1.0,
            [0.0, 0.0, 0.0, 1.0],
            0.0,
        );
        let drawing_key =
            sdf_atlas::uv_key([drawing.text_verts[5].uv[0], drawing.text_verts[5].uv[1]]);

        let height_before_stamp = atlas.height();
        let label = plot_stamp_label();
        let (stamp_quads, _) =
            layout_glyph_quads(&mut atlas, 2.5, 0.0, 1.0, 0.0, 1.0, "txt", false, &label);
        let mut stamp = WireModel::default();
        crate::scene::pipeline::text_gpu::push_glyph_vertices(
            &mut stamp.text_verts,
            &stamp_quads,
            [4.0, 3.0, 0.0],
            1.0,
            [0.25, 0.25, 0.25, 1.0],
            0.0,
        );
        assert!(
            atlas.height() > height_before_stamp,
            "the generated stamp must force this small atlas to grow"
        );
        // The single post-stamp snapshot is all the export keeps.
        let glyphs = GlyphSnapshot::from_atlas(&atlas);
        assert!(
            glyphs.table.contains_key(&drawing_key),
            "growth keeps UVs, so the one post-stamp snapshot still serves the page"
        );
        let map = PageMap {
            paper_w: 210.0,
            paper_h: 297.0,
            scale: 1.0,
            rotation_deg: 0,
        };
        let emit = |wire: &WireModel, glyphs: &GlyphSnapshot| {
            let mut out = String::new();
            let emitted = emit_text_outlines(
                &mut out,
                wire,
                OutlineContext {
                    ox: 0.0,
                    oy: 0.0,
                    plot_style: None,
                    options: PlotOptions {
                        text_as_outlines: true,
                        ..Default::default()
                    },
                    map: &map,
                    glyphs,
                },
            );
            (emitted, out)
        };
        let (emitted, page_paths) = emit(&drawing, &glyphs);
        assert!(emitted && !page_paths.is_empty(), "page text: {page_paths}");
        let (emitted, stamp_paths) = emit(&stamp, &glyphs);
        assert!(
            emitted && !stamp_paths.is_empty(),
            "stamp text: {stamp_paths}"
        );
    }

    #[test]
    fn xml_escape_replaces_every_codepoint_forbidden_by_xml_10() {
        assert_eq!(
            xml_escape("A\u{0}\u{2}\u{b}\u{c}\u{1f}B"),
            "A\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}B"
        );
        assert_eq!(xml_escape("\u{fffe}\u{ffff}"), "\u{fffd}\u{fffd}");
        assert_eq!(xml_escape("tab\tline\ncarriage\r"), "tab\tline\ncarriage\r");
        assert_eq!(
            xml_escape("C1:\u{80}; Unicode:č😀"),
            "C1:\u{80}; Unicode:č😀"
        );
    }

    #[test]
    fn forbidden_xml_control_in_text_does_not_invalidate_the_document() {
        let mut wire = WireModel::default();
        wire.searchable_text = vec![crate::scene::model::wire_model::SearchableTextRun {
            text: "before\u{2}after".to_string(),
            origin: [10.0, 20.0, 0.0],
            height: 2.0,
            rotation: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
            bold: false,
            font: "Arial".to_string(),
            adv_width: 10.0,
        }];
        let content = crate::io::plot::PlotContent {
            wires: std::sync::Arc::new(vec![PlotWire {
                wire,
                draw_depth: 0.5,
            }]),
            ..Default::default()
        };
        let svg = build_svg(&page_with(content, 210.0, 297.0), None).expect("builds");
        assert!(svg.contains("before\u{fffd}after"), "{svg}");
        roxmltree::Document::parse(&svg).expect("the exported SVG is well-formed XML");
    }

    #[test]
    fn clip_rect_becomes_a_clip_path() {
        let mut page = page_with(Default::default(), 210.0, 297.0);
        page.clip = Some((10.0, 20.0, 100.0, 50.0));
        let svg = build_svg(&page, None).expect("builds");
        assert!(svg.contains("<clipPath id=\"plot-clip\">"), "{svg}");
        // y-flipped: rect y = 297 - (20 + 50) = 227.
        assert!(
            svg.contains("x=\"10\" y=\"227\" width=\"100\" height=\"50\""),
            "{svg}"
        );
    }
}
