// Solid creation and kernel modelling tools.

pub mod boolean_cmd;
pub mod edge_cmd;
pub mod primitive_cmd;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct ModelModule;

const BOX_ICON: &[u8] = include_bytes!("../../../assets/icons/box3d.svg");
const CYLINDER_ICON: &[u8] = include_bytes!("../../../assets/icons/cylinder3d.svg");
const CONE_ICON: &[u8] = include_bytes!("../../../assets/icons/cone3d.svg");
const SPHERE_ICON: &[u8] = include_bytes!("../../../assets/icons/sphere3d.svg");
const PYRAMID_ICON: &[u8] = include_bytes!("../../../assets/icons/pyramid3d.svg");
const WEDGE_ICON: &[u8] = include_bytes!("../../../assets/icons/wedge3d.svg");
const TORUS_ICON: &[u8] = include_bytes!("../../../assets/icons/torus3d.svg");
const POLYSOLID_ICON: &[u8] = include_bytes!("../../../assets/icons/polysolid.svg");
const EXTRUDE_ICON: &[u8] = include_bytes!("../../../assets/icons/extrude.svg");
const REVOLVE_ICON: &[u8] = include_bytes!("../../../assets/icons/revolve.svg");
const LOFT_ICON: &[u8] = include_bytes!("../../../assets/icons/loft.svg");
const SWEEP_ICON: &[u8] = include_bytes!("../../../assets/icons/sweep.svg");
const PRESSPULL_ICON: &[u8] = include_bytes!("../../../assets/icons/presspull.svg");
const UNION_ICON: &[u8] = include_bytes!("../../../assets/icons/union.svg");
const SUBTRACT_ICON: &[u8] = include_bytes!("../../../assets/icons/subtract.svg");
const INTERSECT_ICON: &[u8] = include_bytes!("../../../assets/icons/intersect.svg");
const FILLET_ICON: &[u8] = include_bytes!("../../../assets/icons/fillet.svg");
const CHAMFER_ICON: &[u8] = include_bytes!("../../../assets/icons/chamfer.svg");

/// Helper to declare a ribbon tool that fires a named command.
fn tool(id: &'static str, label: &'static str, icon: &'static [u8]) -> ToolDef {
    ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: ModuleEvent::Command(id.to_string()),
    }
}

impl CadModule for ModelModule {
    fn id(&self) -> &'static str {
        "model"
    }
    fn title(&self) -> &'static str {
        "Modelling"
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
                            items: vec![
                                ("BOX", "Box", IconKind::Svg(BOX_ICON)),
                                ("CYLINDER", "Cylinder", IconKind::Svg(CYLINDER_ICON)),
                                ("CONE", "Cone", IconKind::Svg(CONE_ICON)),
                                ("SPHERE", "Sphere", IconKind::Svg(SPHERE_ICON)),
                                ("PYRAMID", "Pyramid", IconKind::Svg(PYRAMID_ICON)),
                                ("WEDGE", "Wedge", IconKind::Svg(WEDGE_ICON)),
                                ("TORUS", "Torus", IconKind::Svg(TORUS_ICON)),
                                ("POLYSOLID", "Polysolid", IconKind::Svg(POLYSOLID_ICON)),
                            ],
                            default: "BOX",
                        },
                        RibbonItem::LargeTool(tool("EXTRUDE", "Extrude", EXTRUDE_ICON)),
                        RibbonItem::LargeTool(tool("REVOLVE", "Revolve", REVOLVE_ICON)),
                        RibbonItem::LargeTool(tool("LOFT", "Loft", LOFT_ICON)),
                        RibbonItem::LargeTool(tool("SWEEP", "Sweep", SWEEP_ICON)),
                        RibbonItem::LargeTool(tool("PRESSPULL", "Presspull", PRESSPULL_ICON)),
                    ],
                },
                RibbonGroup {
                    title: "Boolean",
                    tools: vec![
                        RibbonItem::LargeTool(tool("UNION", "Union", UNION_ICON)),
                        RibbonItem::LargeTool(tool("SUBTRACT", "Subtract", SUBTRACT_ICON)),
                        RibbonItem::LargeTool(tool("INTERSECT", "Intersect", INTERSECT_ICON)),
                    ],
                },
                RibbonGroup {
                    title: "Edges",
                    tools: vec![
                        RibbonItem::LargeTool(tool("SOLIDFILLET", "Fillet", FILLET_ICON)),
                        RibbonItem::LargeTool(tool("SOLIDCHAMFER", "Chamfer", CHAMFER_ICON)),
                    ],
                },
            ]
        })
    }
}
