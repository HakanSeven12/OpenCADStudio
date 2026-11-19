// Civil 3D objects: COGO points and point groups, alignments, TIN surfaces and layout
// profiles, written as their native records by the codec's civil writers. The commands reuse
// the surface module's steps; `app::civil` carries them out.

use crate::modules::surface::command::{Filter, Opt, Step};
use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct CivilModule;

macro_rules! icon {
    ($name:literal) => {
        IconKind::Svg(include_bytes!(concat!("../../../assets/icons/civil/", $name, ".svg")))
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

/// The civil commands this module runs.
pub const COMMANDS: &[&str] = &[
    "CREATEPOINTS",
    "CREATEPOINTGROUP",
    "CREATEALIGNMENT",
    "CREATESURFACE",
    "CREATEPROFILE",
    "CREATEPROFILEVIEW",
    "CREATEFEATURELINE",
    "CREATECORRIDOR",
    "CREATESAMPLELINES",
    "CREATESECTIONVIEWS",
];

const ALIGNMENT_OPTIONS: &[Opt] = &[
    Opt { label: "Radius", keyword: "R", key: "radius", prompt: "Curve radius at the corners (0 for none)" },
    Opt { label: "Spiral length", keyword: "S", key: "spiral", prompt: "Spiral length on each side of a curve (0 for none)" },
];

/// The steps of civil command `name`; `None` for a command this module does not run.
pub fn steps(name: &str) -> Option<Vec<Step>> {
    use Filter::*;
    Some(match name {
        "CREATEPOINTS" => vec![Step::Points { prompt: "Specify location of point (x,y,z) or press Enter to end:" }],
        "CREATEPOINTGROUP" => vec![Step::Objects { prompt: "Select COGO points for the point group:", filter: CogoPoints, slot: 0 }],
        "CREATEALIGNMENT" => vec![
            Step::Objects { prompt: "Select a line or polyline to make an alignment of:", filter: Paths, slot: 0 },
            Step::Accept { prompt: "Press Enter to create the alignment or [Radius/Spiral length]:", options: ALIGNMENT_OPTIONS },
        ],
        "CREATESURFACE" => vec![Step::Objects { prompt: "Select COGO points or points to make the surface of:", filter: SurfacePoints, slot: 0 }],
        "CREATEPROFILE" => vec![
            Step::Objects { prompt: "Select an alignment for the profile:", filter: Alignments, slot: 0 },
            Step::Points { prompt: "Specify PVI (station,elevation) or press Enter to end:" },
            Step::Value { prompt: "Specify vertical curve length at the inner PVIs (0 for none)", key: "curve", default: 0.0 },
        ],
        "CREATEPROFILEVIEW" => vec![
            Step::Objects { prompt: "Select an alignment for the profile view:", filter: Alignments, slot: 0 },
            Step::Point { prompt: "Specify lower left corner of the profile view:" },
        ],
        "CREATECORRIDOR" => vec![
            Step::Objects { prompt: "Select a baseline alignment that has a profile:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select an assembly:", filter: Assemblies, slot: 1 },
        ],
        "CREATESAMPLELINES" => vec![
            Step::Objects { prompt: "Select an alignment for the sample lines:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select TIN surfaces to sample or press Enter for none:", filter: TinSurfaces, slot: 1 },
            Step::Value { prompt: "Specify sample line increment", key: "increment", default: 20.0 },
            Step::Value { prompt: "Specify swath width on each side", key: "swath", default: 20.0 },
        ],
        "CREATESECTIONVIEWS" => vec![
            Step::Objects { prompt: "Select a sample line group:", filter: SampleLineGroups, slot: 0 },
            Step::Point { prompt: "Specify the center line point of the first section view:" },
        ],
        "CREATEFEATURELINE" => vec![Step::Objects { prompt: "Select lines or polylines to make feature lines of:", filter: Paths, slot: 0 }],
        _ => return None,
    })
}

impl CadModule for CivilModule {
    fn id(&self) -> &'static str {
        "civil"
    }

    fn title(&self) -> &'static str {
        "Civil"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Points",
                    tools: vec![
                        tool("CREATEPOINTS", "Create Points", icon!("points")),
                        tool("CREATEPOINTGROUP", "Create Point Group", icon!("pointgroup")),
                    ],
                },
                RibbonGroup {
                    title: "Design",
                    tools: vec![
                        tool("CREATEALIGNMENT", "Create Alignment", icon!("alignment")),
                        tool("CREATESURFACE", "Create Surface", icon!("surface")),
                        tool("CREATEPROFILE", "Create Profile", icon!("profile")),
                        tool("CREATEPROFILEVIEW", "Create Profile View", icon!("profileview")),
                        tool("CREATEFEATURELINE", "Create Feature Line", icon!("featureline")),
                        tool("CREATECORRIDOR", "Create Corridor", icon!("corridor")),
                        tool("CREATESAMPLELINES", "Create Sample Lines", icon!("samplelines")),
                        tool("CREATESECTIONVIEWS", "Create Section Views", icon!("sectionviews")),
                    ],
                },
            ]
        })
    }
}
