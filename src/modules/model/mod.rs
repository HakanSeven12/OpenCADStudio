// Solid creation and kernel modelling tools.

pub mod boolean_cmd;
pub mod cylinder_cmd;
pub mod edge_cmd;
pub mod polysolid_cmd;
pub mod primitive_cmd;
pub mod sectionplane_cmd;
pub mod shell_cmd;
pub mod slice_cmd;
pub mod solidedit_cmd;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct ModelModule;

const BOX_ICON: &[u8] = include_bytes!("../../../assets/icons/model/box.svg");
const CYLINDER_ICON: &[u8] = include_bytes!("../../../assets/icons/model/cylinder.svg");
const CONE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/cone.svg");
const SPHERE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/sphere.svg");
const PYRAMID_ICON: &[u8] = include_bytes!("../../../assets/icons/model/pyramid.svg");
const WEDGE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/wedge.svg");
const TORUS_ICON: &[u8] = include_bytes!("../../../assets/icons/model/torus.svg");
const POLYSOLID_ICON: &[u8] = include_bytes!("../../../assets/icons/model/polysolid.svg");
const EXTRUDE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/extrude.svg");
const REVOLVE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/revolve.svg");
const LOFT_ICON: &[u8] = include_bytes!("../../../assets/icons/model/loft.svg");
const SWEEP_ICON: &[u8] = include_bytes!("../../../assets/icons/model/sweep.svg");
const PRESSPULL_ICON: &[u8] = include_bytes!("../../../assets/icons/model/presspull.svg");
const UNION_ICON: &[u8] = include_bytes!("../../../assets/icons/model/union.svg");
const SUBTRACT_ICON: &[u8] = include_bytes!("../../../assets/icons/model/subtract.svg");
const INTERSECT_ICON: &[u8] = include_bytes!("../../../assets/icons/model/intersect.svg");
const FILLET_ICON: &[u8] = include_bytes!("../../../assets/icons/model/fillet.svg");
const CHAMFER_ICON: &[u8] = include_bytes!("../../../assets/icons/model/chamfer.svg");
const SHELL_ICON: &[u8] = include_bytes!("../../../assets/icons/model/shell.svg");
const SLICE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/slice.svg");
const INTERFERE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/interfere.svg");
const THICKEN_ICON: &[u8] = include_bytes!("../../../assets/icons/model/thicken.svg");
const EXTRACT_EDGES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/extract_edges.svg");
const IMPRINT_ICON: &[u8] = include_bytes!("../../../assets/icons/model/imprint.svg");
const COLOR_EDGES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/color_edges.svg");
const COPY_EDGES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/copy_edges.svg");
const EXTRUDE_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/extrude_faces.svg");
const TAPER_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/taper_faces.svg");
const MOVE_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/move_faces.svg");
const COPY_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/copy_faces.svg");
const OFFSET_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/offset_faces.svg");
const DELETE_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/delete_faces.svg");
const ROTATE_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/rotate_faces.svg");
const COLOR_FACES_ICON: &[u8] = include_bytes!("../../../assets/icons/model/color_faces.svg");
const SEPARATE_ICON: &[u8] = include_bytes!("../../../assets/icons/model/separate.svg");
const CLEAN_ICON: &[u8] = include_bytes!("../../../assets/icons/model/clean.svg");
const CHECK_ICON: &[u8] = include_bytes!("../../../assets/icons/model/check.svg");

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
                    title: "Solid Editing",
                    tools: vec![
                        RibbonItem::LargeTool(tool("UNION", "Union", UNION_ICON)),
                        RibbonItem::LargeTool(tool("SUBTRACT", "Subtract", SUBTRACT_ICON)),
                        RibbonItem::LargeTool(tool("INTERSECT", "Intersect", INTERSECT_ICON)),
                        RibbonItem::LargeTool(tool("INTERFERE", "Interfere", INTERFERE_ICON)),
                        RibbonItem::LargeTool(tool("SLICE", "Slice", SLICE_ICON)),
                        RibbonItem::LargeTool(tool("THICKEN", "Thicken", THICKEN_ICON)),
                        RibbonItem::LargeDropdown {
                            id: "SOLIDEDIT_EDGES",
                            label: "Extract Edges",
                            icon: IconKind::Svg(EXTRACT_EDGES_ICON),
                            items: vec![
                                ("XEDGES", "Extract Edges", IconKind::Svg(EXTRACT_EDGES_ICON)),
                                ("IMPRINT", "Imprint", IconKind::Svg(IMPRINT_ICON)),
                                ("SOLIDEDIT EDGE COLOR", "Color Edges", IconKind::Svg(COLOR_EDGES_ICON)),
                                ("SOLIDEDIT EDGE COPY", "Copy Edges", IconKind::Svg(COPY_EDGES_ICON)),
                            ],
                            default: "XEDGES",
                        },
                        RibbonItem::LargeDropdown {
                            id: "SOLIDEDIT_FACES",
                            label: "Extrude Faces",
                            icon: IconKind::Svg(EXTRUDE_FACES_ICON),
                            items: vec![
                                ("SOLIDEDIT FACE EXTRUDE", "Extrude Faces", IconKind::Svg(EXTRUDE_FACES_ICON)),
                                ("SOLIDEDIT FACE TAPER", "Taper Faces", IconKind::Svg(TAPER_FACES_ICON)),
                                ("SOLIDEDIT FACE MOVE", "Move Faces", IconKind::Svg(MOVE_FACES_ICON)),
                                ("SOLIDEDIT FACE COPY", "Copy Faces", IconKind::Svg(COPY_FACES_ICON)),
                                ("SOLIDEDIT FACE OFFSET", "Offset Faces", IconKind::Svg(OFFSET_FACES_ICON)),
                                ("SOLIDEDIT FACE DELETE", "Delete Faces", IconKind::Svg(DELETE_FACES_ICON)),
                                ("SOLIDEDIT FACE ROTATE", "Rotate Faces", IconKind::Svg(ROTATE_FACES_ICON)),
                                ("SOLIDEDIT FACE COLOR", "Color Faces", IconKind::Svg(COLOR_FACES_ICON)),
                            ],
                            default: "SOLIDEDIT FACE EXTRUDE",
                        },
                        RibbonItem::LargeDropdown {
                            id: "SOLIDEDIT_BODY",
                            label: "Separate",
                            icon: IconKind::Svg(SEPARATE_ICON),
                            items: vec![
                                ("SOLIDEDIT BODY SEPARATE", "Separate", IconKind::Svg(SEPARATE_ICON)),
                                ("SOLIDEDIT BODY CLEAN", "Clean", IconKind::Svg(CLEAN_ICON)),
                                ("SHELL", "Shell", IconKind::Svg(SHELL_ICON)),
                                ("SOLIDEDIT BODY CHECK", "Check", IconKind::Svg(CHECK_ICON)),
                            ],
                            default: "SOLIDEDIT BODY SEPARATE",
                        },
                    ],
                },
                RibbonGroup {
                    title: "Edges",
                    tools: vec![
                        RibbonItem::LargeTool(tool("FILLETEDGE", "Fillet Edge", FILLET_ICON)),
                        RibbonItem::LargeTool(tool("CHAMFEREDGE", "Chamfer", CHAMFER_ICON)),
                    ],
                },
            ]
        })
    }
}
