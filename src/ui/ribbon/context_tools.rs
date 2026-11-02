//! Selection-driven tools: while only underlays of one kind (or only xrefs) are
//! selected their actions show on the right-edge toolbar; they go when the
//! selection changes. Contrast and fade stay in the Properties panel.

use super::*;
use crate::modules::{ModuleEvent, ToolDef};

/// The switches of the selected underlay (the first one) the toolbar shows
/// as on.
#[derive(Clone, Debug, PartialEq)]
pub struct UnderlayContext {
    pub kind: codec::entities::UnderlayType,
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

/// Underlay tools: display, clipping, options, layers and (PDF only)
/// import.
pub fn pdf_underlay_tools(kind: codec::entities::UnderlayType) -> Vec<ToolDef> {
    const FRAMES: &[u8] = include_bytes!("../../../assets/icons/underlay_frames.svg");
    let mut tools = vec![
        tool("_PDFULMONO", "Display in\nMonochrome", FRAMES),
        tool("_PDFULCLIP", "Create Clipping\nBoundary", CLIP),
        tool("_PDFULUNCLIP", "Remove\nClipping", UNCLIP),
        tool("_PDFULSHOW", "Show\nUnderlay", FRAMES),
        tool(
            "_PDFULSNAP",
            "Enable\nSnap",
            include_bytes!("../../../assets/icons/snap_underlays.svg"),
        ),
        tool(
            "EXTERNALREFERENCES",
            "External\nReferences",
            crate::ui::icons::FOLDER_OPEN,
        ),
        tool(
            "ULAYERS",
            "Edit\nLayers",
            include_bytes!("../../../assets/icons/underlay_layers.svg"),
        ),
    ];
    if kind == codec::entities::UnderlayType::Pdf {
        tools.push(tool(
            "_PDFULIMPORT",
            "Import As\nObjects",
            include_bytes!("../../../assets/icons/cui_import.svg"),
        ));
    }
    tools
}

/// Point cloud tools: the three crops, showing, inverting and removing
/// them, and the palette.
pub fn point_cloud_tools() -> Vec<ToolDef> {
    vec![
        tool("_PCCROPRECT", "Rectangular\nCrop", CLIP),
        tool("_PCCROPPOLY", "Polygonal\nCrop", CLIP),
        tool("_PCCROPCIRC", "Circular\nCrop", CLIP),
        tool(
            "_PCCROPSHOW",
            "Show\nCropping",
            include_bytes!("../../../assets/icons/underlay_frames.svg"),
        ),
        tool("_PCCROPINVERT", "Invert\nCropping", CLIP),
        tool("_PCUNCROP", "Remove All\nCropping", UNCLIP),
        tool(
            "EXTERNALREFERENCES",
            "External\nReferences",
            crate::ui::icons::FOLDER_OPEN,
        ),
    ]
}

/// Picked table cell tools: rows, columns, merging, locking and editing.
pub fn table_cell_tools() -> Vec<ToolDef> {
    vec![
        tool(
            "_TCELLROWABOVE",
            "Insert Row\nAbove",
            include_bytes!("../../../assets/icons/table_row_above.svg"),
        ),
        tool(
            "_TCELLROWBELOW",
            "Insert Row\nBelow",
            include_bytes!("../../../assets/icons/table_row_below.svg"),
        ),
        tool(
            "_TCELLDELROWS",
            "Delete\nRows",
            include_bytes!("../../../assets/icons/table_row_delete.svg"),
        ),
        tool(
            "_TCELLCOLLEFT",
            "Insert Column\nLeft",
            include_bytes!("../../../assets/icons/table_col_left.svg"),
        ),
        tool(
            "_TCELLCOLRIGHT",
            "Insert Column\nRight",
            include_bytes!("../../../assets/icons/table_col_right.svg"),
        ),
        tool(
            "_TCELLDELCOLS",
            "Delete\nColumns",
            include_bytes!("../../../assets/icons/table_col_delete.svg"),
        ),
        tool(
            "_TCELLMERGE",
            "Merge\nCells",
            include_bytes!("../../../assets/icons/table_merge.svg"),
        ),
        tool(
            "_TCELLUNMERGE",
            "Unmerge\nCells",
            include_bytes!("../../../assets/icons/table_unmerge.svg"),
        ),
        tool(
            "_TCELLLOCK",
            "Cell\nLocking",
            include_bytes!("../../../assets/icons/table_cell_lock.svg"),
        ),
        tool(
            "_TCELLMATCH",
            "Match\nCell",
            include_bytes!("../../../assets/icons/match_prop.svg"),
        ),
        tool(
            "_TCELLALIGN",
            "Alignment",
            include_bytes!("../../../assets/icons/align_hcenter.svg"),
        ),
        tool(
            "_TCELLBORDERS",
            "Cell\nBorders",
            include_bytes!("../../../assets/icons/table_cell_borders.svg"),
        ),
        tool(
            "_TCELLFORMAT",
            "Data\nFormat",
            include_bytes!("../../../assets/icons/table_cell_format.svg"),
        ),
        tool(
            "_TCELLBLOCK",
            "Insert\nBlock",
            include_bytes!("../../../assets/icons/edit_block.svg"),
        ),
        tool(
            "_TCELLFIELD",
            "Insert\nField",
            include_bytes!("../../../assets/icons/text.svg"),
        ),
        tool(
            "_TCELLFORMULA",
            "Formula",
            include_bytes!("../../../assets/icons/table_cell_formula.svg"),
        ),
        tool(
            "_TCELLCONTENTS",
            "Manage Cell\nContents",
            include_bytes!("../../../assets/icons/table_cell_contents.svg"),
        ),
        tool(
            "_TCELLLINK",
            "Link\nCell",
            include_bytes!("../../../assets/icons/data_link.svg"),
        ),
        tool(
            "_TCELLDOWNLOAD",
            "Download\nfrom Source",
            include_bytes!("../../../assets/icons/sync.svg"),
        ),
        tool(
            "_TCELLEDIT",
            "Edit\nText",
            include_bytes!("../../../assets/icons/ddedit.svg"),
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
        tool(
            "_XREFOPEN",
            "Open\nReference",
            crate::ui::icons::FOLDER_OPEN,
        ),
        tool("_XREFCLIP", "Create Clipping\nBoundary", CLIP),
        tool("_XREFUNCLIP", "Remove\nClipping", UNCLIP),
        tool(
            "EXTERNALREFERENCES",
            "External\nReferences",
            crate::ui::icons::FOLDER_OPEN,
        ),
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

    /// Only point clouds are selected: whether the first shows its crops.
    pub fn set_point_cloud_context(&mut self, context: Option<bool>) {
        self.point_cloud_ctx = context;
    }

    pub fn point_cloud_context(&self) -> Option<bool> {
        self.point_cloud_ctx
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
