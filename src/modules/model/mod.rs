// Solid creation and kernel modelling tools.

pub mod boolean_cmd;
pub mod cylinder_cmd;
pub mod edge_cmd;
pub mod polysolid_cmd;
pub mod primitive_cmd;
pub mod sectionplane_cmd;
pub mod shell_cmd;
pub mod slice_cmd;

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};

pub struct ModelModule;

const BOX_ICON: &[u8] = include_bytes!("../../../assets/icons/model/box.svg");

impl CadModule for ModelModule {
    fn id(&self) -> &'static str {
        "model"
    }
    fn title(&self) -> &'static str {
        "Model"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Create",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "MODEL_PRIMITIVES",
                            label: "Box",
                            icon: IconKind::Svg(BOX_ICON),
                            items: crate::modules::ribbon_command_items(&[
                                "BOX",
                                "CYLINDER",
                                "CONE",
                                "SPHERE",
                                "PYRAMID",
                                "WEDGE",
                                "TORUS",
                                "POLYSOLID",
                            ]),
                            default: "BOX",
                        },
                        RibbonItem::LargeTool(crate::modules::ribbon_command("EXTRUDE")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("REVOLVE")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("LOFT")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("SWEEP")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("PRESSPULL")),
                    ],
                },
                RibbonGroup {
                    title: "Boolean",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ribbon_command("UNION")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("SUBTRACT")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("INTERSECT")),
                    ],
                },
                RibbonGroup {
                    title: "Edges",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ribbon_command("FILLETEDGE")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("CHAMFEREDGE")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("SHELL")),
                    ],
                },
            ]
        })
    }
}
