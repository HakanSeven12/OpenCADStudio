// MLEADERADD / MLEADERREMOVE / MLEADERALIGN / MLEADERCOLLECT commands.
//
// MLEADERADD:    pick a multileader → pick new arrowhead point → adds a leader line
// MLEADERREMOVE: pick a multileader → pick a leader line to remove
// MLEADERALIGN:  select multileaders → pick base alignment direction
// MLEADERCOLLECT: select block-content multileaders → pick collection point

use acadrust::entities::{LeaderLine, MultiLeader};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::{closest_point, Curve, Line};
use glam::{DVec3, Vec3};

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

// ── MLEADERADD ────────────────────────────────────────────────────────────────

pub const ICON_ADD: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_add.svg"));

pub fn tool_add() -> ToolDef {
    ToolDef {
        id: "MLEADERADD",
        label: "Add Leader",
        icon: ICON_ADD,
        event: ModuleEvent::Command("MLEADERADD".to_string()),
    }
}

enum AddStep {
    PickMLeader,
    PickArrowhead {
        handle: Handle,
        entity: Option<EntityType>,
    },
    CollectPoints {
        handle: Handle,
        entity: EntityType,
        pts: Vec<DVec3>,
    },
}

pub struct MLeaderAddCommand {
    step: AddStep,
}

impl MLeaderAddCommand {
    pub fn new() -> Self {
        Self {
            step: AddStep::PickMLeader,
        }
    }
}

impl CadCommand for MLeaderAddCommand {
    fn name(&self) -> &'static str {
        "MLEADERADD"
    }

    fn prompt(&self) -> String {
        match &self.step {
            AddStep::PickMLeader => t!("MLEADERADD  Select a multileader:").into_owned(),
            AddStep::PickArrowhead { .. } => {
                t!("MLEADERADD  Specify arrowhead location:").into_owned()
            }
            AddStep::CollectPoints { pts, .. } => t!(
                "MLEADERADD  Specify next leader point (%{count} pts, Enter to finish):",
                count = pts.len()
            )
            .into_owned(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, AddStep::PickMLeader)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.step = AddStep::PickArrowhead {
            handle,
            entity: None,
        };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &mut self.step {
            AddStep::PickArrowhead { handle, entity } => {
                if let Some(ent) = entity.take() {
                    if !matches!(ent, EntityType::MultiLeader(_)) {
                        return CmdResult::Cancel;
                    }
                    let h = *handle;
                    self.step = AddStep::CollectPoints {
                        handle: h,
                        entity: ent,
                        pts: vec![pt],
                    };
                    return CmdResult::NeedPoint;
                }
                CmdResult::NeedPoint
            }
            AddStep::CollectPoints { pts, .. } => {
                pts.push(pt);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if let AddStep::CollectPoints {
            handle,
            entity,
            pts,
        } = &mut self.step
        {
            if pts.len() < 1 {
                return CmdResult::Cancel;
            }
            let h = *handle;
                if let EntityType::MultiLeader(ref mut ml) = entity {
                let points: Vec<Vector3> = pts
                    .iter()
                    .map(|p| Vector3::new(p.x, p.y, p.z))
                    .collect();
                let template_root = ml.context.leader_roots.first().cloned();
                let template_line = template_root
                    .as_ref()
                    .and_then(|root| root.lines.first())
                    .cloned();
                let path_type = ml.path_type;
                let line_color = ml.line_color;
                let line_type_handle = ml.line_type_handle;
                let line_weight = ml.line_weight;
                let arrowhead_handle = ml.arrowhead_handle;
                let arrowhead_size = ml.arrowhead_size;
                let root = ml.context.add_leader_root();
                if let Some(template) = template_root {
                    root.connection_point = template.connection_point;
                    root.direction = template.direction;
                    root.landing_distance = template.landing_distance;
                    root.text_attachment_direction = template.text_attachment_direction;
                }
                let line = root.create_line(points);
                if let Some(template) = template_line {
                    line.path_type = template.path_type;
                    line.line_color = template.line_color;
                    line.line_type_handle = template.line_type_handle;
                    line.line_weight = template.line_weight;
                    line.arrowhead_handle = template.arrowhead_handle;
                    line.arrowhead_size = template.arrowhead_size;
                    line.override_flags = template.override_flags;
                } else {
                    line.path_type = path_type;
                    line.line_color = line_color;
                    line.line_type_handle = line_type_handle;
                    line.line_weight = line_weight;
                    line.arrowhead_handle = arrowhead_handle;
                    line.arrowhead_size = arrowhead_size;
                }
            }
            let updated = std::mem::replace(entity, EntityType::XLine(Default::default()));
            return CmdResult::ReplaceEntity(h, vec![updated]);
        }
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let existing_pts = match &self.step {
            AddStep::CollectPoints { pts, .. } => pts.clone(),
            _ => return None,
        };
        let mut all_pts = existing_pts;
        all_pts.push(pt);
        Some(preview_wire(
            &all_pts.iter().map(|p| p.as_vec3()).collect::<Vec<_>>(),
        ))
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        if let AddStep::PickArrowhead { entity: slot, .. } = &mut self.step {
            *slot = Some(entity);
        }
    }
}

// ── MLEADERREMOVE ─────────────────────────────────────────────────────────────

pub const ICON_REMOVE: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_remove.svg"));

pub fn tool_remove() -> ToolDef {
    ToolDef {
        id: "MLEADERREMOVE",
        label: "Remove Leader",
        icon: ICON_REMOVE,
        event: ModuleEvent::Command("MLEADERREMOVE".to_string()),
    }
}

enum RemoveStep {
    PickMLeader,
    PickLeaderToRemove {
        handle: Handle,
        entity: Option<EntityType>,
    },
}

pub struct MLeaderRemoveCommand {
    step: RemoveStep,
}

impl MLeaderRemoveCommand {
    pub fn new() -> Self {
        Self {
            step: RemoveStep::PickMLeader,
        }
    }
}

impl CadCommand for MLeaderRemoveCommand {
    fn name(&self) -> &'static str {
        "MLEADERREMOVE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            RemoveStep::PickMLeader => t!("MLEADERREMOVE  Select a multileader:").into_owned(),
            RemoveStep::PickLeaderToRemove { .. } => {
                t!("MLEADERREMOVE  Click near the leader line to remove:").into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, RemoveStep::PickMLeader)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.step = RemoveStep::PickLeaderToRemove {
            handle,
            entity: None,
        };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult { let pt = pt.as_vec3();
        if let RemoveStep::PickLeaderToRemove { handle, entity } = &mut self.step {
            if let Some(mut ent) = entity.take() {
                let h = *handle;
                if let EntityType::MultiLeader(ref mut ml) = ent {
                    let pick = Vector3::new(pt.x as f64, pt.y as f64, pt.z as f64);
                    let best = ml
                        .context
                        .leader_roots
                        .iter()
                        .enumerate()
                        .flat_map(|(root_index, root)| {
                            root.lines.iter().enumerate().map(move |(line_index, line)| {
                                let mut distance = line
                                    .points
                                    .windows(2)
                                    .map(|segment| point_segment_distance_xy(pick, segment[0], segment[1]))
                                    .fold(f64::INFINITY, f64::min);
                                if let Some(last) = line.points.last().copied() {
                                    distance = distance.min(point_segment_distance_xy(
                                        pick,
                                        last,
                                        root.connection_point,
                                    ));
                                }
                                (root_index, line_index, distance)
                            })
                        })
                        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
                    let Some((root_index, line_index, _)) = best else {
                        return CmdResult::Cancel;
                    };
                    if ml.total_leader_line_count() <= 1 {
                        return CmdResult::Cancel;
                    }
                    let root = &mut ml.context.leader_roots[root_index];
                    root.lines.remove(line_index);
                    if root.lines.is_empty() {
                        ml.context.leader_roots.remove(root_index);
                    }
                } else {
                    return CmdResult::Cancel;
                }
                return CmdResult::ReplaceEntity(h, vec![ent]);
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        if let RemoveStep::PickLeaderToRemove { entity: slot, .. } = &mut self.step {
            *slot = Some(entity);
        }
    }
}

// ── MLEADERALIGN ─────────────────────────────────────────────────────────────

pub const ICON_ALIGN: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_align.svg"));

pub fn tool_align() -> ToolDef {
    ToolDef {
        id: "MLEADERALIGN",
        label: "Align Leaders",
        icon: ICON_ALIGN,
        event: ModuleEvent::Command("MLEADERALIGN".to_string()),
    }
}

enum AlignStep {
    Gathering,
    PickDirection { handles: Vec<Handle> },
    PickEndDir { handles: Vec<Handle>, from: DVec3 },
}

pub struct MLeaderAlignCommand {
    step: AlignStep,
}

impl MLeaderAlignCommand {
    pub fn new() -> Self {
        Self {
            step: AlignStep::Gathering,
        }
    }
}

impl CadCommand for MLeaderAlignCommand {
    fn name(&self) -> &'static str {
        "MLEADERALIGN"
    }

    fn prompt(&self) -> String {
        match &self.step {
            AlignStep::Gathering => {
                t!("MLEADERALIGN  Select multileaders to align (Enter when done):").into_owned()
            }
            AlignStep::PickDirection { .. } => {
                t!("MLEADERALIGN  Specify direction — pick start point:").into_owned()
            }
            AlignStep::PickEndDir { .. } => {
                t!("MLEADERALIGN  Specify end point of alignment direction:").into_owned()
            }
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, AlignStep::Gathering)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        self.step = AlignStep::PickDirection { handles };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &mut self.step {
            AlignStep::PickDirection { handles } => {
                let h = handles.clone();
                self.step = AlignStep::PickEndDir {
                    handles: h,
                    from: pt,
                };
                CmdResult::NeedPoint
            }
            AlignStep::PickEndDir { handles, from } => {
                CmdResult::AlignMLeaders {
                    handles: handles.clone(),
                    from: *from,
                    to: pt,
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}

// ── MLEADERCOLLECT ────────────────────────────────────────────────────────────

pub const ICON_COLLECT: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_collect.svg"));

pub fn tool_collect() -> ToolDef {
    ToolDef {
        id: "MLEADERCOLLECT",
        label: "Collect Leaders",
        icon: ICON_COLLECT,
        event: ModuleEvent::Command("MLEADERCOLLECT".to_string()),
    }
}

enum CollectStep {
    Gathering,
    PickLocation { handles: Vec<Handle> },
}

pub struct MLeaderCollectCommand {
    step: CollectStep,
}

impl MLeaderCollectCommand {
    pub fn new() -> Self {
        Self {
            step: CollectStep::Gathering,
        }
    }
}

impl CadCommand for MLeaderCollectCommand {
    fn name(&self) -> &'static str {
        "MLEADERCOLLECT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            CollectStep::Gathering => {
                t!("MLEADERCOLLECT  Select multileaders to collect (Enter when done):").into_owned()
            }
            CollectStep::PickLocation { .. } => {
                t!("MLEADERCOLLECT  Specify collected multileader location:").into_owned()
            }
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, CollectStep::Gathering)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        self.step = CollectStep::PickLocation { handles };
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let CollectStep::PickLocation { handles } = &self.step {
            return CmdResult::CollectMLeaders {
                handles: handles.clone(),
                point: pt,
            };
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn preview_wire(pts: &[Vec3]) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
        name: "mleader_edit_preview".into(),
        points: pts.iter().map(|p| [p.x, p.y, p.z]).collect(),
        points_low: Vec::new(),
        color: WireModel::CYAN,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        aci: 0,
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

fn point_segment_distance_xy(point: Vector3, start: Vector3, end: Vector3) -> f64 {
    closest_point(
        &Curve::Line(Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
        }),
        [point.x, point.y],
    )
    .distance
}

// Silence unused-import warning for MultiLeader and LeaderLine if not used in all paths
fn _uses_ml_types(_ml: &MultiLeader, _ll: &LeaderLine) {}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["MLEADERADD"] });  // MLeaderAddCommand
inventory::submit!(crate::command::CommandRegistration { names: &["MLEADERALIGN"] });  // MLeaderAlignCommand
inventory::submit!(crate::command::CommandRegistration { names: &["MLEADERCOLLECT"] });  // MLeaderCollectCommand
inventory::submit!(crate::command::CommandRegistration { names: &["MLEADERREMOVE"] });  // MLeaderRemoveCommand
