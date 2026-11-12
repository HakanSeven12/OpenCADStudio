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
    /// Drawing layer of each plotted entity, keyed by the handle its wire or
    /// hatch name carries; the PDF groups output into one layer per entry.
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
