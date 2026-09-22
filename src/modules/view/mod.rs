// View module — viewport tools, navigation, visual styles, palettes, interface.

mod cascade;
mod file_tabs;
mod layout_tabs;
pub mod limits;
mod orbit;
mod ortho;
mod pan;
mod persp;
pub mod plot_window;
pub mod quick_print;
mod properties_palette;
mod sheetset;
mod tile_horiz;
mod tile_vert;
pub mod visual_style;
mod tool_palettes;
pub mod ucs_cmd;
mod ucs_icon;
mod view_front;
mod view_iso;
mod view_right;
mod view_top;
mod viewcube;
mod vports_config;
mod vports_join;
mod vports_named;
mod vports_restore;
mod zoom_ext;
mod zoom_in;
mod zoom_out;
pub mod zoom_window;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct ViewModule;

impl CadModule for ViewModule {
    fn id(&self) -> &'static str {
        "view"
    }
    fn title(&self) -> &'static str {
        "View"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                // ── Viewport Tools ───────────────────────────────────────────────
                RibbonGroup {
                    title: "Viewport Tools",
                    tools: vec![
                        RibbonItem::LargeTool(ucs_icon::tool()),
                        RibbonItem::LargeTool(viewcube::tool()),
                    ],
                },
                // ── Navigate ─────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Navigate",
                    tools: vec![
                        RibbonItem::LargeTool(zoom_ext::tool()),
                        RibbonItem::Tool(zoom_window::tool()),
                        RibbonItem::Tool(zoom_in::tool()),
                        RibbonItem::Tool(zoom_out::tool()),
                        RibbonItem::Tool(pan::tool()),
                        RibbonItem::Tool(orbit::tool()),
                    ],
                },
                // ── Model Viewports ───────────────────────────────────────────────
                RibbonGroup {
                    title: "Model Viewports",
                    tools: vec![
                        RibbonItem::LargeTool(vports_config::tool()),
                        RibbonItem::Tool(vports_named::tool()),
                        RibbonItem::Tool(vports_join::tool()),
                        RibbonItem::Tool(vports_restore::tool()),
                    ],
                },
                // ── Visual Style ──────────────────────────────────────────────────
                RibbonGroup {
                    // WIREFRAME and SOLID ids are special-cased in ribbon.rs
                    // for toggle-state highlighting based on Ribbon::wireframe.
                    title: "Visual Style",
                    tools: vec![RibbonItem::LargeDropdown {
                        id: "VISUAL_STYLE",
                        label: "Visual\nStyle",
                        icon: visual_style::VISUAL_STYLES[0].icon,
                        items: crate::modules::ribbon_command_items(
                            &visual_style::VISUAL_STYLES
                            .iter()
                                .map(|style| style.command)
                                .collect::<Vec<_>>(),
                        ),
                        default: visual_style::VISUAL_STYLES[0].command,
                    }],
                },
                // ── Projection ────────────────────────────────────────────────────
                RibbonGroup {
                    // ORTHO and PERSP ids are special-cased in ribbon.rs
                    // for toggle-state highlighting based on Camera::projection.
                    title: "Projection",
                    tools: vec![
                        RibbonItem::LargeTool(ortho::tool()),
                        RibbonItem::LargeTool(persp::tool()),
                    ],
                },
                // ── Preset Views ──────────────────────────────────────────────────
                RibbonGroup {
                    title: "Preset",
                    tools: vec![
                        RibbonItem::Tool(view_top::tool()),
                        RibbonItem::Tool(view_front::tool()),
                        RibbonItem::Tool(view_right::tool()),
                        RibbonItem::Tool(view_iso::tool()),
                    ],
                },
                // ── Palettes ──────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Palettes",
                    tools: vec![
                        RibbonItem::LargeTool(tool_palettes::tool()),
                        RibbonItem::LargeTool(properties_palette::tool()),
                        RibbonItem::LargeTool(sheetset::tool()),
                    ],
                },
                // ── Interface ─────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Interface",
                    tools: vec![
                        RibbonItem::LargeTool(file_tabs::tool()),
                        RibbonItem::LargeTool(layout_tabs::tool()),
                        RibbonItem::Tool(tile_horiz::tool()),
                        RibbonItem::Tool(tile_vert::tool()),
                        RibbonItem::Tool(cascade::tool()),
                    ],
                },
                // ── Plot ──────────────────────────────────────────────────────────
                // Model space has no paper-space side toolbar, so Page Setup
                // (format/orientation/pick window for PLOTWINDOW) needs an
                // entry here too.
                RibbonGroup {
                    title: "Plot",
                    tools: vec![RibbonItem::Tool(crate::modules::ribbon_command("PAGESETUP"))],
                },
            ]
        })
    }
}
