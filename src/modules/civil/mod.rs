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
    "CREATEPOINTLABELSTYLE",
    "CREATEALIGNMENT",
    "CREATEOFFSETALIGNMENT",
    "CREATESURFACE",
    "ADDDESIGNSPEEDS",
    "ADDSTATIONEQUATIONS",
    "ADDSUPERELEVATION",
    "CREATEPROFILE",
    "CREATESURFACEPROFILE",
    "CREATEOFFSETPROFILE",
    "CREATEPROFILEVIEW",
    "CREATEFEATURELINE",
    "EDITFEATURELINE",
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
    Opt { label: "Vertical exaggeration", keyword: "V", key: "exaggeration", prompt: "Vertical exaggeration of the view", kind: OptKind::Number },
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

/// The alignment types CREATEALIGNMENT writes.
pub const ALIGNMENT_TYPES: [&str; 3] = ["Centerline", "Utility", "Rail"];

fn alignment_types() -> Vec<&'static str> {
    ALIGNMENT_TYPES.to_vec()
}

const ALIGNMENT_OPTIONS: &[Opt] = &[
    Opt { label: "Radius", keyword: "R", key: "radius", prompt: "Curve radius at the corners (0 for none)", kind: OptKind::Number },
    Opt { label: "Spiral length", keyword: "S", key: "spiral", prompt: "Spiral length on each side of a curve (0 for none)", kind: OptKind::Number },
    Opt { label: "Type", keyword: "T", key: "type", prompt: "Alignment type (? to list)", kind: OptKind::Choice(alignment_types) },
    Opt { label: "Gauge", keyword: "G", key: "gauge", prompt: "Track gauge of a rail alignment", kind: OptKind::Number },
];

/// What CREATEPROFILE's curve value gives.
pub const PROFILE_CURVES: [&str; 3] = ["Length", "K value", "Radius"];

fn profile_curves() -> Vec<&'static str> {
    PROFILE_CURVES.to_vec()
}

/// How CREATEPROFILE writes the profile: by its PVIs, or entity by entity with fixed
/// tangents and parabolas.
pub const PROFILE_LAYOUTS: [&str; 2] = ["PVIs", "Fixed entities"];

fn profile_layouts() -> Vec<&'static str> {
    PROFILE_LAYOUTS.to_vec()
}

const PROFILE_OPTIONS: &[Opt] = &[
    Opt { label: "Curve type", keyword: "C", key: "curve_type", prompt: "What the curve value gives (? to list)", kind: OptKind::Choice(profile_curves) },
    Opt { label: "Layout", keyword: "L", key: "layout", prompt: "Write the profile by its PVIs or as fixed entities (? to list)", kind: OptKind::Choice(profile_layouts) },
];

const SUPERELEVATION_OPTIONS: &[Opt] = &[
    Opt { label: "Normal slope", keyword: "N", key: "normal", prompt: "Normal crown slope of the lanes (0.02 falls 2%)", kind: OptKind::Number },
    Opt { label: "Superelevation rate", keyword: "R", key: "rate", prompt: "Full superelevation rate (0.06 is 6%)", kind: OptKind::Number },
    Opt { label: "Runoff length", keyword: "L", key: "runoff", prompt: "Runoff length from level crown to full superelevation", kind: OptKind::Number },
];

/// What EDITFEATURELINE does with each point.
pub const FEATURE_LINE_EDITS: [&str; 2] = ["Insert", "Delete"];

fn feature_line_edits() -> Vec<&'static str> {
    FEATURE_LINE_EDITS.to_vec()
}

const FEATURE_LINE_OPTIONS: &[Opt] = &[
    Opt { label: "Mode", keyword: "M", key: "mode", prompt: "Insert or set elevation points, or delete points (? to list)", kind: OptKind::Choice(feature_line_edits) },
];

/// The orientations a point label style may take.
pub const LABEL_ORIENTATIONS: [&str; 3] = ["Object", "View", "World"];

fn label_orientations() -> Vec<&'static str> {
    LABEL_ORIENTATIONS.to_vec()
}

const LABEL_STYLE_OPTIONS: &[Opt] = &[
    Opt { label: "Text height", keyword: "H", key: "height", prompt: "Text height of the label's texts", kind: OptKind::Number },
    Opt { label: "Color", keyword: "C", key: "color", prompt: "Color number of the label's texts (1-255)", kind: OptKind::Number },
    Opt { label: "Orientation", keyword: "O", key: "orientation", prompt: "Orient the label to the object, the view or the world (? to list)", kind: OptKind::Choice(label_orientations) },
];

/// Which way the stations ahead of ADDSTATIONEQUATIONS' equations run.
pub const EQUATION_DIRECTIONS: [&str; 2] = ["Increasing", "Decreasing"];

fn equation_directions() -> Vec<&'static str> {
    EQUATION_DIRECTIONS.to_vec()
}

const EQUATION_OPTIONS: &[Opt] = &[
    Opt { label: "Direction", keyword: "D", key: "direction", prompt: "Whether the stations ahead increase or decrease (? to list)", kind: OptKind::Choice(equation_directions) },
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

/// What CREATECORRIDOR locks its region to.
pub const CORRIDOR_LOCKS: [&str; 2] = ["Stations", "Geometry"];

fn corridor_locks() -> Vec<&'static str> {
    CORRIDOR_LOCKS.to_vec()
}

const CORRIDOR_OPTIONS: &[Opt] = &[
    Opt { label: "Surfaces", keyword: "S", key: "surfaces", prompt: "Corridor surfaces to build (? to list)", kind: OptKind::Choice(corridor_surfaces) },
    Opt { label: "Lock", keyword: "L", key: "lock", prompt: "Lock the region to the alignment's stations or geometry (? to list)", kind: OptKind::Choice(corridor_locks) },
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
            Step::Accept { prompt: "Press Enter to create the alignment or [Radius/Spiral length/Type/Gauge]:", options: ALIGNMENT_OPTIONS },
        ],
        "CREATEOFFSETALIGNMENT" => vec![
            Step::Objects { prompt: "Select the parent alignment:", filter: Alignments, slot: 0 },
            Step::Value { prompt: "Specify offset from the parent (negative to the left)", key: "offset", default: 3.5 },
        ],
        "ADDDESIGNSPEEDS" => vec![
            Step::Objects { prompt: "Select an alignment for the design speeds:", filter: Alignments, slot: 0 },
            Step::Points { prompt: "Specify station,design speed or press Enter to end:" },
        ],
        "ADDSTATIONEQUATIONS" => vec![
            Step::Objects { prompt: "Select an alignment for the station equations:", filter: Alignments, slot: 0 },
            Step::Points { prompt: "Specify station,station ahead or press Enter to end:" },
            Step::Accept { prompt: "Press Enter to add the station equations or [Direction]:", options: EQUATION_OPTIONS },
        ],
        "ADDSUPERELEVATION" => vec![
            Step::Objects { prompt: "Select an alignment with curves for the superelevation:", filter: Alignments, slot: 0 },
            Step::Accept { prompt: "Press Enter to add the superelevation or [Normal slope/Superelevation rate/Runoff length]:", options: SUPERELEVATION_OPTIONS },
        ],
        "EDITFEATURELINE" => vec![
            Step::Objects { prompt: "Select a feature line to edit:", filter: FeatureLines, slot: 0 },
            Step::Points { prompt: "Specify point (x,y,elevation) or press Enter to end:" },
            Step::Accept { prompt: "Press Enter to edit the feature line or [Mode]:", options: FEATURE_LINE_OPTIONS },
        ],
        "CREATEPOINTLABELSTYLE" => vec![
            Step::Objects { prompt: "Select COGO points to give the label style or press Enter for none:", filter: CogoPoints, slot: 0 },
            Step::Accept { prompt: "Press Enter to create the label style or [Text height/Color/Orientation]:", options: LABEL_STYLE_OPTIONS },
        ],
        "CREATESURFACE" => vec![
            Step::Objects { prompt: "Select COGO points or points to make the surface of:", filter: SurfacePoints, slot: 0 },
            Step::Objects { prompt: "Select lines or polylines to use as breaklines or press Enter for none:", filter: OptionalPaths, slot: 1 },
            Step::Objects { prompt: "Select a closed polyline as the outer boundary or press Enter for none:", filter: OptionalPaths, slot: 2 },
        ],
        "CREATEPROFILE" => vec![
            Step::Objects { prompt: "Select an alignment for the profile:", filter: Alignments, slot: 0 },
            Step::Points { prompt: "Specify PVI (station,elevation) or press Enter to end:" },
            Step::Value { prompt: "Specify vertical curve length, K value or radius at the inner PVIs (0 for none)", key: "curve", default: 0.0 },
            Step::Accept { prompt: "Press Enter to create the profile or [Curve type/Layout]:", options: PROFILE_OPTIONS },
        ],
        "CREATESURFACEPROFILE" => vec![
            Step::Objects { prompt: "Select an alignment for the surface profile:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select the TIN surface to sample:", filter: TinSurfaces, slot: 1 },
            Step::Value { prompt: "Specify offset from the alignment (negative to the left)", key: "offset", default: 0.0 },
        ],
        "CREATEOFFSETPROFILE" => vec![
            Step::Objects { prompt: "Select an offset alignment:", filter: Alignments, slot: 0 },
            Step::Value { prompt: "Specify slope across the offset (rise over run)", key: "slope", default: 0.0 },
        ],
        "CREATEPROFILEVIEW" => vec![
            Step::Objects { prompt: "Select an alignment for the profile view:", filter: Alignments, slot: 0 },
            Step::Point { prompt: "Specify lower left corner of the profile view:" },
            Step::Accept {
                prompt: "Press Enter to create the view or [Start station/End station/Minimum elevation/Maximum elevation/Vertical exaggeration]:",
                options: VIEW_OPTIONS,
            },
        ],
        "CREATECORRIDOR" => vec![
            Step::Objects { prompt: "Select a baseline alignment that has a profile:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select an assembly:", filter: Assemblies, slot: 1 },
            Step::Accept { prompt: "Press Enter to create the corridor or [Surfaces/Lock]:", options: CORRIDOR_OPTIONS },
        ],
        "CREATESAMPLELINES" => vec![
            Step::Objects { prompt: "Select an alignment for the sample lines:", filter: Alignments, slot: 0 },
            Step::Objects { prompt: "Select TIN surfaces to sample or press Enter for none:", filter: TinSurfaces, slot: 1 },
            Step::Value { prompt: "Specify sample line increment", key: "increment", default: 20.0 },
            Step::Value { prompt: "Specify swath width on each side", key: "swath", default: 20.0 },
        ],
        "CREATESECTIONVIEWS" => vec![
            Step::Objects { prompt: "Select a sample line group:", filter: SampleLineGroups, slot: 0 },
            Step::Point { prompt: "Specify the lower left corner of the section views:" },
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
                        tool("CREATEPOINTLABELSTYLE", "Create Point Label Style", icon!("pointlabelstyle")),
                    ],
                },
                RibbonGroup {
                    title: "Design",
                    tools: vec![
                        tool("CREATEALIGNMENT", "Create Alignment", icon!("alignment")),
                        tool("CREATEOFFSETALIGNMENT", "Create Offset Alignment", icon!("offsetalignment")),
                        tool("ADDDESIGNSPEEDS", "Add Design Speeds", icon!("designspeeds")),
                        tool("ADDSTATIONEQUATIONS", "Add Station Equations", icon!("stationequations")),
                        tool("ADDSUPERELEVATION", "Add Superelevation", icon!("superelevation")),
                        tool("CREATESURFACE", "Create Surface", icon!("surface")),
                        tool("CREATEPROFILE", "Create Profile", icon!("profile")),
                        tool("CREATESURFACEPROFILE", "Create Surface Profile", icon!("surfaceprofile")),
                        tool("CREATEOFFSETPROFILE", "Create Offset Profile", icon!("offsetprofile")),
                        tool("CREATEPROFILEVIEW", "Create Profile View", icon!("profileview")),
                        tool("CREATEFEATURELINE", "Create Feature Line", icon!("featureline")),
                        tool("EDITFEATURELINE", "Edit Feature Line", icon!("editfeatureline")),
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
