// Civil 3D objects: COGO points and point groups, alignments, TIN surfaces and layout
// profiles, written as their native records by the codec's civil writers. The commands reuse
// the surface module's steps; `app::civil` carries them out.

use crate::modules::surface::command::{Filter, Opt, OptKind, Step};
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
    "CREATEPARCEL",
    "CREATECORRIDOR",
    "CREATESAMPLELINES",
    "CREATESECTIONVIEWS",
    "CREATEINTERSECTION",
    "CREATEASSEMBLY",
    "CREATEPIPENETWORK",
    "CREATEPRESSURENETWORK",
];

const VIEW_OPTIONS: &[Opt] = &[
    Opt { label: "Start station", keyword: "S", key: "station_start", prompt: "Start station of the view", kind: OptKind::Number },
    Opt { label: "End station", keyword: "E", key: "station_end", prompt: "End station of the view", kind: OptKind::Number },
    Opt { label: "Minimum elevation", keyword: "MI", key: "elevation_min", prompt: "Minimum elevation of the view", kind: OptKind::Number },
    Opt { label: "Maximum elevation", keyword: "MA", key: "elevation_max", prompt: "Maximum elevation of the view", kind: OptKind::Number },
];

const ASSEMBLY_OPTIONS: &[Opt] = &[
    Opt { label: "Lane width", keyword: "LW", key: "LaneWidth", prompt: "Lane width", kind: OptKind::Number },
    Opt { label: "Lane slope", keyword: "LS", key: "LaneSlope", prompt: "Lane cross slope (rise over run, -0.02 falls 2%)", kind: OptKind::Number },
    Opt { label: "Shoulder width", keyword: "SW", key: "ShoulderWidth", prompt: "Shoulder width", kind: OptKind::Number },
    Opt { label: "Shoulder slope", keyword: "SS", key: "ShoulderSlope", prompt: "Shoulder cross slope (rise over run)", kind: OptKind::Number },
    Opt { label: "Pavement depth", keyword: "D", key: "PaveDepth", prompt: "Pavement depth", kind: OptKind::Number },
    Opt { label: "Side slope width", keyword: "W", key: "SideSlopeWidth", prompt: "Side slope width", kind: OptKind::Number },
    Opt { label: "Side slope", keyword: "G", key: "SideSlope", prompt: "Side slope (rise over run, -0.5 is 2:1 down)", kind: OptKind::Number },
];

const ALIGNMENT_OPTIONS: &[Opt] = &[
    Opt { label: "Radius", keyword: "R", key: "radius", prompt: "Curve radius at the corners (0 for none)", kind: OptKind::Number },
    Opt { label: "Spiral length", keyword: "S", key: "spiral", prompt: "Spiral length on each side of a curve (0 for none)", kind: OptKind::Number },
];

const POINT_GROUP_OPTIONS: &[Opt] = &[
    Opt { label: "Name pattern", keyword: "N", key: "name", prompt: "Point names to include (* any run, ? one character, comma between patterns)", kind: OptKind::Text },
    Opt { label: "Description pattern", keyword: "D", key: "description", prompt: "Point descriptions to include (* any run, ? one character, comma between patterns)", kind: OptKind::Text },
    Opt { label: "Minimum elevation", keyword: "MI", key: "elevation_min", prompt: "Lowest elevation to include", kind: OptKind::Number },
    Opt { label: "Maximum elevation", keyword: "MA", key: "elevation_max", prompt: "Highest elevation to include", kind: OptKind::Number },
];

/// Whether a family of the codec's parts list is a pipe's (every pipe family's name ends
/// so; no structure family's does).
fn is_pipe_family(name: &str) -> bool {
    name.ends_with("Pipe") || name.ends_with("Culvert")
}

fn structure_families() -> Vec<&'static str> {
    document::classes::civil::network_part_families().into_iter().filter(|f| !is_pipe_family(f)).collect()
}

fn pipe_families() -> Vec<&'static str> {
    document::classes::civil::network_part_families().into_iter().filter(|f| is_pipe_family(f)).collect()
}

/// The surfaces CREATECORRIDOR offers to build, by the link codes they follow.
pub const CORRIDOR_SURFACES: [&str; 4] = ["Top", "Datum", "Top and Datum", "None"];

fn corridor_surfaces() -> Vec<&'static str> {
    CORRIDOR_SURFACES.to_vec()
}

const CORRIDOR_OPTIONS: &[Opt] = &[
    Opt { label: "Surfaces", keyword: "S", key: "surfaces", prompt: "Corridor surfaces to build (? to list)", kind: OptKind::Choice(corridor_surfaces) },
];

const PIPE_NETWORK_OPTIONS: &[Opt] = &[
    Opt { label: "Structure family", keyword: "S", key: "structure", prompt: "Structure family (? to list)", kind: OptKind::Choice(structure_families) },
    Opt { label: "Pipe family", keyword: "P", key: "pipe", prompt: "Pipe family (? to list)", kind: OptKind::Choice(pipe_families) },
];

fn pressure_pipe_sizes() -> Vec<&'static str> {
    document::classes::civil::pressure_part_descriptions().into_iter().filter(|d| d.starts_with("pipe-")).collect()
}

const PRESSURE_NETWORK_OPTIONS: &[Opt] = &[
    Opt { label: "Pipe size", keyword: "P", key: "pipe", prompt: "Pressure pipe (? to list)", kind: OptKind::Choice(pressure_pipe_sizes) },
];

/// The package parameters CREATEASSEMBLY's options set.
pub const ASSEMBLY_KEYS: [&str; 7] = ["LaneWidth", "LaneSlope", "ShoulderWidth", "ShoulderSlope", "PaveDepth", "SideSlopeWidth", "SideSlope"];

/// The steps of civil command `name`; `None` for a command this module does not run.
pub fn steps(name: &str) -> Option<Vec<Step>> {
    use Filter::*;
    Some(match name {
        "CREATEPOINTS" => vec![Step::Points { prompt: "Specify location of point (x,y,z) or press Enter to end:" }],
        "CREATEPOINTGROUP" => vec![
            Step::Objects { prompt: "Select COGO points for the point group or press Enter to choose them by name, description or elevation:", filter: CogoPoints, slot: 0 },
            Step::Accept {
                prompt: "Press Enter to create the point group or [Name pattern/Description pattern/Minimum elevation/Maximum elevation]:",
                options: POINT_GROUP_OPTIONS,
            },
        ],
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
            Step::Accept {
                prompt: "Press Enter to create the view or [Start station/End station/Minimum elevation/Maximum elevation]:",
                options: VIEW_OPTIONS,
            },
        ],
        "CREATECORRIDOR" => vec![
            Step::Objects { prompt: "Select a baseline alignment that has a profile:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select an assembly:", filter: Assemblies, slot: 1 },
            Step::Accept { prompt: "Press Enter to create the corridor or [Surfaces]:", options: CORRIDOR_OPTIONS },
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
        "CREATEINTERSECTION" => vec![
            Step::Objects { prompt: "Select the main road alignment:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select the crossing road alignment:", filter: Alignments, slot: 1 },
        ],
        "CREATEPARCEL" => vec![Step::Objects { prompt: "Select lines or polylines to make lot lines of:", filter: Paths, slot: 0 }],
        "CREATEASSEMBLY" => vec![
            Step::Point { prompt: "Specify assembly baseline location:" },
            Step::Accept {
                prompt: "Press Enter to create the assembly or [Lane width/Lane slope/Shoulder width/Shoulder slope/Pavement depth/Side slope width/Side slope]:",
                options: ASSEMBLY_OPTIONS,
            },
        ],
        "CREATEPIPENETWORK" => vec![
            Step::Points { prompt: "Specify structure location or press Enter to end:" },
            Step::Accept { prompt: "Press Enter to create the pipe network or [Structure family/Pipe family]:", options: PIPE_NETWORK_OPTIONS },
        ],
        "CREATEPRESSURENETWORK" => vec![
            Step::Points { prompt: "Specify pipe vertex or press Enter to end:" },
            Step::Accept { prompt: "Press Enter to create the pressure network or [Pipe size]:", options: PRESSURE_NETWORK_OPTIONS },
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
                        tool("CREATEPARCEL", "Create Parcel", icon!("parcel")),
                        tool("CREATEASSEMBLY", "Create Assembly", icon!("assembly")),
                        tool("CREATECORRIDOR", "Create Corridor", icon!("corridor")),
                        tool("CREATESAMPLELINES", "Create Sample Lines", icon!("samplelines")),
                        tool("CREATESECTIONVIEWS", "Create Section Views", icon!("sectionviews")),
                        tool("CREATEINTERSECTION", "Create Intersection", icon!("intersection")),
                    ],
                },
                RibbonGroup {
                    title: "Networks",
                    tools: vec![
                        tool("CREATEPIPENETWORK", "Create Pipe Network", icon!("pipenetwork")),
                        tool("CREATEPRESSURENETWORK", "Create Pressure Network", icon!("pressurenetwork")),
                    ],
                },
            ]
        })
    }
}
