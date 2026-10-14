//! Contextual "Block Editor" ribbon tab, shown only while a BEDIT session is
//! open: saving or discarding the block, attribute definitions, and the
//! constraint tools that drive parametric blocks.

use crate::modules::parametric::ParametricModule;
use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct BlockEditorModule;

impl CadModule for BlockEditorModule {
    fn id(&self) -> &'static str {
        "block_editor"
    }

    fn title(&self) -> &'static str {
        "Block Editor"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            let command = |id: &'static str, label: &'static str, icon: &'static [u8]| {
                RibbonItem::LargeTool(ToolDef {
                    id,
                    label,
                    icon: IconKind::Svg(icon),
                    event: ModuleEvent::Command(id.to_string()),
                })
            };
            let mut groups = vec![
                RibbonGroup {
                    title: "Open and Save",
                    tools: vec![
                        command(
                            "BEDIT",
                            "Edit Block",
                            include_bytes!("../../../assets/icons/edit_block.svg"),
                        ),
                        RibbonItem::LargeDropdown {
                            id: "BLOCK_SAVE",
                            label: "Save Block",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/mt_ok.svg")),
                            items: vec![
                                (
                                    "BSAVE",
                                    "Save Block",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/mt_ok.svg"
                                    )),
                                ),
                                (
                                    "BSAVEAS",
                                    "Save Block As",
                                    IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/ui/save.svg"
                                    )),
                                ),
                            ],
                            default: "BSAVE",
                        },
                        command(
                            "BCLOSE",
                            "Close Block Editor",
                            include_bytes!("../../../assets/icons/mt_cancel.svg"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Attributes",
                    tools: vec![command(
                        "ATTDEF",
                        "Attribute Definition",
                        include_bytes!("../../../assets/icons/attdef.svg"),
                    )],
                },
            ];
            // Parametric blocks are driven by the same constraints and
            // parameters as the Parametric tab.
            groups.extend(ParametricModule.ribbon_groups().iter().cloned());
            groups
        })
    }
}
