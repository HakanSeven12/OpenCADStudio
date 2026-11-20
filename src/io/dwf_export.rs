// DWF export — plots the shared paper-space `PlotPage` model to a DWF 6
// package: a zip after a "(DWF V06.00)" tag holding a manifest and, for each
// sheet, a descriptor and a W2D graphics stream. It is the package the DWF
// underlay reader (`scene::model::underlay_vector::dwf`) opens, written the
// other way round.
//
// Scope of this first version:
// - Wires (curves flattened, dashes cut as on screen), lineweights, colours
//   through the plot style, solid fills, hatches, wipeouts and text runs.
// - Raster images and the plot stamp are left out.
// - Graphics units are 1/1200 inch on paper, y up, origin at the sheet's
//   lower left. The stream's `Units` carry the plot scale, so a reader maps
//   the sheet back to the units it was plotted from.
// - Everything is cut to the sheet (and to the plot window, when the page
//   has one): the stream itself clips nothing.

use std::io::Write as _;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::io::pdf_export::{adapt_to_paper, dash_ranges, MIN_PLOT_PATTERN_MM};
use crate::io::plot::{PlotOptions, PlotPage, PlotWire};
use crate::io::plot_style::PlotStyleTable;
use crate::io::svg_export::plotted_color;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::underlay_vector::CAP_PER_EM;
use crate::scene::text::glyph_to_world::{self, GlyphSnapshot, QuadBasis};
use crate::scene::text::sdf_atlas::GlyphExport;
use crate::scene::WireModel;

/// Graphics units to a millimetre of paper: each is 1/1200 inch.
const UNITS_PER_MM: f64 = 1200.0 / 25.4;
/// Screen pixels (96 dpi) to millimetres.
const PX_TO_MM: f64 = 25.4 / 96.0;
/// Points to millimetres: the thinnest pen is a tenth of a point.
const PT_TO_MM: f64 = 25.4 / 72.0;
/// The most points one polyline or triangle strip of the stream counts: a
/// byte, or a zero byte and sixteen bits more for the counts past 255.
const MAX_COUNT: usize = 255 + 1 + u16::MAX as usize;
/// How far a flattened curve may leave its curve, in graphics units.
const CURVE_TOLERANCE: f64 = 0.5;
/// The package tag every DWF 6 file starts with, before its zip.
const PACKAGE_TAG: &[u8] = b"(DWF V06.00)";
/// The format's own identifiers for a plotted sheet: its section type and
/// the interface a package of such sheets declares.
const EPLOT_SECTION: &str = "com.autodesk.dwf.ePlot";
const EPLOT_INTERFACE_ID: &str = "715941D4-1AC2-4545-8185-BC40E053B551";
const EPLOT_INTERFACE_HREF: &str = "http://www.autodesk.com/viewers";

/// One sheet of the package: a plot page under the name readers list it by.
pub struct DwfSheet<'a> {
    pub name: &'a str,
    pub page: &'a PlotPage,
}

/// The bytes of a DWF 6 package with one section per sheet, in order.
pub fn build_dwf(
    sheets: &[DwfSheet<'_>],
    fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<Vec<u8>, String> {
    // The atlas as it stands, once for the package, when a page has text
    // laid out on it.
    let glyphs = sheets
        .iter()
        .flat_map(|sheet| sheet.page.content.wires.iter())
        .any(|wire| !wire.wire.text_verts.is_empty())
        .then(GlyphSnapshot::snapshot)
        .flatten();
    build_dwf_with_glyphs(sheets, fallback_plot_style, glyphs.as_ref())
}

/// [`build_dwf`] with the glyphs of the atlas the pages' text was laid out on.
fn build_dwf_with_glyphs(
    sheets: &[DwfSheet<'_>],
    fallback_plot_style: Option<&PlotStyleTable>,
    glyphs: Option<&GlyphSnapshot>,
) -> Result<Vec<u8>, String> {
    if sheets.is_empty() {
        return Err("Nothing to export: no sheet was given.".to_string());
    }
    let unit_mm = 1.0 / UNITS_PER_MM;
    let mut manifest_sections = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, sheet) in sheets.iter().enumerate() {
        let page = sheet.page;
        let stream = page_stream(page, fallback_plot_style, glyphs);
        let section_id = object_id(&stream, index, 0);
        let stream_id = object_id(&stream, index, 1);
        let section = format!("{EPLOT_SECTION}_{section_id}");
        let stream_href = format!("{section}\\{stream_id}.w2d");
        let descriptor_href = format!("{section}\\descriptor.xml");
        let (paper_w, paper_h) = (page.paper_w.max(1.0), page.paper_h.max(1.0));
        let descriptor = format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<ePlot:Page xmlns:ePlot=\"DWF-ePlot:1.2\" version=\"1.2\" name=\"{name}\" ",
                "plotOrder=\"{order}\" color=\"128 128 128\">",
                "<ePlot:Paper units=\"mm\" width=\"{w}\" height=\"{h}\" color=\"255 255 255\" ",
                "clip=\"0 0 {w} {h}\"/>",
                "<ePlot:Resources>",
                "<ePlot:GraphicResource role=\"2d streaming graphics\" mime=\"application/x-w2d\" ",
                "href=\"{href}\" size=\"{size}\" objectId=\"{id}\" ",
                "transform=\"{unit} 0 0 0 0 {unit} 0 0 0 0 1 0 0 0 0 1\"/>",
                "</ePlot:Resources></ePlot:Page>\n",
            ),
            name = xml_attr(sheet.name),
            order = index + 1,
            w = number(paper_w),
            h = number(paper_h),
            href = xml_attr(&stream_href),
            size = stream.len(),
            id = stream_id,
            unit = unit_mm,
        );
        manifest_sections.push_str(&format!(
            concat!(
                "<dwf:Section type=\"{kind}\" name=\"{section}\" title=\"{title}\"><dwf:Toc>",
                "<dwf:Resource role=\"2d streaming graphics\" mime=\"application/x-w2d\" ",
                "href=\"{stream}\"/>",
                "<dwf:Resource role=\"descriptor\" mime=\"text/xml\" href=\"{descriptor}\"/>",
                "</dwf:Toc></dwf:Section>",
            ),
            kind = EPLOT_SECTION,
            section = xml_attr(&section),
            title = xml_attr(sheet.name),
            stream = xml_attr(&stream_href),
            descriptor = xml_attr(&descriptor_href),
        ));
        files.push((descriptor_href.replace('\\', "/"), descriptor.into_bytes()));
        files.push((stream_href.replace('\\', "/"), stream));
    }
    let manifest_id = object_id(manifest_sections.as_bytes(), sheets.len(), 2);
    let manifest = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<dwf:Manifest xmlns:dwf=\"DWF-Manifest:6.0\" version=\"6.0\" objectId=\"{id}\">",
            "<dwf:Interfaces><dwf:Interface objectId=\"{interface}\" name=\"ePlot\" ",
            "href=\"{href}\"/></dwf:Interfaces>",
            "<dwf:Properties>",
            "<dwf:Property name=\"SourceProductName\" value=\"Open CAD Studio\"/>",
            "<dwf:Property name=\"SourceProductVersion\" value=\"{version}\"/>",
            "</dwf:Properties>",
            "<dwf:Sections>{sections}</dwf:Sections></dwf:Manifest>\n",
        ),
        id = manifest_id,
        interface = EPLOT_INTERFACE_ID,
        href = EPLOT_INTERFACE_HREF,
        version = xml_attr(env!("OCS_APP_VERSION")),
        sections = manifest_sections,
    );

    // The zip follows the tag in one file, its offsets counted from the
    // start of the file: the writer begins after the tag.
    let mut cursor = std::io::Cursor::new(PACKAGE_TAG.to_vec());
    cursor.set_position(PACKAGE_TAG.len() as u64);
    let mut zip = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let failed = |error: &dyn std::fmt::Display| format!("Failed to build DWF package: {error}");
    for (name, data) in
        std::iter::once(("manifest.xml".to_string(), manifest.into_bytes())).chain(files)
    {
        zip.start_file(name, options).map_err(|e| failed(&e))?;
        zip.write_all(&data).map_err(|e| failed(&e))?;
    }
    Ok(zip.finish().map_err(|e| failed(&e))?.into_inner())
}

/// Export several sheets into one DWF file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_dwf_pages(
    sheets: &[DwfSheet<'_>],
    path: &Path,
    fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    let bytes = build_dwf(sheets, fallback_plot_style)?;
    let temp_path = super::save_temp_path(path);
    if let Err(error) = std::fs::write(&temp_path, bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write DWF data: {error}"));
    }
    if let Err(error) = super::replace_save_file(&temp_path, path, false) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to replace DWF file: {error}"));
    }
    Ok(())
}

/// Export one page to a DWF file, as the sheet `name`.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_dwf(page: &PlotPage, name: &str, path: &Path) -> Result<(), String> {
    export_dwf_pages(&[DwfSheet { name, page }], path, None)
}

#[cfg(target_arch = "wasm32")]
pub fn export_dwf(_page: &PlotPage, _name: &str, _path: &Path) -> Result<(), String> {
    Err("DWF export is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub fn export_dwf_pages(
    _sheets: &[DwfSheet<'_>],
    _path: &Path,
    _fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    Err("DWF export is not available in the web version.".into())
}

/// An identifier in the format's `8-4-4-4-12` shape, fixed by what it names
/// so the same sheets always build the same package.
fn object_id(data: &[u8], index: usize, role: u8) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.update((index as u64).to_le_bytes());
    hasher.update([role]);
    let hash = hasher.finalize();
    let hex: String = hash[..16].iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A number as short as it reads back exactly: no exponent, no trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.9}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

fn xml_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Controls an XML 1.0 document cannot carry.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

// ── The page on the sheet ───────────────────────────────────────────────────

/// Plot coordinates to graphics units: the plot scale and the page's quarter
/// turns, as the PDF and SVG backends place them, with y up.
struct PageMap {
    paper_w: f64,
    paper_h: f64,
    scale: f64,
    rotation_deg: i32,
}

impl PageMap {
    fn apply(&self, x: f64, y: f64) -> [f64; 2] {
        let s = self.scale;
        let (xt, yt) = match self.rotation_deg {
            90 => (self.paper_w - s * y, s * x),
            180 => (self.paper_w - s * x, self.paper_h - s * y),
            270 => (s * y, self.paper_h - s * x),
            _ => (s * x, s * y),
        };
        [xt * UNITS_PER_MM, yt * UNITS_PER_MM]
    }
}

/// The sheet's graphics stream.
fn page_stream(
    page: &PlotPage,
    fallback_plot_style: Option<&PlotStyleTable>,
    glyphs: Option<&GlyphSnapshot>,
) -> Vec<u8> {
    let paper_w = page.paper_w.max(1.0);
    let paper_h = page.paper_h.max(1.0);
    let map = PageMap {
        paper_w,
        paper_h,
        scale: page.scale.max(1e-6) as f64,
        rotation_deg: page.rotation_deg,
    };
    let plot_style = page.plot_style.as_ref().or(fallback_plot_style);
    let (w, h) = (
        (paper_w * UNITS_PER_MM).round() as i32,
        (paper_h * UNITS_PER_MM).round() as i32,
    );
    // What is drawn: the sheet, narrowed to the plot window when there is
    // one. Half a unit of slack keeps what lies on an edge.
    let mut bounds = [
        -0.49,
        -0.49,
        paper_w * UNITS_PER_MM + 0.49,
        paper_h * UNITS_PER_MM + 0.49,
    ];
    if let Some((cx, cy, cw, ch)) = page.clip {
        let corners = [
            map.apply(cx as f64, cy as f64),
            map.apply((cx + cw) as f64, cy as f64),
            map.apply((cx + cw) as f64, (cy + ch) as f64),
            map.apply(cx as f64, (cy + ch) as f64),
        ];
        let low = |axis: usize| {
            corners
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
        };
        let high = |axis: usize| {
            corners
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let window = [low(0), low(1), high(0), high(1)];
        if window.iter().all(|v| v.is_finite()) && window[0] < window[2] && window[1] < window[3] {
            bounds = [
                bounds[0].max(window[0]),
                bounds[1].max(window[1]),
                bounds[2].min(window[2]),
                bounds[3].min(window[3]),
            ];
        }
    }

    let units_per_drawing_unit = UNITS_PER_MM * map.scale;
    let step = number(units_per_drawing_unit);
    let units = format!("(({step} 0 0 0)(0 {step} 0 0)(0 0 1 0)(0 0 0 1))");
    let unit_mm = number(1.0 / UNITS_PER_MM);
    let mut stream = Stream::new();
    stream.extended("W2D V06.00");
    stream.extended(&format!(
        "Creator 'Open CAD Studio {}'",
        env!("OCS_APP_VERSION")
    ));
    stream.extended(&format!("Units '' {units}"));
    stream.extended(&format!("View 0,0 {w},{h}"));
    stream.extended(&format!(
        "PlotInfo show 0 mm {pw} {ph} 0 0 {pw} {ph} (({unit_mm} 0 0)(0 {unit_mm} 0)(0 0 1))",
        pw = number(paper_w),
        ph = number(paper_h),
    ));
    stream.out.push(b'V');
    stream.extended(&format!(
        "Viewport '' (Contour 1 4 {w},0 {w},{h} 0,{h} 0,0) (Units '' {units})"
    ));
    if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
        emit_page_content(&mut stream, page, plot_style, glyphs, &map, bounds);
    }
    stream.extended("EndOfDWF");
    stream.out
}

enum DrawItem<'a> {
    WireFill(&'a PlotWire),
    Hatch(&'a HatchModel),
    Wire(&'a PlotWire),
    Text(&'a PlotWire),
}

/// The page's content in the order the PDF and SVG backends draw it: by
/// depth, fills under hatches under lines under text.
fn emit_page_content(
    stream: &mut Stream,
    page: &PlotPage,
    plot_style: Option<&PlotStyleTable>,
    glyphs: Option<&GlyphSnapshot>,
    map: &PageMap,
    bounds: [f64; 4],
) {
    let content = &page.content;
    let offset = (page.offset_x, page.offset_y);
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
    for (wires, hatches, wipeouts) in [
        (first_wires, first_hatches, first_wipeouts),
        (second_wires, second_hatches, second_wipeouts),
    ] {
        let mut items = Vec::with_capacity(wires.len() * 2 + hatches.len() + wipeouts.len());
        let mut sequence = 0usize;
        for wire in wires {
            if !wire.wire.fill_tris.is_empty() {
                items.push((wire.draw_depth, 0u8, sequence, DrawItem::WireFill(wire)));
                sequence += 1;
            }
            items.push((wire.draw_depth, 2u8, sequence, DrawItem::Wire(wire)));
            sequence += 1;
            if !wire.wire.searchable_text.is_empty() || !wire.wire.text_verts.is_empty() {
                items.push((wire.draw_depth, 3u8, sequence, DrawItem::Text(wire)));
                sequence += 1;
            }
        }
        for hatch in wipeouts.iter().chain(hatches.iter()) {
            items.push((hatch.draw_depth, 1u8, sequence, DrawItem::Hatch(hatch)));
            sequence += 1;
        }
        items.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        for (_, _, _, item) in items {
            let context = Context {
                offset,
                plot_style,
                glyphs,
                options,
                map,
                bounds,
            };
            match item {
                DrawItem::WireFill(wire) => emit_wire_fills(stream, &wire.wire, &context),
                DrawItem::Hatch(hatch) => emit_hatch(stream, hatch, &context),
                DrawItem::Wire(wire) => emit_wire(stream, &wire.wire, &context),
                DrawItem::Text(wire) => emit_text(stream, &wire.wire, &context),
            }
        }
    }
}

struct Context<'a> {
    offset: (f64, f64),
    plot_style: Option<&'a PlotStyleTable>,
    /// The atlas the page's text was laid out on, when it could be read.
    glyphs: Option<&'a GlyphSnapshot>,
    options: PlotOptions,
    map: &'a PageMap,
    bounds: [f64; 4],
}

impl Context<'_> {
    /// A plot coordinate on the sheet, in graphics units.
    fn place(&self, x: f64, y: f64) -> [f64; 2] {
        self.map.apply(x + self.offset.0, y + self.offset.1)
    }

    /// A pen width in graphics units, at least one.
    fn pen(&self, width_mm: f64) -> i32 {
        (width_mm * UNITS_PER_MM)
            .round()
            .clamp(1.0, i32::MAX as f64) as i32
    }

    /// The width a lineweight plots at, in millimetres: the plot style's
    /// when it sets one, as the other backends resolve it.
    fn lineweight_mm(&self, style_mm: Option<f64>, line_weight_px: f32) -> f64 {
        let physical = if self.options.object_lineweights {
            style_mm.unwrap_or(line_weight_px as f64 * PX_TO_MM)
        } else {
            0.1 * PT_TO_MM
        }
        .max(0.1 * PT_TO_MM);
        if self.options.scale_lineweights {
            physical * self.map.scale
        } else {
            physical
        }
    }
}

fn rgb_bytes([r, g, b]: [f32; 3]) -> [u8; 3] {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [channel(r), channel(g), channel(b)]
}

// ── Wires ───────────────────────────────────────────────────────────────────

fn emit_wire(stream: &mut Stream, wire: &WireModel, context: &Context) {
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
    let mut style_mm: Option<f64> = None;
    let mut screening = 1.0;
    let mut color_overridden = false;
    if let Some(table) = context.plot_style {
        if wire.aci > 0 {
            if let Some(color) = table.resolve_color(wire.aci) {
                [r, g, b] = color;
                color_overridden = true;
            }
            style_mm = table.resolve_lineweight(wire.aci).map(|mm| mm as f64);
            screening = table.resolve_screening(wire.aci);
        }
    }
    if !color_overridden && (wire.aci != 0 || [r, g, b] == [1.0; 3]) {
        [r, g, b] = adapt_to_paper([r, g, b]);
    }
    let color = rgb_bytes(plotted_color([r, g, b], a, screening, context.options));
    let scale = context.map.scale;
    let width_mm = if wire.world_width > 0.0 {
        wire.world_width as f64 * scale
    } else {
        context.lineweight_mm(style_mm, wire.line_weight_px)
    };
    let pen = context.pen(width_mm);
    let view_h = context.map.paper_h / scale;

    // A dashed wire is cut into its dashes as the screen lays them out. A
    // pattern shorter on paper than a pen resolves plots as a solid line.
    let dashed = wire.pattern_length > 1e-6
        && wire.pattern.iter().any(|v| *v != 0.0)
        && wire.pattern_length * scale as f32 >= MIN_PLOT_PATTERN_MM;
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    if dashed {
        let dashes = crate::scene::pipeline::wire_gpu::wire_distances(wire);
        let mut run: Vec<[f64; 2]> = Vec::new();
        for index in 0..wire.points.len().saturating_sub(1) {
            if !wire.points[index][0].is_finite() || !wire.points[index + 1][0].is_finite() {
                runs.push(std::mem::take(&mut run));
                continue;
            }
            let start = wire.point_world(index, view_h);
            let end = wire.point_world(index + 1, view_h);
            let point = |t: f32| {
                context.place(
                    start.x + (end.x - start.x) * t as f64,
                    start.y + (end.y - start.y) * t as f64,
                )
            };
            let ranges = dash_ranges(
                dashes.dists[index],
                dashes.dists[index + 1],
                dashes.run_totals[index],
                wire.pattern_length,
                &wire.pattern,
            );
            if ranges.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            for [from, to] in ranges {
                // A dash running on across a vertex stays one stroke.
                if from > 1e-5 || run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                    run.push(point(from));
                }
                run.push(point(to));
                if to < 1.0 - 1e-5 {
                    runs.push(std::mem::take(&mut run));
                }
            }
        }
        runs.push(run);
    } else if let Some(spans) = crate::io::plot::wire_arc_beziers(wire, view_h) {
        // A circle or arc is drawn along its curve, not along the screen's
        // chords.
        let mut run: Vec<[f64; 2]> = Vec::new();
        for span in spans {
            let [p0, c1, c2, p3] = span.map(|p| context.place(p.x, p.y));
            if run.is_empty() {
                run.push(p0);
            }
            flatten_cubic(&mut run, p0, c1, c2, p3);
        }
        runs.push(run);
    } else {
        let mut run: Vec<[f64; 2]> = Vec::new();
        for (index, &[x, y, _]) in wire.points.iter().enumerate() {
            if x.is_nan() || y.is_nan() {
                runs.push(std::mem::take(&mut run));
                continue;
            }
            let p = wire.point_world(index, view_h);
            run.push(context.place(p.x, p.y));
        }
        runs.push(run);
    }

    let mut styled = false;
    for run in runs {
        for piece in clip_polyline(&run, context.bounds) {
            let points = rounded(&piece);
            if points.len() < 2 {
                continue;
            }
            if !styled {
                stream.color(color);
                stream.weight(pen);
                styled = true;
            }
            stream.polyline(&points);
        }
    }
}

/// The points of a cubic after its start, close enough to it that the
/// polyline through them never leaves the curve by more than the tolerance.
fn flatten_cubic(out: &mut Vec<[f64; 2]>, p0: [f64; 2], c1: [f64; 2], c2: [f64; 2], p3: [f64; 2]) {
    let second = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        ((a[0] - 2.0 * b[0] + c[0]).powi(2) + (a[1] - 2.0 * b[1] + c[1]).powi(2)).sqrt()
    };
    let bend = second(p0, c1, c2).max(second(c1, c2, p3));
    let pieces = if bend.is_finite() {
        ((0.75 * bend / CURVE_TOLERANCE).sqrt().ceil() as usize).clamp(1, 1024)
    } else {
        1
    };
    for step in 1..=pieces {
        let t = step as f64 / pieces as f64;
        let u = 1.0 - t;
        let weight = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push([
            weight[0] * p0[0] + weight[1] * c1[0] + weight[2] * c2[0] + weight[3] * p3[0],
            weight[0] * p0[1] + weight[1] * c1[1] + weight[2] * c2[1] + weight[3] * p3[1],
        ]);
    }
}

/// Solid triangle fills (SOLID fills, arrowheads, 2-D overlays).
fn emit_wire_fills(stream: &mut Stream, wire: &WireModel, context: &Context) {
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
        if let Some(table) = context.plot_style {
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
        [r, g, b] = plotted_color([r, g, b], a, screening, context.options);
    }
    let color = rgb_bytes([r, g, b]);
    for (index, triangle) in wire.fill_tris.chunks_exact(3).enumerate() {
        let corner = |k: usize| {
            let [x, y, _] = triangle[k];
            let low = wire
                .fill_tris_low
                .get(index * 3 + k)
                .copied()
                .unwrap_or([0.0; 3]);
            context.place(x as f64 + low[0] as f64, y as f64 + low[1] as f64)
        };
        fill_polygon(
            stream,
            color,
            &[corner(0), corner(1), corner(2)],
            context.bounds,
        );
    }
}

// ── Hatches and wipeouts ────────────────────────────────────────────────────

fn emit_hatch(stream: &mut Stream, hatch: &HatchModel, context: &Context) {
    if hatch.boundary.is_empty() {
        return;
    }
    let is_wipeout = hatch.name == "WIPEOUT_FILL";
    let [mut r, mut g, mut b, a] = hatch.color;
    if a < 0.01 {
        return;
    }
    let mut screening = 1.0;
    let mut style_mm: Option<f64> = None;
    let mut color_overridden = false;
    if !is_wipeout {
        if let Some(table) = context.plot_style {
            if hatch.aci > 0 {
                if let Some(color) = table.resolve_color(hatch.aci) {
                    [r, g, b] = color;
                    color_overridden = true;
                }
                screening = table.resolve_screening(hatch.aci);
                style_mm = table.resolve_lineweight(hatch.aci).map(|mm| mm as f64);
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
    [r, g, b] = plotted_color([r, g, b], a, screening, context.options);

    if matches!(hatch.pattern, HatchPattern::Pattern(_)) {
        let pen = context.pen(context.lineweight_mm(style_mm, hatch.line_weight_px));
        let fine = crate::io::plot::hatch_with_fine_arcs(hatch);
        let segments = fine.as_ref().unwrap_or(hatch).pattern_segments_for_plot();
        let color = rgb_bytes([r, g, b]);
        let mut styled = false;
        for [p, q] in segments {
            let (a, b) = (context.place(p[0], p[1]), context.place(q[0], q[1]));
            let Some((a, b)) = clip_segment(a, b, context.bounds) else {
                continue;
            };
            let mut points = rounded(&[a, b]);
            if points.len() < 2 {
                // A dot of the pattern: a mark one pen wide.
                let [x, y] = points[0];
                points.push([x.saturating_add(pen.max(1)), y]);
            }
            if !styled {
                stream.color(color);
                stream.weight(pen);
                styled = true;
            }
            stream.polyline(&points);
        }
        return;
    }

    if let HatchPattern::Gradient { color2, .. } = &hatch.pattern {
        // One colour between the gradient's two.
        let second = plotted_color(
            adapt_to_paper([color2[0], color2[1], color2[2]]),
            color2[3],
            screening,
            context.options,
        );
        [r, g, b] = [
            (r + second[0]) * 0.5,
            (g + second[1]) * 0.5,
            (b + second[2]) * 0.5,
        ];
    }
    let color = rgb_bytes([r, g, b]);
    let (vertices, indices) = hatch.fill_mesh();
    let corner = |index: u32| {
        let [x, y] = vertices[index as usize];
        context.place(
            x as f64 + hatch.world_origin[0],
            y as f64 + hatch.world_origin[1],
        )
    };
    for triangle in indices.chunks_exact(3) {
        if triangle
            .iter()
            .any(|index| *index as usize >= vertices.len())
        {
            continue;
        }
        fill_polygon(
            stream,
            color,
            &[
                corner(triangle[0]),
                corner(triangle[1]),
                corner(triangle[2]),
            ],
            context.bounds,
        );
    }
}

/// A convex polygon filled inside `bounds`, as triangles.
fn fill_polygon(stream: &mut Stream, color: [u8; 3], polygon: &[[f64; 2]], bounds: [f64; 4]) {
    if polygon.iter().flatten().any(|v| !v.is_finite()) {
        return;
    }
    let points = rounded_ring(&clip_polygon(polygon, bounds));
    for k in 1..points.len().saturating_sub(1) {
        let triangle = [points[0], points[k], points[k + 1]];
        let [a, b, c] = triangle.map(|p| [p[0] as i64, p[1] as i64]);
        // A triangle without area draws nothing.
        if (b[0] - a[0]) * (c[1] - a[1]) == (c[0] - a[0]) * (b[1] - a[1]) {
            continue;
        }
        stream.color(color);
        stream.triangles(&triangle);
    }
}

// ── Text ────────────────────────────────────────────────────────────────────

/// What a glyph quad of a wire draws.
enum PlacedQuad<'a> {
    /// A glyph of the atlas and the basis that places its geometry.
    Glyph(&'a GlyphExport, QuadBasis),
    /// The atlas' solid tile, by its corners: an underline, overline or
    /// strike bar.
    Bar([[f64; 2]; 4]),
}

/// The visible glyph quads of a wire against the atlas they were laid out
/// on, each with its colour. `None` when one of them cannot be placed: the
/// wire's runs then stand for all of its text.
fn placed_quads<'a>(
    wire: &WireModel,
    glyphs: &'a GlyphSnapshot,
) -> Option<Vec<(PlacedQuad<'a>, [f32; 4])>> {
    if !wire.text_verts.len().is_multiple_of(6) {
        return None;
    }
    let finite = |point: &[f32; 2]| point.iter().all(|v| v.is_finite());
    let mut placed = Vec::with_capacity(wire.text_verts.len() / 6);
    for quad in glyph_to_world::wire_quads(wire) {
        let color = quad[0].color;
        if !color[3].is_finite() {
            return None;
        }
        if color[3] < 0.01 {
            continue;
        }
        // Lower left, lower right, upper right, upper left.
        let corners = [0, 1, 2, 5].map(|index| {
            let vertex = &quad[index];
            [
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
            ]
        });
        if corners.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        let key = glyph_to_world::quad_uv_key(quad);
        if let Some(glyph) = glyphs.glyph(key) {
            let basis = QuadBasis::of(quad, glyph)?;
            let drawable = if glyph.fill_tris.is_empty() {
                glyph.strokes.iter().any(|stroke| !stroke.is_empty())
                    && glyph.strokes.iter().flatten().all(finite)
            } else {
                glyph.fill_tris.len().is_multiple_of(3) && glyph.fill_tris.iter().all(finite)
            };
            if !drawable || !finite(&glyph.plane_min) || !finite(&glyph.plane_max) {
                return None;
            }
            placed.push((PlacedQuad::Glyph(glyph, basis), color));
        } else if key == glyphs.solid_key {
            placed.push((PlacedQuad::Bar(corners), color));
        } else {
            return None;
        }
    }
    Some(placed)
}

/// A wire's text. A font of pen strokes has no outline a reader could load
/// by its name, so its glyphs are drawn: strokes with the pen the screen
/// gives them. Text in an outline font stays text, in its font, unless the
/// page asks for `text_as_outlines`; its underline, overline and strike bars
/// are drawn either way.
fn emit_text(stream: &mut Stream, wire: &WireModel, context: &Context) {
    let mut style_color: Option<[f32; 3]> = None;
    let mut style_mm: Option<f64> = None;
    let mut screening = 1.0;
    if let Some(table) = context.plot_style {
        if wire.aci > 0 {
            style_color = table.resolve_color(wire.aci);
            screening = table.resolve_screening(wire.aci);
            if context.options.object_lineweights {
                style_mm = table.resolve_lineweight(wire.aci).map(|mm| mm as f64);
            }
        }
    }
    let plotted = |[r, g, b, a]: [f32; 4]| {
        let rgb = style_color.unwrap_or_else(|| adapt_to_paper([r, g, b]));
        rgb_bytes(plotted_color(rgb, a, screening, context.options))
    };

    let quads = context
        .glyphs
        .and_then(|glyphs| placed_quads(wire, glyphs))
        .unwrap_or_default();
    // One pen glyph and the wire's text is drawn whole, so that none of it is
    // written twice.
    let drawn = quads.iter().any(|(quad, _)| match quad {
        PlacedQuad::Glyph(glyph, _) => {
            context.options.text_as_outlines || glyph.fill_tris.is_empty()
        }
        PlacedQuad::Bar(_) => false,
    });
    let scale = context.map.scale;
    for (quad, color) in &quads {
        let color = plotted(*color);
        match quad {
            PlacedQuad::Bar(corners) => {
                let corners = corners.map(|[x, y]| context.place(x, y));
                fill_polygon(stream, color, &corners, context.bounds);
            }
            PlacedQuad::Glyph(..) if !drawn => {}
            PlacedQuad::Glyph(glyph, basis) if !glyph.fill_tris.is_empty() => {
                for triangle in glyph.fill_tris.chunks_exact(3) {
                    let corners: Vec<[f64; 2]> = triangle
                        .iter()
                        .map(|point| {
                            let [x, y] = basis.world(point);
                            context.place(x, y)
                        })
                        .collect();
                    fill_polygon(stream, color, &corners, context.bounds);
                }
            }
            PlacedQuad::Glyph(glyph, basis) => {
                // The plot style's lineweight when it sets one, else the pen
                // the glyph was drawn with; a bold glyph's is 1.7 times as wide.
                let width_mm = match style_mm {
                    Some(mm) => {
                        let mm = if glyph.bold { mm * 1.7 } else { mm };
                        if context.options.scale_lineweights {
                            mm * scale
                        } else {
                            mm
                        }
                    }
                    None => {
                        glyph_to_world::nominal_pen_world_mm(
                            glyph.bold,
                            basis.glyph_unit_mm() as f32,
                        ) * scale
                    }
                }
                .max(0.1 * PT_TO_MM);
                let pen = context.pen(width_mm);
                for stroke in &glyph.strokes {
                    let points: Vec<[f64; 2]> = stroke
                        .iter()
                        .map(|point| {
                            let [x, y] = basis.world(point);
                            context.place(x, y)
                        })
                        .collect();
                    let pieces = match points[..] {
                        // A stroke of one point is a dot: a mark one pen wide.
                        [[x, y]] => {
                            let half = pen as f64 / 2.0;
                            clip_polyline(&[[x - half, y], [x + half, y]], context.bounds)
                        }
                        _ => clip_polyline(&points, context.bounds),
                    };
                    for piece in pieces {
                        let piece = rounded(&piece);
                        if piece.len() < 2 {
                            continue;
                        }
                        stream.color(color);
                        stream.weight(pen);
                        stream.polyline(&piece);
                    }
                }
            }
        }
    }
    if drawn {
        return;
    }

    let page_turn = (context.map.rotation_deg as f64).to_radians();
    for run in &wire.searchable_text {
        if run.text.is_empty() || run.height <= 0.0 || !run.height.is_finite() {
            continue;
        }
        let origin = context.place(run.origin[0], run.origin[1]);
        let b = context.bounds;
        if !(origin[0].is_finite() && origin[1].is_finite())
            || origin[0] < b[0]
            || origin[0] > b[2]
            || origin[1] < b[1]
            || origin[1] > b[3]
        {
            continue;
        }
        let color = plotted(run.color);
        // The run's height is its capitals'; the stream's font height is the em.
        let em = (run.height as f64 * context.map.scale * UNITS_PER_MM / CAP_PER_EM)
            .round()
            .clamp(1.0, i32::MAX as f64) as i32;
        let turn = (run.rotation as f64 + page_turn).rem_euclid(std::f64::consts::TAU);
        let rotation = (turn / std::f64::consts::TAU * 65536.0).round() as u32 % 65536;
        stream.color(color);
        stream.font(&font_family(&run.font), em, rotation as u16);
        stream.text(
            [origin[0].round() as i32, origin[1].round() as i32],
            &run.text,
        );
    }
}

/// The font a run is named by in the stream: its family, without the code
/// page and big-font parts a resolved stroke font carries.
fn font_family(name: &str) -> String {
    let family = name
        .split([
            crate::scene::text::font_face::BIG_FONT_SEP,
            crate::scene::text::font_face::CODE_PAGE_SEP,
        ])
        .next()
        .unwrap_or(name)
        .trim();
    let family: String = family
        .chars()
        .filter(|c| !matches!(c, '\'' | '(' | ')' | '\\') && !c.is_control())
        .collect();
    if family.is_empty() {
        "Arial".to_string()
    } else {
        family
    }
}

// ── Cutting to the sheet ────────────────────────────────────────────────────

/// The part of a segment inside `bounds` (min x, min y, max x, max y).
fn clip_segment(a: [f64; 2], b: [f64; 2], bounds: [f64; 4]) -> Option<([f64; 2], [f64; 2])> {
    if [a[0], a[1], b[0], b[1]].iter().any(|v| !v.is_finite()) {
        return None;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a[0] - bounds[0]),
        (dx, bounds[2] - a[0]),
        (-dy, a[1] - bounds[1]),
        (dy, bounds[3] - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > t1 {
                return None;
            }
            t0 = t0.max(t);
        } else {
            if t < t0 {
                return None;
            }
            t1 = t1.min(t);
        }
    }
    // An end that is not cut stays the very point it was, so the pieces of
    // a polyline still meet.
    let at = |t: f64| {
        if t <= 0.0 {
            a
        } else if t >= 1.0 {
            b
        } else {
            [a[0] + dx * t, a[1] + dy * t]
        }
    };
    Some((at(t0), at(t1)))
}

/// A polyline as the pieces of it inside `bounds`.
fn clip_polyline(points: &[[f64; 2]], bounds: [f64; 4]) -> Vec<Vec<[f64; 2]>> {
    let mut pieces: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut piece: Vec<[f64; 2]> = Vec::new();
    for pair in points.windows(2) {
        match clip_segment(pair[0], pair[1], bounds) {
            Some((a, b)) => {
                if piece.last() != Some(&a) {
                    if piece.len() > 1 {
                        pieces.push(std::mem::take(&mut piece));
                    }
                    piece.clear();
                    piece.push(a);
                }
                piece.push(b);
            }
            None => {
                if piece.len() > 1 {
                    pieces.push(std::mem::take(&mut piece));
                }
                piece.clear();
            }
        }
    }
    if piece.len() > 1 {
        pieces.push(piece);
    }
    pieces
}

/// A convex polygon cut to `bounds`.
fn clip_polygon(polygon: &[[f64; 2]], bounds: [f64; 4]) -> Vec<[f64; 2]> {
    let mut ring = polygon.to_vec();
    // Each side of the rectangle in turn: the axis, the limit, and whether
    // inside is above it.
    for (axis, limit, above) in [
        (0, bounds[0], true),
        (0, bounds[2], false),
        (1, bounds[1], true),
        (1, bounds[3], false),
    ] {
        if ring.is_empty() {
            break;
        }
        let inside = |p: &[f64; 2]| {
            if above {
                p[axis] >= limit
            } else {
                p[axis] <= limit
            }
        };
        let mut cut = Vec::with_capacity(ring.len() + 4);
        for index in 0..ring.len() {
            let (from, to) = (ring[index], ring[(index + 1) % ring.len()]);
            if inside(&from) {
                cut.push(from);
            }
            if inside(&from) != inside(&to) {
                let t = (limit - from[axis]) / (to[axis] - from[axis]);
                let mut crossing = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ];
                crossing[axis] = limit;
                cut.push(crossing);
            }
        }
        ring = cut;
    }
    ring
}

/// Points on the stream's integer grid, without repeats in a row.
fn rounded(points: &[[f64; 2]]) -> Vec<[i32; 2]> {
    let mut out: Vec<[i32; 2]> = Vec::with_capacity(points.len());
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() {
            continue;
        }
        let q = [p[0].round() as i32, p[1].round() as i32];
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    out
}

/// [`rounded`] for a closed ring: its last point is not its first again.
fn rounded_ring(points: &[[f64; 2]]) -> Vec<[i32; 2]> {
    let mut out = rounded(points);
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

// ── The W2D stream ──────────────────────────────────────────────────────────

/// A W2D stream being written. Coordinates are relative: each point is
/// given from the one before it, across opcodes, so the writer keeps where
/// the last one ended. Colour, pen and font are written when they change.
struct Stream {
    out: Vec<u8>,
    at: [i32; 2],
    color: Option<[u8; 3]>,
    weight: Option<i32>,
    font_name: Option<String>,
    font: Option<(i32, u16)>,
}

impl Stream {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            at: [0, 0],
            color: None,
            weight: None,
            font_name: None,
            font: None,
        }
    }

    /// An extended ASCII opcode: "(Name operands)".
    fn extended(&mut self, body: &str) {
        self.out.push(b'(');
        self.out.extend_from_slice(body.as_bytes());
        self.out.push(b')');
    }

    fn color(&mut self, rgb: [u8; 3]) {
        if self.color != Some(rgb) {
            self.out.push(0x03);
            self.out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            self.color = Some(rgb);
        }
    }

    fn weight(&mut self, weight: i32) {
        if self.weight != Some(weight) {
            self.out.push(0x17);
            self.out.extend_from_slice(&weight.to_le_bytes());
            self.weight = Some(weight);
        }
    }

    /// A count of points or characters: a byte, or zero and sixteen bits
    /// holding what is past 256.
    fn count(&mut self, count: usize) {
        if count <= 255 {
            self.out.push(count as u8);
        } else {
            self.out.push(0);
            self.out
                .extend_from_slice(&((count - 256) as u16).to_le_bytes());
        }
    }

    /// `points` under one opcode, each relative to the point before it:
    /// `short` with 16-bit steps when every step fits, `wide` with 32-bit
    /// steps otherwise.
    fn run(&mut self, short: u8, wide: u8, points: &[[i32; 2]]) {
        let mut from = self.at;
        let steps: Vec<[i32; 2]> = points
            .iter()
            .map(|p| {
                let step = [p[0].wrapping_sub(from[0]), p[1].wrapping_sub(from[1])];
                from = *p;
                step
            })
            .collect();
        let fits = steps.iter().flatten().all(|v| i16::try_from(*v).is_ok());
        self.out.push(if fits { short } else { wide });
        self.count(points.len());
        for [dx, dy] in steps {
            if fits {
                self.out.extend_from_slice(&(dx as i16).to_le_bytes());
                self.out.extend_from_slice(&(dy as i16).to_le_bytes());
            } else {
                self.out.extend_from_slice(&dx.to_le_bytes());
                self.out.extend_from_slice(&dy.to_le_bytes());
            }
        }
        self.at = from;
    }

    /// A polyline through `points`, split where it outgrows one count; each
    /// piece starts on the point the one before ended on.
    fn polyline(&mut self, points: &[[i32; 2]]) {
        let mut start = 0;
        while start + 1 < points.len() {
            let end = (start + MAX_COUNT).min(points.len());
            self.run(0x10, b'p', &points[start..end]);
            start = end - 1;
        }
    }

    /// A strip of filled triangles: each point closes one with the two
    /// before it.
    fn triangles(&mut self, points: &[[i32; 2]]) {
        if (3..=MAX_COUNT).contains(&points.len()) {
            self.run(0x14, b't', points);
        }
    }

    /// The font of the text that follows: its family, em height in graphics
    /// units and rotation in 65536ths of a turn.
    fn font(&mut self, family: &str, em: i32, rotation: u16) {
        if self.font_name.as_deref() != Some(family) {
            self.extended(&format!("FontExtension '{family}' '{family}'"));
            self.font_name = Some(family.to_string());
            self.font = None;
        }
        if self.font != Some((em, rotation)) {
            // The fields the mask names, in its bit order: character set,
            // family class (swiss), height and rotation.
            self.out.push(0x06);
            self.out.extend_from_slice(&0x006au16.to_le_bytes());
            self.out.push(0);
            self.out.push(32);
            self.out.extend_from_slice(&em.to_le_bytes());
            self.out.extend_from_slice(&rotation.to_le_bytes());
            self.font = Some((em, rotation));
        }
    }

    /// Text with its baseline starting at `origin`.
    fn text(&mut self, origin: [i32; 2], text: &str) {
        self.out.push(b'x');
        self.out
            .extend_from_slice(&origin[0].wrapping_sub(self.at[0]).to_le_bytes());
        self.out
            .extend_from_slice(&origin[1].wrapping_sub(self.at[1]).to_le_bytes());
        self.at = origin;
        let plain = text
            .bytes()
            .all(|b| (0x20..0x7f).contains(&b) && b != b'\'' && b != b'\\');
        if plain {
            self.out.push(b'\'');
            self.out.extend_from_slice(text.as_bytes());
            self.out.push(b'\'');
        } else {
            // What is not plain goes between braces: a 32-bit count of
            // characters and their UTF-16 units.
            let units: Vec<u16> = text.encode_utf16().collect();
            self.out.push(b'{');
            self.out
                .extend_from_slice(&(units.len() as i32).to_le_bytes());
            for unit in units {
                self.out.extend_from_slice(&unit.to_le_bytes());
            }
            self.out.push(b'}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::plot::{PlotContent, PlotOptions, PlotWire};
    use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
    use crate::scene::model::underlay_vector::dwf as reader;
    use crate::scene::model::underlay_vector::{Segment, Sheet};
    use crate::scene::model::wire_model::SearchableTextRun;
    use crate::scene::WireModel;
    use std::io::Read;

    /// One graphics unit on paper, in millimetres: what a coordinate may be
    /// off by after rounding to the stream's integers.
    const UNIT_MM: f64 = 25.4 / 1200.0;

    fn page(content: PlotContent, w: f64, h: f64) -> PlotPage {
        PlotPage {
            content,
            paper_w: w,
            paper_h: h,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            content_rotation: 0.0,
            scale: 1.0,
            clip: None,
            options: PlotOptions::default(),
            plot_style: None,
            geopdf: None,
        }
    }

    fn wire(points: Vec<[f32; 3]>, color: [f32; 4]) -> WireModel {
        WireModel::solid("1A".to_string(), points, color, false)
    }

    fn wires(list: Vec<WireModel>) -> PlotContent {
        PlotContent {
            wires: std::sync::Arc::new(
                list.into_iter()
                    .map(|wire| PlotWire {
                        wire,
                        draw_depth: 0.5,
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn square_hatch(pattern: HatchPattern, name: &str, color: [f32; 4]) -> HatchModel {
        HatchModel {
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
            name: name.to_string(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: 0.3,
        }
    }

    fn run(text: &str, origin: [f64; 2], height: f32, rotation: f32, font: &str) -> WireModel {
        let mut wire = WireModel::default();
        wire.searchable_text = vec![SearchableTextRun {
            text: text.to_string(),
            origin: [origin[0], origin[1], 0.0],
            height,
            rotation,
            color: [0.0, 0.0, 1.0, 1.0],
            bold: false,
            font: font.to_string(),
            adv_width: 10.0,
        }];
        wire
    }

    /// The page as the sheet "Model" of a package, read back by the underlay
    /// reader.
    fn read(page: &PlotPage) -> Sheet {
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page,
            }],
            None,
        )
        .expect("builds");
        reader::sheet(&bytes, "Model").expect("the reader opens the sheet")
    }

    /// [`read`] with the glyphs of the atlas the page's text was laid out on.
    fn read_with(page: &PlotPage, glyphs: &GlyphSnapshot) -> Sheet {
        let bytes = build_dwf_with_glyphs(
            &[DwfSheet {
                name: "Model",
                page,
            }],
            None,
            Some(glyphs),
        )
        .expect("builds");
        reader::sheet(&bytes, "Model").expect("the reader opens the sheet")
    }

    /// `text` 10 high at (20, 20) in a font of pen strokes: its glyph quads,
    /// the run the search layer carries for it and the atlas it was laid out
    /// on. An atlas of its own, so no other test moves its tiles.
    fn pen_text(text: &str) -> (WireModel, GlyphSnapshot) {
        laid_out(text, 0.0, 1.0, 0.0)
    }

    /// [`pen_text`] turned by `rotation`, with a width factor and an oblique
    /// angle; its run is turned with it.
    fn laid_out(
        text: &str,
        rotation: f32,
        width_factor: f32,
        oblique: f32,
    ) -> (WireModel, GlyphSnapshot) {
        let mut atlas = crate::scene::text::sdf_atlas::GlyphAtlas::new(256, 256);
        let (quads, _) = crate::scene::text::glyph_quads::layout_glyph_quads(
            &mut atlas,
            10.0,
            rotation,
            width_factor,
            oblique,
            1.0,
            "txt",
            false,
            text,
        );
        let mut wire = run(text, [20.0, 20.0], 10.0, rotation, "txt");
        crate::scene::pipeline::text_gpu::push_glyph_vertices(
            &mut wire.text_verts,
            &quads,
            [20.0, 20.0, 0.0],
            1.0,
            [0.0, 0.0, 1.0, 1.0],
            0.0,
        );
        (wire, GlyphSnapshot::from_atlas(&atlas))
    }

    /// Makes a glyph of the atlas a filled one: the lower right half of its
    /// tile.
    fn fill_half(glyph: &mut crate::scene::text::sdf_atlas::GlyphExport) {
        let (low, high) = (glyph.plane_min, glyph.plane_max);
        glyph.fill_tris = vec![low, [high[0], low[1]], high];
    }

    /// The corners of a glyph quad: lower left, lower right, upper right,
    /// upper left.
    fn quad_corners(quad: &[crate::scene::pipeline::text_gpu::TextVertex]) -> [[f64; 2]; 4] {
        [0, 1, 2, 5].map(|index| {
            let vertex = &quad[index];
            [
                vertex.pos[0] as f64 + vertex.pos_low[0] as f64,
                vertex.pos[1] as f64 + vertex.pos_low[1] as f64,
            ]
        })
    }

    /// The area of a glyph quad.
    fn quad_area(quad: &[crate::scene::pipeline::text_gpu::TextVertex]) -> f64 {
        let [a, b, _, d] = quad_corners(quad);
        ((b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0])).abs()
    }

    /// The straight pieces of the sheet's stroked paths, in order.
    fn strokes(sheet: &Sheet) -> Vec<[[f64; 2]; 2]> {
        sheet
            .paths
            .iter()
            .filter(|path| path.stroke.is_some())
            .flat_map(|path| &path.subpaths)
            .flat_map(|subpath| &subpath.segments)
            .map(|segment| match segment {
                Segment::Line(a, b) => [*a, *b],
                Segment::Cubic(a, _, _, d) => [*a, *d],
            })
            .collect()
    }

    /// Twice the area the sheet's filled paths cover, each closed subpath a
    /// polygon.
    fn filled_area(sheet: &Sheet) -> f64 {
        sheet
            .paths
            .iter()
            .filter(|path| path.fill.is_some())
            .flat_map(|path| &path.subpaths)
            .map(|subpath| {
                subpath
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        Segment::Line(a, b) | Segment::Cubic(a, _, _, b) => {
                            a[0] * b[1] - b[0] * a[1]
                        }
                    })
                    .sum::<f64>()
                    .abs()
                    / 2.0
            })
            .sum()
    }

    fn near(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
        (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
    }

    fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        (0..archive.len())
            .map(|index| {
                let mut file = archive.by_index(index).expect("an entry");
                let mut data = Vec::new();
                file.read_to_end(&mut data).expect("inflates");
                (file.name().to_string(), data)
            })
            .collect()
    }

    #[test]
    fn an_empty_page_is_a_dwf_6_package_with_one_sheet() {
        let empty = page(PlotContent::default(), 297.0, 210.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        assert!(bytes.starts_with(b"(DWF V06.00)"), "the package tag");
        let files = unzip(&bytes);
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names[0], "manifest.xml", "{names:?}");
        assert_eq!(
            names
                .iter()
                .filter(|name| name.ends_with("descriptor.xml"))
                .count(),
            1,
            "{names:?}"
        );
        let stream = files
            .iter()
            .find(|(name, _)| name.ends_with(".w2d"))
            .map(|(_, data)| data)
            .expect("a graphics stream");
        assert!(stream.starts_with(b"(W2D V06.00)"));
        assert!(stream.ends_with(b"(EndOfDWF)"));
        assert_eq!(reader::sheet_names(&bytes), Some(vec!["Model".to_string()]));
    }

    #[test]
    fn the_zip_counts_its_offsets_from_the_start_of_the_file() {
        let empty = page(PlotContent::default(), 297.0, 210.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        // The end record names where the directory starts, and the
        // directory where its first file does: right after the tag.
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("the zip's end record");
        let offset = |at: usize| {
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize
        };
        let directory = offset(end + 16);
        assert_eq!(&bytes[directory..directory + 4], b"PK\x01\x02");
        assert_eq!(offset(directory + 42), 12);
        assert_eq!(&bytes[12..16], b"PK\x03\x04");
    }

    #[test]
    fn the_sheet_reads_back_at_its_paper_size() {
        let sheet = read(&page(PlotContent::default(), 297.0, 210.0));

        let [x0, y0, x1, y1] = sheet.rect;
        assert!(near([x0, y0], [0.0, 0.0], UNIT_MM), "{:?}", sheet.rect);
        assert!(near([x1, y1], [297.0, 210.0], UNIT_MM), "{:?}", sheet.rect);
    }

    #[test]
    fn the_descriptor_names_the_paper_and_the_scale_to_it() {
        let empty = page(PlotContent::default(), 420.0, 297.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "A3 <plan>",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        let files = unzip(&bytes);
        let descriptor = files
            .iter()
            .find(|(name, _)| name.ends_with("descriptor.xml"))
            .map(|(_, data)| String::from_utf8(data.clone()).expect("utf-8"))
            .expect("a descriptor");
        let xml = roxmltree::Document::parse(&descriptor).expect("well-formed descriptor");
        let page_node = xml.root_element();
        assert_eq!(page_node.attribute("name"), Some("A3 <plan>"));
        let paper = xml
            .descendants()
            .find(|node| node.tag_name().name() == "Paper")
            .expect("a Paper element");
        assert_eq!(paper.attribute("units"), Some("mm"));
        assert_eq!(paper.attribute("width"), Some("420"));
        assert_eq!(paper.attribute("height"), Some("297"));
        let graphics = xml
            .descendants()
            .find(|node| node.tag_name().name() == "GraphicResource")
            .expect("a graphics resource");
        let transform: Vec<f64> = graphics
            .attribute("transform")
            .expect("a transform")
            .split_whitespace()
            .map(|value| value.parse().expect("a number"))
            .collect();
        assert_eq!(transform.len(), 16);
        assert!((transform[0] - UNIT_MM).abs() < 1e-12, "{transform:?}");
        assert!((transform[5] - UNIT_MM).abs() < 1e-12, "{transform:?}");
        let manifest = files
            .iter()
            .find(|(name, _)| name == "manifest.xml")
            .map(|(_, data)| String::from_utf8(data.clone()).expect("utf-8"))
            .expect("a manifest");
        roxmltree::Document::parse(&manifest).expect("well-formed manifest");
    }

    #[test]
    fn a_wire_reads_back_where_it_was_drawn_in_its_colour() {
        let content = wires(vec![wire(
            vec![[10.0, 20.0, 0.0], [110.0, 20.0, 0.0], [110.0, 70.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(near(lines[0][0], [10.0, 20.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[0][1], [110.0, 20.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[1][1], [110.0, 70.0], UNIT_MM), "{lines:?}");
        assert_eq!(
            sheet.paths[0].stroke.map(|stroke| stroke.0),
            Some([255, 0, 0])
        );
    }

    #[test]
    fn a_plot_scale_reads_back_in_drawing_units() {
        // Half a millimetre of paper for each drawing unit.
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [100.0, 40.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);
        let mut scaled = page(content, 297.0, 210.0);
        scaled.scale = 0.5;

        let sheet = read(&scaled);

        let lines = strokes(&sheet);
        assert!(near(lines[0][1], [100.0, 40.0], 2.0 * UNIT_MM), "{lines:?}");
        assert!(
            near(
                [sheet.rect[2], sheet.rect[3]],
                [594.0, 420.0],
                2.0 * UNIT_MM
            ),
            "{:?}",
            sheet.rect
        );
    }

    #[test]
    fn a_pen_up_starts_a_new_stroke() {
        let content = wires(vec![wire(
            vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [f32::NAN, f32::NAN, f32::NAN],
                [50.0, 5.0, 0.0],
                [60.0, 5.0, 0.0],
            ],
            [0.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), 2, "no stroke joins the two runs: {lines:?}");
        assert!(near(lines[1][0], [50.0, 5.0], UNIT_MM), "{lines:?}");
    }

    #[test]
    fn a_lineweight_reads_back_in_millimetres() {
        let mut thick = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        );
        // 96 px to the inch: 0.5 mm.
        thick.line_weight_px = 0.5 * 96.0 / 25.4;

        let sheet = read(&page(wires(vec![thick]), 297.0, 210.0));

        let width = sheet.paths[0].stroke.expect("a stroke").1;
        assert!((width - 0.5).abs() <= UNIT_MM, "{width}");
    }

    #[test]
    fn a_white_wire_plots_black_as_on_paper() {
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [1.0, 1.0, 1.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        assert_eq!(
            sheet.paths[0].stroke.map(|stroke| stroke.0),
            Some([0, 0, 0])
        );
    }

    #[test]
    fn a_wire_drawn_on_top_comes_later_in_the_stream() {
        let red = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        );
        let blue = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [0.0, 0.0, 1.0, 1.0],
        );
        let content = PlotContent {
            wires: std::sync::Arc::new(vec![
                PlotWire {
                    wire: red,
                    draw_depth: 0.8,
                },
                PlotWire {
                    wire: blue,
                    draw_depth: 0.2,
                },
            ]),
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        let colors: Vec<[u8; 3]> = sheet
            .paths
            .iter()
            .filter_map(|path| path.stroke.map(|stroke| stroke.0))
            .collect();
        assert_eq!(colors, vec![[0, 0, 255], [255, 0, 0]]);
    }

    #[test]
    fn a_filled_triangle_reads_back_filled() {
        let mut fill = WireModel::default();
        fill.color = [0.0, 0.0, 1.0, 1.0];
        fill.fill_tris = vec![[5.0, 5.0, 0.0], [25.0, 5.0, 0.0], [5.0, 15.0, 0.0]];

        let sheet = read(&page(wires(vec![fill]), 297.0, 210.0));

        let filled: Vec<_> = sheet
            .paths
            .iter()
            .filter(|path| path.fill.is_some())
            .collect();
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].fill, Some([0, 0, 255]));
        assert!(
            (filled_area(&sheet) - 100.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
    }

    #[test]
    fn a_solid_hatch_reads_back_as_its_area() {
        let content = PlotContent {
            hatches: vec![square_hatch(
                HatchPattern::Solid,
                "SOLID",
                [1.0, 1.0, 0.0, 1.0],
            )],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        assert!(
            (filled_area(&sheet) - 400.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
        assert!(sheet
            .paths
            .iter()
            .all(|path| path.fill == Some([255, 255, 0])));
    }

    #[test]
    fn a_wipeout_reads_back_white() {
        let content = PlotContent {
            wipeouts: vec![square_hatch(
                HatchPattern::Solid,
                "WIPEOUT_FILL",
                [0.2, 0.2, 0.2, 1.0],
            )],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        assert!(
            (filled_area(&sheet) - 400.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
        assert!(sheet
            .paths
            .iter()
            .all(|path| path.fill == Some([255, 255, 255])));
    }

    #[test]
    fn a_pattern_hatch_reads_back_as_its_lines() {
        // Horizontal lines 5 apart across a square from 20 to 40.
        let pattern = HatchPattern::Pattern(vec![PatFamily {
            angle_deg: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: 5.0,
            dashes: Vec::new(),
        }]);
        let content = PlotContent {
            hatches: vec![square_hatch(pattern, "LINES", [0.0, 1.0, 0.0, 1.0])],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert!(lines.len() >= 3, "{lines:?}");
        for [a, b] in &lines {
            assert!((a[1] - b[1]).abs() <= UNIT_MM, "a level line: {a:?} {b:?}");
            assert!((a[0] - b[0]).abs() > 19.0, "across the square: {a:?} {b:?}");
        }
        assert_eq!(filled_area(&sheet), 0.0, "no fill for a pattern");
    }

    #[test]
    fn text_reads_back_with_its_font_height_and_rotation() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let content = wires(vec![run("ROOM 12", [20.0, 30.0], 5.0, quarter, "Arial")]);

        let sheet = read(&page(content, 297.0, 210.0));

        assert_eq!(sheet.texts.len(), 1);
        let text = &sheet.texts[0];
        assert_eq!(text.text, "ROOM 12");
        assert!(
            near(text.origin, [20.0, 30.0], UNIT_MM),
            "{:?}",
            text.origin
        );
        assert!((text.height - 5.0).abs() < 0.02, "{}", text.height);
        assert!(
            (text.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-3,
            "{}",
            text.rotation
        );
        assert_eq!(text.font, "Arial");
        assert_eq!(text.color, [0, 0, 255]);
    }

    #[test]
    fn text_beyond_ascii_reads_back() {
        let content = wires(vec![
            run("Ñandú ø 25 — «sala»", [20.0, 30.0], 5.0, 0.0, "Arial"),
            run("it's 3\\4", [20.0, 50.0], 5.0, 0.0, "Arial"),
        ]);

        let sheet = read(&page(content, 297.0, 210.0));

        let texts: Vec<&str> = sheet.texts.iter().map(|text| text.text.as_str()).collect();
        assert_eq!(texts, vec!["Ñandú ø 25 — «sala»", "it's 3\\4"]);
    }

    #[test]
    fn text_beyond_ascii_goes_between_braces() {
        let content = wires(vec![run("Ñandú", [20.0, 30.0], 5.0, 0.0, "Arial")]);
        let written = page(content, 297.0, 210.0);

        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page: &written,
            }],
            None,
        )
        .expect("builds");

        let files = unzip(&bytes);
        let stream = files
            .iter()
            .find(|(name, _)| name.ends_with(".w2d"))
            .map(|(_, data)| data)
            .expect("a graphics stream");
        // A brace, the count of characters in 32 bits, the UTF-16 units, a brace.
        let mut string = vec![b'{'];
        string.extend_from_slice(&5i32.to_le_bytes());
        for unit in "Ñandú".encode_utf16() {
            string.extend_from_slice(&unit.to_le_bytes());
        }
        string.push(b'}');
        assert!(
            stream
                .windows(string.len())
                .any(|window| window == string.as_slice()),
            "the string in the stream's binary form"
        );
    }

    #[test]
    fn text_in_a_font_of_pen_strokes_goes_out_as_its_strokes() {
        use crate::scene::text::glyph_to_world::{self, QuadBasis};

        let (text, glyphs) = pen_text("A");
        // Every piece of the glyph's strokes, where its quad places it.
        let quad = glyph_to_world::wire_quads(&text).next().expect("a glyph");
        let glyph = glyphs
            .glyph(glyph_to_world::quad_uv_key(quad))
            .expect("the glyph of the quad");
        assert!(glyph.fill_tris.is_empty(), "a glyph of pen strokes");
        let basis = QuadBasis::of(quad, glyph).expect("a quad with a size");
        let expected: Vec<[[f64; 2]; 2]> = glyph
            .strokes
            .iter()
            .flat_map(|stroke| stroke.windows(2))
            .map(|piece| [basis.world(&piece[0]), basis.world(&piece[1])])
            .collect();
        assert!(expected.len() >= 3, "{expected:?}");
        let pen_mm = glyph_to_world::nominal_pen_world_mm(false, basis.glyph_unit_mm() as f32);

        let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

        assert!(sheet.texts.is_empty(), "{:?}", sheet.texts);
        let drawn = strokes(&sheet);
        assert_eq!(drawn.len(), expected.len(), "{drawn:?}");
        for (got, want) in drawn.iter().zip(&expected) {
            assert!(
                near(got[0], want[0], UNIT_MM) && near(got[1], want[1], UNIT_MM),
                "{got:?} for {want:?}"
            );
        }
        for path in &sheet.paths {
            let (color, width) = path.stroke.expect("a stroke");
            assert_eq!(color, [0, 0, 255]);
            assert!(
                (width - pen_mm).abs() <= UNIT_MM,
                "{width} for a pen of {pen_mm}"
            );
            assert!(path.fill.is_none());
        }
    }

    #[test]
    fn text_whose_glyphs_the_atlas_does_not_know_stays_text() {
        let (mut text, glyphs) = pen_text("A");
        for vertex in &mut text.text_verts {
            vertex.uv = [-1.0, -1.0];
        }

        let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

        let texts: Vec<&str> = sheet.texts.iter().map(|text| text.text.as_str()).collect();
        assert_eq!(texts, vec!["A"]);
        assert!(sheet.paths.is_empty(), "{:?}", sheet.paths);
    }

    #[test]
    fn a_filled_glyph_stays_text_until_outlines_are_asked_for() {
        let (text, mut glyphs) = pen_text("A");
        glyphs.table.values_mut().for_each(fill_half);
        let half_quad = quad_area(&text.text_verts[..6]) / 2.0;
        let mut written = page(wires(vec![text]), 297.0, 210.0);

        let as_text = read_with(&written, &glyphs);
        written.options.text_as_outlines = true;
        let as_outlines = read_with(&written, &glyphs);

        let texts: Vec<&str> = as_text
            .texts
            .iter()
            .map(|text| text.text.as_str())
            .collect();
        assert_eq!(texts, vec!["A"]);
        assert!(as_text.paths.is_empty(), "{:?}", as_text.paths);

        assert!(as_outlines.texts.is_empty(), "{:?}", as_outlines.texts);
        assert!(
            strokes(&as_outlines).is_empty(),
            "a filled glyph has no pen"
        );
        let area = filled_area(&as_outlines);
        assert!(
            (area - half_quad).abs() < half_quad * 0.01,
            "{area} of {half_quad}"
        );
        assert!(as_outlines
            .paths
            .iter()
            .all(|path| path.fill == Some([0, 0, 255])));
    }

    #[test]
    fn a_wire_of_pen_and_filled_glyphs_goes_out_drawn() {
        let (text, mut glyphs) = pen_text("AB");
        glyphs
            .table
            .values_mut()
            .filter(|glyph| glyph.ch == 'B')
            .for_each(fill_half);
        let half_quad = quad_area(&text.text_verts[6..12]) / 2.0;

        let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

        assert!(sheet.texts.is_empty(), "{:?}", sheet.texts);
        assert!(!strokes(&sheet).is_empty(), "the strokes of the A");
        let area = filled_area(&sheet);
        assert!(
            (area - half_quad).abs() < half_quad * 0.01,
            "{area} of {half_quad}"
        );
    }

    #[test]
    fn an_underline_goes_out_as_a_filled_bar() {
        use crate::scene::text::glyph_to_world;

        // Under text that is drawn, and under text that stays text.
        for filled in [false, true] {
            let (text, mut glyphs) = pen_text(r"\LG\l");
            if filled {
                glyphs.table.values_mut().for_each(fill_half);
            }
            let bar = glyph_to_world::wire_quads(&text)
                .find(|quad| glyph_to_world::quad_uv_key(quad) == glyphs.solid_key)
                .expect("the bar of the underline");
            let bar_area = quad_area(bar);
            let [low, _, high, _] = quad_corners(bar);
            // Each edge may move half a graphics unit.
            let slack = ((high[0] - low[0]) + (high[1] - low[1])) * UNIT_MM;

            let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

            assert_eq!(sheet.texts.len(), usize::from(filled));
            let area = filled_area(&sheet);
            assert!(
                (area - bar_area).abs() <= slack,
                "{area} of {bar_area}, filled glyphs: {filled}"
            );
            let bar_path = sheet
                .paths
                .iter()
                .find(|path| path.fill.is_some())
                .expect("a filled path");
            for segment in &bar_path.subpaths[0].segments {
                let (Segment::Line(a, _) | Segment::Cubic(a, _, _, _)) = segment;
                assert!(
                    a[0] >= low[0] - UNIT_MM
                        && a[0] <= high[0] + UNIT_MM
                        && a[1] >= low[1] - UNIT_MM
                        && a[1] <= high[1] + UNIT_MM,
                    "{a:?} outside the bar {low:?} {high:?}"
                );
            }
        }
    }

    #[test]
    fn a_pen_font_text_turns_with_the_page_and_follows_the_plot_scale() {
        use crate::scene::text::glyph_to_world::{self, QuadBasis};

        let (text, glyphs) = pen_text("A");
        let quad = glyph_to_world::wire_quads(&text).next().expect("a glyph");
        let glyph = glyphs
            .glyph(glyph_to_world::quad_uv_key(quad))
            .expect("the glyph of the quad");
        let basis = QuadBasis::of(quad, glyph).expect("a quad with a size");
        let first = basis.world(&glyph.strokes[0][0]);
        let pen_mm = glyph_to_world::nominal_pen_world_mm(false, basis.glyph_unit_mm() as f32);
        let mut written = page(wires(vec![text]), 297.0, 210.0);
        written.scale = 0.5;
        written.rotation_deg = 90;

        let sheet = read_with(&written, &glyphs);

        // The sheet reads back in drawing units: a quarter turn about the
        // origin, then the paper's width (594 at this scale) to the right.
        let turned = [594.0 - first[1], first[0]];
        let drawn = strokes(&sheet);
        assert!(
            near(drawn[0][0], turned, 2.0 * UNIT_MM),
            "{:?} for {turned:?}",
            drawn[0][0]
        );
        // The pen is the glyph's: half as wide on paper at half the scale.
        let width = sheet.paths[0].stroke.expect("a stroke").1;
        assert!(
            (width - pen_mm).abs() <= 2.0 * UNIT_MM,
            "{width} for a pen of {pen_mm}"
        );
    }

    #[test]
    fn a_filled_glyph_stays_text_when_its_run_turns_with_it() {
        let (text, mut glyphs) = laid_out("A", 0.5, 1.0, 0.0);
        glyphs.table.values_mut().for_each(fill_half);

        let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

        assert_eq!(sheet.texts.len(), 1, "{:?}", sheet.texts);
        assert!(
            (sheet.texts[0].rotation - 0.5).abs() < 1e-3,
            "{}",
            sheet.texts[0].rotation
        );
        assert!(sheet.paths.is_empty(), "{:?}", sheet.paths);
    }

    #[test]
    fn a_filled_glyph_a_run_cannot_place_goes_out_as_outlines() {
        // A run carries a font, a height and a rotation. It does not carry a
        // width factor or an oblique angle, a glyph seen from the side, a
        // mirrored one, or a baseline other than its own.
        let narrow = laid_out("A", 0.0, 0.5, 0.0);
        let slanted = laid_out("A", 0.0, 1.0, 0.35);
        let mut tilted = laid_out("A", 0.0, 1.0, 0.0);
        for vertex in &mut tilted.0.text_verts {
            let (dx, dy) = (vertex.pos[0] - 20.0, vertex.pos[1] - 20.0);
            vertex.pos[0] = 20.0 + dx + 0.5 * dy;
            vertex.pos[1] = 20.0 + 0.5 * dy;
        }
        let mut mirrored = laid_out("A", 0.0, 1.0, 0.0);
        for vertex in &mut mirrored.0.text_verts {
            vertex.pos[0] = 40.0 - vertex.pos[0];
        }
        let mut astray = laid_out("A", 0.0, 1.0, 0.0);
        astray.0.searchable_text[0].rotation = 0.5;

        for (case, (text, mut glyphs)) in [
            ("a width factor", narrow),
            ("an oblique angle", slanted),
            ("seen from the side", tilted),
            ("mirrored", mirrored),
            ("off its run's baseline", astray),
        ] {
            glyphs.table.values_mut().for_each(fill_half);

            let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

            assert!(sheet.texts.is_empty(), "{case}: {:?}", sheet.texts);
            assert!(filled_area(&sheet) > 1.0, "{case}: nothing is drawn");
        }
    }

    #[test]
    fn filled_glyphs_without_a_run_are_drawn() {
        let (mut text, mut glyphs) = laid_out("A", 0.0, 1.0, 0.0);
        text.searchable_text.clear();
        glyphs.table.values_mut().for_each(fill_half);

        let sheet = read_with(&page(wires(vec![text]), 297.0, 210.0), &glyphs);

        assert!(sheet.texts.is_empty(), "{:?}", sheet.texts);
        assert!(filled_area(&sheet) > 1.0, "nothing is drawn");
    }

    #[test]
    fn several_sheets_keep_their_names_and_order() {
        let first = page(
            wires(vec![wire(
                vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
                [0.0, 0.0, 0.0, 1.0],
            )]),
            297.0,
            210.0,
        );
        let second = page(
            wires(vec![wire(
                vec![[0.0, 0.0, 0.0], [0.0, 30.0, 0.0]],
                [0.0, 0.0, 0.0, 1.0],
            )]),
            420.0,
            297.0,
        );

        let bytes = build_dwf(
            &[
                DwfSheet {
                    name: "Plan",
                    page: &first,
                },
                DwfSheet {
                    name: "Details",
                    page: &second,
                },
            ],
            None,
        )
        .expect("builds");

        assert_eq!(
            reader::sheet_names(&bytes),
            Some(vec!["Plan".to_string(), "Details".to_string()])
        );
        let details = reader::sheet(&bytes, "Details").expect("the second sheet");
        assert!(near(strokes(&details)[0][1], [0.0, 30.0], UNIT_MM));
        assert!(near(
            [details.rect[2], details.rect[3]],
            [420.0, 297.0],
            UNIT_MM
        ));
    }

    #[test]
    fn a_long_wire_keeps_every_point() {
        // More points than one polyline of the stream can count.
        let points: Vec<[f32; 3]> = (0..70_000)
            .map(|index| [(index % 200) as f32, (index / 200) as f32 * 0.5, 0.0])
            .collect();
        let count = points.len();

        let sheet = read(&page(
            wires(vec![wire(points, [0.0, 0.0, 0.0, 1.0])]),
            297.0,
            210.0,
        ));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), count - 1);
        assert!(near(lines[count - 2][1], [199.0, 174.5], UNIT_MM));
    }

    #[test]
    fn a_large_sheet_keeps_its_far_points() {
        // Across an A0 sheet: further than a 16-bit step reaches.
        let content = wires(vec![wire(
            vec![[5.0, 5.0, 0.0], [1180.0, 835.0, 0.0], [5.0, 835.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 1189.0, 841.0));

        let lines = strokes(&sheet);
        assert!(near(lines[0][1], [1180.0, 835.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[1][1], [5.0, 835.0], UNIT_MM), "{lines:?}");
    }

    #[test]
    fn a_dashed_wire_is_cut_into_its_dashes() {
        let mut dashed = wire(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        );
        dashed.pattern_length = 10.0;
        dashed.pattern = [6.0, -4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

        let sheet = read(&page(wires(vec![dashed]), 297.0, 210.0));

        let lines = strokes(&sheet);
        assert!(lines.len() >= 9, "several dashes: {lines:?}");
        let lit: f64 = lines.iter().map(|[a, b]| (b[0] - a[0]).abs()).sum();
        assert!(lit > 50.0 && lit < 75.0, "about six tenths lit: {lit}");
    }

    #[test]
    fn a_page_turned_a_quarter_reads_back_turned() {
        // The page turns the content a quarter: +x of the content runs up
        // the sheet, +y towards its left edge.
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);
        let mut turned = page(content, 210.0, 297.0);
        turned.rotation_deg = 90;

        let sheet = read(&turned);

        let lines = strokes(&sheet);
        assert!(near(lines[0][0], [210.0, 0.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[0][1], [210.0, 100.0], UNIT_MM), "{lines:?}");
    }
}
