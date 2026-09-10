// Manage module — customization and drawing cleanup tools.

mod about;
mod audit;
mod cui_export;
mod cui_import;
mod find_nonpurgeable;
mod options;
mod overkill;
mod purge;
mod user_interface;

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};

pub struct ManageModule;

impl CadModule for ManageModule {
    fn id(&self) -> &'static str {
        "manage"
    }
    fn title(&self) -> &'static str {
        "Manage"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                // ── Customization ─────────────────────────────────────────────────
                RibbonGroup {
                    title: "Customization",
                    tools: vec![
                        RibbonItem::LargeTool(user_interface::tool()),
                        RibbonItem::LargeTool(crate::modules::ToolDef {
                            id: "TOOLPALETTES",
                            label: "Tool\nPalettes",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/tool_palettes.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command("TOOLPALETTES".to_string()),
                        }),
                        RibbonItem::Tool(cui_import::tool()),
                        RibbonItem::Tool(cui_export::tool()),
                        RibbonItem::Dropdown {
                            id: "ALIASEDIT_DROPDOWN",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/edit_aliases.svg"
                            )),
                            items: vec![
                                (
                                    "ALIASEDIT",
                                    "Edit Aliases",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/edit_aliases.svg"
                                    )),
                                ),
                                (
                                    "CUILOAD",
                                    "Load Partial CUI",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/cui_import.svg"
                                    )),
                                ),
                            ],
                            default: "ALIASEDIT",
                        },
                    ],
                },
                // ── Cleanup ───────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Cleanup",
                    tools: vec![
                        RibbonItem::LargeTool(find_nonpurgeable::tool()),
                        RibbonItem::Tool(purge::tool()),
                        RibbonItem::Tool(overkill::tool()),
                        RibbonItem::Tool(audit::tool()),
                    ],
                },
                // ── Application ───────────────────────────────────────────────────
                RibbonGroup {
                    title: "Application",
                    tools: vec![
                        // The tool and its icon already existed but were never
                        // added to a group, so the dialog could only be reached
                        // by typing OPTIONS or from the start page.
                        RibbonItem::LargeTool(options::tool()),
                        RibbonItem::LargeTool(about::tool()),
                    ],
                },
            ]
        })
    }
}
