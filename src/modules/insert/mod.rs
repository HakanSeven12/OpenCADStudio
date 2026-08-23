// Insert module — references, point clouds, blocks, attributes, import, content.

mod attdef;
mod attedit;
mod attman;
mod attsync;
pub mod base_point;
mod content_browser;
pub(crate) mod create_block;
mod design_center;
mod edit_block;
pub(crate) mod insert_block;
mod landxml;
pub mod minsert;
mod mview_block;
mod open_obj;
mod pc_attach;
mod snap_underlays;
pub(crate) mod solid3d_cmds;
mod underlay_layers;
pub(crate) mod wblock;
mod xadjust;
pub(crate) mod xattach;
mod xclip;

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};

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
                        RibbonItem::LargeTool(xclip::tool()),
                        RibbonItem::LargeTool(xadjust::tool()),
                        RibbonItem::Tool(underlay_layers::tool()),
                        RibbonItem::Dropdown {
                            id: "FRAMES_DROPDOWN",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/underlay_frames.svg"
                            )),
                            items: vec![
                                (
                                    "FRAMES0",
                                    "Frames Off",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/underlay_frames.svg"
                                    )),
                                ),
                                (
                                    "FRAMES1",
                                    "Frames On",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/underlay_frames.svg"
                                    )),
                                ),
                                (
                                    "FRAMES2",
                                    "Frames On, Not Plotted",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/underlay_frames.svg"
                                    )),
                                ),
                            ],
                            default: "FRAMES1",
                        },
                        RibbonItem::Tool(snap_underlays::tool()),
                    ],
                },
                // ── Point Cloud ───────────────────────────────────────────────────
                RibbonGroup {
                    title: "Point Cloud",
                    tools: vec![RibbonItem::LargeTool(pc_attach::tool())],
                },
                // ── Block ─────────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Block",
                    tools: vec![
                        RibbonItem::LargeTool(mview_block::tool()),
                        RibbonItem::LargeTool(insert_block::tool()),
                        RibbonItem::Tool(create_block::tool()),
                        RibbonItem::Tool(edit_block::tool()),
                        RibbonItem::Tool(base_point::tool()),
                    ],
                },
                // ── Attributes ────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Attributes",
                    tools: vec![
                        RibbonItem::LargeTool(attdef::tool()),
                        RibbonItem::LargeTool(attedit::tool()),
                        RibbonItem::Tool(attman::tool()),
                        RibbonItem::Tool(attsync::tool()),
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
