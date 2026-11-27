// View module — viewport tools, navigation, visual styles, palettes, interface.

mod assistant_palette;
mod cascade;
mod count_palette;
pub mod dview;
mod file_tabs;
mod files_palette;
mod graphic_attributes_palette;
mod layout_tabs;
pub mod limits;
pub(crate) mod orbit;
pub(crate) mod ortho;
mod pan;
pub(crate) mod persp;
pub mod plot_window;
mod properties_palette;
pub mod quick_print;
mod sheetset;
pub mod snapang_cmd;
mod tile_horiz;
mod tile_vert;
mod tool_palettes;
pub mod ucs_cmd;
mod ucs_icon;
pub mod view_cmd;
pub(crate) mod view_front;
pub(crate) mod view_iso;
pub(crate) mod view_right;
pub(crate) mod view_top;
mod viewcube;
pub mod visual_style;
pub(crate) mod vports_config;
mod vports_highlight;
mod vports_join;
mod vports_named;
mod vports_restore;
pub(crate) mod zoom_ext;
pub(crate) mod zoom_in;
pub(crate) mod zoom_out;
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
                        vports_config::dropdown(),
                        RibbonItem::Tool(vports_named::tool()),
                        RibbonItem::Tool(vports_join::tool()),
                        RibbonItem::Tool(vports_restore::tool()),
                        RibbonItem::Tool(vports_highlight::tool()),
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
                        items: visual_style::VISUAL_STYLES
                            .iter()
                            .map(|style| (style.command, style.label, style.icon))
                            .collect(),
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
                // ── Object Snap ───────────────────────────────────────────────────
                // One-shot overrides for the running point prompt; shown as
                // a toolbar with TOOLBAR "Object Snap".
                RibbonGroup {
                    title: "Object Snap",
                    tools: object_snap_tools(),
                },
                // ── Palettes ──────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Palettes",
                    tools: vec![
                        RibbonItem::LargeTool(tool_palettes::tool()),
                        RibbonItem::LargeTool(properties_palette::tool()),
                        RibbonItem::LargeTool(count_palette::tool()),
                        RibbonItem::LargeTool(files_palette::tool()),
                        RibbonItem::LargeTool(graphic_attributes_palette::tool()),
                        RibbonItem::LargeTool(sheetset::tool()),
                        RibbonItem::LargeTool(assistant_palette::tool()),
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
                    tools: vec![RibbonItem::Tool(ToolDef {
                        id: "PAGESETUP",
                        label: "Page Setup",
                        icon: IconKind::Svg(include_bytes!("../../../assets/icons/pagesetup.svg")),
                        event: ModuleEvent::Command("PAGESETUP".to_string()),
                    })],
                },
            ]
        })
    }
}

fn object_snap_tools() -> Vec<RibbonItem> {
    use crate::icon_assets as icons;
    let tool = |id, label, command: &str, icon| {
        RibbonItem::Tool(ToolDef {
            id,
            label,
            icon: IconKind::Svg(icon),
            event: ModuleEvent::Command(command.to_string()),
        })
    };
    vec![
        tool("OSNAP_FROM", "From", "_FROM", icons::from_icon()),
        tool("OSNAP_ENDP", "Endpoint", "_ENDP", icons::OSNAP_ENDPOINT),
        tool("OSNAP_MID", "Midpoint", "_MID", icons::OSNAP_MIDPOINT),
        tool("OSNAP_INT", "Intersection", "_INT", icons::OSNAP_INTERSECTION),
        tool("OSNAP_APPINT", "Apparent Intersection", "_APPINT", icons::OSNAP_APPARENT),
        tool("OSNAP_EXT", "Extension", "_EXT", icons::OSNAP_EXTENSION),
        tool("OSNAP_CEN", "Center", "_CEN", icons::OSNAP_CENTER),
        tool("OSNAP_GCEN", "Geometric Center", "_GCEN", icons::OSNAP_GEOMETRIC_CENTER),
        tool("OSNAP_QUA", "Quadrant", "_QUA", icons::OSNAP_QUADRANT),
        tool("OSNAP_TAN", "Tangent", "_TAN", icons::OSNAP_TANGENT),
        tool("OSNAP_PER", "Perpendicular", "_PER", icons::OSNAP_PERPENDICULAR),
        tool("OSNAP_PAR", "Parallel", "_PAR", icons::OSNAP_PARALLEL),
        tool("OSNAP_INS", "Insertion", "_INS", icons::OSNAP_INSERTION),
        tool("OSNAP_NOD", "Node", "_NOD", icons::OSNAP_NODE),
        tool("OSNAP_NEA", "Nearest", "_NEA", icons::OSNAP_NEAREST),
        tool("OSNAP_M2P", "Mid Between 2 Points", "_M2P", icons::mtp_icon()),
        tool("OSNAP_SETTINGS", "Osnap Settings...", "OSNAP", icons::ST_OSNAP),
    ]
}
