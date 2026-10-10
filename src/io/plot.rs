// Shared plot model — the paper-space geometry every plot backend consumes.
//
// `PdfPageInput`/`PdfPlotOptions` used to live in `pdf_export`, which made the
// scene, the print pipeline and now the SVG exporter all import from the PDF
// backend. The structs themselves are backend-agnostic (wires, hatches,
// images, page size/offset/clip), so they live here; `pdf_export` re-exports
// the historical `Pdf*` names as aliases so existing call sites keep working.

use crate::io::plot_style::PlotStyleTable;
use crate::scene::WireModel;
use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::image_model::ImageModel;

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

/// Output controls shared by preview, plot exports, and printer rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlotOptions {
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
        }
    }
}
