//! Selection-driven tools: while only PDF underlays (or only xrefs) are
//! selected their actions show on the right-edge toolbar; they go when the
//! selection changes. Contrast and fade stay in the Properties panel.

use super::*;
use crate::modules::{ModuleEvent, ToolDef};

/// The switches of the selected underlay (the first one) the toolbar shows
/// as on.
#[derive(Clone, Debug, PartialEq)]
pub struct UnderlayContext {
    pub monochrome: bool,
    pub shown: bool,
    pub snap: bool,
}

fn tool(id: &'static str, label: &'static str, icon: &'static [u8]) -> ToolDef {
    ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: ModuleEvent::Command(id.to_string()),
    }
}

const CLIP: &[u8] = include_bytes!("../../../assets/icons/xclip.svg");
/// The clip icon with a small red cross at its lower right.
const UNCLIP: &[u8] = include_bytes!("../../../assets/icons/xclip_remove.svg");

/// PDF underlay tools: display, clipping, options, layers and import.
pub fn pdf_underlay_tools() -> Vec<ToolDef> {
    const FRAMES: &[u8] = include_bytes!("../../../assets/icons/underlay_frames.svg");
    vec![
        tool("_PDFULMONO", "Display in\nMonochrome", FRAMES),
        tool("_PDFULCLIP", "Create Clipping\nBoundary", CLIP),
        tool("_PDFULUNCLIP", "Remove\nClipping", UNCLIP),
        tool("_PDFULSHOW", "Show\nUnderlay", FRAMES),
        tool(
            "_PDFULSNAP",
            "Enable\nSnap",
            include_bytes!("../../../assets/icons/snap_underlays.svg"),
        ),
        tool("EXTERNALREFERENCES", "External\nReferences", crate::ui::icons::FOLDER_OPEN),
        tool(
            "ULAYERS",
            "Edit\nLayers",
            include_bytes!("../../../assets/icons/underlay_layers.svg"),
        ),
        tool(
            "_PDFULIMPORT",
            "Import As\nObjects",
            include_bytes!("../../../assets/icons/cui_import.svg"),
        ),
    ]
}

/// External reference tools: edit, open, clipping and the palette.
pub fn xref_tools() -> Vec<ToolDef> {
    vec![
        tool(
            "_XREFEDIT",
            "Edit Reference\nIn-Place",
            include_bytes!("../../../assets/icons/edit_block.svg"),
        ),
        tool("_XREFOPEN", "Open\nReference", crate::ui::icons::FOLDER_OPEN),
        tool("_XREFCLIP", "Create Clipping\nBoundary", CLIP),
        tool("_XREFUNCLIP", "Remove\nClipping", UNCLIP),
        tool("EXTERNALREFERENCES", "External\nReferences", crate::ui::icons::FOLDER_OPEN),
    ]
}

impl Ribbon {
    /// The selection context: the selected underlay's switches, or that
    /// only xrefs are selected, or neither.
    pub fn set_underlay_context(&mut self, context: Option<UnderlayContext>, xref: bool) {
        self.underlay_ctx = context;
        self.xref_ctx = xref;
    }

    pub fn underlay_context(&self) -> Option<&UnderlayContext> {
        self.underlay_ctx.as_ref()
    }

    pub fn xref_context(&self) -> bool {
        self.xref_ctx
    }

    /// Select a tab by its module id.
    pub fn select_by_id(&mut self, id: &str) -> bool {
        match self.modules.iter().position(|m| m.id() == id) {
            Some(index) => {
                self.select(index);
                true
            }
            None => false,
        }
    }
}
