// Insert module — references, point clouds, blocks, attributes, import, content.

mod attdef;
pub(crate) mod attedit;
pub(crate) mod attman;
pub mod base_point;
pub(crate) mod block_cmdline;
mod content_browser;
pub(crate) mod create_block;
mod design_center;
pub(crate) mod dgn_import;
pub(crate) mod tds_import;
pub(crate) mod edit_block;
pub(crate) mod hyperlink_cmd;
pub(crate) mod image_transparency;
pub(crate) mod insert_block;
pub mod minsert;
mod mview_block;
pub(crate) mod pc_attach;
pub(crate) mod pc_crop;
pub(crate) mod pc_extract;
pub(crate) mod pc_stylize;
pub(crate) mod pdf_attach;
pub(crate) mod pdf_clip;
pub(crate) mod pdf_import;
pub(crate) mod pdf_shx_text;
pub(crate) mod picker;
mod snap_underlays;
pub(crate) mod solid3d_cmds;
mod underlay_layers;
pub(crate) mod wblock;
pub(crate) mod xadjust;
pub(crate) mod xattach;
pub(crate) mod xclip;
pub(crate) mod xref_cmd;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

/// Insert > Block Definition: the Create Block / Write Block split button.
pub(crate) const CREATE_BLOCK_DROPDOWN: &str = "CREATE_BLOCK_DROPDOWN";

const IMPORT_ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/import_obj.svg"));

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
                        // Split button: the face runs the last one chosen.
                        RibbonItem::LargeDropdown {
                            id: CREATE_BLOCK_DROPDOWN,
                            label: "Create Block",
                            icon: create_block::tool().icon,
                            items: vec![
                                ("BLOCK", "Create Block", create_block::tool().icon),
                                ("WBLOCK", "Write Block", wblock::tool().icon),
                            ],
                            default: "BLOCK",
                        },
                        RibbonItem::LargeTool(attdef::tool()),
                        RibbonItem::LargeTool(ToolDef {
                            label: "Manage Attributes",
                            ..attman::tool()
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            label: "Block Editor",
                            ..edit_block::tool()
                        }),
                    ],
                },
                // ── Reference ────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Reference",
                    tools: vec![
                        RibbonItem::LargeTool(xattach::tool()),
                        // PDF, a raster image, DWF, DGN or a point cloud;
                        // the face runs the last one chosen.
                        RibbonItem::LargeDropdown {
                            id: "UNDERLAY_ATTACH",
                            label: "Attach Underlay",
                            icon: pdf_attach::ICON,
                            items: vec![
                                ("PDFATTACH", "Attach PDF", pdf_attach::ICON),
                                ("IMAGEATTACH", "Attach Image", pdf_attach::ICON),
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
                            // An empty label shows the current state.
                            label: "",
                            icon: snap_underlays::ICON,
                            items: vec![
                                ("UOSNAP1", "Snap to Underlays ON", snap_underlays::ICON),
                                ("UOSNAP0", "Snap to Underlays OFF", snap_underlays::ICON),
                                // Shown (not selectable) while PDFOSNAP, DWFOSNAP
                                // and DGNOSNAP differ (UOSNAP 2).
                                ("UOSNAP2", "*Underlay OSnap's vary*", snap_underlays::ICON),
                            ],
                            default: "UOSNAP1",
                        },
                    ],
                },
                // ── Import (slide-out: Land XML) ──────────────────────────────────
                RibbonGroup {
                    title: "Import",
                    tools: vec![
                        // The face names and runs the last one chosen. A `;`
                        // answers the started command's prompt, as in a menu
                        // macro.
                        RibbonItem::LargeDropdown {
                            id: "IMPORT_FILE",
                            label: "",
                            icon: IMPORT_ICON,
                            items: vec![
                                ("PDFIMPORT;FILE", "PDF Import", underlay_layers::ICON),
                                ("DGNIMPORT", "DGN Import", underlay_layers::ICON),
                                ("IMPORT", "Import…", IMPORT_ICON),
                            ],
                            default: "PDFIMPORT;FILE",
                        },
                        RibbonItem::LabeledTool(ToolDef {
                            id: "PDFSHXTEXT",
                            label: "Recognize SHX Text",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/text.svg")),
                            event: ModuleEvent::Command("PDFSHXTEXT".to_string()),
                        }),
                        RibbonItem::LabeledTool(ToolDef {
                            id: "PDFSHXTEXT_SETTINGS",
                            label: "Recognition Settings",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/options_tool.svg"
                            )),
                            event: ModuleEvent::Command(
                                "PDFSHXTEXT;SETTINGS".to_string(),
                            ),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "TXT2MTXT",
                            label: "Combine Text",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/mtext.svg")),
                            event: ModuleEvent::Command("TXT2MTXT".to_string()),
                        }),
                    ],
                },
                // ── Data ─────────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Data",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "FIELD",
                            label: "Field",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/text.svg")),
                            event: ModuleEvent::Command("FIELD".to_string()),
                        }),
                        RibbonItem::LabeledTool(ToolDef {
                            id: "UPDATEFIELD",
                            label: "Update Fields",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/ui/refresh.svg"
                            )),
                            event: ModuleEvent::Command("UPDATEFIELD".to_string()),
                        }),
                        RibbonItem::LabeledTool(ToolDef {
                            id: "INSERTOBJ",
                            label: "OLE Object",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/paste.svg")),
                            event: ModuleEvent::Command("INSERTOBJ".to_string()),
                        }),
                        RibbonItem::LabeledTool(ToolDef {
                            id: "HYPERLINK",
                            label: "Hyperlink",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/ui/arrow_long_right.svg"
                            )),
                            event: ModuleEvent::Command("HYPERLINK".to_string()),
                        }),
                    ],
                },
                // ── Linking & Extraction ─────────────────────────────────────────
                RibbonGroup {
                    title: "Linking & Extraction",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            label: "Data\nLink",
                            ..crate::modules::annotate::data_link::tool()
                        }),
                        RibbonItem::LabeledTool(
                            crate::modules::annotate::data_link::download_tool(),
                        ),
                        RibbonItem::LabeledTool(
                            crate::modules::annotate::data_link::upload_tool(),
                        ),
                        RibbonItem::LabeledTool(ToolDef {
                            label: "Extract Data",
                            ..crate::modules::annotate::data_extract::tool()
                        }),
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
