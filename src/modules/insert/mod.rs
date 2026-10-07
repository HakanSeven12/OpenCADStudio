// Insert module — references, point clouds, blocks, attributes, import, content.

mod attdef;
pub(crate) mod attedit;
mod attman;
pub mod base_point;
mod content_browser;
pub(crate) mod create_block;
mod design_center;
pub(crate) mod edit_block;
pub(crate) mod image_transparency;
pub(crate) mod insert_block;
mod landxml;
pub(crate) mod picker;
pub mod minsert;
mod mview_block;
mod open_obj;
pub(crate) mod pc_attach;
pub(crate) mod pc_crop;
pub(crate) mod pc_extract;
pub(crate) mod pc_stylize;
pub(crate) mod pdf_attach;
pub(crate) mod pdf_clip;
pub(crate) mod pdf_import;
mod snap_underlays;
pub(crate) mod solid3d_cmds;
mod underlay_layers;
pub(crate) mod wblock;
mod xadjust;
pub(crate) mod xattach;
pub(crate) mod xref_cmd;
pub(crate) mod xclip;

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};

const FRAMES_ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/underlay_frames.svg"));

pub struct InsertModule;

impl CadModule for InsertModule {
    fn id(&self) -> &'static str {
        "insert"
    }
    fn title(&self) -> &'static str {
        "Insert"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                // ── Reference ────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Reference",
                    tools: vec![
                        RibbonItem::LargeTool(xattach::tool()),
                        // PDF, DWF, DGN or a point cloud; the face runs the
                        // last one chosen.
                        RibbonItem::LargeDropdown {
                            id: "UNDERLAY_ATTACH",
                            label: "Attach Underlay",
                            icon: pdf_attach::ICON,
                            items: vec![
                                ("PDFATTACH", "Attach PDF", pdf_attach::ICON),
                                ("DWFATTACH", "Attach DWF", pdf_attach::ICON),
                                ("DGNATTACH", "Attach DGN", pdf_attach::ICON),
                                ("POINTCLOUDATTACH", "Attach Point Cloud", pc_attach::ICON),
                            ],
                            default: "PDFATTACH",
                        },
                        RibbonItem::LargeTool(xclip::tool()),
                        RibbonItem::LargeTool(xadjust::tool()),
                        RibbonItem::LabeledTool(underlay_layers::tool()),
                        // An empty label shows the chosen item's.
                        RibbonItem::LabeledDropdown {
                            id: "FRAMES_DROPDOWN",
                            label: "",
                            icon: FRAMES_ICON,
                            items: vec![
                                ("FRAMES0", "Hide frames", FRAMES_ICON),
                                ("FRAMES1", "Display and plot frames", FRAMES_ICON),
                                ("FRAMES2", "Display but don't plot frames", FRAMES_ICON),
                                // Shown (not selectable) while the frame
                                // variables differ from each other.
                                ("FRAMES3", "*Frames vary*", FRAMES_ICON),
                            ],
                            default: "FRAMES1",
                        },
                        RibbonItem::LabeledDropdown {
                            id: "UOSNAP_DROPDOWN",
                            label: "Snap to Underlays",
                            icon: snap_underlays::ICON,
                            items: vec![
                                ("UOSNAP1", "Snap to Underlays ON", snap_underlays::ICON),
                                ("UOSNAP0", "Snap to Underlays OFF", snap_underlays::ICON),
                            ],
                            default: "UOSNAP1",
                        },
                    ],
                },
                // ── Block ─────────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Block",
                    tools: vec![
                        RibbonItem::LargeTool(mview_block::tool()),
                        insert_block::gallery(),
                        RibbonItem::LabeledDropdown {
                            id: "ATTEDIT_DROPDOWN",
                            label: "Edit Attribute",
                            icon: attedit::ICON,
                            items: vec![
                                ("ATTEDIT", "Single", attedit::ICON),
                                ("-ATTEDIT", "Multiple", attedit::ICON),
                            ],
                            default: "ATTEDIT",
                        },
                        // ATTMODE: the face shows the drawing's current setting.
                        RibbonItem::LabeledDropdown {
                            id: "ATTMODE_DROPDOWN",
                            label: "",
                            icon: attedit::ICON,
                            items: vec![
                                ("ATTMODE1", "Retain Attribute Display", attedit::ICON),
                                ("ATTMODE2", "Display All Attributes", attedit::ICON),
                                ("ATTMODE0", "Hide All Attributes", attedit::ICON),
                            ],
                            default: "ATTMODE1",
                        },
                    ],
                },
                // ── Block Definition (slide-out: Set Base Point, Synchronize) ─────
                RibbonGroup {
                    title: "Block Definition",
                    tools: vec![
                        RibbonItem::LargeTool(create_block::tool()),
                        RibbonItem::LargeTool(attdef::tool()),
                        RibbonItem::LargeTool(attman::tool()),
                        RibbonItem::LargeTool(edit_block::tool()),
                    ],
                },
                // ── Import ────────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Import",
                    tools: vec![
                        RibbonItem::LargeTool(open_obj::tool()),
                        RibbonItem::LargeTool(landxml::tool()),
                    ],
                },
                // ── Content ───────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Content",
                    tools: vec![
                        RibbonItem::LargeTool(content_browser::tool()),
                        RibbonItem::LargeTool(design_center::tool()),
                    ],
                },
            ]
        })
    }
}
