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
    /// Named plot style of each plotted entity (ByLayer and ByBlock already
    /// resolved), keyed like `entity_layers`; a named (STB) table plots each
    /// entity with this style.
    pub entity_plot_styles: std::collections::HashMap<document::Handle, String>,
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
    pub scale: f32,
    pub clip: Option<(f32, f32, f32, f32)>,
    pub options: PlotOptions,
    pub plot_style: Option<PlotStyleTable>,
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
/// which a layout viewport has already carried onto the sheet. `None` for any
/// other wire, or when a point is off that circle: the chords are plotted.
pub(crate) fn wire_arc_beziers(wire: &WireModel, view_h: f64) -> Option<Vec<[glam::DVec2; 4]>> {
    use crate::scene::model::wire_model::TangentGeom;
    use glam::DVec2;
    let curve = matches!(
        wire.tangent_geoms.as_slice(),
        [TangentGeom::Circle { .. } | TangentGeom::PlanarCircle { .. } | TangentGeom::Arc { .. }]
    );
    if !curve
        || wire.world_width > 0.0
        || wire.point_marker.is_some()
        || wire.stub.is_some()
        || wire.points.len() < 5
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
    let n = points.len();
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
    if points
        .iter()
        .any(|point| (point.distance(center) - radius).abs() > radius * 1.0e-3)
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
    let start = (a - center).to_angle();
    // A full turn sums to a hair over 2π; that is still four quarters.
    let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2 - 1.0e-9).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |t: f64| center + radius * DVec2::from_angle(t);
    let tangent = |t: f64| radius * DVec2::from_angle(t).perp();
    Some(
        (0..pieces)
            .map(|piece| {
                let (t0, t1) = (start + step * piece as f64, start + step * (piece + 1) as f64);
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
            ..Default::default()
        }
    }

    #[test]
    fn circles_and_arcs_plot_as_curves() {
        for sweep in [1.0, -2.5, std::f64::consts::TAU] {
            let spans = super::wire_arc_beziers(&arc_wire(sweep, 48), 1.0).unwrap();
            assert_eq!(spans.len(), (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize);
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
}
