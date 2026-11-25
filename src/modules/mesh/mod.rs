// Mesh modelling: mesh primitives, meshes through curves, smoothing, creases, face editing
// and conversion of meshes to solids and surfaces.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use document::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::surface::command::{Filter, Opt, OptKind, Step};
use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct MeshModule;

macro_rules! icon {
    ($name:literal) => {
        IconKind::Svg(include_bytes!(concat!("../../../assets/icons/mesh/", $name, ".svg")))
    };
}

fn def(id: &'static str, label: &'static str, icon: IconKind) -> ToolDef {
    ToolDef {
        id,
        label,
        icon,
        event: ModuleEvent::Command(id.to_string()),
    }
}

/// The mesh commands run as steps (see [`steps`]).
pub const COMMANDS: &[&str] = &[
    "MESHSMOOTH",
    "MESHSMOOTHMORE",
    "MESHSMOOTHLESS",
    "MESHREFINE",
    "MESHCREASE",
    "MESHUNCREASE",
    "MESHEXTRUDE",
    "MESHSPLIT",
    "MESHMERGE",
    "MESHCAP",
    "MESHCOLLAPSE",
    "MESHSPIN",
    "CONVTOSOLID",
    "CONVTOSURFACE",
    "REVSURF",
    "RULESURF",
    "TABSURF",
    "EDGESURF",
    "MESHOPTIONS",
];

/// A setting of the mesh commands, typed like a system variable.
pub struct Var {
    pub name: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Whole numbers only.
    pub integer: bool,
}

const fn int(name: &'static str, default: i32, min: i32, max: i32) -> Var {
    Var { name, default: default as f64, min: min as f64, max: max as f64, integer: true }
}

/// The settings, by name.
pub const VARS: &[Var] = &[
    int("DIVMESHBOXHEIGHT", 3, 1, 256),
    int("DIVMESHBOXLENGTH", 3, 1, 256),
    int("DIVMESHBOXWIDTH", 3, 1, 256),
    int("DIVMESHCONEAXIS", 8, 3, 512),
    int("DIVMESHCONEBASE", 3, 1, 512),
    int("DIVMESHCONEHEIGHT", 3, 1, 512),
    int("DIVMESHCYLAXIS", 8, 3, 512),
    int("DIVMESHCYLBASE", 3, 1, 512),
    int("DIVMESHCYLHEIGHT", 3, 1, 512),
    int("DIVMESHPYRBASE", 3, 1, 256),
    int("DIVMESHPYRHEIGHT", 3, 1, 256),
    int("DIVMESHPYRLENGTH", 3, 1, 256),
    int("DIVMESHSPHEREAXIS", 12, 3, 512),
    int("DIVMESHSPHEREHEIGHT", 6, 2, 1024),
    int("DIVMESHTORUSPATH", 8, 3, 512),
    int("DIVMESHTORUSSECTION", 8, 3, 512),
    int("DIVMESHWEDGEHEIGHT", 3, 1, 64),
    int("DIVMESHWEDGELENGTH", 4, 1, 64),
    int("DIVMESHWEDGESLOPE", 3, 1, 64),
    int("DIVMESHWEDGEWIDTH", 3, 1, 64),
    Var { name: "FACETERDEVNORMAL", default: 40.0, min: 0.0, max: 91.0, integer: false },
    Var { name: "FACETERDEVSURFACE", default: 0.001, min: 0.0, max: f64::MAX, integer: false },
    int("FACETERMESHTYPE", 0, 0, 2),
    int("FACETERSMOOTHLEV", 1, 0, 4),
    int("SMOOTHMESHCONVERT", 0, 0, 3),
    int("SMOOTHMESHMAXLEV", 4, 1, 255),
    int("SURFACEAUTOTRIM", 0, 0, 1),
];

fn values() -> &'static Mutex<HashMap<&'static str, f64>> {
    static VALUES: OnceLock<Mutex<HashMap<&'static str, f64>>> = OnceLock::new();
    VALUES.get_or_init(|| Mutex::new(VARS.iter().map(|v| (v.name, v.default)).collect()))
}

/// The setting `name` (as listed in [`VARS`]).
pub fn var(name: &str) -> f64 {
    values().lock().unwrap_or_else(|e| e.into_inner()).get(name).copied().unwrap_or(0.0)
}

/// Sets `name` to `value`; `Err` names the range it must be in.
pub fn set_var(name: &str, value: f64) -> Result<(), String> {
    let v = VARS.iter().find(|v| v.name == name).ok_or_else(|| format!("{name}?"))?;
    if !value.is_finite() || value < v.min || value > v.max || (v.integer && value.fract() != 0.0) {
        return Err(if v.integer {
            crate::tf!("Requires an integer between {min} and {max}.", min = v.min, max = v.max).into_owned()
        } else {
            crate::tf!("Requires a value between {min} and {max}.", min = v.min, max = v.max).into_owned()
        });
    }
    values().lock().unwrap_or_else(|e| e.into_inner()).insert(v.name, value);
    Ok(())
}

fn count(name: &str) -> usize {
    var(name).max(1.0) as usize
}

/// The primitive tessellation divisions as set.
pub fn divisions() -> crate::entities::mesh_edit::Divisions {
    crate::entities::mesh_edit::Divisions {
        box_: [count("DIVMESHBOXLENGTH"), count("DIVMESHBOXWIDTH"), count("DIVMESHBOXHEIGHT")],
        cone: [count("DIVMESHCONEAXIS"), count("DIVMESHCONEHEIGHT"), count("DIVMESHCONEBASE")],
        cylinder: [count("DIVMESHCYLAXIS"), count("DIVMESHCYLHEIGHT"), count("DIVMESHCYLBASE")],
        pyramid: [count("DIVMESHPYRLENGTH"), count("DIVMESHPYRHEIGHT"), count("DIVMESHPYRBASE")],
        sphere: [count("DIVMESHSPHEREAXIS"), count("DIVMESHSPHEREHEIGHT")],
        torus: [count("DIVMESHTORUSPATH"), count("DIVMESHTORUSSECTION")],
        wedge: [count("DIVMESHWEDGELENGTH"), count("DIVMESHWEDGEWIDTH"), count("DIVMESHWEDGEHEIGHT"), count("DIVMESHWEDGESLOPE")],
    }
}

/// The smoothness level new mesh primitives get (MESH's Settings).
static PRIMITIVE_LEVEL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn primitive_level() -> i32 {
    PRIMITIVE_LEVEL.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_primitive_level(level: i32) {
    PRIMITIVE_LEVEL.store(level.clamp(0, 4), std::sync::atomic::Ordering::Relaxed);
}

/// The mesh primitives MESH makes, by keyword, and the solid command that takes their size.
pub const PRIMITIVES: &[(&str, &str, &str)] = &[
    ("Box", "BOX", "BOX"),
    ("Cone", "CONE", "CONE"),
    ("CYlinder", "CYLINDER", "CYLINDER"),
    ("Pyramid", "PYRAMID", "PYRAMID"),
    ("Sphere", "SPHERE", "SPHERE"),
    ("Wedge", "WEDGE", "WEDGE"),
    ("Torus", "TORUS", "TORUS"),
];

/// The primitive keyword `typed` names: a word, or the capitals of one (`CY`).
pub fn primitive_named(typed: &str) -> Option<&'static str> {
    let typed = typed.trim().to_ascii_uppercase();
    if typed.is_empty() {
        return None;
    }
    PRIMITIVES.iter().find_map(|(label, keyword, _)| {
        let short: String = label.chars().take_while(|c| c.is_ascii_uppercase()).collect();
        (keyword.starts_with(&typed) && typed.len() >= short.len()).then_some(*keyword)
    })
}

/// The divisions each primitive takes, for MESHPRIMITIVEOPTIONS.
fn primitive_vars(keyword: &str) -> &'static [Step] {
    macro_rules! v {
        ($name:literal, $prompt:literal) => {
            Step::Value { prompt: $prompt, key: $name, default: 0.0 }
        };
    }
    match keyword {
        "BOX" => &[
            v!("DIVMESHBOXLENGTH", "Divisions along the length"),
            v!("DIVMESHBOXWIDTH", "Divisions along the width"),
            v!("DIVMESHBOXHEIGHT", "Divisions along the height"),
        ],
        "CONE" => &[
            v!("DIVMESHCONEAXIS", "Divisions around the base"),
            v!("DIVMESHCONEHEIGHT", "Divisions along the height"),
            v!("DIVMESHCONEBASE", "Divisions from the base centre to its edge"),
        ],
        "CYLINDER" => &[
            v!("DIVMESHCYLAXIS", "Divisions around the base"),
            v!("DIVMESHCYLHEIGHT", "Divisions along the height"),
            v!("DIVMESHCYLBASE", "Divisions from the base centre to its edge"),
        ],
        "PYRAMID" => &[
            v!("DIVMESHPYRLENGTH", "Divisions along each base side"),
            v!("DIVMESHPYRHEIGHT", "Divisions along the height"),
            v!("DIVMESHPYRBASE", "Divisions from the base centre to its edge"),
        ],
        "SPHERE" => &[
            v!("DIVMESHSPHEREAXIS", "Divisions around the axis"),
            v!("DIVMESHSPHEREHEIGHT", "Divisions from pole to pole"),
        ],
        "WEDGE" => &[
            v!("DIVMESHWEDGELENGTH", "Divisions along the length"),
            v!("DIVMESHWEDGEWIDTH", "Divisions along the width"),
            v!("DIVMESHWEDGEHEIGHT", "Divisions along the height"),
            v!("DIVMESHWEDGESLOPE", "Divisions along the slope"),
        ],
        "TORUS" => &[
            v!("DIVMESHTORUSPATH", "Divisions along the sweep path"),
            v!("DIVMESHTORUSSECTION", "Divisions around the tube"),
        ],
        _ => &[],
    }
}

const MESH_OPTIONS: &[Opt] = &[
    Opt { label: "Mesh type", keyword: "T", key: "FACETERMESHTYPE", prompt: "Mesh type (0 optimized, 1 mostly quads, 2 triangles)", kind: OptKind::Number },
    Opt { label: "Max distance", keyword: "D", key: "FACETERDEVSURFACE", prompt: "Maximum distance between the mesh and the original surface", kind: OptKind::Number },
    Opt { label: "Max angle", keyword: "A", key: "FACETERDEVNORMAL", prompt: "Maximum angle between new faces, degrees", kind: OptKind::Number },
    Opt { label: "Smoothness", keyword: "S", key: "FACETERSMOOTHLEV", prompt: "Smoothness level after conversion (0 to 4)", kind: OptKind::Number },
];

/// The steps of mesh command `name` (`rest`, what followed it typed); `None` for a command this
/// module does not run.
pub fn steps(name: &str, rest: &str) -> Option<Vec<Step>> {
    use Filter::*;
    Some(match name {
        "MESHSMOOTH" => vec![Step::Objects { prompt: "Select objects to convert:", filter: MeshSources, slot: 0 }],
        "MESHSMOOTHMORE" => vec![Step::Objects { prompt: "Select mesh objects to increase the smoothness level:", filter: Meshes, slot: 0 }],
        "MESHSMOOTHLESS" => vec![Step::Objects { prompt: "Select mesh objects to decrease the smoothness level:", filter: Meshes, slot: 0 }],
        "MESHREFINE" => vec![Step::Objects { prompt: "Select mesh objects to refine:", filter: Meshes, slot: 0 }],
        "MESHCREASE" => vec![
            Step::Picks { prompt: "Select mesh edges or faces to crease:", slot: 0, once: false },
            Step::Value { prompt: "Specify crease value [Always]", key: "crease", default: -1.0 },
        ],
        "MESHUNCREASE" => vec![Step::Picks { prompt: "Select crease to remove:", slot: 0, once: false }],
        "MESHEXTRUDE" => vec![
            Step::Picks { prompt: "Select mesh faces to extrude:", slot: 0, once: false },
            Step::Value { prompt: "Specify height of extrusion", key: "height", default: 1.0 },
        ],
        "MESHSPLIT" => vec![
            Step::Picks { prompt: "Select a mesh face to split:", slot: 0, once: true },
            Step::Point { prompt: "Specify first split point on face edge:" },
            Step::Point { prompt: "Specify second split point on face edge:" },
        ],
        "MESHMERGE" => vec![Step::Picks { prompt: "Select adjacent mesh faces to merge:", slot: 0, once: false }],
        "MESHCAP" => vec![Step::Picks { prompt: "Select connecting mesh edges to create a new mesh face:", slot: 0, once: false }],
        "MESHCOLLAPSE" => vec![Step::Picks { prompt: "Select mesh face or edge to collapse:", slot: 0, once: true }],
        "MESHSPIN" => vec![
            Step::Picks { prompt: "Select first triangular mesh face to spin:", slot: 0, once: true },
            Step::Picks { prompt: "Select second adjacent triangular mesh face to spin:", slot: 1, once: true },
        ],
        "CONVTOSOLID" => vec![Step::Objects { prompt: "Select mesh objects to convert:", filter: AnyMeshes, slot: 0 }],
        "CONVTOSURFACE" => vec![Step::Objects { prompt: "Select objects to convert:", filter: SurfaceSources, slot: 0 }],
        "REVSURF" => vec![
            Step::Picks { prompt: "Select object to revolve:", slot: 0, once: true },
            Step::Picks { prompt: "Select object that defines the axis of revolution:", slot: 1, once: true },
            Step::Value { prompt: "Specify start angle", key: "start", default: 0.0 },
            Step::Value { prompt: "Specify included angle (+=ccw, -=cw)", key: "angle", default: 360.0 },
        ],
        "RULESURF" => vec![
            Step::Picks { prompt: "Select first defining curve:", slot: 0, once: true },
            Step::Picks { prompt: "Select second defining curve:", slot: 1, once: true },
        ],
        "TABSURF" => vec![
            Step::Picks { prompt: "Select object for path curve:", slot: 0, once: true },
            Step::Picks { prompt: "Select object for direction vector:", slot: 1, once: true },
        ],
        // The fourth edge goes after the first, in slot 0.
        "EDGESURF" => vec![
            Step::Picks { prompt: "Select object 1 for surface edge:", slot: 0, once: true },
            Step::Picks { prompt: "Select object 2 for surface edge:", slot: 1, once: true },
            Step::Picks { prompt: "Select object 3 for surface edge:", slot: 2, once: true },
            Step::Picks { prompt: "Select object 4 for surface edge:", slot: 0, once: true },
        ],
        "MESHOPTIONS" => vec![Step::Accept {
            prompt: "Press Enter to apply or [Mesh type/Max distance/Max angle/Smoothness]:",
            options: MESH_OPTIONS,
        }],
        "MESHPRIMITIVEOPTIONS" => {
            let steps = primitive_vars(primitive_named(rest)?);
            steps.to_vec()
        }
        "MESH" if rest.eq_ignore_ascii_case("SETTINGS") || rest.eq_ignore_ascii_case("SE") => {
            vec![Step::Value { prompt: "Specify level of smoothness", key: "level", default: 0.0 }]
        }
        _ => return None,
    })
}

/// Whether `entity` is a mesh MESHSMOOTH converts.
pub fn mesh_source(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Solid3D(_) | EntityType::Surface(_) | EntityType::Region(_) | EntityType::Face3D(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_)
    )
}

/// A prompt that picks one of `choices` (label, keyword) and runs `then` with it.
pub struct ChooseCommand {
    name: &'static str,
    prompt: &'static str,
    choices: Vec<(&'static str, &'static str)>,
    default: &'static str,
    then: fn(&str) -> String,
}

impl ChooseCommand {
    /// MESH: which primitive to make (or its Settings).
    pub fn mesh() -> Self {
        let mut choices: Vec<(&'static str, &'static str)> = PRIMITIVES.iter().map(|(label, keyword, _)| (*label, *keyword)).collect();
        choices.push(("SEttings", "SETTINGS"));
        Self {
            name: "MESH",
            prompt: "Select primitive [Box/Cone/CYlinder/Pyramid/Sphere/Wedge/Torus/SEttings]",
            choices,
            default: last_primitive(),
            then: |k| {
                if k != "SETTINGS" {
                    remember_primitive(k);
                }
                format!("MESH {k}")
            },
        }
    }

    /// MESHPRIMITIVEOPTIONS: which primitive's divisions to set.
    pub fn primitive_options() -> Self {
        Self {
            name: "MESHPRIMITIVEOPTIONS",
            prompt: "Select primitive to set its divisions [Box/Cone/CYlinder/Pyramid/Sphere/Wedge/Torus]",
            choices: PRIMITIVES.iter().map(|(label, keyword, _)| (*label, *keyword)).collect(),
            default: "BOX",
            then: |k| format!("MESHPRIMITIVEOPTIONS {k}"),
        }
    }

    fn label_of(&self, keyword: &str) -> &'static str {
        self.choices.iter().find(|(_, k)| *k == keyword).map(|(l, _)| *l).unwrap_or("Box")
    }
}

static LAST_PRIMITIVE: Mutex<&'static str> = Mutex::new("BOX");

fn last_primitive() -> &'static str {
    *LAST_PRIMITIVE.lock().unwrap_or_else(|e| e.into_inner())
}

fn remember_primitive(keyword: &str) {
    if let Some(k) = PRIMITIVES.iter().map(|p| p.1).find(|k| *k == keyword) {
        *LAST_PRIMITIVE.lock().unwrap_or_else(|e| e.into_inner()) = k;
    }
}

impl CadCommand for ChooseCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        format!("{} <{}>:", crate::t!(self.prompt), self.label_of(self.default))
    }

    fn options(&self) -> Vec<CmdOption> {
        self.choices.iter().map(|(label, keyword)| CmdOption::new(label, keyword)).collect()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim().to_ascii_uppercase();
        if typed.is_empty() {
            return None;
        }
        let chosen = self.choices.iter().find_map(|(label, keyword)| {
            let short: String = label.chars().take_while(|c| c.is_ascii_uppercase()).collect();
            (keyword.starts_with(&typed) && typed.len() >= short.len()).then_some(*keyword)
        });
        Some(match chosen {
            Some(keyword) => CmdResult::Dispatch((self.then)(keyword)),
            None => CmdResult::ReportError(crate::t!("Invalid option keyword.").into_owned()),
        })
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch((self.then)(self.default))
    }
}

/// A solid primitive's result made a mesh of the same size and place.
pub fn mesh_instead(result: CmdResult) -> CmdResult {
    match result {
        CmdResult::CommitSolid { history, .. } => match crate::entities::mesh_edit::primitive(&history, &divisions()) {
            Some(mut mesh) => {
                mesh.subdivision_level = primitive_level();
                CmdResult::CommitAndExit(EntityType::Mesh(mesh))
            }
            None => CmdResult::Cancel,
        },
        other => other,
    }
}

/// Handles grouped by entity, in pick order.
pub fn by_entity(picks: &[(Handle, DVec3)]) -> Vec<(Handle, Vec<DVec3>)> {
    let mut out: Vec<(Handle, Vec<DVec3>)> = Vec::new();
    for &(h, p) in picks {
        match out.iter_mut().find(|(k, _)| *k == h) {
            Some((_, points)) => points.push(p),
            None => out.push((h, vec![p])),
        }
    }
    out
}

impl CadModule for MeshModule {
    fn id(&self) -> &'static str {
        "mesh"
    }

    fn title(&self) -> &'static str {
        "Mesh"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: OnceLock<Vec<RibbonGroup>> = OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Primitives",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "MESH_PRIMITIVES",
                            label: ocs_plugin_api::ribbon::FOLLOW_CHOSEN,
                            icon: icon!("box"),
                            items: vec![
                                ("MESH BOX", "Mesh Box", icon!("box")),
                                ("MESH CYLINDER", "Mesh Cylinder", icon!("cylinder")),
                                ("MESH CONE", "Mesh Cone", icon!("cone")),
                                ("MESH SPHERE", "Mesh Sphere", icon!("sphere")),
                                ("MESH PYRAMID", "Mesh Pyramid", icon!("pyramid")),
                                ("MESH WEDGE", "Mesh Wedge", icon!("wedge")),
                                ("MESH TORUS", "Mesh Torus", icon!("torus")),
                            ],
                            default: "MESH BOX",
                        },
                        RibbonItem::ToolGrid {
                            columns: vec![
                                vec![def("REVSURF", "Revolved Surface", icon!("revsurf")), def("RULESURF", "Ruled Surface", icon!("rulesurf"))],
                                vec![def("EDGESURF", "Edge Surface", icon!("edgesurf")), def("TABSURF", "Tabulated Surface", icon!("tabsurf"))],
                                vec![def("MESHPRIMITIVEOPTIONS", "Mesh Primitive Options", icon!("primitiveoptions"))],
                            ],
                        },
                    ],
                },
                RibbonGroup {
                    title: "Mesh",
                    tools: vec![
                        RibbonItem::LargeTool(def("MESHSMOOTH", "Smooth Object", icon!("smooth"))),
                        RibbonItem::LabeledTool(def("MESHSMOOTHMORE", "Smooth More", icon!("smoothmore"))),
                        RibbonItem::LabeledTool(def("MESHSMOOTHLESS", "Smooth Less", icon!("smoothless"))),
                        RibbonItem::LabeledTool(def("MESHREFINE", "Refine Mesh", icon!("refine"))),
                        RibbonItem::LargeTool(def("MESHCREASE", "Add Crease", icon!("crease"))),
                        RibbonItem::LargeTool(def("MESHUNCREASE", "Remove Crease", icon!("uncrease"))),
                        RibbonItem::LabeledTool(def("MESHOPTIONS", "Tessellation Options", icon!("options"))),
                    ],
                },
                RibbonGroup {
                    title: "Mesh Edit",
                    tools: vec![
                        RibbonItem::LargeTool(def("MESHEXTRUDE", "Extrude Face", icon!("extrude"))),
                        RibbonItem::LabeledTool(def("MESHSPLIT", "Split Face", icon!("split"))),
                        RibbonItem::LabeledTool(def("MESHMERGE", "Merge Face", icon!("merge"))),
                        RibbonItem::LabeledTool(def("MESHCAP", "Close Hole", icon!("cap"))),
                        RibbonItem::LabeledTool(def("MESHCOLLAPSE", "Collapse Face or Edge", icon!("collapse"))),
                        RibbonItem::LabeledTool(def("MESHSPIN", "Spin Triangle Face", icon!("spin"))),
                    ],
                },
                RibbonGroup {
                    title: "Convert Mesh",
                    tools: vec![
                        RibbonItem::LabeledTool(def("CONVTOSOLID", "Convert to Solid", icon!("tosolid"))),
                        RibbonItem::LabeledTool(def("CONVTOSURFACE", "Convert to Surface", icon!("tosurface"))),
                        RibbonItem::LargeDropdown {
                            id: "SMOOTHMESHCONVERT",
                            label: ocs_plugin_api::ribbon::FOLLOW_CHOSEN,
                            icon: icon!("smoothoptimized"),
                            items: vec![
                                ("SMOOTHMESHCONVERT 0", "Smooth, optimized", icon!("smoothoptimized")),
                                ("SMOOTHMESHCONVERT 1", "Smooth, not optimized", icon!("smooth")),
                                ("SMOOTHMESHCONVERT 2", "Faceted, optimized", icon!("facetedoptimized")),
                                ("SMOOTHMESHCONVERT 3", "Faceted, not optimized", icon!("faceted")),
                            ],
                            default: "SMOOTHMESHCONVERT 0",
                        },
                    ],
                },
            ]
        })
    }
}
