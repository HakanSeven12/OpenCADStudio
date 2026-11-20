// Surface modelling: surfaces through curves and between surfaces, their editing, control
// vertices, curves on them and the analysis shown on them. The modelling is Solidcore's.

pub mod command;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};
use command::{Filter, Opt, OptKind, Step};

pub struct SurfaceModule;

macro_rules! icon {
    ($name:literal) => {
        IconKind::Svg(include_bytes!(concat!("../../../assets/icons/surface/", $name, ".svg")))
    };
}

fn tool(id: &'static str, label: &'static str, icon: IconKind) -> RibbonItem {
    RibbonItem::LargeTool(ToolDef {
        id,
        label,
        icon,
        event: ModuleEvent::Command(id.to_string()),
    })
}

fn small(id: &'static str, label: &'static str, icon: IconKind) -> RibbonItem {
    RibbonItem::LabeledTool(ToolDef {
        id,
        label,
        icon,
        event: ModuleEvent::Command(id.to_string()),
    })
}

/// The surface commands this module runs.
pub const COMMANDS: &[&str] = &[
    "PLANESURF",
    "SURFNETWORK",
    "SURFPATCH",
    "SURFBLEND",
    "SURFFILLET",
    "SURFEXTEND",
    "SURFTRIM",
    "SURFUNTRIM",
    "SURFOFFSET",
    "SURFSCULPT",
    "SURFEXTRACTCURVE",
    "CONVTONURBS",
    "CVSHOW",
    "CVHIDE",
    "CVADD",
    "CVREMOVE",
    "CVREBUILD",
    "3DEDITBAR",
    "PROJECTGEOMETRY",
    "ANALYSISZEBRA",
    "ANALYSISCURVATURE",
    "ANALYSISDRAFT",
    "ANALYSISOPTIONS",
];

const BLEND_OPTIONS: &[Opt] = &[
    Opt { label: "Continuity", keyword: "CON", key: "continuity", prompt: "Continuity at both edges (0 G0, 1 G1, 2 G2)", kind: OptKind::Number },
    Opt { label: "Bulge magnitude", keyword: "B", key: "bulge", prompt: "Bulge magnitude (0 to 1)", kind: OptKind::Number },
];

const FILLET_OPTIONS: &[Opt] = &[
    Opt { label: "Radius", keyword: "R", key: "radius", prompt: "Specify radius", kind: OptKind::Number },
    Opt { label: "Trim surface", keyword: "T", key: "trim", prompt: "Trim the surfaces to the fillet (1 yes, 0 no)", kind: OptKind::Number },
];

const EXTEND_OPTIONS: &[Opt] = &[
    Opt { label: "Modes", keyword: "M", key: "stretch", prompt: "Extension mode (0 Extend, 1 Stretch)", kind: OptKind::Number },
];

const EXTRACT_OPTIONS: &[Opt] = &[
    Opt { label: "Direction", keyword: "D", key: "direction", prompt: "Isoline direction (0 U, 1 V)", kind: OptKind::Number },
];

const CV_OPTIONS: &[Opt] = &[
    Opt { label: "Direction", keyword: "D", key: "direction", prompt: "Direction of the control vertex row (0 U, 1 V)", kind: OptKind::Number },
];

const ANALYSIS_OPTIONS: &[Opt] = &[
    Opt { label: "Stripes", keyword: "S", key: "stripes", prompt: "Number of zebra stripes", kind: OptKind::Number },
    Opt { label: "Curvature", keyword: "C", key: "curvature", prompt: "Curvature shown (0 Gaussian, 1 mean, 2 maximum, 3 minimum)", kind: OptKind::Number },
    Opt { label: "Minimum draft", keyword: "MI", key: "draft_min", prompt: "Draft angle shown at the low colour, degrees", kind: OptKind::Number },
    Opt { label: "Maximum draft", keyword: "MA", key: "draft_max", prompt: "Draft angle shown at the high colour, degrees", kind: OptKind::Number },
];

/// The steps of surface command `name`; `None` for a command this module does not run.
pub fn steps(name: &str) -> Option<Vec<Step>> {
    use Filter::*;
    Some(match name {
        "PLANESURF" => vec![
            Step::Point { prompt: "Specify first corner or press Enter to select objects:" },
            Step::Point { prompt: "Specify other corner:" },
        ],
        "SURFNETWORK" => vec![
            Step::Objects { prompt: "Select curves in first direction:", filter: Curves, slot: 0 },
            Step::Objects { prompt: "Select curves in second direction:", filter: Curves, slot: 1 },
        ],
        "SURFPATCH" => vec![Step::Objects { prompt: "Select curves to patch:", filter: Curves, slot: 0 }],
        "SURFBLEND" => vec![
            Step::Picks { prompt: "Select first surface edge to blend:", slot: 0, once: true },
            Step::Picks { prompt: "Select second surface edge to blend:", slot: 1, once: true },
            Step::Accept { prompt: "Press Enter to accept the blend surface or [Continuity/Bulge magnitude]:", options: BLEND_OPTIONS },
        ],
        "SURFFILLET" => vec![
            Step::Picks { prompt: "Select first surface to fillet:", slot: 0, once: true },
            Step::Picks { prompt: "Select second surface to fillet:", slot: 1, once: true },
            Step::Accept { prompt: "Press Enter to accept the fillet surface or [Radius/Trim surface]:", options: FILLET_OPTIONS },
        ],
        "SURFEXTEND" => vec![
            Step::Picks { prompt: "Select surface edges to extend:", slot: 0, once: false },
            Step::Value { prompt: "Specify extension distance", key: "distance", default: 1.0 },
            Step::Accept { prompt: "Press Enter to extend or [Modes]:", options: EXTEND_OPTIONS },
        ],
        "SURFTRIM" => vec![
            Step::Objects { prompt: "Select surfaces to trim:", filter: Surfaces, slot: 0 },
            Step::Objects { prompt: "Select cutting curves:", filter: Curves, slot: 1 },
            Step::Picks { prompt: "Select area to trim:", slot: 0, once: true },
        ],
        "SURFUNTRIM" => vec![Step::Picks { prompt: "Select surface to un-trim:", slot: 0, once: true }],
        "SURFOFFSET" => vec![
            Step::Objects { prompt: "Select surfaces to offset:", filter: Surfaces, slot: 0 },
            Step::Value { prompt: "Specify offset distance", key: "distance", default: 1.0 },
        ],
        "SURFSCULPT" => vec![Step::Objects { prompt: "Select surfaces to sculpt into a solid:", filter: Surfaces, slot: 0 }],
        "SURFEXTRACTCURVE" => vec![
            Step::Picks { prompt: "Select point on surface:", slot: 0, once: true },
            Step::Accept { prompt: "Press Enter to extract or [Direction]:", options: EXTRACT_OPTIONS },
        ],
        "CONVTONURBS" => vec![Step::Objects { prompt: "Select surfaces or solids to convert:", filter: Bodies, slot: 0 }],
        "CVSHOW" => vec![Step::Objects { prompt: "Select NURBS surfaces to show their control vertices:", filter: Surfaces, slot: 0 }],
        "CVHIDE" => Vec::new(),
        "CVADD" | "CVREMOVE" => vec![
            Step::Picks { prompt: "Select point on a NURBS surface:", slot: 0, once: true },
            Step::Accept { prompt: "Press Enter to accept or [Direction]:", options: CV_OPTIONS },
        ],
        "CVREBUILD" => vec![
            Step::Objects { prompt: "Select a NURBS surface to rebuild:", filter: Surfaces, slot: 0 },
            Step::Value { prompt: "Number of control vertices in U", key: "u_count", default: 8.0 },
            Step::Value { prompt: "Number of control vertices in V", key: "v_count", default: 8.0 },
            Step::Value { prompt: "Degree in U", key: "u_degree", default: 3.0 },
            Step::Value { prompt: "Degree in V", key: "v_degree", default: 3.0 },
        ],
        "3DEDITBAR" => vec![
            Step::Picks { prompt: "Select point on a NURBS surface:", slot: 0, once: true },
            Step::Point { prompt: "Specify new location:" },
        ],
        "PROJECTGEOMETRY" => vec![
            Step::Objects { prompt: "Select curves to project:", filter: Curves, slot: 0 },
            Step::Picks { prompt: "Select a surface or solid face to project onto:", slot: 0, once: true },
        ],
        "ANALYSISZEBRA" | "ANALYSISCURVATURE" | "ANALYSISDRAFT" => Vec::new(),
        "ANALYSISOPTIONS" => vec![Step::Accept {
            prompt: "Press Enter to apply or [Stripes/Curvature/Minimum draft/Maximum draft]:",
            options: ANALYSIS_OPTIONS,
        }],
        _ => return None,
    })
}

impl CadModule for SurfaceModule {
    fn id(&self) -> &'static str {
        "surface"
    }

    fn title(&self) -> &'static str {
        "Surface"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Create",
                    tools: vec![
                        tool("SURFNETWORK", "Network", icon!("network")),
                        tool("PLANESURF", "Planar", icon!("planesurf")),
                        tool("SURFBLEND", "Blend", icon!("blend")),
                        tool("SURFPATCH", "Patch", icon!("patch")),
                        tool("SURFOFFSET", "Offset", icon!("offset")),
                    ],
                },
                RibbonGroup {
                    title: "Edit",
                    tools: vec![
                        tool("SURFFILLET", "Fillet", icon!("fillet")),
                        small("SURFTRIM", "Trim", icon!("trim")),
                        small("SURFUNTRIM", "Untrim", icon!("untrim")),
                        small("SURFEXTEND", "Extend", icon!("extend")),
                        tool("SURFSCULPT", "Sculpt", icon!("sculpt")),
                    ],
                },
                RibbonGroup {
                    title: "Control Vertices",
                    tools: vec![
                        small("CVSHOW", "Show CV", icon!("cvshow")),
                        small("CVHIDE", "Hide CV", icon!("cvhide")),
                        small("CONVTONURBS", "Convert to NURBS", icon!("convtonurbs")),
                        small("CVREBUILD", "Rebuild", icon!("cvrebuild")),
                        small("CVADD", "Add", icon!("cvadd")),
                        small("CVREMOVE", "Remove", icon!("cvremove")),
                        tool("3DEDITBAR", "CV Edit Bar", icon!("editbar")),
                    ],
                },
                RibbonGroup {
                    title: "Curves",
                    tools: vec![
                        tool("SURFEXTRACTCURVE", "Extract", icon!("extractcurve")),
                        tool("PROJECTGEOMETRY", "Project Geometry", icon!("projectgeometry")),
                    ],
                },
                RibbonGroup {
                    title: "Analysis",
                    tools: vec![
                        small("ANALYSISZEBRA", "Zebra", icon!("zebra")),
                        small("ANALYSISCURVATURE", "Curvature", icon!("curvature")),
                        small("ANALYSISDRAFT", "Draft", icon!("draft")),
                        small("ANALYSISOPTIONS", "Analysis Options", icon!("analysisoptions")),
                    ],
                },
            ]
        })
    }
}
