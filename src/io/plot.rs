// Shared plot model — the paper-space geometry every plot backend consumes.
//
// `PdfPageInput`/`PdfPlotOptions` used to live in `pdf_export`, which made the
// scene, the print pipeline and now the SVG exporter all import from the PDF
// backend. The structs themselves are backend-agnostic (wires, hatches,
// images, page size/offset/clip), so they live here; `pdf_export` re-exports
// the historical `Pdf*` names as aliases so existing call sites keep working.

use crate::io::plot_style::PlotStyleTable;
use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::image_model::ImageModel;
use crate::scene::WireModel;

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

/// Resolve export attribution per occurrence, retaining the handle map for
/// direct, synthetic, and externally constructed plot models.
pub(crate) fn wire_layer<'a>(
    wire: &'a WireModel,
    entity_layers: &'a std::collections::HashMap<document::Handle, String>,
) -> Option<&'a str> {
    wire.effective_layer.as_deref().or_else(|| {
        crate::scene::Scene::handle_from_wire_name(&wire.name)
            .and_then(|handle| entity_layers.get(&handle))
            .map(String::as_str)
    })
}

pub(crate) fn hatch_layer<'a>(
    hatch: &'a HatchModel,
    entity_layers: &'a std::collections::HashMap<document::Handle, String>,
) -> Option<&'a str> {
    hatch.effective_layer.as_deref().or_else(|| {
        hatch
            .entity
            .and_then(|handle| entity_layers.get(&handle))
            .map(String::as_str)
    })
}

/// Decoded image geometry plus inherited block/viewport clip boundaries.
#[derive(Clone, Debug)]
pub struct PlotImage {
    pub image: ImageModel,
    pub clips: Vec<Vec<[f64; 2]>>,
}

/// Image resolution preset from the plot dialog's Quality dropdown. Vector
/// output is resolution-independent; this only caps raster content. Normal
/// and High keep the native pixels; Low trades raster detail for size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlotQuality {
    Low,
    #[default]
    Normal,
    High,
}

impl PlotQuality {
    /// Accepts the dialog's raw string, including legacy names.
    pub fn from_dialog(value: &str) -> Self {
        match value {
            "Low" | "Draft" => Self::Low,
            "High" | "Presentation" | "Maximum" | "Custom" => Self::High,
            _ => Self::Normal,
        }
    }

    /// Maximum effective DPI for embedded raster images; `None` keeps native pixels.
    pub(crate) fn target_dpi(self) -> Option<u32> {
        match self {
            Self::Low => Some(150),
            Self::Normal | Self::High => None,
        }
    }
}

/// Parses a DPI field of the plot dialog. Blank (or zero / non-numeric) means
/// "not set": native raster pixels, full-precision vectors. Decimals such as
/// `600.0` or `600,5` count as typed values instead of silently reading as blank;
/// a comma before exactly three digits is a thousands separator (`1,200`).
pub fn parse_dpi(text: &str) -> Option<u32> {
    let text = text.trim();
    let text = match text.split_once(',') {
        Some((_, tail)) if tail.len() == 3 && tail.bytes().all(|b| b.is_ascii_digit()) => {
            text.replace(',', "")
        }
        _ => text.replace(',', "."),
    };
    let value: f64 = text.parse().ok()?;
    if !value.is_finite() || value < 0.5 {
        return None;
    }
    Some(value.round().min(u32::MAX as f64) as u32)
}

/// The shaded-viewport plot modes, in dialog, listing and validation order.
pub const SHADE_PLOT_MODES: &[&str] = &[
    "As displayed",
    "2D Wireframe",
    "3D Wireframe",
    "Hidden Line",
    "Flat Shaded",
    "Gouraud Shaded",
    "Flat Shaded + Edges",
    "Gouraud Shaded + Edges",
];

/// Output controls shared by preview, plot exports, and printer rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlotOptions {
    pub object_lineweights: bool,
    pub scale_lineweights: bool,
    pub transparency: bool,
    pub stamp: bool,
    pub merge_lines: bool,
    pub quality: PlotQuality,
    /// Raster image resolution cap in DPI; overrides `quality` when set.
    pub raster_dpi: Option<u32>,
    /// Vector resolution in DPI: path points snap to that grid on paper,
    /// trading precision for size. `None` keeps full precision.
    pub vector_dpi: Option<u32>,
    /// SVG: emit positioned glyph geometry instead of visible `<text>` runs.
    /// PDF has its own text pipeline and does not read this flag.
    pub text_as_outlines: bool,
    /// SVG: keep stroke widths constant under subsequent SVG transforms.
    /// CAD plot scale is unchanged and PDF ignores this flag.
    pub non_scaling_stroke: bool,
    /// SVG: tag every drawn item with its drawing layer as a `data-layer`
    /// group, so SVG consumers can toggle layers dynamically in CSS. PDF
    /// ignores this flag.
    pub layer_attributes: bool,
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
    /// Legacy handle-based layer fallback. Per-occurrence `effective_layer`
    /// metadata takes precedence for expanded block and xref content.
    pub entity_layers: std::collections::HashMap<document::Handle, String>,
    /// Effective STB style keyed by the plotted object and optional block member.
    pub entity_plot_styles:
        std::collections::HashMap<(document::Handle, Option<document::Handle>), String>,
    /// The hyperlinks of the plotted objects, for formats that carry links.
    pub links: Vec<PlotLink>,
    /// The name links reach this page by, when a page of the same file is
    /// their target (a named destination of the PDF).
    pub destination: Option<String>,
}

/// A hyperlink on a plotted object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotLink {
    /// The object the link sits on: its plotted geometry is the link's area.
    pub entity: Option<document::Handle>,
    /// The area in drawing coordinates (`[x0, y0, x1, y1]`) where the object
    /// plots nothing, such as a text of spaces.
    pub bounds: Option<[f64; 4]>,
    /// The address as stored: a web address, a file, or empty for the
    /// drawing itself.
    pub url: String,
    /// The named location: a layout or a view.
    pub location: String,
    /// The folder a relative file address is taken from.
    pub base: Option<std::path::PathBuf>,
    /// A page of the same file the link opens (its `destination`), when the
    /// target is one of the pages written with it.
    pub destination: Option<String>,
}

impl PlotLink {
    /// A web, mail or other address with a scheme, not a file.
    pub fn is_web(&self) -> bool {
        let url = self.url.trim();
        let scheme = url.split_once(':').map(|(scheme, _)| scheme).unwrap_or("");
        // `C:` is a drive, not a scheme.
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }

    /// The drawing a file link opens: its address taken from `base` when
    /// relative, the drawing `own` for an empty address. `None` for a web
    /// address.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn drawing(&self, own: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        let url = self.url.trim().trim_matches('"');
        if url.is_empty() {
            return own.map(std::path::Path::to_path_buf);
        }
        if self.is_web() && !url.to_ascii_lowercase().starts_with("file:") {
            return None;
        }
        let url = url
            .strip_prefix("file:///")
            .map(|rest| {
                if cfg!(windows) {
                    rest.to_string()
                } else {
                    format!("/{rest}")
                }
            })
            .unwrap_or_else(|| url.to_string());
        let mut path = std::path::PathBuf::from(&url);
        let base = self.base.clone().unwrap_or_default();
        // A link written on Windows names its folders with backslashes.
        #[cfg(not(windows))]
        if url.contains('\\') && !base.join(&path).exists() {
            path = std::path::PathBuf::from(url.replace('\\', "/"));
        }
        if path.is_relative() {
            path = base.join(path);
        }
        Some(std::fs::canonicalize(&path).unwrap_or(path))
    }

    /// The layouts the named location can mean: the whole location, and the
    /// layout of a `view,layout` pair.
    pub fn layout_candidates(&self) -> Vec<&str> {
        let location = self.location.trim();
        let mut names = vec![location];
        if let Some((_, layout)) = location.rsplit_once(',') {
            names.push(layout.trim());
        }
        names.retain(|name| !name.is_empty());
        names
    }
}

/// Owned geometry and settings for one plot page.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct PlotPage {
    pub content: PlotContent,
    /// Page dimensions in mm, after any 90/270-degree rotation.
    pub paper_w: f64,
    pub paper_h: f64,
    /// Absolute-world offsets stay f64 to preserve local detail at UTM coordinates.
    pub offset_x: f64,
    pub offset_y: f64,
    pub rotation_deg: i32,
    /// Counter-clockwise turn (radians) of the offset content before scale
    /// and page rotation: a model plot of a twisted view comes out as seen on
    /// screen. `clip` is in the turned space.
    pub content_rotation: f64,
    pub scale: f32,
    pub clip: Option<(f32, f32, f32, f32)>,
    pub options: PlotOptions,
    pub plot_style: Option<PlotStyleTable>,
    /// Explicit model-space GeoPDF registration. Plot-file export may consume
    /// this; preview and ordinary printing always ignore it.
    pub geopdf: Option<crate::io::geopdf_export::GeoPdfFrame>,
}

/// Bounds of the rectangle `[x0, y0, x1, y1]` turned counter-clockwise by
/// `angle` radians about the origin.
pub fn turned_bounds(angle: f64, [x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    if angle == 0.0 {
        return [x0, y0, x1, y1];
    }
    let (sin, cos) = angle.sin_cos();
    let mut out = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        let (tx, ty) = (cos * x - sin * y, sin * x + cos * y);
        out = [out[0].min(tx), out[1].min(ty), out[2].max(tx), out[3].max(ty)];
    }
    out
}

impl PlotPage {
    pub(crate) fn validate_plot_styles(
        &self,
        fallback: Option<&PlotStyleTable>,
    ) -> Result<(), String> {
        let Some(table) = self
            .plot_style
            .as_ref()
            .or(fallback)
            .filter(|table| table.is_stb)
        else {
            return Ok(());
        };
        for ((handle, member), style) in &self.content.entity_plot_styles {
            let matches = table
                .named_entries
                .keys()
                .filter(|entry| entry.eq_ignore_ascii_case(style))
                .count();
            if matches != 1 {
                let object = member.unwrap_or(*handle);
                return Err(format!(
                    "Table {:?}, object {object}: style {style:?} has {matches} matches.",
                    table.name,
                ));
            }
        }
        Ok(())
    }
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            object_lineweights: true,
            scale_lineweights: false,
            transparency: false,
            stamp: false,
            merge_lines: false,
            quality: PlotQuality::Normal,
            raster_dpi: None,
            vector_dpi: None,
            text_as_outlines: false,
            non_scaling_stroke: false,
            layer_attributes: false,
        }
    }
}

/// A circle or arc wire as cubic Bézier spans `[start, control, control,
/// end]`, in the coordinates of its own points, so a plot draws the true
/// curve instead of the screen's chords (#2854). The wire's tangent geometry
/// only says that it is one circle or arc; the curve is fitted to the points,
/// which a layout viewport has already carried onto the sheet. A spline or
/// ellipse wire (no straight-segment vertices, no circle) is drawn through
/// its points by [`smooth_spans`] (#3526). `None` for any other wire, or when
/// a point is off that circle: the chords are plotted.
pub(crate) fn wire_arc_beziers(wire: &WireModel, view_h: f64) -> Option<Vec<[glam::DVec2; 4]>> {
    use crate::scene::model::wire_model::TangentGeom;
    use glam::DVec2;
    let arc = matches!(
        wire.tangent_geoms.as_slice(),
        [TangentGeom::Circle { .. } | TangentGeom::PlanarCircle { .. } | TangentGeom::Arc { .. }]
    );
    let sampled = wire.key_vertices.is_empty()
        && matches!(
            wire.tangent_geoms.as_slice(),
            [] | [TangentGeom::PlanarEllipse { .. }]
        );
    if !(arc || sampled)
        || wire.world_width > 0.0
        || wire.point_marker.is_some()
        || wire.stub.is_some()
        || wire.points.len() < if arc { 5 } else { 3 }
    {
        return None;
    }
    let points: Vec<DVec2> = (0..wire.points.len())
        .map(|index| wire.point_world(index, view_h).truncate())
        .collect();
    // A NaN pen-up (a clipped or dashed piece) is left to the chords.
    if !points.iter().all(|point| point.is_finite()) {
        return None;
    }
    if arc {
        circle_beziers(&points)
    } else {
        smooth_spans(&points)
    }
}

/// Chords sampled from a smooth curve (a spline or an ellipse tessellated for
/// the screen, at most 7.5° a step) as cubic Bézier spans through the same
/// points (#3526). A vertex turning more than 8° is a corner and stays one;
/// a chord leaving a run of smooth vertices curves only when it is at most
/// twice as long as its neighbour in the run, so a line meeting a curve stays
/// straight. Straight chords come back as straight spans. `None` when nothing
/// curves.
fn smooth_spans(points: &[glam::DVec2]) -> Option<Vec<[glam::DVec2; 4]>> {
    use glam::DVec2;
    const MAX_TURN: f64 = 8.0 * std::f64::consts::PI / 180.0;
    let n = points.len();
    if n < 3 {
        return None;
    }
    let dir = |i: usize| (points[i + 1] - points[i]).normalize_or_zero();
    let tangent: Vec<Option<DVec2>> = (0..n)
        .map(|i| {
            if i == 0 || i + 1 == n {
                return None;
            }
            let (a, b) = (dir(i - 1), dir(i));
            (a != DVec2::ZERO && b != DVec2::ZERO && a.angle_to(b).abs() <= MAX_TURN)
                .then(|| (a + b).normalize())
        })
        .collect();
    let length = |i: usize| points[i].distance(points[i + 1]);
    // The end tangent of a parabola through the run: the inner one mirrored
    // in the chord.
    let mirror = |t: DVec2, d: DVec2| (2.0 * t.dot(d) * d - t).normalize_or_zero();
    let mut curved = false;
    let spans = (0..n - 1)
        .map(|i| {
            let (p, q, len, d) = (points[i], points[i + 1], length(i), dir(i));
            let ends = match (tangent[i], tangent[i + 1]) {
                (Some(a), Some(b)) => Some((a, b)),
                (Some(a), None) if len <= 2.0 * length(i - 1) => Some((a, mirror(a, d))),
                (None, Some(b)) if len <= 2.0 * length(i + 1) => Some((mirror(b, d), b)),
                _ => None,
            };
            let Some((t0, t1)) = ends else {
                return [p, p.lerp(q, 1.0 / 3.0), p.lerp(q, 2.0 / 3.0), q];
            };
            curved = true;
            [p, p + t0 * len / 3.0, q - t1 * len / 3.0, q]
        })
        .collect();
    curved.then_some(spans)
}

/// The one circle through `points`, turning one way at most once round it:
/// `(centre, radius, start angle, signed sweep)`.
fn fit_circle(points: &[glam::DVec2]) -> Option<(glam::DVec2, f64, f64, f64)> {
    use glam::DVec2;
    let n = points.len();
    if n < 3 {
        return None;
    }
    let (a, b, c) = (points[0], points[n / 3], points[2 * n / 3]);
    let (ab, ac) = (b - a, c - a);
    let det = 2.0 * ab.perp_dot(ac);
    if det.abs() <= f64::EPSILON * ab.length_squared().max(ac.length_squared()) {
        return None;
    }
    let center = a + DVec2::new(
        ac.y * ab.length_squared() - ab.y * ac.length_squared(),
        ab.x * ac.length_squared() - ac.x * ab.length_squared(),
    ) / det;
    let radius = center.distance(a);
    // Tolerances follow the run's own size: against the radius alone, a run
    // of nearly collinear points fits a huge circle whose arc leaves the
    // boundary far behind (#3611).
    let span = points.iter().map(|point| point.distance(a)).fold(0.0, f64::max);
    let tolerance = radius.min(span) * 1.0e-3;
    if radius > span * 1.0e3
        || points
            .iter()
            .any(|point| (point.distance(center) - radius).abs() > tolerance)
    {
        return None;
    }
    // The points turn one way round the centre, at most once.
    let mut sweep = 0.0;
    for pair in points.windows(2) {
        let turn = (pair[0] - center).angle_to(pair[1] - center);
        if turn.abs() >= std::f64::consts::PI || turn * sweep < 0.0 {
            return None;
        }
        sweep += turn;
    }
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU + 1.0e-6 {
        return None;
    }
    Some((center, radius, (a - center).to_angle(), sweep))
}

/// Points on one circle as cubic Bézier spans of at most a quarter turn.
fn circle_beziers(points: &[glam::DVec2]) -> Option<Vec<[glam::DVec2; 4]>> {
    use glam::DVec2;
    let (center, radius, start, sweep) = fit_circle(points)?;
    // A full turn sums to a hair over 2π; that is still four quarters.
    let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2 - 1.0e-9)
        .ceil()
        .max(1.0) as usize;
    let step = sweep / pieces as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |t: f64| center + radius * DVec2::from_angle(t);
    let tangent = |t: f64| radius * DVec2::from_angle(t).perp();
    Some(
        (0..pieces)
            .map(|piece| {
                let (t0, t1) = (
                    start + step * piece as f64,
                    start + step * (piece + 1) as f64,
                );
                [
                    at(t0),
                    at(t0) + handle * tangent(t0),
                    at(t1) - handle * tangent(t1),
                    at(t1),
                ]
            })
            .collect(),
    )
}

/// Index ranges `start..=end` of a fill ring whose points walk one circle —
/// the arcs of a hatch boundary, sampled into chords when it was built
/// (#3076). A run is at least four points turning under 20° a step, so the
/// corners of a rectangle or hexagon on a common circle stay corners.
// ponytail: a regular polygon of 18+ sides plots round; carry the boundary
// edges through the viewport projection if that ever matters.
fn ring_arc_runs(ring: &[glam::DVec2]) -> Vec<(usize, usize)> {
    const MAX_STEP: f64 = 20.0 * std::f64::consts::PI / 180.0;
    let mut runs = Vec::new();
    let mut start = 0;
    while start + 3 < ring.len() {
        let Some((center, radius, _, sweep)) = fit_circle(&ring[start..start + 3]) else {
            start += 1;
            continue;
        };
        let mut end = start;
        while end + 1 < ring.len() {
            let next = ring[end + 1];
            let turn = (ring[end] - center).angle_to(next - center);
            if (next.distance(center) - radius).abs()
                > radius.min(ring[start].distance(next)) * 1.0e-3
                || turn * sweep <= 0.0
                || turn.abs() > MAX_STEP
            {
                break;
            }
            end += 1;
        }
        if end - start >= 3 && fit_circle(&ring[start..=end]).is_some() {
            runs.push((start, end));
            start = end;
        } else {
            start += 1;
        }
    }
    runs
}

/// A fill ring as a path of `(point, is_control_point)`: the chords it holds,
/// with every arc run among them replaced by its Bézier spans.
pub(crate) fn curved_ring(ring: &[glam::DVec2]) -> Vec<(glam::DVec2, bool)> {
    let mut path = Vec::with_capacity(ring.len());
    let Some(&first) = ring.first() else {
        return path;
    };
    path.push((first, false));
    // The chords from the point already on the path; a spline among them is
    // drawn as its curve (#3526).
    let chords = |path: &mut Vec<(glam::DVec2, bool)>, run: &[glam::DVec2]| match smooth_spans(run) {
        Some(spans) => {
            for [_, c1, c2, point] in spans {
                path.extend([(c1, true), (c2, true), (point, false)]);
            }
        }
        None => path.extend(run.iter().skip(1).map(|&point| (point, false))),
    };
    let mut from = 0;
    for (start, end) in ring_arc_runs(ring) {
        chords(&mut path, &ring[from..=start]);
        for [_, c1, c2, point] in circle_beziers(&ring[start..=end]).unwrap_or_default() {
            path.extend([(c1, true), (c2, true), (point, false)]);
        }
        from = end;
    }
    chords(&mut path, &ring[from..]);
    path
}

/// The hatch with each arc run of its boundary resampled at one degree, so
/// plotted pattern lines end on the curve rather than on its chords.
pub(crate) fn hatch_with_fine_arcs(hatch: &HatchModel) -> Option<HatchModel> {
    use glam::DVec2;
    let origin = DVec2::from(hatch.world_origin);
    let mut boundary = Vec::with_capacity(hatch.boundary.len());
    let mut changed = false;
    for ring in hatch.boundary.split(|p| p[0].is_nan() || p[1].is_nan()) {
        if !boundary.is_empty() {
            boundary.push([f32::NAN, f32::NAN]);
        }
        let points: Vec<DVec2> =
            ring.iter().map(|&[x, y]| DVec2::new(x as f64, y as f64) + origin).collect();
        let mut next = 0;
        for (start, end) in ring_arc_runs(&points) {
            let (center, radius, from, sweep) = fit_circle(&points[start..=end])?;
            boundary.extend_from_slice(&ring[next..start]);
            let steps = (sweep.abs().to_degrees().ceil() as usize).max(1);
            boundary.extend((0..steps).map(|step| {
                let point = center
                    + radius * DVec2::from_angle(from + sweep * step as f64 / steps as f64)
                    - origin;
                [point.x as f32, point.y as f32]
            }));
            next = end;
            changed = true;
        }
        boundary.extend_from_slice(&ring[next..]);
    }
    changed.then(|| HatchModel {
        boundary: std::sync::Arc::new(boundary),
        ..hatch.clone()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_dpi_blank_is_unset_and_typed_values_are_kept() {
        assert_eq!(super::parse_dpi(""), None);
        assert_eq!(super::parse_dpi("  "), None);
        assert_eq!(super::parse_dpi("0"), None);
        assert_eq!(super::parse_dpi("abc"), None);
        assert_eq!(super::parse_dpi(" 600 "), Some(600));
        assert_eq!(super::parse_dpi("2400"), Some(2400));
        assert_eq!(super::parse_dpi("600.0"), Some(600));
        assert_eq!(super::parse_dpi("1200,4"), Some(1200));
        assert_eq!(super::parse_dpi("1,200"), Some(1200));
    }

    use crate::scene::model::wire_model::TangentGeom;
    use crate::scene::WireModel;

    fn arc_wire(sweep: f64, segments: usize) -> WireModel {
        let (center, radius) = ([1000.0, 2000.0], 5.0);
        WireModel {
            geom: std::sync::Arc::new(crate::scene::WireGeom {
                points: (0..=segments)
                    .map(|i| {
                        let t = 0.3 + sweep * i as f64 / segments as f64;
                        let x = center[0] + radius * t.cos();
                        let y = center[1] + radius * t.sin();
                        [x as f32, y as f32, 0.0]
                    })
                    .collect(),
                tangent_geoms: vec![TangentGeom::Circle {
                    center: [1000.0, 2000.0, 0.0],
                    radius: 5.0,
                }],
                ..crate::scene::WireGeom::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn circles_and_arcs_plot_as_curves() {
        for sweep in [1.0, -2.5, std::f64::consts::TAU] {
            let spans = super::wire_arc_beziers(&arc_wire(sweep, 48), 1.0).unwrap();
            assert_eq!(
                spans.len(),
                (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize
            );
            let center = glam::DVec2::new(1000.0, 2000.0);
            for [p0, c1, c2, p3] in &spans {
                // The curve's midpoint lies on the circle.
                let mid = (*p0 + 3.0 * *c1 + 3.0 * *c2 + *p3) / 8.0;
                assert!((mid.distance(center) - 5.0).abs() < 1.0e-3, "{sweep}");
            }
            let end = 0.3 + sweep;
            let last = spans.last().unwrap()[3];
            assert!(last.distance(center + 5.0 * glam::DVec2::from_angle(end)) < 1.0e-3);
        }
        // A wire that is not a circle or arc keeps its chords.
        let mut polygon = arc_wire(std::f64::consts::TAU, 6);
        polygon.tangent_geoms.clear();
        assert!(super::wire_arc_beziers(&polygon, 1.0).is_none());
    }

    #[test]
    fn hatch_ring_arcs_plot_as_curves() {
        use glam::DVec2;
        // A half disc: the diameter, then the arc back sampled at 7.5°.
        let mut ring: Vec<DVec2> = (0..=24)
            .map(|i| 10.0 * DVec2::from_angle(std::f64::consts::PI * i as f64 / 24.0))
            .collect();
        ring.insert(0, DVec2::new(-10.0, 0.0));
        let path = super::curved_ring(&ring);
        assert_eq!(path.iter().filter(|(_, control)| *control).count(), 4);
        assert!(path.last().unwrap().0.distance(ring[25]) < 1.0e-9);
        // A rectangle keeps its corners.
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
            .map(DVec2::from);
        assert!(super::curved_ring(&square).iter().all(|(_, control)| !control));
        // A slightly bowed side and the corner after it stay chords: fitted
        // to a huge circle, the run took the corner in and its arc ended 3
        // units off it (#3611).
        let bowed = [[0.0, 0.0], [10.0, 0.001], [20.0, 0.0015], [30.0, 0.001], [40.0, 0.0]]
            .into_iter()
            .chain([[41.0, -3.0], [0.0, -3.0]])
            .map(DVec2::from)
            .collect::<Vec<_>>();
        assert!(super::ring_arc_runs(&bowed).is_empty());
    }
}
